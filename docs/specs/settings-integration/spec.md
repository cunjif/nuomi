# SPEC: Nuomi Settings 运行时整合（settings-integration）

> 状态：待批准（阶段一属重大架构变更，需先写 ADR 并获用户确认再动工）
> 前置：`docs/specs/cli-agents-m1.md`（M-CLI1 ✅）、`docs/specs/team-shell-m1.md`（M-TEAM1 ✅）、`docs/specs/auto-team-m1.md`（M-FORM1 ✅）、`docs/specs/harness-kernel-v1.md`（K6 ✅）。
> 需求来源：用户请求「分析设置下各设置项的潜在运行异常，模型提供方和 Roles 之间的整合，Roles、Teams 和对话功能之间的整合，给出互联互通统一整体应用的方案和实施计划」；需求权威仍为 `pr.md`。
> 事实基础：`principle.md:255` 已自认「facade boot 是 env 单一 Provider 遗留路径」；`team_runner::materialize`（`crates/nuomi-core/src/services/team_runner.rs:68`）是多 Provider 真实运行时，`facade::run_task_in_session`（`crates/nuomi-core/src/facade.rs:333`）与 `commands::run_conversation_turn`（`src-tauri/src/commands.rs:101`）走 facade boot 路径不读 DB 配置。

## 0. 问题诊断 / Problem Diagnosis

### 0.1 核心架构缺陷：Settings 配置与运行时执行脱节

| 执行路径 | 入口 | 是否读 DB Provider 配置 | 是否应用 Role 覆盖 | 是否走 Team 拓扑 |
|---|---|---|---|---|
| Board `runTeamOnTask` | `team_runner::materialize` | ✅ 是 | ✅ 是 | ✅ 是 |
| 普通对话 chat | `run_conversation_turn` → `facade::run_task_in_session` | ❌ 否（env 单 Provider） | ❌ 否 | ❌ 否 |
| Group Conversation | `run_conversation_turn`（绑 Team 但走单 provider） | ❌ 否 | ❌ 否 | ❌ 否（假群聊） |
| Scheduler | 待核实（候选路径之一） | ? | ? | ? |

**后果**：
- Settings 中大部分配置（Provider 参数、Role 绑定、Team 拓扑）对 chat/group 对话是"只显示不执行"的死配置。
- `resolve_agent` 解析出的 agent 绑定只用于 UI 显示，`run_conversation_turn` 完全忽略。
- Group Conversation 是"假群聊"——绑定了 Team 但走单 provider 对话路径。

### 0.2 数据完整性隐患（设置项潜在运行异常）

- **重命名 key 迁移**：Provider/Role/Team 重命名后，引用旧 key 的配置（如 Role 的 provider_id、Team 的 member role_id）是否级联更新待核实；存在悬空引用风险。
- **删除级联**：删除 Provider/Role 时，依赖它们的 Role/Team/Session 是否级联清理或拒绝删除待核实；存在孤儿行风险。
- **AgentProfile 绑定**：Role 的 `params.agent_profile_id` 引用已删除的 CLI Agent profile 时，运行时解析失败的处理路径（skip + warning 还是硬错误）需统一。

## 1. 决策记录摘要 / Decisions

| # | 决策 |
|---|---|
| D1 | **"配置即执行"原则**：所有执行路径（chat / group / board / scheduler）统一消费 Settings 配置——`run_task_in_session` 复用 `team_runner::materialize` 逻辑从 DB 动态物化 providers，并消费 `resolve_agent` 解析结果应用 Role 覆盖（systemPrompt / temperature / toolAllowlist）。env 单 Provider 路径降级为"DB 无配置时的兜底"，不再是对话主路径。 |
| D2 | **阶段一属重大架构变更**：改 `run_task_in_session` 执行路径影响所有对话，必须先写 ADR（`docs/adr/0011-settings-runtime-unification.md`）并获用户确认再动工；ADR 需明确回滚边界与灰度策略。 |
| D3 | **Group Conversation 走 Team 路径**：会话绑定了 Team 时，`run_conversation_turn` 分派到 `team_runner::run_team`（群聊拓扑）而非单 provider 对话；未绑定 Team 时走单 Role/Provider 路径（D1 物化的 DB provider）。 |
| D4 | **Role 覆盖注入点**：在 `team_runner::materialize` 物化 provider 后、调用 LLM 前应用 Role 覆盖——system_prompt_override 拼接、temperature 覆盖、toolAllowlist 过滤；与 M-TEAM1 既有 Role 覆盖机制对齐，不另起一套。 |
| D5 | **数据完整性**：重命名/删除走"引用预校验 + 级联更新或拒绝"二选一策略（具体策略在 ADR 中定）；`agent_profile_id` 解析失败统一为 skip + warning + EventRecord（不硬中断团队执行），与 `materialize` 既有 skip-with-warning 约定一致。 |
| D6 | **不破坏 Board 路径**：Board 的 `runTeamOnTask` 已正确走 `materialize`，本次整合只统一其他路径向其看齐，不改 Board 既有行为（回归测试守护）。 |
| D7 | **测试策略**：每阶段先写失败测试暴露脱节，再实现修复；新增"配置即执行"端到端测试——Settings 配 Provider/Role/Team → 普通对话消费该配置（FakeLlm 断言收到的 systemPrompt/temperature 与配置一致）；零外网。 |
| D8 | **观测**：物化阶段发布 `provider.materialized` / `role.applied` 调试事件（DEBUG 级，可关），便于用户在 Run 详情时间线确认"我配的 Role 真的生效了"。 |

## 2. 目标 / Goals

1. **配置即执行**：Settings 中配置的 Provider/Role/Team 对所有执行路径（chat / group / board / scheduler）一致生效，消除"只显示不执行"的死配置。
2. **真群聊**：Group Conversation 绑定 Team 时走真实群聊拓扑（Selector + Handoff + WhiteBoard），不再是单 provider 假群聊。
3. **数据完整性**：重命名/删除/绑定解析的引用关系一致，无悬空引用、无孤儿行；解析失败有统一可观测的处理路径。
4. **可观测闭环**：用户能在 Run 详情时间线确认配置已生效（调试事件 + 既有 EventRecord）。
5. **不回归**：Board 既有行为零变更；env 兜底路径在 DB 无配置时仍可用。

## 3. 用户故事 / User Stories

- **US1** 开发者在 Settings 配置了一个 Provider（OpenAI 兼容 + 代理 + 低温）并绑定到默认 Role；在普通对话页发一条消息，FakeLlm 断言收到的 temperature 与 systemPrompt 与 Settings 配置一致——配置真的生效了。
- **US2** 开发者创建一个 Team（pipeline：架构师→编码→审查）并在 Group Conversation 会话绑定该 Team；发起对话后，Run 详情时间线显示三站依次执行 + WhiteBoard 笔记流转——是真群聊而非单 provider。
- **US3** 开发者删除一个被某 Role 引用的 Provider，系统或拒绝删除并列出引用方、或级联清理并明确告知；不留孤儿行。
- **US4** 开发者绑定的 CLI Agent profile 已被删除，发起团队任务时该成员被 skip + warning + EventRecord 记录，其余成员继续执行，Run 详情可见跳过原因。
- **US5** 开发者未配置任何 DB Provider，仅设了 env 变量；普通对话仍可用（env 兜底路径），但 Settings 提示"当前为 env 兜底，配置 DB Provider 后生效"。

## 4. 验收标准 / Acceptance Criteria

**阶段一：Provider 运行时脱节修复**
- [ ] AC1 `run_task_in_session` 在 DB 有 Provider 配置时走 `materialize` 物化路径（不再走 env 单 Provider 主路径）；DB 无配置时回退 env 兜底并发布提示事件。端到端测试：Settings 配 Provider → 普通对话消费该 Provider（FakeLlm 断言）。
- [ ] AC2 env 兜底路径保留且仅在 DB 无配置时启用；兜底时发布 `provider.env_fallback` 事件可观测。

**阶段二：Role 参数注入 + Group Conversation 走 Team 路径**
- [ ] AC3 Role 覆盖（systemPrompt / temperature / toolAllowlist）在物化后、LLM 调用前应用；FakeLlm 断言收到的参数与 Role 配置一致。
- [ ] AC4 Group Conversation 绑定 Team 时分派到 `team_runner::run_team`（群聊拓扑）；未绑定 Team 时走单 Role/Provider 路径；端到端测试覆盖两分支。
- [ ] AC5 `resolve_agent` 解析结果被 `run_conversation_turn` 消费（不再仅 UI 显示）。

**阶段三：数据完整性**
- [ ] AC6 重命名/删除 Provider/Role 走引用预校验：有引用时或拒绝（列出引用方）或级联更新，策略与 ADR 一致；tempfile SQLite 断言无悬空引用。
- [ ] AC7 `agent_profile_id` 解析失败统一 skip + warning + EventRecord，不硬中断团队执行；Run 详情可见跳过原因。

**阶段四：Scheduler/Integrations 增强**
- [ ] AC8 Scheduler 执行路径统一消费 Settings 配置（与 D1 一致）；具体范围在 ADR 中定。

**横切**
- [ ] AC9 Board 既有 `runTeamOnTask` 行为零变更（回归测试守护）。
- [ ] AC10 调试事件 `provider.materialized` / `role.applied` / `provider.env_fallback` 进 EventRecord，Run 详情时间线可见。
- [ ] AC11 双端质量门全绿：`cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test` 与 `pnpm typecheck && pnpm lint && pnpm test`。

## 5. 非目标 / Non-goals

新增 Multi-Agent 编排机制（现有 pipeline/router/group_chat 三拓扑之外不加）；Telemetry / 飞书 / QQBot 增强；UI 主题打磨 / Monaco 体验 / 自发组队 dry-run 预览（属其他里程碑）；重写 `team_runner::materialize`（本次只复用与扩展消费端，不改其物化逻辑）。

## 6. 技术约束 / Technical Constraints

- 锁定栈不变：Rust(edition 2021) + tokio + SQLite(rusqlite+WAL) + Tauri 2 + React18/TS strict。遵守 `.opencode/rules/rust-core.md`（SQLite 操作 `spawn_blocking` 包裹、每模块 thiserror 枚举、库路径无 `unwrap()/expect()`）与 `.opencode/rules/ipc-contract.md`（camelCase DTO、稳定 code、四步契约链）。
- 迁移只增不改：本里程碑优先零新 migration（既有 provider_configs/roles/teams/agent_profiles 表就绪）；若数据完整性方案需要新 migration（如级联约束），按 append-only 编号新增，不编辑已发布文件。
- 不改 `team_runner::materialize` 物化逻辑：本次只在消费端（`run_task_in_session` / `run_conversation_turn`）复用与分派，物化函数本身不改（回归测试守护）。
- env 兜底保留：DB 无 Provider 配置时回退 env 单 Provider 路径，保证既有 env 用户零中断。
- 测试纪律：单测零外网——FakeLlm 注入为主；repo 测试用 tempfile SQLite；修 bug 先写失败测试。
- ADR 先行：阶段一动工前必须完成 `docs/adr/0011-settings-runtime-unification.md` 并获用户确认。

## 7. 实施计划 / Implementation Plan

| 阶段 | 优先级 | 内容 | 前置 | 验收 |
|---|---|---|---|---|
| 阶段一 | 最高 | ADR 0011 定稿 → 改 `run_task_in_session` 走 `materialize` 物化路径（DB 有配置时）+ env 兜底保留 + 调试事件 | 用户确认 ADR | AC1, AC2, AC9, AC11 |
| 阶段二 | 高 | Role 覆盖注入（D4）+ Group Conversation 走 Team 路径（D3）+ `resolve_agent` 消费化 | 阶段一 | AC3, AC4, AC5 |
| 阶段三 | 中 | 数据完整性：重命名/删除级联策略（D5）+ `agent_profile_id` 解析失败统一处理 | 阶段一 | AC6, AC7 |
| 阶段四 | 低 | Scheduler/Integrations 执行路径统一消费 Settings 配置 | 阶段一 | AC8 |
| 收尾 | — | 调试事件时间线可见 + 全量质量门复核 + AC 清单逐项核验报告 | 阶段一–四 | AC10, AC11 |

## 8. 风险与回滚 / Risks & Rollback

- **风险**：改 `run_task_in_session` 执行路径影响所有对话，回归面大。**缓解**：先写失败测试暴露脱节，再实现；Board 路径回归测试守护；env 兜底保留保证零配置用户不受影响。
- **风险**：Group Conversation 改走 Team 路径后，既有群聊会话的续传行为可能变化。**缓解**：续传时按会话绑定 Team 的有无分派，未绑定 Team 的既有会话走原路径。
- **回滚边界**：阶段一若出问题，回滚至 env 单 Provider 主路径（DB 配置仅 Board 生效），不影响 Board 既有功能。具体回滚步骤写入 ADR 0011。

## 9. 待核实项 / To Be Verified

- Scheduler 执行路径当前是否读 DB 配置（AC8 前置）。
- 重命名/删除 Provider/Role 当前的级联处理实现（AC6 基线）。
- `resolve_agent` 当前解析结果的所有消费点（AC5 范围）。
