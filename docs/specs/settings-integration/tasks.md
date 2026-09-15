# TASKS: Nuomi Settings 运行时整合（settings-integration）

> 对应规格：`docs/specs/settings-integration/spec.md`
> 对应设计：`docs/specs/settings-integration/design.md`
> 任务规划原则：垂直切割（按业务功能分组）· 可验收 · 原子性 · 有序性（被依赖者在前）。
> 关键约束（来自已评审确认的设计决策，实施时必须遵守）：
> 1. **两阶段删除策略**（替代单一 DeletePolicy 枚举）：阶段一拒绝删除并列出引用方 + 提供"仍要删除"入口；阶段二用户确认后 `force=true` 删除 Provider 并将上层引用置空（`SET provider_id = NULL`）；运行时遇到空引用发 `provider.missing` 提示用户补配，若用到上层 agent 则提示需设置 Provider。
> 2. **直接改 `run_turn` 签名**：增加 provider/model/overlay 三参，`Some` 走物化、`None` 回退 env 兜底；不新增 `run_turn_with_provider` 变体，既有调用点同步适配传 `None`。
> 3. **Scheduler AC8 细化**：分派点从 `run_task_in_session` 改为 `run_conversation_turn`；物化时机在 turn 内部不跨 tick 缓存；env 兜底失败时标记 Task failed 而非静默 missed dispatch。
> 4. **统一执行分派层**：`run_conversation_turn` 先探测 DB 配置 → 按 `session.team_id` 分派到 team_runner（真群聊）或物化的单 Role/Provider 路径 → env 兜底；不改 `materialize` 物化逻辑，只在消费端复用与分派；IPC 签名零变更。

---

## 1. ADR 0011 定稿与用户确认（前置门禁）

> 阶段一属重大架构变更（改 `run_task_in_session` 执行路径影响所有对话），必须先写 ADR 并获用户确认再动工（spec D2 / 技术约束 §6）。

### 1.1 撰写 ADR 0011 文档
- [ ] 在 `docs/adr/0011-settings-runtime-unification.md` 撰写架构决策记录，内容必须覆盖：(a) 决策摘要——"配置即执行"原则与统一执行分派层方案；(b) 回滚边界——阶段一出问题时回滚至 env 单 Provider 主路径（DB 配置仅 Board 生效），不影响 Board 既有功能的具体步骤；(c) 灰度策略——env 兜底保留保证零配置用户零中断；(d) 两阶段删除策略定夺——引用置空（`SET provider_id = NULL`）优于级联删除上层实体，保留 Role/Team 供用户重新绑定；(e) `run_turn` 签名改造决策——直接改签名而非新增并行变体，避免双路径分叉；(f) 零新 migration 论证——引用列已为 `Option` 可空，应用层 `UPDATE SET NULL` 实现两阶段删除。文档格式对齐既有 ADR（`docs/adr/0001` ~ `0010`）。
- **验收**：ADR 文档存在且六要素齐全；与既有 ADR 编号连续、格式一致。
- **依赖**：无。

### 1.2 暂停等待用户确认 ADR
- [ ] 将 ADR 0011 提交用户评审，明确询问"是否确认 ADR 0011 并允许阶段一动工"，在收到用户明确确认（如"确认"/"OK"/"同意"）前不得开始任务组 2 的任何编码工作。
- **验收**：用户确认记录在案（可在 ADR 末尾追加"状态：已确认 @ <日期>"）。
- **依赖**：1.1。

---

## 2. 统一执行分派层落地（阶段一：AC1, AC2, AC9）

> 目标：普通对话 chat 消费 DB Provider 配置（AC1）；env 兜底路径保留且可观测（AC2）；Board 既有行为零变更（AC9）。
> 核心转变：`run_conversation_turn` 从"直接调 facade env Provider"变为"先探测 DB 配置 → 分派到 team_runner 或物化的单 Role/Provider 路径 → env 兜底"。

### 2.1 改造 facade `run_turn` 签名增加物化注入参数
- [ ] 在 `crates/nuomi-core/src/facade.rs:363` 改造 `run_turn` 签名，增加三个参数：`provider: Option<Arc<dyn LlmProvider>>`（`None` → 回退 `self.provider` env 兜底）、`model: Option<String>`（`None` → 回退 `self.model`）、`overlay: Option<RoleOverlay>`（`None` → 不应用 Role 覆盖）。`run_turn` 内部统一走分派逻辑：传入 `Some(provider)` 时用物化 provider 构造 `LoopEngine`，传入 `None` 时回退 `self.provider`。`overlay` 为 `Some` 时拼接 systemPrompt、覆盖 `LoopConfig.temperature`、过滤 ToolRegistry（具体覆盖应用逻辑在任务 3.1 完成，本任务先打通参数传递通道并保留 `None` 等价行为）。**不新增 `run_turn_with_provider` 并行变体**。`RoleOverlay` 结构体定义在 `crates/nuomi-core/src/services/` 下新建模块（见任务 2.2）。
- **验收**：`run_turn` 新签名编译通过；`provider`/`model`/`overlay` 均传 `None` 时行为与改造前等价（既有 transcript 持久化、delta bridge、事件发布铁律保持）。
- **依赖**：1.2。

### 2.2 适配 `run_turn` 既有调用点传 `None` 走 env 兜底
- [ ] 在 `crates/nuomi-core/src/facade.rs` 同步适配 `run_task`（line 299）、`run_task_in_session`（line 333）等既有调用点：调用 `run_turn` 时传 `None`/`None`/`None`，行为与改造前等价。`dispatch_run`（Board `run_task`）若调用 `run_turn` 也同步适配。确保无遗漏调用点（用 `grep` 全量排查 `run_turn(` 调用）。
- **验收**：全量调用点适配完成；`cargo build` 通过；既有 REPL `run_task` 与 Board `dispatch_run` 行为不变（单测守护）。
- **依赖**：2.1。

### 2.3 新增 SingleRoleMaterializer 服务（模块 B）
- [ ] 在 `crates/nuomi-core/src/services/` 新建文件（如 `single_role_materializer.rs`），实现 `SingleRoleContext` enum（`Materialized { provider, model, overlay }` / `EnvFallback`）与 `RoleOverlay` struct（`system_prompt: Option<String>` / `temperature: Option<f64>` / `tool_allowlist: Vec<String>`）。实现 `pub async fn materialize_single_role(db_path, secrets, resolved: Option<ResolvedAgent>) -> CoreResult<SingleRoleContext>`：(a) 调既有 `team_runner::materialize` 取 `MaterializedProviders`（**不改物化逻辑**）；(b) DB 无 `provider_configs` → 返回 `EnvFallback`；(c) DB 有配置 → 按 `resolved` 解析的 Role id 加载 Role，按 `role.provider_id`/`agent_profile_id` 从 `materialized.providers` 选 provider，提取 `RoleOverlay`（`system_prompt_override`/`temperature`/`tool_allowlist`）；(d) Role 加载失败（NotFound）降级为无 overlay，不硬错；(e) 物化 warnings 收集待 DebugEventPublisher 落库。在 `services/mod.rs` 注册新模块。
- **验收**：`materialize_single_role` 在 DB 有配置时返回 `Materialized`；DB 无配置时返回 `EnvFallback`；`materialize` 物化逻辑未被修改（diff 守护）。
- **依赖**：2.1。

### 2.4 新增 DebugEventPublisher 服务（模块 D）
- [ ] 在 `crates/nuomi-core/src/services/` 新建文件（如 `debug_event_publisher.rs`），实现三个发布函数：`emit_materialized(bus, session_id, provider_id)`、`emit_role_applied(bus, session_id, role_id, overlay)`、`emit_env_fallback(bus, session_id)`。事件经既有 `EventBus.publish` 广播 + `repos::events::append` 落 `events` 表（aggregate = "session"，DEBUG 级可关，生产默认开）。payload 携带 provider_id/role_id/overlay 摘要（强类型，非裸 JSON）。在 `services/mod.rs` 注册新模块。
- **验收**：三个函数能发布事件并落 EventRecord；Run 详情时间线可见事件；DEBUG 级可通过配置关闭。
- **依赖**：无（可与 2.3 并行）。

### 2.5 改造 `run_conversation_turn` 分派决策（模块 A）
- [ ] 在 `src-tauri/src/commands.rs:101` 改造 `run_conversation_turn`：入口加载 Session 行 → 调既有 `resolve_agent` 解析 agent 绑定 → 按 `session.team_id` 有无分派：(a) `team_id.is_some()` → 调 `core_run_team`（群聊拓扑，阶段二任务 3.2 细化，本任务先打通分派骨架）；(b) `team_id.is_none()` → 调 `materialize_single_role`，match 返回值：`Materialized` → 调 `run_turn(session_id, text, Some(provider), Some(model), Some(overlay))` + 发布 `provider.materialized`/`role.applied` 事件；`EnvFallback` → 发布 `provider.env_fallback` 事件 + 调 `run_turn(session_id, text, None, None, None)`。**IPC 签名零变更**（入参出参不变，行为内部统一）。异常映射：Session 不存在 → `session.not_found`；物化全失败 → `provider.none_materialized`（新增错误码）。
- **验收**：DB 有 Provider 配置时普通对话走物化路径（AC1）；DB 无配置时回退 env 兜底并发布 `provider.env_fallback`（AC2）；IPC 契约签名不变（零前端改动、零 binding 重生成）。
- **依赖**：2.2, 2.3, 2.4。

### 2.6 Board 路径回归测试守护（AC9）
- [ ] 为 Board 既有 `impl_run_team_on_task`（`src-tauri/src/commands.rs:2474`）→ `core_run_team` → `materialize` 路径编写回归测试，断言改造前后行为等价（FakeLlm 注入，断言 Team 拓扑执行、Role 覆盖、WhiteBoard 笔记流转不变）。测试用 tempfile SQLite + FakeLlm，零外网。回归测试纳入 CI 质量门。
- **验收**：Board 路径回归测试全绿；`impl_run_team_on_task` 行为零变更（AC9）。
- **依赖**：2.5。

---

## 3. Role 覆盖注入与真群聊分派（阶段二：AC3, AC4, AC5）

> 目标：Role 覆盖在物化后 LLM 调用前应用（AC3）；Group Conversation 绑定 Team 走真实群聊（AC4）；`resolve_agent` 解析结果被执行路径消费（AC5）。

### 3.1 实现 Role 覆盖注入点对齐既有编排器逻辑（D4）
- [ ] 在 `crates/nuomi-core/src/facade.rs` 的 `run_turn` 内（任务 2.1 打通的 `overlay` 参数通道）实现 `RoleOverlay` 应用逻辑：(a) `system_prompt` 为 `Some` 时拼接 override 到 `SystemPromptService` 默认 prompt；(b) `temperature` 为 `Some` 时覆盖 `LoopConfig.temperature`；(c) `tool_allowlist` 非空时过滤 `ToolRegistry` 仅保留白名单工具。注入点在构造 `LoopEngine` 前。**与 `PipelineExecutor`（`crates/nuomi-core/src/orchestrator/pipeline.rs:42`）/ `GroupChatExecutor`（`group_chat.rs:227`）既有 Role 覆盖注入逻辑对齐，不另起一套**。
- **验收**：FakeLlm 断言收到的 systemPrompt/temperature/tools 与 Role 配置一致（AC3）；与编排器既有覆盖逻辑行为等价。
- **依赖**：2.5。

### 3.2 Group Conversation 绑定 Team 时分派到 `core_run_team`
- [ ] 在 `src-tauri/src/commands.rs` 的 `run_conversation_turn` 分派决策（任务 2.5 打通的 `team_id.is_some()` 分支）接入 `core_run_team`（群聊拓扑：Selector + Handoff + WhiteBoard）。续传时按 `session.team_id` 有无分派：未绑定 Team 的既有群聊会话走原单 Role/Provider 路径，保证续传行为兼容。`core_run_team` 返回的 `TeamRunOutcome` 映射为统一 `RunResultDto`。
- **验收**：Group Conversation 绑定 Team 时走真实群聊拓扑（AC4，US2）；未绑定 Team 时走单 Role/Provider 路径；既有群聊会话续传行为兼容（风险缓解）。
- **依赖**：2.5。

### 3.3 `resolve_agent` 解析结果被 `run_conversation_turn` 消费
- [ ] 确认 `run_conversation_turn`（任务 2.5）在分派前调用 `resolve_agent`（`crates/nuomi-core/src/services/conversation_service.rs:77`），将解析结果（`ResolvedAgent` 的 Role id 或 CLI profile id）传入 `materialize_single_role`，驱动 Role 覆盖与 provider 选择。解析失败（引用的 profile/role 不存在）静默降级到下一级（既有 `resolve_agent` 约定），不报错。补全 `resolve_agent` 当前所有消费点排查（spec §9 待核实项），确认无其他仅 UI 显示的脱节点。
- **验收**：`resolve_agent` 解析结果驱动执行路径（AC5）；解析失败降级不硬错；无其他"只显示不执行"脱节点。
- **依赖**：2.5。

---

## 4. 数据完整性与两阶段删除（阶段三：AC6, AC7）

> 目标：重命名/删除走引用预校验 + 两阶段交互（AC6）；`agent_profile_id` 解析失败统一 skip+warning+EventRecord（AC7）。
> 策略（D5 + ADR 0011）：两阶段删除——阶段一拒绝 + 引用方列表 + "仍要删除"入口；阶段二删除 + 引用置空（`SET provider_id = NULL`）；运行时空引用发 `provider.missing` 提示补配。不再提供"级联清理"选项。

### 4.1 新增 ReferencePreCheck 引用扫描服务（模块 C 阶段一）
- [ ] 在 `crates/nuomi-core/src/services/` 新建文件（如 `reference_pre_check.rs`），实现 `EntityRefs` struct（`roles: Vec<String>` / `teams: Vec<String>` / `sessions: Vec<String>`）。实现 `check_provider_refs(conn, provider_id) -> Result<EntityRefs, StoreError>`：扫描 `roles.provider_id` / `roles.provider_ids`（数组含该 id）引用方。实现 `check_role_refs(conn, role_id) -> Result<EntityRefs, StoreError>`：扫描 `teams.member_role_ids` + `sessions.agent` 引用方。所有 SQLite 访问在 `spawn_blocking` 内。在 `services/mod.rs` 注册新模块。
- **验收**：两个函数正确返回引用方列表；tempfile SQLite 测试覆盖有引用/无引用场景。
- **依赖**：1.2。

### 4.2 实现两阶段删除策略（模块 C 阶段二 + 事务）
- [ ] 在 `crates/nuomi-core/src/services/reference_pre_check.rs` 实现 `delete_and_nullify_provider_refs(conn, provider_id) -> Result<(), StoreError>`：在**单一 SQLite 事务**内执行 `DELETE FROM provider_configs WHERE id = ?` + `UPDATE roles SET provider_id = NULL WHERE provider_id = ?` + 从 `roles.provider_ids` 数组移除该 id。原子性防半状态。同理实现 `delete_and_nullify_role_refs(conn, role_id)`：删除 Role + `UPDATE teams SET member_role_ids = 移除该 id` + `UPDATE sessions SET agent = NULL WHERE agent 引用该 role`。在删除命令层（`src-tauri/src/commands.rs` 的 `deleteProvider`/`deleteRole`）接入两阶段：首次调用（`force=false` 或不传）有引用时返回 `entity.referenced`（携带 `EntityRefs`）；用户确认后二次调用 `force=true` 执行 `delete_and_nullify_*`。向后兼容——不传 `force` 时行为等价于首次调用。
- **验收**：阶段一有引用时拒绝删除并返回引用方列表（AC6）；阶段二删除 + 引用置空原子完成；tempfile SQLite 断言无悬空引用、无孤儿行。
- **依赖**：4.1。

### 4.3 扩展 `deleteProvider`/`deleteRole` IPC 契约
- [ ] 在 `src-tauri/src/commands.rs` 的 `deleteProvider`/`deleteRole` 命令增加 `force: bool` 入参（默认 `false` 向后兼容），有引用且 `force=false` 时返回 IPC 错误码 `entity.referenced`（payload 为 `EntityRefs` 序列化，camelCase DTO，遵守 `.opencode/rules/ipc-contract.md`）。`force=true` 时执行删除 + 引用置空。前端展示引用方列表 + "仍要删除"按钮（前端改动最小化——仅删除对话框交互，IPC 契约签名扩展但既有调用不传 `force` 行为等价）。重新生成 IPC bindings（`pnpm contracts:gen`）。
- **验收**：IPC 契约扩展完成；`entity.referenced` 错误码携带强类型 `EntityRefs`；向后兼容（不传 `force` 等价首次调用）；bindings 重生成。
- **依赖**：4.2。

### 4.4 运行时空引用检测与 `provider.missing` 补配提示
- [ ] 在 `crates/nuomi-core/src/services/reference_pre_check.rs` 实现 `detect_missing_provider(role: &Role) -> Option<MissingProviderHint>`：`role.provider_id.is_none()` 且无 `agent_profile_id` 兜底时返回 `MissingProviderHint { role_id, role_name }`。在执行路径（`SingleRoleMaterializer` / `team_runner`）加载 Role 后调用检测，有 hint 时发布 `provider.missing` 提示事件（WARNING 级，经 DebugEventPublisher 落 EventRecord）。不硬中断——若 env 兜底可用则降级执行并附加提示，若不可用则返回 `provider.missing` 错误。UI 提示"该 Role 未绑定 Provider，请先设置"。
- **验收**：空引用 Role 被检测并发布 `provider.missing` 提示（AC6）；env 兜底可用时降级执行 + 提示；不可用时返回错误 + UI 补配引导；不硬中断。
- **依赖**：2.3, 4.2。

### 4.5 `agent_profile_id` 解析失败统一 skip+warning+EventRecord（AC7）
- [ ] 确认 `materialize` 既有 `CliAgentClient::new` 失败时 skip+warning 约定（`crates/nuomi-core/src/services/team_runner.rs:119`）在 `SingleRoleMaterializer` 路径同样生效：物化后检查 `MaterializedProviders.warnings`，经 DebugEventPublisher 落 EventRecord（WARNING 级，Run 详情可见跳过原因）。facade 路径（原无物化）在任务 2.5 接入物化后自动受益。不硬中断团队执行——其余成员继续。
- **验收**：`agent_profile_id` 解析失败时该成员 skip + warning + EventRecord（AC7，US4）；Run 详情可见跳过原因；不硬中断。
- **依赖**：2.3, 2.4。

---

## 5. Scheduler 执行路径统一消费 Settings 配置（阶段四：AC8）

> 目标：Scheduler chat 路径统一消费 DB 配置（AC8），与普通 chat 走同一分派入口。
> 细化（design §2.1.3）：分派点从 `run_task_in_session` 改为 `run_conversation_turn`；物化时机在 turn 内部不跨 tick 缓存；env 兜底失败标记 Task failed 而非静默 missed dispatch。

### 5.1 Scheduler chat 分派目标改为 `run_conversation_turn`
- [ ] 在 `src-tauri/src/schedule_dispatcher.rs:100` 将 `target_kind=chat` 的分派目标从 `run_task_in_session`（facade env 单 Provider）改为 `run_conversation_turn`（统一入口），由 ConversationDispatcher 统一走 D1 分派逻辑（探测 DB 配置 → 物化 / env 兜底）。**不改 dispatcher 的订阅/触发/错误处理逻辑**，只改分派目标。`target_kind=group`（→ `impl_run_team_on_task`）与 `target_kind=task`（无自动分派）保持不变。更新文件头注释（line 3-4）。
- **验收**：Scheduler chat 目标消费 DB 配置（AC8）；与普通 chat 走同一分派入口；dispatcher 订阅/触发逻辑不变。
- **依赖**：2.5。

### 5.2 env 兜底失败时标记 Task failed（非静默 missed dispatch）
- [ ] 在 `src-tauri/src/schedule_dispatcher.rs` 的 chat 分派错误处理中：env 兜底若失败（env 也无配置）时发布 `provider.missing` 错误事件并**标记 Task 为 failed**（而非既有"只 log 不传播"的静默 missed dispatch），确保用户能在 Settings 页面发现配置缺失——无人值守场景不能依赖用户实时观察 Run 详情。若引用被置空的 Role（任务 4.4 两阶段删除后）被 Scheduler 任务引用，运行时检测到 `provider_id.is_none()` → 发布 `provider.missing` 提示，Task 标记 failed + 错误信息"Role X 未绑定 Provider，请先设置"。
- **验收**：env 兜底失败时 Task 标记 failed + `provider.missing` 事件；用户可在 Settings 发现配置缺失；非静默 missed dispatch。
- **依赖**：4.4, 5.1。

### 5.3 物化时机约束——turn 内部不跨 tick 缓存
- [ ] 确认 Scheduler tick 触发的 chat 任务，物化发生在 `run_conversation_turn` **内部**（ConversationDispatcher → SingleRoleMaterializer → `materialize`），与普通 chat 路径完全一致——**不在 Scheduler 层物化**，避免双物化点。物化结果（provider + overlay）生命周期仅限该次 turn，**不跨 tick 缓存**——Provider 配置变更后下次 tick 自动生效，无缓存陈旧风险。代码审查确认无静态/全局缓存引入。
- **验收**：物化仅在 turn 内部；无跨 tick 缓存；Provider 配置变更下次 tick 自动生效。
- **依赖**：5.1。

---

## 6. 端到端测试与质量门核验（横切：AC10, AC11 + 各阶段测试）

> 测试策略（D7）：每阶段先写失败测试暴露脱节，再实现修复；FakeLlm 注入为主；tempfile SQLite；零外网。Scheduler chat 路径与普通 chat 共享同一测试矩阵。

### 6.1 阶段一端到端测试——配置即执行（AC1, AC2, US1, US5）
- [ ] 先写失败测试暴露脱节：Settings 配 Provider（OpenAI 兼容 + 代理 + 低温）并绑定默认 Role → 普通对话发消息 → FakeLlm 断言收到的 temperature 与 systemPrompt 与 Settings 配置一致（US1）。DB 无配置时断言回退 env 兜底 + `provider.env_fallback` 事件（US5, AC2）。测试在 `crates/nuomi-core/` 或 `src-tauri/` 测试模块，tempfile SQLite + FakeLlm，零外网。
- **验收**：失败测试先红后绿；FakeLlm 断言 configuration 生效；env 兜底事件可观测。
- **依赖**：2.5。

### 6.2 阶段二端到端测试——Role 覆盖与真群聊两分支（AC3, AC4, US2）
- [ ] 端到端测试覆盖两分支：(a) 单 Role 路径——FakeLlm 断言 Role 覆盖（systemPrompt/temperature/toolAllowlist）与配置一致（AC3）；(b) Group Conversation 绑定 Team（pipeline：架构师→编码→审查）→ Run 详情时间线显示三站依次执行 + WhiteBoard 笔记流转（AC4, US2）；(c) 未绑定 Team 的群聊会话走单 Role 路径。tempfile SQLite + FakeLlm。
- **验收**：两分支测试全绿；真群聊拓扑可观测；Role 覆盖断言通过。
- **依赖**：3.1, 3.2。

### 6.3 阶段三 tempfile SQLite 测试——数据完整性（AC6, AC7, US3, US4）
- [ ] tempfile SQLite 测试覆盖：(a) 删除被引用的 Provider → 阶段一拒绝 + 返回引用方列表（US3）；(b) `force=true` 删除 + 引用置空 → 断言无悬空引用、无孤儿行（AC6）；(c) 运行时检测空引用 Role → `provider.missing` 提示（AC6）；(d) `agent_profile_id` 解析失败 → skip+warning+EventRecord，其余成员继续（AC7, US4）。零外网。
- **验收**：数据完整性测试全绿；无悬空引用/孤儿行；skip+warning 可观测。
- **依赖**：4.3, 4.4, 4.5。

### 6.4 阶段四 Scheduler chat 测试矩阵（AC8）
- [ ] Scheduler chat 路径与普通 chat 路径共享同一测试矩阵（D7）：FakeLlm 断言 Scheduler 触发的 chat 任务消费 DB 配置（与普通 chat 一致）；env 兜底失败时 Task 标记 failed + `provider.missing` 事件；物化不跨 tick 缓存（配置变更下次 tick 生效）。tempfile SQLite + FakeLlm。
- **验收**：Scheduler chat 与普通 chat 行为一致；env 兜底失败 Task failed；无缓存陈旧。
- **依赖**：5.2, 5.3。

### 6.5 调试事件时间线可见验证（AC10）
- [ ] 端到端验证 `provider.materialized` / `role.applied` / `provider.env_fallback` / `provider.missing` 事件经 EventBus → EventRecord 落库，Run 详情时间线可见。DEBUG 级可通过配置关闭，生产默认开。事件 payload 携带 provider_id/role_id/overlay 摘要（强类型）。
- **验收**：四类事件时间线可见（AC10）；DEBUG 级可关；payload 强类型。
- **依赖**：2.4, 4.4。

### 6.6 双端质量门全绿（AC11）
- [ ] 运行双端质量门并附输出摘要：Rust 端 `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test`；前端 `pnpm typecheck && pnpm lint && pnpm test`。零警告、零 `unwrap()`（测试外）、零 `as any`/`@ts-ignore`。bindings 已重生成（`pnpm contracts:gen`）且与 Rust 类型一致。
- **验收**：双端质量门全绿（AC11）；输出摘要附于完成报告。
- **依赖**：6.1, 6.2, 6.3, 6.4, 6.5。

---

## 7. 验收与回顾（收尾）

> 最终验证确保交付质量与设计一致性。

### 7.1 AC 清单逐项核验报告
- [ ] 对照 spec.md §4 验收标准 AC1–AC11 逐项核验，每项标注"通过/未通过 + 证据（测试名/命令输出摘要）"。未通过项必须有跟进措施与负责人。
- **验收**：AC1–AC11 全部标注状态与证据；未通过项有跟进计划。
- **依赖**：6.6。

### 7.2 设计与实现一致性核对
- [ ] 对照 design.md §2.1.3 分派状态机、§2.2 接口清单、§2.3 数据模型，核对实现与设计一致：`SingleRoleContext` enum 两分支强制 match、`RoleOverlay` 字段全 Option/Vec、`EntityRefs` 强类型、`force: bool` 驱动两阶段、`MissingProviderHint` 强类型。`materialize` 物化逻辑未被修改（diff 守护）。IPC 签名零变更（`run_conversation_turn`/`submitTask`）。
- **验收**：设计与实现一致；`materialize` 未改；IPC 签名零变更。
- **依赖**：7.1。

### 7.3 变更范围最终确认与原理沉淀
- [ ] 确认变更范围与 spec.md §5 非目标一致（未扩大范围）：未新增 Multi-Agent 编排机制、未增强 Telemetry/飞书/QQBot、未打磨 UI 主题/Monaco/自发组队 dry-run、未重写 `team_runner::materialize`。将实现原理（设计思想、关键权衡、数据流/调用链）追加到根目录 `principle.md`（gitignored，AGENTS.md §8 工作流硬规则 8）。
- **验收**：变更范围未越界；`principle.md` 已更新实现原理。
- **依赖**：7.2。
