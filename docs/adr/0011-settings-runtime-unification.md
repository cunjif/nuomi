# ADR 0011: Settings 运行时统一（配置即执行）

- 状态：Accepted（2026-09-14，用户确认）
- 关联：SPEC `docs/specs/settings-integration/spec.md`、DESIGN `docs/specs/settings-integration/design.md`、ADR 0002（多通道事件）
- 背景：`principle.md:255` 自认 facade boot 是 env 单一 Provider 遗留路径；`team_runner::materialize`（`crates/nuomi-core/src/services/team_runner.rs:68`）是多 Provider 真实运行时，但仅被 Board 路径消费。普通对话 chat / Group Conversation / Scheduler chat 走 `facade::run_task_in_session` → `run_turn`（`self.provider` 来自 env，不读 `provider_configs` 表），导致 Settings 中大部分配置"只显示不执行"。

## 决策

1. **"配置即执行"原则与统一执行分派层**。所有执行路径（chat / group / board / scheduler）统一消费 Settings 配置——`run_conversation_turn` 先探测 DB 配置 → 按 `session.team_id` 有无分派到 `team_runner`（真群聊）或物化的单 Role/Provider 路径 → DB 无配置时回退 env 兜底。env 单 Provider 路径降级为"DB 无配置时的兜底"，不再是对话主路径。Board 既有 `impl_run_team_on_task` 路径不改（已正确走 `materialize`），本次只统一其他路径向其看齐。

2. **`run_turn` 签名直接改造，不新增并行变体**。在 `crates/nuomi-core/src/facade.rs` 的 `run_turn` 增加 `provider: Option<Arc<dyn LlmProvider>>` / `model: Option<String>` / `overlay: Option<RoleOverlay>` 三参：`Some` 走物化、`None` 回退 `self.provider` env 兜底。既有调用点（`run_task` / `run_task_in_session` / `dispatch_run`）同步适配传 `None`，行为等价。不新增 `run_turn_with_provider` 并行变体——避免双路径分叉与长期维护负担。

3. **两阶段删除策略：引用置空优于级联删除**。删除被引用的 Provider/Role 时：阶段一拒绝删除并返回引用方列表 + 提供"仍要删除"入口；阶段二（用户确认 `force=true`）在单一 SQLite 事务内执行 `DELETE` + `UPDATE ... SET provider_id = NULL`（引用置空），原子性防半状态。不再提供"级联清理"选项——引用置空保留上层实体（Role/Team），用户可重新绑定 provider，比级联删除上层实体更安全且可逆。运行时遇到空引用（`role.provider_id.is_none()` 且无 `agent_profile_id` 兜底）发 `provider.missing` 提示事件（WARNING 级），env 兜底可用则降级执行 + 附加提示，不可用则返回错误 + UI 补配引导。

4. **零新 migration**。引用列（`roles.provider_id` / `roles.provider_ids` / `teams.member_role_ids` / `sessions.agent`）已为 `Option` 可空，两阶段删除的"引用置空"是应用层 `UPDATE ... SET NULL`，兼容现有 schema，不需新 FK 约束或新迁移文件。迁移只增不改铁律保持。

5. **Scheduler AC8 细化**。`schedule_dispatcher` 的 `target_kind=chat` 分派目标从 `run_task_in_session` 改为 `run_conversation_turn`（统一入口），由 ConversationDispatcher 统一走分派逻辑。物化发生在 turn 内部，不跨 tick 缓存——Provider 配置变更后下次 tick 自动生效。env 兜底失败时标记 Task 为 failed（而非静默 missed dispatch），确保无人值守场景用户能在 Settings 发现配置缺失。不改 dispatcher 的订阅/触发逻辑。

6. **调试事件可观测**。物化阶段发布 `provider.materialized` / `role.applied` / `provider.env_fallback` / `provider.missing` 事件，经 EventBus → EventRecord 落库，Run 详情时间线可见。DEBUG 级可通过配置关闭，生产默认开。

## 回滚边界

阶段一若出问题，回滚至 env 单 Provider 主路径（DB 配置仅 Board 生效），不影响 Board 既有功能。具体步骤：

1. 将 `run_conversation_turn` 的分派决策回退为直接调 `facade::run_task_in_session`（env Provider），移除 SingleRoleMaterializer 调用。
2. `run_turn` 的 `provider`/`model`/`overlay` 三参保留但既有调用点传 `None`（等价 env 兜底），或回退签名（git revert）。
3. Board 路径（`impl_run_team_on_task` → `core_run_team` → `materialize`）未改，零影响。
4. Scheduler `target_kind=chat` 分派目标回退为 `run_task_in_session`。
5. 两阶段删除 / 调试事件为新增服务，回滚即移除新文件，不影响既有删除命令行为。

回滚不涉及 migration（零新 migration），无数据回滚风险。

## 灰度策略

- **env 兜底保留**：DB 无 `provider_configs` 时回退 env 单 Provider 路径，保证既有 env 用户零中断。
- **续传兼容**：Group Conversation 改走 Team 路径后，既有群聊会话续传时按 `session.team_id` 有无分派——未绑定 Team 的既有会话走原单 Role/Provider 路径，行为不变。
- **IPC 签名零变更**：`run_conversation_turn` 入参出参不变（行为内部统一），前端零改动、零 binding 重生成。`deleteProvider`/`deleteRole` 增加 `force: bool` 入参默认 `false` 向后兼容（不传等价首次调用）。

## 权衡

- **直接改 `run_turn` 签名 vs 新增并行变体**：选直接改——双路径分叉的长期维护负担高于一次性适配既有调用点（3 处，均传 `None` 等价）。
- **引用置空 vs 级联删除上层实体**：选引用置空——保留 Role/Team 供用户重新绑定，可逆且安全；级联删除会丢失用户精心配置的 Role/Team 拓扑。
- **物化不跨 tick 缓存 vs Scheduler 层缓存**：选不缓存——Provider 配置变更下次 tick 自动生效，无缓存陈旧风险；缓存省的物化成本（毫秒级 SQLite 读）不抵陈旧风险。
- **改 `run_task_in_session` 执行路径影响面大**：先写失败测试暴露脱节再实现；Board 路径回归测试守护；env 兜底保留保证零配置用户不受影响。

## 后置项（roadmap）

- 阶段二 Role 覆盖注入与真群聊分派（AC3/AC4/AC5）
- 阶段三数据完整性两阶段删除落地（AC6/AC7）
- 阶段四 Scheduler 执行路径统一消费（AC8）
- 端到端测试与双端质量门全绿（AC10/AC11）
