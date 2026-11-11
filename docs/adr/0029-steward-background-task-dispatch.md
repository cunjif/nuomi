# ADR 0029: 管家任务调度——Background Task 立项看板 + 双路径派发（点名直派 / 提案新团）

- **状态**: Accepted（用户已确认，2026-11-10）
- **日期**: 2026-11-10
- **决策者**: user + conductor
- **前置**: Steward 工具注册表（steward/tools.rs，设计 §2.1.3.2）、P2-F 组 2 proposals 管线（migrations/0039，`resolve_proposal`）、ADR-0023 审批式先例、M-TEAM1 team_runner 物化执行

## 背景

用户提议：**管家 Agent（Steward）作为整个应用的管理 Agent，可以调动任意一个或多个 RoleAgent 完成 Background Tasks；任务的立项、状态、来源、人员、环境信息等明细登记到 Kanban。**

现状缺口：

1. **管家工具面无任务能力**——内置四工具（`read_app_state` / `propose_config_change` / `trigger_evolution` / `list_schedules`）只能读状态、提案改配置、触发进化、列日程，不能立任务也不能派活。
2. **tasks 表无溯源元数据**——现有列（id/session_id/title/description/status/workspace_id/时间戳）无法回答"这张卡是谁立的项、派给了谁"。定时触发建卡与手动建卡在数据上不可区分。
3. **提案生产者路径应零分叉**——proposals 表已有三个生产者设想（定时自发组队、idle 积压扫描、未来的手动保存计划），管家是第四个。`resolve_proposal` 对 kind='team_plan' 的批准语义（form_team_from_plan → 逐 task_id `run_team_on_task`，含等待期漂移跳过）应被原样复用而非另起通道。

**硬事实（决策依据）**：`team_former.rs` 的 `validate_plan` 要求 `plan.members.len() >= 2`——现有组队管线**最小成员数为 2**，"单成员 Team"在现管线下无法通过校验。因此"调动一个 RoleAgent"不能靠单成员团落地，须由直派路径覆盖（决策 1）。

## 决策

### 设计决策

| # | 决策点 | 选择 |
|---|---|---|
| 1 | 派发路径 | **双路径**：用户/管家**点名既有 team** → 直派（决策 3a）；**只给任务不给人员** → 提案式新团（决策 3b）。不做单成员 Team（`validate_plan` 最小 2 是既有核心约束，拓扑对 1 人无意义，不为绕校验而改核心） |
| 2 | 立项语义 | **即时落 backlog 卡片**（写一行 tasks 数据，零配额副作用，非挂起）。与 ADR-0023 的信任边界一致：审批针对"消耗配额的执行"，不针对"记账" |
| 3a | 直派语义 | 新工具 `dispatch_task_now(task_id, team_id)`：**Suspend 型**——对话内 SuspendCheck 确认（复用 `steward_confirm_config_change` 的 confirm_handle 范式，`StewardReply` 新增 `TaskDispatchPending` 变体），用户确认后直接 `impl_run_team_on_task`。零 LLM 规划、零新建 Role/Team |
| 3b | 提案语义 | 新工具 `propose_team_dispatch(task_ids, preferences?)`：`preview_team`（零写入 dry-run）规划 → 落 kind='team_plan'、source='steward' 的 pending 提案 → 工具挂起（suspend_on_call，同 `propose_config_change` 范式）。多成员本就 ≥2，现有校验无障碍 |
| 4 | 批准/确认语义 | **提案路径复用 `resolve_proposal` 零改动**（form_team_from_plan → 逐 task_id `run_team_on_task`，既有跳过逻辑天然兼容等待期漂移）；**直派路径**走新确认 IPC `steward_confirm_task_dispatch`。两条路径共享"批准后回填 tasks.assignee_team_id" |
| 5 | 明细登记 | **tasks 表扩列**（迁移 0050，只增不改）：`source`（'manual'/'schedule'/'steward'）、`assignee_team_id`（派发批准后回填）、`created_by`（steward 会话 id / schedule id / NULL）。**状态**由 Run 状态机驱动（state_changed 铁律自动继承），**人员**由 assignee_team_id + 关联 roles 表达 |
| 6 | 环境信息 | **不新增列**：任务环境 = workspace_id 归属（既有）+ 授权目录 ⊆ 工作区工作目录的沙箱约束（ADR-0022/0026 沉淀）。管家立项时用会话所属 workspace，不接受跨工作区参数；执行环境由既有沙箱链路把关 |
| 7 | 自动放行 | **本版不提供**：管家是用户在场对话驱动，低风险免审（risk_level 分级 + app_settings 白名单）留待后续增强，与 ADR-0023 否决"无人值守自动执行"的立场一致 |
| 8 | 工具面 | **新增 4 个 thin wrapper 工具**：`create_background_task`（立项，Result 型）、`propose_team_dispatch`（提案新团，Suspend 型）、`dispatch_task_now`（点名直派，Suspend 型）、`list_tasks`（看板查询，Result 型，补 snapshot 未覆盖 tasks 的盲区） |
| 9 | 刷卡上限 | `steward.tools.max_tasks_per_loop`，默认 **5**（对齐 ADR-0023 idle 积压扫描"取前 5"语义）：app_settings 键、默认值兜底、**不进 Settings UI**（键名文档化，避免面板膨胀）；粒度 = **每次 agent-loop run**；超出时工具报错回喂模型（模型可见失败原因，自行收敛） |

### 1. 双路径派发，不做单成员团

"调动一个 RoleAgent"最自然的落地是**用户点名**：直派工具拿着既有 team_id（单 Role 的既成团或用户指定团）直接走 `run_team_on_task`——执行层本就只校验"team 已存在 + 任务是 backlog/queued"（schedule_dispatcher 的 group 路径同款）。无人员指定时才交给规划器选人（≥2 成员，现有校验无障碍）。执行通道唯一 = 可观测性唯一；单成员团被否决（改核心校验的代价 vs "1 人团队"的语义尴尬不成比例）。

### 2. 立项即时落卡

管家在对话中说"帮我把 X 立为后台任务"时，`create_background_task` 直接写 tasks 行（status='backlog'，source='steward'，created_by=管家会话 id）。backlog 卡片在看板上用户可见、可删、不跑任何模型——写行不是危险副作用。用户回来若不认可，删卡即可；这与把立项也塞进审批收件箱相比，避免了"记一笔账也要审批"的噪音。

### 3. 两条派发路径，同一信任模型

- **直派**：Suspend 型工具产出 `TaskDispatchPending { task_id, team_id, confirm_handle }`，对话内确认（与 `propose_config_change` 的 ConfigProposal 完全同款 UX）——用户在场、意图明确，对话内确认即闭环，不进收件箱。
- **提案**：Suspend 型工具产出 kind='team_plan' 提案进审批收件箱——规划器替用户选人，preview 所见即执行所得，须走完整审批。
- 两者都以"用户点头"为执行前提，无静默消耗配额路径。

### 4. 批准/确认后的执行

- 提案路径：`resolve_proposal` 本体零改动（现状仅接受 kind='team_plan'，本 ADR 不引入新 kind）；form_team_from_plan 建团 → 逐 task_id `run_team_on_task` → 回填 assignee_team_id。
- 直派路径：确认 IPC 校验 pending 状态后直接 `impl_run_team_on_task` → 回填 assignee_team_id。
- 回填实现为 repo 层一个更新函数，两条路径各调一次，不改 resolve 流程结构。

### 5. 看板明细字段

| 明细 | 落点 | 语义 |
|---|---|---|
| 立项 | title / description | 管家从对话指令提炼 |
| 状态 | tasks.status | backlog → queued → running → done/failed/cancelled，由 Run 状态机驱动 |
| 来源 | source | 'steward' / 'schedule' / 'manual'（旧行默认 'manual'） |
| 人员 | assignee_team_id → team.roles | 派发批准后回填；backlog 期为空 |
| 环境 | workspace_id + 授权目录 | 既有列，不新增 |

## 向后兼容

- 迁移 0050 为 `ALTER TABLE tasks ADD COLUMN` 三列带默认值（source DEFAULT 'manual'，两个可空列），旧库零行为变更，旧卡片语义不变。
- steward 工具不是 IPC 面，llm_loop 通过注册表自动带上新工具；TaskDto 扩 source/assignee_team_id 字段属契约变更 → 契约先行（同一提交 regen bindings + 更新 Board 消费端）。
- `resolve_proposal` / ApprovalsView / form_team_from_plan / run_team_on_task 全部零改动——管家是纯增量生产者。
- `StewardReply` 新增 `TaskDispatchPending` 变体（serde tag 区分），旧消息反序列化不受影响；前端管家聊天界面按变体渲染确认按钮（同 ConfigProposal 范式）。

## 权衡

- **优点**：复用既有设施（工具注册表 / proposals 管线 / preview_team / run_team_on_task / SuspendCheck 确认范式），净新增面小（4 个工具 + 1 个迁移 + TaskDto 扩字段 + 1 条确认 IPC + Board 徽标）；执行通道唯一；双路径覆盖"点名"与"托付"两种用户意图；审批/确认双闸无静默消耗配额路径。
- **代价**：直派路径多一条确认 IPC 与一个 reply 变体（均在既有范式内）；两条派发路径需共享 assignee 回填（repo 层单函数，收敛一处）。
- **替代方案**：
  - (a) 一律新团（含点名场景）——否决。用户点名既有 team 被迫重规划+新建团，多耗一次 LLM 规划且团行堆积；且单 Agent 场景被最小 2 成员校验卡死，无解。
  - (b) 新提案类型 kind='task_dispatch'（直派也进收件箱）——否决。对话内 SuspendCheck 确认已够（用户在场）；resolve 需加分支，收益为零。
  - (c) 放宽 `validate_plan` 至最小 1 成员——否决。动核心校验（planner prompt、既有断言、单测全要跟改）；pipeline/router/group_chat 拓扑对 1 人无意义，"1 人团队"语义尴尬。
  - (d) 独立 background_tasks 新实体表——否决。违反绿地纪律"宁可扩列，不出现第二套并行约定"；Kanban/Board 已消费 tasks 表。
  - (e) 立项也走提案（全审批）——否决。backlog 卡片零副作用，全审批徒增收件箱噪音，且与用户"立项登记到 Kanban"的即时诉求相悖。
  - (f) 低风险任务自动放行——后置。需要 risk_level 分级与白名单体系，超出本版范围。
  - (g) 刷卡上限进 Settings UI——否决（本版）。调优参数键名文档化 + 默认值兜底即可（ADR-0023 (e) 同款先例）。

## 风险与缓解

| 风险 | 缓解 |
|---|---|
| 管家单轮刷卡（连建 N 张任务卡） | `create_background_task` 按 `steward.tools.max_tasks_per_loop`（默认 5）限次，超出报错回喂模型；对话流内立卡对用户可见；backlog 卡可删 |
| `preview_team` 调用消耗一次 LLM 规划 | 仅在挂起提案前调用一次；失败记 warn 不重试（下轮对话自然重试） |
| 直派确认前任务/team 状态漂移（被删、被跑、team 被删） | 确认 IPC 复用 `impl_run_team_on_task` 的既有校验（task.not_found / task.invalid_status / team.not_found），失败如实回给用户 |
| 提案等待期任务被手动跑掉/删除 | resolve 既有跳过逻辑零改动兼容（"skipping is the honest outcome"） |
| 管家规划引用了不存在的 role/provider（幻觉） | form_team_from_plan 批准时重校验当前目录，drift 失败提案保持 pending 可重试可拒绝 |
| 管家把任务立到错误 workspace | 工具用 ctx.workspace_id（管家会话所属工作区），不接受跨工作区参数 |
| 契约变更漏 regen | 契约先行硬规则：TaskDto 扩字段提交内 `pnpm contracts:gen` + `pnpm contracts:check` |

## 影响范围

### 迁移
- `migrations/0050_task_source_and_assignee.sql` — tasks 加 source / assignee_team_id / created_by 三列（只增不改）

### 后端 core（crates/nuomi-core）
- `domain` Task 实体扩三字段
- `store/repos/tasks_runs.rs` — insert_task 写入新列；新增按 id 回填 assignee_team_id 的更新函数
- `steward/kernel.rs` — `StewardReply` 新增 `TaskDispatchPending { task_id, team_id, confirm_handle }` 变体
- `steward/tools.rs` — 4 个新工具 + 注册进 `with_builtin_tools`（追加在 list_schedules 之后）；两个 Suspend 型工具（`only_config_proposal_tool_suspends` 测试需同步修正）；`create_background_task` 读 app_settings 上限键（带默认值）

### src-tauri
- `commands.rs` — TaskDto 扩字段【契约】；`impl_create_task`（若 Board 已有建卡命令则复用并透传 source/created_by）
- `impl_resolve_proposal` — 批准路径追加"回填 assignee_team_id"一步（一行级增量，不改流程结构）
- 直派确认 IPC `steward_confirm_task_dispatch`（tauri_cmds + lib.rs 注册）【契约】
- steward settings 读取：`steward.tools.max_tasks_per_loop`（默认 5）

### 前端
- Board 卡片追加来源徽标（steward/schedule/manual）与执行人（team 名）显示 + i18n zh-CN/en
- 管家聊天界面渲染 `TaskDispatchPending` 确认按钮（同 ConfigProposal 范式）+ i18n
- ApprovalsView 零改动（proposals 合并展示既有，source='steward' 自然出现）

### 不涉及
- 自动放行 / risk_level 分级 / OS 通知 / scheduler UI / 新提案类型 / 单成员团——明确排除

## 实施计划

迁移 0050 → domain/repo 扩列 + 单测 → StewardReply 变体 + steward 四工具 + 单测（含 suspend 断言更新与上限路径）→ TaskDto 契约 regen【契约】→ resolve 回填 assignee + 直派确认 IPC【契约】→ Board 徽标 + 管家确认按钮 + i18n → 门禁。

## 验证门禁

- `cargo test -p nuomi-core`：工具参数校验、单轮刷卡上限（默认 5 / 超限报错）、backlog 落行字段断言、直派挂起/确认路径、提案挂起/批准/回填路径、resolve 既有用例不回归
- `cargo clippy --all-targets -- -D warnings` 零警告
- `pnpm contracts:gen` 无未提交 diff + `pnpm contracts:check`
- `pnpm typecheck && pnpm lint && pnpm test -- --no-file-parallelism`
- 人工验证：`pnpm tauri dev` 下与管家对话——
  1. "把 X 立为后台任务" → 看板出现 source=steward 的 backlog 卡
  2. "组队执行它" → 收件箱出现提案 → 批准后建团、卡片 assignee 回填、Run 起飞且状态随生命周期流转
  3. "用 Y 团队跑它" → 对话内出现直派确认 → 确认后 Run 起飞、assignee 回填
