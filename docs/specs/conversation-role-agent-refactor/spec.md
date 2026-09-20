# SPEC: 对话以 Role Agent 为单位 + CLI Agent 会话 id 持久化（conversation-role-agent-refactor）

> 状态：待批准（属重大架构变更，需先写 ADR 并获用户确认再动工）
> 前置：`docs/specs/settings-integration/spec.md`（✅ 2026-09-15 落地）、`docs/specs/cli-agents-m1.md`（M-CLI1 ✅）、`docs/specs/role-agents-ui/spec.md`。
> 需求来源：用户请求「重构"对话"功能：1. 对话单位以 Role Agent 为最小单位，而不是 Provider 或 CLI Agent，但没有配置 Role Agent 时显示配置提示；2. CLI Agent 在连续对话中必须能够与其自身的模型服务器保持唯一会话 ID，对话框必须持有和持久化 CLI Agent 的会话 id，会话 id 需要通过命令行参数传入，对话中的动态参数和预定义参数需要拼接到一起传给 CLI Agent，而不是动态参数覆盖预定义参数」；需求权威仍为 `pr.md`。
> 事实基础：`AgentRefKind::{Cli, Role}`（`entities.rs:42`）；前端空状态守门以 Provider 为维度（`ConversationView.tsx:38-52`）；`AgentChip`/`AgentPickerPopover` 已实现但未接线；CLI Agent 每次 `stream()` spawn 新子进程（`cli.rs:319`），`feed_claude_code` 丢弃 `session_id`（`cli.rs:601-634`）；`resolve_args`（`cli.rs:506`）只做 `{prompt}` 替换无动态参数；sessions 表无 `cli_session_id`。

## 0. 问题诊断 / Problem Diagnosis

### 0.1 对话对象维度错位

| 维度 | 现状 | 问题 |
|---|---|---|
| Session 绑定 | `(AgentRefKind::{Cli,Role}, id)` | 允许直接绑裸 CLI Agent，不符合"Role Agent 为最小单位" |
| 前端空状态守门 | Provider 维度（`providersQuery.data` 为空） | 应为 Role Agent 维度 |
| 对话对象选择 UI | `AgentChip`/`AgentPickerPopover` 未接线；`/agent` 唯一入口 | 已实现组件闲置，用户无可视化选择入口 |
| resolve_agent 默认链 | 含"第一个 enabled AgentProfile(Cli)"档 | 应去掉，CLI Agent 须通过 Role 绑定 |

### 0.2 CLI Agent 会话 id 完全缺失

- 每次 `stream()` spawn 新子进程（`cli.rs:319`），连续对话每轮开新 CLI 会话，无法利用 CLI Agent 自身 cache → 费用与延迟双高。
- Claude Code 输出 JSON 含 `session_id`（`cli.rs:777` 测试 fixture），但 `feed_claude_code`（`cli.rs:601-634`）只提取 text/usage，**丢弃 session_id**。
- `ResumableSession` trait 是空 marker（`traits.rs:51`），`CliAgentClient` 显式返回 `None`（`cli.rs:300`）。
- `AgentProfile` / `agent_profiles` 表 / `AgentProfileInput` 均无 session_id / resume 相关字段。

### 0.3 参数处理无动态参数概念

- `resolve_args`（`cli.rs:506-527`）只做 `{prompt}` 占位符替换，无动态参数拼接逻辑。
- 无法注入 `--resume <id>` 等运行时会话保持参数。

## 1. 决策记录摘要 / Decisions

| # | 决策 |
|---|---|
| D1 | **对话以 Role Agent 为最小单位**：废弃 `AgentRefKind::Cli` 作为对话对象；CLI Agent 须先绑到 Role 形成 Role Agent（`isRoleReady === true`）才能使用。既有 `agent_kind='cli'` 的 session 由 migration 0020 清空 agent 绑定，打开时提示重新选择。 |
| D2 | **空状态守门改为 Role Agent 维度**：`ConversationView` 判定从"无 Provider"改为"无 isRoleReady Role"；空时提示"尚未配置 Role Agent" + 跳 Settings。 |
| D3 | **接线 AgentChip + AgentPickerPopover**：接到 `Composer.leftSlot`；数据源只列 isRoleReady Role；`/agent` 候选同步；`resolve_agent` 默认链去掉 Cli 档。 |
| D4 | **CLI session id 作用域 = per (session, role_agent)**：新表 `session_cli_handles`，不同 Role Agent 各持独立句柄，群聊互不干扰。 |
| D5 | **绑定变更传截断上下文重建**：`agent_profile_id` 变更时不传旧 session id，传最新 N token（默认 64K，可配）创建新会话，upsert handle。 |
| D6 | **会话保持参数：方言默认 + resume_args 可覆盖**：ClaudeCode `--resume {session_id}`、OpenCode `--session {session_id}`、Codex 留空、Plain 无。 |
| D7 | **预定义 args + 动态 args 拼接不覆盖**：`resolve_args(predefined, dynamic, prompt)` 合并 argv，`{prompt}`/`{session_id}` 占位符替换。 |
| D8 | **CLI adapter 提取并回写 session id**：`feed_claude_code` 提取 session_id；`stream` 接收 + 返回 session id；`run_conversation_turn` 查/回写 handle。 |

## 2. 目标 / Goals

1. **对话单位统一**：对话以 Role Agent 为最小单位，不再直接选 Provider 或裸 CLI Agent；未配置时显示配置提示。
2. **CLI 会话复用**：连续对话复用 CLI Agent 自身会话 id（如 `--resume <id>`），利用 CLI cache 降低费用与延迟。
3. **参数拼接**：预定义参数 + 动态参数拼接传给 CLI Agent，不覆盖。
4. **群聊隔离**：不同 Role Agent 各持独立 CLI 会话句柄，互不干扰。
5. **绑定变更平滑**：Role Agent 换绑 CLI Agent 时传截断上下文重建会话，不硬中断。
6. **不回归**：既有 Provider 直连对话（env 兜底）仍可用；Board 路径零变更。

## 3. 用户故事 / User Stories

- **US1** 用户未配置任何 Role Agent，打开对话页看到"尚未配置 Role Agent，去 Settings 配置"提示 + 按钮；点击跳转 Settings 的 Roles 区域。
- **US2** 用户在对话框点击左下角 AgentChip，弹出 AgentPickerPopover 只展示已配置的 Role Agent（isRoleReady）；选择后该会话绑定该 Role Agent，后续对话走该 Role Agent 的 Provider/CLI Agent + Role overlay。
- **US3** 用户的 Role Agent 绑定了 Claude Code CLI Agent；首轮对话后 Claude Code 返回 session_id，nuomi 持久化；第二轮对话 nuomi 以 `--resume <session_id>` 调用 Claude Code，复用其会话上下文，费用降低。
- **US4** 用户在群聊中配了两个 Role Agent 都绑同一个 Claude Code CLI Agent；两 Agent 各持独立 cli_session_id，互不干扰，不传同一个 session id。
- **US5** 用户把某 Role Agent 绑定的 CLI Agent 从 Claude Code 换成 Codex；下次对话时 nuomi 不传旧 session id，改为把最近 64K token 上下文传给 Codex 创建新会话，保存 Codex 返回的新 session id。
- **US6** 用户在 CLI Agent 设置页配了预定义 args `--output-format stream-json --verbose`，nuomi 对话时动态注入 `--resume <id>`，最终传给 Claude Code 的是 `--output-format stream-json --verbose --resume <id>`（拼接而非覆盖）。
- **US7** 用户打开一个旧会话（曾绑裸 CLI Agent），提示"该会话的对话对象已失效，请重新选择 Role Agent"；选择后恢复对话。

## 4. 验收标准 / Acceptance Criteria

**阶段一：对话以 Role Agent 为单位**
- [ ] AC1 `ConversationView` 空状态守门判定为 `roles.filter(isRoleReady)` 为空（非 providers 为空）；空时显示"尚未配置 Role Agent" + 跳 Settings 按钮。
- [ ] AC2 `AgentChip` + `AgentPickerPopover` 接到 `Composer.leftSlot`（ChatView + GroupConversationView）；Popover 数据源只列 isRoleReady Role，不列裸 CLI Agent。
- [ ] AC3 `NewConversationDialog` 增加 Role Agent 选择步骤（可选，不选走默认链）；`/agent` 候选只列 Role Agent。
- [ ] AC4 `resolve_agent` 默认链去掉"第一个 enabled AgentProfile(Cli)"档 → Session 绑定 → 全局默认 → 第一个 builtin 且 isRoleReady Role → None。
- [ ] AC5 migration 0020 清空既有 `agent_kind='cli'` 的 session 的 agent 绑定；打开时提示重新选择 Role Agent（非硬错误，历史可查看）。

**阶段二：CLI Agent 会话 id 持久化**
- [ ] AC6 新表 `session_cli_handles(session_id, role_agent_id, agent_profile_id, cli_session_id, updated_at)` 创建，复合主键 `(session_id, role_agent_id)`。
- [ ] AC7 `AgentProfile` 新增 `resume_args: Option<String>` 字段（migration 0020 加列）；`AgentProfileInput`/`AgentProfileDto` 镜像（camelCase）。
- [ ] AC8 `feed_claude_code` 提取输出 JSON 的 `session_id`（不再丢弃）；stream 返回值含 `cli_session_id: Option<String>`。
- [ ] AC9 首次调用 CLI Agent（handle 不存在）不传 session id；CLI Agent 返回 session_id 后 insert `session_cli_handles`。
- [ ] AC10 连续对话复用：handle 存在且 `agent_profile_id` 一致时，以 `--resume <cli_session_id>`（或方言对应参数）调用 CLI Agent。

**阶段三：参数拼接 + 绑定变更交接**
- [ ] AC11 `resolve_args(predefined, dynamic, prompt)` 拼接预定义 args + 动态 args 成完整 argv，`{prompt}`/`{session_id}` 占位符替换；动态 args 不覆盖预定义 args。
- [ ] AC12 方言默认 resume 模板：ClaudeCode `--resume {session_id}`、OpenCode `--session {session_id}`、Codex 留空、Plain 无；`AgentProfile.resume_args` 非空时覆盖方言默认。
- [ ] AC13 Role Agent 绑定的 `agent_profile_id` 变更时（handle.agent_profile_id != 当前），不传旧 session id，传截断上下文（最新 N token，默认 65536，`app_settings.cli_context_handover_tokens` 可配）给新 CLI Agent 创建新会话；upsert handle 更新 agent_profile_id + cli_session_id。
- [ ] AC14 群聊中每个 Role Agent 各持独立 `session_cli_handles` 行，互不干扰（不同 role_agent_id → 不同 cli_session_id）。

**横切**
- [ ] AC15 既有 Provider 直连对话（env 兜底）仍可用；Board `runTeamOnTask` 路径零变更（回归测试守护）。
- [ ] AC16 `AgentRefKind::Cli` 标记 `#[deprecated]`，代码层面不再产生新的 Cli 绑定（新建 session agent 只用 Role）。
- [ ] AC17 双端质量门全绿：`cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test` 与 `pnpm typecheck && pnpm lint && pnpm test`。

## 5. 影响范围 / Impact

| 层 | 文件 | 改动 |
|---|---|---|
| migration | `0020_conversation_role_agent_cli_session.sql`（新增） | 新表 session_cli_handles + agent_profiles 加 resume_args 列 + app_settings 加 cli_context_handover_tokens + 清空既有 cli 绑定 |
| domain | `entities.rs` | AgentProfile 加 resume_args；AgentRefKind::Cli 标记 deprecated；新实体 SessionCliHandle |
| store | `repos/sessions.rs` / 新 `repos/session_cli_handles.rs` | handle CRUD + 既有 Cli 绑定清空 |
| adapters | `cli.rs` | feed_claude_code 提取 session_id；stream 接收/返回 session id；resolve_args 拼接 |
| services | `conversation_service.rs` / `single_role_materializer.rs` | resolve_agent 默认链调整；handle 查/回写 |
| commands | `commands.rs` | run_conversation_turn 串联 handle；AgentProfileInput/Dto 加 resumeArgs |
| facade | `facade.rs` | run_turn 传递 external_session_id |
| 前端 | `ConversationView` / `AgentChip` / `AgentPickerPopover` / `NewConversationDialog` / `CliAgentForm` / `agentCommands` | 守门改造 + 接线 + 数据源 + resume_args 输入 |
