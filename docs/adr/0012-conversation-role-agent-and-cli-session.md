# ADR 0012: 对话以 Role Agent 为单位 + CLI Agent 会话 id 持久化

- 状态：Proposed（2026-09-17，待用户确认）
- 关联：SPEC `docs/specs/conversation-role-agent-refactor/spec.md`、ADR 0011（Settings 运行时统一）、`docs/specs/cli-agents-m1.md`（M-CLI1）
- 背景：当前对话对象维度是 `AgentRefKind::{Cli, Role}`（`entities.rs:42`），但前端空状态守门以 Provider 为维度（`ConversationView.tsx:38-52`）；`AgentChip`/`AgentPickerPopover` 已实现但未接线；CLI Agent 每次 `stream()` spawn 新子进程（`cli.rs:319`），Claude Code 输出的 `session_id` 被 `feed_claude_code` 丢弃（`cli.rs:601-634`），连续对话无法利用 CLI Agent 自身 cache，费用与延迟双高；`resolve_args`（`cli.rs:506-527`）只做 `{prompt}` 占位符替换，无动态参数拼接。

## 决策

1. **对话以 Role Agent 为最小单位**。Role Agent = `isRoleReady === true` 的 Role（`roleReady.ts:13`，绑定了 Provider 或 CLI Agent）。废弃 `AgentRefKind::Cli` 作为对话对象——CLI Agent 必须先绑定到 Role 形成 Role Agent 才能在对话中使用。既有 `sessions.agent_kind='cli'` 的行由 migration 0020 清空 agent 绑定（`agent_kind=NULL, agent_ref_id=NULL`），用户打开时提示重新选择 Role Agent。`AgentRefKind` 枚举保留 `Cli` 变体避免编译破坏（序列化/DB 兼容），但标记 `#[deprecated(note = "对话对象已统一为 Role Agent，CLI Agent 须通过 Role 绑定")]`，代码层面不再产生新的 Cli 绑定。

2. **空状态守门改为 Role Agent 维度**。`ConversationView.tsx:38-52` 判定从"无 Provider"（`providersQuery.data` 为空）改为"无 isRoleReady Role"（`roles.filter(isRoleReady)` 为空）；空时显示"尚未配置 Role Agent" + 跳 Settings 按钮。新建对话（`NewConversationDialog`）增加 Role Agent 选择步骤（可选，不选走默认链）。

3. **接线 AgentChip + AgentPickerPopover**。把已实现的 `AgentChip`（`AgentChip.tsx:26`）+ `AgentPickerPopover`（`AgentPickerPopover.tsx:23`）接到 `Composer.leftSlot`（`ChatView.tsx:74` / `GroupConversationView.tsx:92`）；Popover 数据源只列 `isRoleReady` 的 Role（Role Agent），不再列裸 CLI Agent。`/agent` slash 命令候选同步改为只列 Role Agent。`resolve_agent` 默认链去掉"第一个 enabled AgentProfile(Cli)"档 → Session 绑定 → 全局默认 → 第一个 builtin 且 isRoleReady 的 Role → None。

4. **CLI Agent 会话 id 作用域 = per (session, role_agent)**。不同 Role Agent 各持独立的 CLI 会话句柄，即使绑定同一个 CLI Agent 也视为不同实例（群聊中互不干扰）。新表 `session_cli_handles`（migration 0020）：`(session_id, role_agent_id)` 复合主键 + `agent_profile_id`（绑定变更检测）+ `cli_session_id`（CLI Agent 自身会话 id，NULL=尚未建立）+ `updated_at`。不放在 `sessions` 表单列——因一个 nuomi 会话可含多个 Role Agent（群聊），per-session 单列无法表达。

5. **CLI Agent 绑定变更时传截断上下文重建会话**。当 `session_cli_handles.agent_profile_id` 与当前 Role Agent 绑定的 `agent_profile_id` 不一致时（用户换了 CLI Agent）：不传旧 `cli_session_id`（对旧工具无效），改为从 events 表读取该 `(session, role_agent)` 的历史，截断到最新 N token（默认 65536，用户可通过 `app_settings.cli_context_handover_tokens` 配置），作为 prompt 传给新 CLI Agent 创建新会话；CLI Agent 返回新 `cli_session_id` 后 upsert handle（更新 `agent_profile_id` + `cli_session_id`）。首次调用（handle 不存在）同理不传 session id。

6. **会话保持参数：方言默认 + AgentProfile.resume_args 可覆盖**。`AgentProfile` 新增 `resume_args: Option<String>` 字段（如 `--resume {session_id}`），`{session_id}` 占位符运行时替换。各方言默认模板：ClaudeCode → `--resume {session_id}`（已确认 `claude --resume <id>`，CLI reference）、OpenCode → `--session {session_id}`（已确认 `opencode run --session <id>`，CLI docs）、Codex → 留空（官方文档未暴露 session 参数，用户自配）、Plain → 无。`AgentProfile.resume_args` 非空时覆盖方言默认。

7. **预定义 args + 动态 args 拼接，不覆盖**。`resolve_args`（`cli.rs:506`）签名改为 `resolve_args(predefined_args, dynamic_args, prompt)`：预定义 args（`AgentProfile.args`）+ 动态 args（resume 参数等）拼接成完整 argv 数组传给子进程，`{prompt}` 占位符替换照旧。动态 args 不覆盖预定义 args——两者合并。动态 args 由 resume 模板展开生成：`resume_args` 模板按空格分词后，`{session_id}` 替换为实际 cli_session_id。

8. **CLI adapter 提取并回写 session id**。`feed_claude_code`（`cli.rs:601`）提取输出 JSON 中的 `session_id`（不再丢弃）；`CliAgentClient::stream` 接收 `external_session_id: Option<String>`，按方言拼接 resume 参数到 dynamic args；stream 返回值增加 `cli_session_id: Option<String>`（CLI Agent 返回的新会话 id）。`run_conversation_turn` 在调用前查 handle 决定是否传 session id（D4/D5），调用后回写 `session_cli_handles`。

## 回滚边界

1. 对话对象维度改动（D1-D3）若出问题：回滚 `ConversationView` 守门为 Provider 维度、`AgentPickerPopover` 数据源恢复列 Cli、`resolve_agent` 默认链恢复 Cli 档。既有 Cli 绑定已被 migration 清空——回滚需手动恢复或接受既有 Cli session 失效（可重新 `/agent` 选择）。
2. CLI session id 持久化（D4-D8）若出问题：`session_cli_handles` 表为新增，回滚即 drop 该表 + 移除 handle 查询/回写逻辑，CLI Agent 回退每次 spawn 新子进程（现状，行为等价）。`AgentProfile.resume_args` 字段为新增列，回滚即忽略该列。
3. 参数拼接（D7）若出问题：`resolve_args` 回退为只处理预定义 args + `{prompt}` 替换（现状），不拼接 dynamic args——CLI Agent 不传 resume 参数，行为等价现状。
4. migration 0020 为纯新增（新表 + 新列 + 既有 Cli 绑定清空），回滚需手动 drop 新表/新列 + 恢复既有 Cli 绑定（如有备份）。

## 灰度策略

- **既有 Cli session 兼容**：migration 清空 `agent_kind='cli'` 的 agent 绑定，用户打开时提示"该会话的对话对象已失效，请重新选择 Role Agent"（非硬错误，会话历史仍可查看）。
- **CLI session id 渐进建立**：首次调用不传 session id（行为等价现状），CLI Agent 返回 id 后才持久化；既有会话续传时无 handle → 首次调用建立 handle，无中断。
- **resume_args 可空**：`AgentProfile.resume_args` 默认 NULL，使用方言默认模板；Codex 方言默认留空 → 不传 resume 参数（行为等价现状），用户配了才启用。
- **IPC 签名**：`AgentProfileInput`/`AgentProfileDto` 增加 `resumeArgs` 字段（可选）；`submitTask` 签名不变（session id 在后端内部处理，前端无感）。

## 权衡

- **废弃 Cli 档 vs 保留兼容**：选废弃——用户明确要求"以 Role Agent 为最小单位，而不是 CLI Agent"；保留枚举变体 + DB 清空既有绑定是兼容性与清洁性的平衡（编译不破坏，运行时干净）。
- **per (session, role_agent) vs per session**：选 per (session, role_agent)——用户明确要求"不同 Role Agent 绑定同一个 CLI Agent 应为两个不同实例，群聊中不应传同一个 session id"；per session 单列无法表达群聊多 Role Agent 各自持 id。
- **绑定变更传截断上下文 vs 清空重开**：选传截断上下文——用户明确要求"将上下文直接传递给该 Role Agent（仅最新 64K Token）创建新对话"；纯清空会丢失对话连续性，传截断上下文让新 CLI Agent 快速恢复语境。
- **方言默认 + 可覆盖 vs 纯硬编码**：选可覆盖——不同 CLI 工具参数会变（如 Claude Code `-r`/`--resume`），用户可能用非标准工具；方言默认提供开箱即用，`resume_args` 覆盖提供灵活性。
- **新表 vs sessions 加 JSON 列**：选新表——`session_cli_handles` 语义清晰、可查询、可索引；sessions 加 JSON map 不可查询且违反关系范式。

## 后置项

- 阶段一：对话对象维度改造（D1-D3，前端为主 + resolve_agent 调整）
- 阶段二：CLI session id 持久化 + 参数拼接（D4-D8，后端为主）
- 阶段三：绑定变更上下文交接（D5，events 截断 + handle 更新）
- 端到端测试与双端质量门全绿
