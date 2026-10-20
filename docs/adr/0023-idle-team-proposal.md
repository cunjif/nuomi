# ADR 0023: 空闲触发器——积压任务的自发组队提案（建议-only 基础版）

- **状态**: Accepted（用户已确认，2026-10-20）
- **日期**: 2026-10-20
- **决策者**: user + conductor
- **前置**: M-FORM1 自发组队（docs/specs/auto-team-m1.md）、打磨③ dry-run 预览（preview_team）、P2-F 组 2 审批式提案（proposals 表，先行交付）

## 背景

pr.md §2「支持自发Agent Team」/ §6「自动创建Agent群里」当前仅有**手动**入口：看板卡片 ⋯ 菜单「自发组队运行」→ preview → 确认 → form+run。用户在 P2-F 规划中确认纳入**空闲触发器基础版**：用户离开一段时间后，应用主动扫描看板积压任务，生成组队提案供用户回来后审批。

现状缺口：

1. **无 idle 检测基础设施**——Rust 侧（tauri 后台进程）无用户输入事件源，无法感知"用户空闲"；前端是唯一能监听 DOM 交互事件的位置。
2. **无人值守自动执行的信任前提不存在**——`form_team` 会新建 Role/Team 行并消耗 provider 配额，用户已明确否决"定时/空闲触发即自动跑"，选择**审批式**（提案进收件箱，批准才执行）。
3. **提案归宿已在 P2-F 组 2 交付**——`proposals` 表（kind='team_plan' / task_id / plan_json / status / workspace_id）+ `list_proposals` / `resolve_proposal` IPC + ApprovalsView 合并展示。空闲触发器是第三个提案生产者（前两个：定时自发组队、未来的手动保存计划）。

## 决策

### 设计决策

| # | 决策点 | 选择 |
|---|---|---|
| 1 | idle 检测位置 | **前端上报**（Shell 层交互事件重置计时器，达标时一次性上报后端） |
| 2 | 触发条件 | **空闲 ≥ 超时阈值** 且 **聚焦工作区 queued 任务 ≥ 积压阈值**（均 app_settings 可配） |
| 3 | 产出语义 | **建议-only**：生成 pending 提案进审批收件箱，绝不自动 form+run |
| 4 | 防骚扰 | **全局至多 1 条待处理 idle 提案**（已存在则跳过）+ **冷却期**（上次 idle 提案生成后 N 小时内不再触发） |
| 5 | 用户回归 | 前端重置计时器停止再触发；**已生成提案不撤回**（保留待审批——这正是审批式的价值） |
| 6 | 提案内容与批准语义 | `preview_team`（零写入）对积压任务聚合生成一份计划；plan_json 记录 task_ids；批准 = `form_team_from_plan` 建团 + 对该批 queued 任务**逐个** `run_team_on_task` |
| 7 | 配置面 | 4 键存 app_settings；**仅 enabled 开关 + 空闲超时进 Settings UI**（Scheduler 区），积压阈值/冷却小时用默认值（键名文档化，后续按需补 UI） |
| 8 | 失焦语义 | 应用失焦/最小化时 DOM 事件不触发 → 计时器持续 → **失焦自然计入空闲**（符合直觉，不做区分） |

### 1. idle 检测：前端上报，后端判定

前端 Shell 层挂一个全局交互监听（pointerdown / keydown，capture + passive），事件重置计时器；计时器到期（空闲达阈值）时调用一次 `idle_check` IPC。后端 `idle_check` 完成**全部**判定——读配置、数 queued、查待处理提案、查冷却——通过则调 `preview_team` 生成计划并落提案。

**理由**：跨平台系统级 idle 查询需额外依赖（Windows GetLastInputInfo / macOS / Linux 各不相同），前端 DOM 事件天然跨平台零依赖；判定收在后端使前端上报只是"信号"，逻辑可单测、配置即时生效。

### 2. 触发条件

- 空闲超时：`idle_proposal.timeout_minutes`，默认 **15**（Settings 可调）。
- 积压阈值：`idle_proposal.backlog_threshold`，默认 **3**——当前聚焦工作区（无聚焦则取全库）status='queued' 的任务数。
- 总开关：`idle_proposal.enabled`，默认 **true**（"自发"精神；Settings 可一键关）。
- 冷却：`idle_proposal.cooldown_hours`，默认 **6**——以最近一条 idle 来源提案的 created_at 起算。

任务文本构造：积压任务按 created_at 升序取前 5 个，标题列表拼入规划输入（描述不足时用标题），plan_json 记录该批 task_ids 供批准后派发。

### 3. 建议-only 语义

空闲判定通过后只做两件事：`preview_team`（既有纯读 dry-run，零 Role/Team 写入）→ 落一条 pending 提案（kind='team_plan'，source='idle'）。**不调用 form_team、不调用 run_team_on_task**。用户回来后在审批收件箱看到提案（成员/拓扑/rationale/关联积压任务数），批准才执行，拒绝即清理。

### 4. 防骚扰三闸

1. **至多一条**：生成前查 proposals 表——已存在 source='idle' 且 status='pending' 的提案则直接跳过（不覆盖、不追加）。
2. **冷却期**：最近一条 idle 提案（无论状态）created_at 距今 < cooldown_hours 则跳过。
3. **回归即停**：前端检测到任何交互即重置计时器——用户在场的时段绝不触发。

### 5. 提案批准语义（复用组 2 的 resolve_proposal）

`resolve_proposal(id, approved=true)` 对 kind='team_plan' 的统一语义：`form_team_from_plan`（按 plan_json 落库建团，跳过 LLM 重规划、保留校验与重名唯一化——preview 所见即执行所得）→ 按 plan_json.task_ids 逐个 `run_team_on_task`。定时自发（组 3）的提案 task_ids 是单元素集，idle 提案是积压任务集——同一 resolve 路径，零分叉。

### 6. 失焦计入空闲

不做"前台才算空闲"的区分：失焦/最小化时无 DOM 事件，计时器自然走完，判定照常。若用户在别处工作回来看到一条提案，deny 即可——冷却期保证不会刷屏。

## 向后兼容

- 全部新增（新 IPC 命令 idle_check、proposals 表依赖组 2 先行交付），零既有行为变更。
- 默认 enabled=true 但触发条件保守（15min + 3 积压 + 冷却 6h）——最坏情况是每天少数几条可一键拒绝的提案。
- 配置键走既有 app_settings blob 通道，缺省全部有默认值，旧库零迁移（proposals 表迁移 0039 由组 2 引入）。

## 权衡

- **优点**：用户离开时积压工作被发现并"预组装"；审批式零信任风险（无静默写库/消耗配额）；复用 preview_team + proposals + resolve 全部既有设施，净新增面小（一个 IPC 命令 + 一个判定函数 + 前端计时器）。
- **代价**：前端常驻一个交互监听 + 计时器（低开销，pointerdown/keydown passive）；idle 判定在后端但触发源在前端——应用关闭则不触发（可接受：应用不在运行，"自发"无从谈起）。
- **替代方案**：
  - (a) Rust 侧系统级 idle 查询（GetLastInputInfo 等）——否决。三平台三套 API + 新依赖；且应用需常驻后台运行才有意义，桌面壳模型下前端 DOM 事件已足够。
  - (b) 空闲触发即自动执行（不进收件箱）——否决。用户已明确选审批式；无人值守静默建 Role/Team 行并消耗配额不可接受。
  - (c) 独立"建议面板"（不进审批收件箱）——否决。用户已选审批收件箱统一体验；复用 proposals 表与 ApprovalsView 零额外 UI 面。
  - (d) 系统级通知（OS notification）推送提案——超出基础版范围，留待后续增强（提案落库后加通知通道是纯增量）。
  - (e) Settings 暴露全部 4 个配置键——否决（本版）。积压阈值/冷却属调优参数，默认值 + 键名文档化已够；避免 Settings 面板膨胀。

## 风险与缓解

| 风险 | 缓解 |
|---|---|
| 假空闲（用户在看视频/阅读，无交互但人在） | 提案是建议-only，回归后一键拒绝；冷却 6h 限频；超时阈值 Settings 可调大 |
| preview_team 调用消耗一次 LLM 规划 | 仅在四闸（enabled/空闲/积压/冷却+至多一条）全过后调用一次；失败记 warn 日志不重试（下次空闲自然重试） |
| 积压任务文本过短导致计划质量差 | 规划输入拼标题+描述（同 schedule_dispatcher 的 prompt 组装语义）；描述为空回退标题 |
| 批准后逐个 run 的爆发（积压 5 个 = 5 个 Run 同时跑） | run_team_on_task 本身受监督后台任务管理；基础版接受串行提交（命令内顺序 await），排队语义由 Run 状态机承载 |
| 前端计时器在 HMR/热重载后重复挂载 | 计时器挂 Shell 顶层 useEffect + cleanup，单例断言（开发态 HMR 双实例直接不触发第二次上报——至多一条闸在后端兜底） |
| tauri-specta bindings 需 regen | 契约先行：idle_check 命令提交内 `pnpm contracts:gen` + `pnpm contracts:check` |

## 影响范围

### 依赖（P2-F 组 2 先行交付，本 ADR 不重复定义）
- `migrations/0039_proposals.sql` — proposals 表
- `list_proposals` / `resolve_proposal` IPC + ApprovalsView 合并展示
- core `form_team_from_plan`

### 后端
- `crates/nuomi-core/src/services/` — idle 判定函数（读配置 + 数 queued + 两闸检查），纯逻辑可单测
- `src-tauri/src/commands.rs` — `impl_idle_check`（判定 → preview_team → 落提案）+ `tauri_cmds.rs` / `lib.rs` 注册
- `src-tauri/src/`（或 core）— app_settings 四键读取（带默认值）

### IPC 契约
- 新命令 `idle_check()` → `ProposalDto | null`（null = 未触发；前端不消费返回值也可）
- `bindings.gen.ts` 重新生成

### 前端
- `src/features/shell/Shell.tsx` — 交互监听 + 空闲计时器 + 到期上报（一次）；提案展示零新增（ApprovalsView 已合并）
- `src/features/settings/` — Scheduler 区追加「空闲组队建议」开关 + 空闲超时输入（两键）
- i18n zh-CN / en — 空闲提案相关词条（收件箱行摘要、Settings 标签）

### 不涉及
- 定时自发组队（组 3）与 ScheduleForm 全量暴露（组 1）——非本 ADR 范围，仅共享组 2 设施
- OS 级通知、系统 idle 查询、后台常驻——明确排除

## 实施计划

P2-F 组 4（依赖组 2 交付）：后端判定函数 + 单测 → idle_check 三处接线【契约】 → 前端计时器与上报 → Settings 两键 + i18n → 门禁。

## 验证门禁

- `cargo test -p nuomi-core`（判定函数单测：未达阈值/积压不足/已有待处理/冷却中/全通过五路径）
- `cargo clippy --all-targets -- -D warnings`（新代码零警告）
- `pnpm contracts:gen` 无未提交 diff + `pnpm contracts:check`
- `pnpm typecheck && pnpm lint && pnpm test -- --no-file-parallelism`（前端计时器触发/回归重置用例，vi.useFakeTimers）
- 人工验证：`pnpm tauri dev` 下积压 3 任务、等待超时（临时调小）→ 收件箱出现提案 → 拒绝后冷却期内不再触发；批准后团队建立 + 积压任务逐个开跑
