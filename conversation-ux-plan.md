# 对话体验升级实现方案（Conversation Experience Plan）

> 状态：**待评审（Draft for review）**　范围：内核优先阶段的对话表面（Composer + 会话类型）
> 权威性：本文件是**实现方案细化文档**，不改变 `pr.md`（需求权威）与 `docs/specs/harness-kernel-v1.md`（内核 SPEC）。若冲突，以 `pr.md` 为准并修订本文件。
> 落地约束：技术栈锁定（AGENTS.md §2）；契约先行（改 Rust 类型 ⇒ 同提交重生成 bindings）；迁移只增不改；无 `any`/`unwrap`/`@ts-ignore`；所有用户可见文案走 i18n（zh-CN 默认 + en）；主题只用 design token。
> 建议后续动作：本方案评审通过后，拆分为 `docs/specs/conversation-ux-m1.md` 正式 SPEC，并对 §2.4 的 4 项决策补写 ADR。

---

## 0. 摘要 / TL;DR

本方案一次性覆盖两组需求：

| # | 需求（用户原话） | 交付章节 | 一句话方案 |
|---|---|---|---|
| R1 | `/` 命令支持切换 Agent，且当前会话 Agent 需常显 | §6 §7 | 命令注册表升级为「分类 + 动态源 + 类型化参数」，新增 `/agent`；会话持久化 Agent 绑定，Composer 常显 **AgentChip** |
| R2 | `@` 附件添加系统 | §8 | 统一 Composer 触发器：`@` 打开「文件 / Agent / 会话」命名空间选择器；附件落盘 + 入库 + 随消息注入上下文 |
| R3 | 输入框直接粘贴剪贴板 | §9 | Composer `onPaste` 分流：文本默认插入，图片/文件转附件并显示缩略图；大文本自动折叠为附件 |
| R4 | 新增会话类型：后台任务 / 多 Agent 群聊 / 定时任务 | §2 §3 §10 | 引入 `ConversationKind` 判别 + 绑定字段；按类型设计 4 套专属 UI；补齐实时事件与并发执行地基 |
| R5 | 针对不同会话类型设计全新 UI/UX | §10 | 每类会话独立的信息架构、状态条、时间线/气泡/托盘/倒计时等专属组件 |

**交付物**：3 个迁移文件、约 20 个新 IPC 命令/DTO 变更、1 套会话类型模型、1 套统一 Composer（含 `/`、`@`、粘贴、AgentChip）、4 套会话类型 UI、1 个后台任务全局托盘、1 个实时事件地基修复。

**关键前置发现（必须优先解决，否则新 UI 会「看起来能跑但永远不刷新」）**：
1. **领域事件未上总线段**：`append_domain_event` 只写 `events` 表、**从不 `bus.publish`**（`src-tauri/src/commands.rs:2864`），因此 `event://domain` 实际只收到 `team.formed`；Board/Scheduler/Approvals 里已有的 `useDomainEvents([DOMAIN_CHANNEL])` 对 `task.*`/`run.*`/`schedule.*` **是死代码**。→ §5 修复。
2. **单内核单会话**：`NuomiKernel::run_task` 持 `state.lock().await` 且始终跑在 kernel 的**单一 current session** 上（`crates/nuomi-core/src/facade.rs:290`、`src-tauri/src/commands.rs:93` 的 `impl_submit_task` 直接忽略 `session_id`）。→ 多会话并发（后台任务 + 前台对话）需要 §2.4 决策 D1。
3. **调度只入队不执行**：`scheduler_service.rs::tick` 只插一条 `session_id = NULL` 的 Task，不建会话、不派发 Run。→ §3.3 + §10.5。
4. **群聊发言未上总线段**：`whiteboard.rs::record_turn` 落库 `message` 事件但**不 `publish`**（`crates/nuomi-core/src/orchestrator/whiteboard.rs:138-165`），只有 `whiteboard.post` 发 `session.whiteboard`（不含 role 信息）。→ §5.3。

---

## 1. 现状调研（含 file:line 证据）

### 1.1 前端对话栈

| 关注点 | 现状 | 证据 |
|---|---|---|
| 会话视图 | `ChatView` = `useSessionStream` + `MessageList` + `ChatInput` | `src/features/chat/ChatView.tsx:17-84` |
| 历史 + 实时合并 | `listEvents` 历史 + `event://session/{id}` 增量，`session.delta` 用非持久序号去重，seq 缺口回填 | `src/features/chat/useSessionStream.ts:54-177` |
| 消息类型 | `ChatEntry = message \| tool_call \| tool_result`；`roleName` 已支持（群聊铺垫过一半） | `useSessionStream.ts:14-23` |
| 气泡渲染 | 用户右/助手左，**纯文本 `whitespace-pre-wrap`，不支持 Markdown** | `src/features/chat/Bubble.tsx:21-36` |
| 列表虚拟化 | >100 条启用 `@tanstack/react-virtual` | `MessageList.tsx:13-33` |
| Composer | textarea + `/` 补全面板；Enter 发送 / Shift+Enter 换行；**无 `@`、无 `onPaste`、无 Agent 显示** | `src/features/chat/ChatInput.tsx:26-213` |
| 提交 | `ipc.submitTask(sessionId, input)`；Rust 端忽略 sessionId | `ChatInput.tsx:111`、`commands.rs:93-105` |
| 视图路由 | `View = chat\|board\|trace\|git\|approvals\|scheduler\|settings\|plugins` | `src/lib/store/uiStore.ts:7`、`src/features/shell/Shell.tsx:217-236` |
| 布局 | **当前是两栏**（`AreaNav` 顶栏 + `LeftRail w-56` + 互斥主区），非 pr.md 的三栏 | `src/features/shell/Shell.tsx:197-215`、`LeftRail.tsx:25` |

### 1.2 命令系统（两套，互不相干）

| 系统 | 文件 | 现状 |
|---|---|---|
| 聊天 `/` 命令 | `src/lib/commands/registry.ts`（`SlashCommand`、`registerCommand`、`parseInput`）、`builtin.ts` | 6 条内置：`/workspace /new /sessions /clear /help /theme`；开放注册式（为插件命令预留），但**无分类、无参数类型、无动态源** |
| 全局面板命令 | `src/lib/commands/palette.ts`、`src/features/shell/QuickOpen.tsx:46-83` | Ctrl+P/Ctrl+Shift+P/Ctrl+F；文件模式复用工作区索引 `scorePath` |

`CommandContext` 目前只有 `{ sessionId, ipc, queryClient, navigate, selectSession, toggleTheme, toast, t }`（`registry.ts:18-29`）——**缺少会话元数据/Agent 上下文**，是「显示并切换 Agent」的直接阻塞点。

### 1.3 Agent / Team / Schedule 数据源（可直接复用）

| 实体 | 查询 | 字段 |
|---|---|---|
| CLI Agent | `ipc.listAgentProfiles()` → `AgentProfileDto` | `id,name,adapter,flavor(claude_code\|codex\|plain),command,args,env,workingDir,enabled` |
| Role（provider 行为覆盖层） | `ipc.listRoles()` → `RoleDto` | `id,name,providerId,providerIds,systemPromptOverride,toolAllowlist,requiredCapabilities,temperature,maxTokens,params,builtin,generated,ephemeral,source` |
| Team | `ipc.listTeams()` → `TeamDto` | `id,name,topology(pipeline\|router\|group_chat),memberRoleIds,config` |
| Schedule | `ipc.listSchedules()` → `ScheduleDto` | `id,name,cronExpr,taskTitle,enabled,nextTriggerAt`（**无 target/agent/team/description**） |
| 既有 UI | `CliAgentsSection` / `RolesSection` / `TeamsSection` / `TeamMemberPicker` / `SchedulerView` | 见 §1.1 之外的 `src/features/settings/*`、`src/features/scheduler/*` |

**Agent 绑定今天只存在于 Role 表单**：`RoleForm.tsx:22-38,86-89` 用 `params.agent_profile_id` 约定把 CLI Agent 绑到 Role；`RolesSection.tsx` 用 `BindingBadge` 展示。**会话/任务/调度层没有任何 Agent 绑定**。

### 1.4 后端会话与执行路径

| 关注点 | 现状 | 证据 |
|---|---|---|
| Session 实体/表 | 仅 `id,title,created_at,updated_at`（`cache_scope` 表中有、结构体没有）→ **无 kind/type/mode** | `crates/nuomi-core/src/domain/entities.rs:9-15`、`migrations/0001_init.sql:5-10`、`migrations/0007_session_cache_scope.sql:6` |
| Repo | `insert/get/touch/update_title/set_cache_scope/cache_scope/list` | `crates/nuomi-core/src/store/repos/sessions.rs:8-104` |
| tasks/runs | 状态机 `RunState`（8 态）与 `RunEvent`（8 事件）齐备；`transition()` 全函数 | `crates/nuomi-core/src/domain/run_state.rs:24-90` |
| 铁律实现 | 先 `events::append(state_changed)` 再 `update_run_status` | `commands.rs:388-406` |
| 单 Agent 后台派发 | `impl_update_task_status(→running)` → `dispatch_run` → `tokio::spawn(kernel.run_task)`，可取消令牌 `RunCancelRegistry` | `commands.rs:239-335`、`state.rs:20-57` |
| 群聊后台派发 | `run_team_on_task` → `transition_run(queued→running)` → `spawn_team_run`，`tokio::select!` 竞速取消令牌 | `commands.rs:2275-2452` |
| 群聊执行器 | Selector（RR/Heuristic/LLM）+ Handoff + WhiteBoard，`max_rounds/max_consecutive/max_hops` 守卫，收敛 = selector 返回 `members.len()` | `crates/nuomi-core/src/orchestrator/group_chat.rs:83-193`、`selector.rs:72-209` |
| 调度器 | 5 段 cron 子集 或 `@every N`；`tick` 仅插 Task、发 `schedule_triggered` 事件、推进 `next_trigger_at` | `crates/nuomi-core/src/services/scheduler_service.rs:280-324` |
| 心跳/孤儿恢复 | 列、repo、状态机边**全都有，但生产无调用**（`heartbeat()`/`OrphanTimeout` 仅测试） | `tasks_runs.rs:152`、`run_state.rs:79-81`、`status.rs:83-112` |

### 1.5 事件与实时性

- 通道路由纯函数：`session.`/`tool.`/`hook.` → `event://session/{id}`；`task.`/`run.`/`approval.`/`schedule.`/`team.` → `event://domain`（`src-tauri/src/events.rs:28-75`）。
- 前端唯一订阅入口 `useDomainEvents`（rAF 批量 + `DeltaSequencer` 去重，`src/lib/events/useDomainEvents.ts:37-114`）。
- **缺口**：`append_domain_event` 不 publish（见 §0）；`record_turn` 不 publish；`transition_run*` 不 publish。→ 新会话类型的「实时」必须建在 §5 之上。
- 已有 `change_log`（CDC 触发器覆盖 sessions/teams/roles/agent_profiles/tasks/runs）与 `tail_after` 分页读取，**无生产调用**，注释直言「intended to back future push-based UI sync」（`migrations/0006_change_log.sql`、`crates/nuomi-core/src/store/repos/change_log.rs:1-73`）。

### 1.6 设计系统与布局

- Token 唯一来源：`tailwind.config.cjs` + `src/styles/global.css`；语义色 `surface / surface-raised / surface-overlay / scrim / ink / ink-muted / ink-accent / state-ok / state-warn / state-danger / cap-* / diff-add`。
- 手绘质感类：`pixel-fill-accent`、`sketch-panel/card/input/btn`、`shadow-sketch-*`、`text-title-hand/note-hand/scribble`、`binder-holes`、`torn-note`、`animate-draw-in`。
- 原子：`src/components/ui/` 14 个（`Button` `Badge` `Card` `Dialog` `EmptyState` `Field/TextareaField/SelectField` `Spinner` `Tabs` `Tooltip` `AsyncBoundary` `Toaster` `ErrorBoundary` `Icon`）；图标仅 `Icon/icons.tsx::iconRegistry`（≈45 个手绘 glyph，**新增图标只能加这里**）。
- 对比度是测试门禁：`src/styles/themeContrast.test.ts` 解析 4 主题 × 28 组语义配对，改色板必过。4 主题：`paper-light / grid-notebook / chalkboard-dark / high-contrast`。
- Markdown 依赖已存在：`marked@^18`（`package.json:29`，WYSIWYG 编辑器在用），可在气泡中复用但需 XSS 清洗（参考 `src/lib/editor-ext/builtin/sanitize.ts`）。

### 1.7 现状差距矩阵（Gap matrix）

| 需求 | 今天有什么 | 缺什么（本方案要补） |
|---|---|---|
| `/` 切换 Agent | 开放命令注册表 | Agent 作为一等命令源；类型化参数；会话绑定 IPC |
| 显示当前 Agent | `roleName` 已在气泡渲染 | 会话级 Agent 绑定 + Chip + 列表徽章 |
| `@` 附件 | 无 | 触发器、选择器、附件模型/存储/IPC、上下文注入 |
| 剪贴板粘贴 | textarea 默认文本粘贴 | 图片/文件/大文本分流 + 缩略图 + 落盘 |
| 后台任务会话 | Board 能派发、能取消 | 会话类型、Run 状态条、全局托盘、心跳/孤儿恢复 |
| 多 Agent 群聊会话 | 执行器/WhiteBoard/Trace 全有 | 专属气泡流/发言者/Handoff 带/黑板坞/Round 控制 |
| 定时任务会话 | cron + Task 入队 | 会话化目标、自动派发、下次触发倒计时、run 历史 |
| 类型化 UI/UX | 单一 ChatView | `ConversationKind` 判别 + 4 套视图 + 统一 Header/Composer |
| 实时刷新 | 只有 session 增量真通 | §5 领域事件地基修复 |

---

## 2. 总体设计

### 2.1 核心概念：Conversation = Session + kind + bindings

保持现有 `Session` 表与 `Run`/`Task`/`Team`/`WhiteBoard` 全部不变，**在 Session 之上增加判别与绑定**，从而把「会话」升级为「对话（Conversation）」：

```
Conversation (会话)
├─ kind: chat | group | background | scheduled   ← 判别式（UI 与执行路径的分派键）
├─ agent?: { kind: cli|role, id }                ← chat 的默认执行体
├─ teamId?                                        ← group 的协作拓扑
├─ taskId?                                        ← background 的任务锚
└─ scheduleId?                                    ← scheduled 的调度锚
```

- **不引入新的顶层实体**，避免与 `sessions`/`tasks`/`runs`/`teams` 四张表并行出第二套约定（绿地纪律：结构先定死）。
- `kind` 是**唯一权威判别**，Rust 枚举 `ConversationKind`（`domain/`）+ DB `CHECK` 约束 + TS union（由 bindings 派生）。
- 绑定字段可为空：`kind=chat` 时 `agent` 空 = 使用「默认 Agent」解析链（§7.1）。

### 2.2 四种会话类型定义与执行映射

| kind | 语义 | 执行路径 | 实时事件 | 主 UI |
|---|---|---|---|---|
| `chat` | 单 Agent 对话（默认） | 会话级 Run（§2.4 D1） | `session.delta/message/tool.*` | §10 统一气泡 + AgentChip |
| `group` | 多 Agent 群聊 | `run_team_session`（后台 spawn，拓扑必须 `group_chat`） | `session.message`(带 role) + `session.whiteboard` | §10.3 发言者气泡流 + Handoff 带 + 黑板坞 |
| `background` | 后台任务 | Task → `dispatch_run` / `run_team_on_task`（detached） | `run.state_changed` + `session.*` | §10.4 Run 状态条 + 工具时间线 + 全局托盘 |
| `scheduled` | 定时任务 | Schedule 触发 → 建会话 → 派发（单个/团队） | `schedule.triggered` + 上述 | §10.5 规则卡 + 倒计时 + 历史条 |

> 设计要点：`background` 与 `scheduled` **不是新的执行引擎**，而是「会话如何被触发/被观察」的两种包装；执行仍复用既有 `dispatch_run` / `spawn_team_run`，避免重复实现并自动继承铁律与取消语义。

### 2.3 分层与模块边界（遵守 AGENTS.md §3）

```
Rust
  crates/nuomi-core/src/domain/         ConversationKind 枚举 + Session 扩展字段 + Schedule 扩展
  crates/nuomi-core/src/store/repos/    sessions/schedules/attachments 仓储
  crates/nuomi-core/src/services/       scheduler_service（会话化 + bus）；conversation_service（新建/绑定）
  crates/nuomi-core/src/orchestrator/   whiteboard.record_turn 补 bus publish；群聊发言事件带 role
  src-tauri/src/commands.rs             会话/附件/后台/调度 IPC 实现（薄，转调 core）
  src-tauri/src/tauri_cmds.rs            #[tauri::command] 包装（4 点锁步）
  src-tauri/src/events.rs               CDC tail → domain 事件桥（§5）

Frontend
  src/lib/ipc/bindings.gen.ts           ⚠️ 生成，禁止手改
  src/lib/ipc/client.ts / test-double   契约消费与测试替身
  src/lib/conversation/                 kinds.ts / agentResolve.ts / attachmentModel.ts
  src/lib/commands/                     registry 升级 + agent/attachment/conversation 命令
  src/features/conversation/            统一会话壳 + composer + 4 套类型视图
  src/features/shell/                   View 扩展 + 会话列表分组 + 后台托盘入口
```

### 2.4 关键设计决策（需 ADR）

**D1 — 并发执行模型：会话级运行时 vs 串行内核**
现状 `NuomiKernel::run_task` 是「单会话 + 全局锁」（`facade.rs:290-291`），无法同时跑「前台 chat」与「后台单 Agent 任务」。

| 选项 | 说明 | 优点 | 缺点 |
|---|---|---|---|
| A（推荐） | 新增 `ConversationRuntime` 注册表（`session_id → {busy, binding, cancel}`）+ `NuomiKernel::run_task_in_session(session_id, agent, input)`：按会话载入历史、跑隔离 Loop，不共享 `state.session_id` | 真正并发；后台任务与前台互不阻塞；为自主 Agent 铺路 | 改动 core facade，需会话隔离测试；provider 并发需受 `maxConcurrency` 约束 |
| B | 保持单内核串行，后台单 Agent 任务改为「排队」；仅团队/群聊走已有独立路径 | 改动最小 | 后台任务会被前台卡住，体验不达标；与「后台任务」语义冲突 |
| C | 每个会话一个 kernel 实例 | 隔离彻底 | 内存/连接爆炸，插件重复注册，成本高 |

**决策：采用 A**（B 作为 A 未落地前的降级开关；C 否决）。理由：会话类型要求「后台任务跑着，你还能在别的会话聊天」，这是产品差异点（pr.md §8）。

**D2 — 实时同步机制：CDC tail 推送 vs 逐点 bus publish**
| 选项 | 说明 |
|---|---|
| A（推荐） | 在 `src-tauri` 增加 CDC tail loop：轮询 `change_log::tail_after`，把 sessions/tasks/runs/teams/agent_profiles 的行变更映射为 `change.<table>` 事件发到 `event://domain`；同时把 `transition_run` 等**关键** topic 通过已有 bus 精确 publish |
| B | 仅逐点补 `bus.publish`（改 `append_domain_event`、`transition_run`、`record_turn`、`scheduler`） |

**决策：A + B 组合**：B 负责语义化 topic（`run.state_changed`、`session.message`、`schedule.triggered`），A 负责兜底所有表行变更（含 CLI 写入、跨表面）。A 复用已有 `change_log`，无需新表，且让 `useDomainEvents([DOMAIN_CHANNEL])` 从死代码变为有效。

**D3 — 附件存储位置**
**决策：工作区沙箱内 `.nuomi/attachments/<session_id>/<sha256>.<ext>`**，经 `WorkspaceService` 校验防逃逸；元数据入库 `attachments`。理由：随工作区可迁移、可被工具读取（Agent 能直接看文件）、复用现有沙箱边界。拒绝存入 DB BLOB（体积/迁移/bus payload 都不友好）。

**D4 — Markdown 气泡**
**决策：M1 阶段保持纯文本 + 代码块折叠**（不改现有 `Bubble` 语义），把 Markdown 渲染列入 M2（依赖已有 `marked` + `sanitize.ts`）。理由：本方案已很大，Markdown 不是本次需求，避免范围蔓延；但群聊/后台的工具输出会复用 `<ToolCard>`。

---

## 3. 数据模型与迁移

> 迁移只增不改（AGENTS.md §7.3）。当前最高版本 `0010`；**`0009` 为历史空缺，新文件从 `0011` 起**，并在 `crates/nuomi-core/src/store/migrations.rs` 的 `MIGRATIONS` 数组注册。

### 3.1 `0011_conversation_kind.sql`

```sql
-- 会话类型判别 + 绑定（Conversation = Session + kind + bindings）
ALTER TABLE sessions ADD COLUMN kind TEXT NOT NULL DEFAULT 'chat'
  CHECK (kind IN ('chat', 'group', 'background', 'scheduled'));
ALTER TABLE sessions ADD COLUMN agent_kind   TEXT;      -- 'cli' | 'role'（NULL = 默认解析链）
ALTER TABLE sessions ADD COLUMN agent_ref_id TEXT;      -- agent_profiles.id 或 roles.id
ALTER TABLE sessions ADD COLUMN team_id      TEXT;      -- group 的 Team
ALTER TABLE sessions ADD COLUMN task_id      TEXT;      -- background 的 Task
ALTER TABLE sessions ADD COLUMN schedule_id  TEXT;      -- scheduled 的 Schedule
CREATE INDEX idx_sessions_kind     ON sessions (kind, updated_at DESC);
CREATE INDEX idx_sessions_task     ON sessions (task_id);
CREATE INDEX idx_sessions_schedule ON sessions (schedule_id);
```
> 说明：不使用外键，理由与 `runs.session_id` 一致（`0002_tasks_runs.sql:19-32` 亦无 FK），避免删除顺序耦合；一致性由 service 层保证。`agent_kind`+`agent_ref_id` 用两列而非单列 `agent`，是为了兼容 CLI Agent 与 Role 两种来源且可建索引。

### 3.2 `0012_attachments.sql`

```sql
CREATE TABLE attachments (
  id          TEXT PRIMARY KEY,
  session_id  TEXT NOT NULL REFERENCES sessions(id),
  seq         INTEGER,              -- 绑定到 user message 的 events.seq（未发送前 NULL）
  kind        TEXT NOT NULL CHECK (kind IN ('file', 'image', 'paste', 'text')),
  name        TEXT NOT NULL,
  mime        TEXT NOT NULL,
  rel_path    TEXT NOT NULL,        -- 相对工作区根，如 .nuomi/attachments/<sid>/<sha>.png
  size_bytes  INTEGER NOT NULL,
  sha256      TEXT NOT NULL,
  created_at  INTEGER NOT NULL
);
CREATE INDEX idx_attachments_session ON attachments (session_id, created_at);
CREATE INDEX idx_attachments_seq     ON attachments (seq);
```
> `kind='paste'` = 剪贴板图片/大文本；`kind='text'` = 超大文本落盘（§9.2）。`seq` 让附件与具体那条用户消息可回放关联。

### 3.3 `0013_schedule_conversations.sql`

```sql
-- 定时任务从「只造 Task」升级为「可按类型造会话并自动派发」
ALTER TABLE schedules ADD COLUMN target_kind   TEXT NOT NULL DEFAULT 'task'
  CHECK (target_kind IN ('task', 'chat', 'group'));
ALTER TABLE schedules ADD COLUMN agent_kind    TEXT;
ALTER TABLE schedules ADD COLUMN agent_ref_id  TEXT;
ALTER TABLE schedules ADD COLUMN team_id       TEXT;
ALTER TABLE schedules ADD COLUMN session_mode  TEXT NOT NULL DEFAULT 'per_trigger'
  CHECK (session_mode IN ('per_trigger', 'reuse'));
ALTER TABLE schedules ADD COLUMN session_id    TEXT;   -- reuse 模式复用的会话
ALTER TABLE schedules ADD COLUMN auto_dispatch INTEGER NOT NULL DEFAULT 1;
```

### 3.4 Rust domain 变更（`crates/nuomi-core/src/domain/`）

```rust
// entities.rs —— 新增判别枚举 + Session 扩展
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationKind { Chat, Group, Background, Scheduled }
impl ConversationKind {
    pub fn parse(s: &str) -> Option<Self> { /* 与 TaskStatus::parse 同风格 */ }
    pub fn as_str(self) -> &'static str { /* ... */ }
}

pub enum AgentRefKind { Cli, Role }

pub struct Session {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    // 新增：
    pub kind: ConversationKind,
    pub agent: Option<(AgentRefKind, String)>,
    pub team_id: Option<String>,
    pub task_id: Option<String>,
    pub schedule_id: Option<String>,
}
```
- `Session` 加字段会波及所有构造点：`facade.rs:132-148,401-406`、`sessions.rs` tests、`whiteboard.rs` tests、`commands.rs` DTO。→ 提供 `Session::new_chat(id, title, now)` 构造器收敛默认值。
- `Schedule` 同步加 `target_kind/agent/team_id/session_mode/session_id/auto_dispatch`（`entities.rs:497-511`）。

### 3.5 向后兼容

- 旧行 `kind` 默认 `'chat'`，行为不变。
- `list_sessions` 保持存在；新增 `list_conversations` 返回更丰富的 DTO。
- 旧 `submit_task` 保持存在（内部转调新的 `submit_message`），避免破坏 CLI/测试替身。

---

## 4. IPC 契约变更（contract-first）

> 规则：`src-tauri/src/tauri_cmds.rs` 加包装 → `lib.rs::specta_builder` 注册 → `pnpm contracts:gen` → 消费端 + `test-double`，**同一提交**完成（`.opencode/rules/ipc-contract.md`）。

### 4.1 新增 / 变更命令

| 命令 | 签名（TS 视角） | 用途 | 备注 |
|---|---|---|---|
| `create_conversation` | `(input: ConversationInput) => ConversationDto` | 按类型新建会话（含绑定） | 新 |
| `list_conversations` | `(kind: ConversationKind \| null) => ConversationDto[]` | 列表（支持类型过滤） | 新 |
| `get_conversation` | `(sessionId: string) => ConversationDto` | 单会话详情（含 Agent 解析名） | 新 |
| `set_conversation_agent` | `(sessionId, agent: AgentRefInput \| null) => ConversationDto` | 切换/清空会话 Agent | 新（R1 核心） |
| `list_agent_options` | `() => AgentOptionDto[]` | 统一 Agent 选择源（CLI + Role 合并） | 新 |
| `submit_message` | `(sessionId, text, attachmentIds: string[]) => RunResultDto` | Composer 提交（带附件） | 新（替代 submit_task 前端用法） |
| `stop_conversation` | `(sessionId: string) => void` | 停止当前会话在跑的 Run | 新 |
| `list_active_runs` | `() => RunDto[]` | 后台任务全局托盘 | 新 |
| `cancel_run` | `(runId: string) => void` | 取消某个后台 Run | 新 |
| `save_attachment` | `(sessionId, name, mime, dataBase64: string) => AttachmentDto` | 落盘 + 入库 | 新 |
| `list_attachments` | `(sessionId: string) => AttachmentDto[]` | 会话附件 | 新 |
| `delete_attachment` | `(attachmentId: string) => void` | 删除 | 新 |
| `upsert_schedule` | `(input: ScheduleInput) => ScheduleDto` | 调度创建/编辑（会话化） | 新（替代 create_schedule 的前端用法） |
| `update_schedule` | `(scheduleId, input: ScheduleInput) => ScheduleDto` | 编辑 | 新 |
| `submit_task` | 保持 | 兼容 | 内部转调 |
| `create_schedule` / `create_session` | 保持 | 兼容 | 内部转调 |

### 4.2 DTO（Rust `#[serde(rename_all="camelCase")]` + `specta::Type`；枚举 `#[serde(tag="type")]` 或 snake_case 字符串）

```ts
export type ConversationKind = "chat" | "group" | "background" | "scheduled";
export type AgentRefKind = "cli" | "role";

export interface AgentRefDto {
  kind: AgentRefKind;
  id: string;
  name: string;              // 解析后的展示名
  flavor?: CliFlavorDto;     // cli 时
  providerName?: string;     // role 时
}

export interface ConversationDto {
  id: string;
  title: string;
  kind: ConversationKind;
  agent: AgentRefDto | null;
  teamId: string | null;
  taskId: string | null;
  scheduleId: string | null;
  createdAt: number;
  updatedAt: number;
  status: "idle" | "running" | "awaiting_approval" | "failed"; // 由活跃 Run 派生（读时计算）
  preview: string | null;    // 最后一条消息摘要，供列表
}

export interface ConversationInput {
  kind: ConversationKind;
  title?: string;
  agent?: AgentRefInput | null;
  teamId?: string | null;
  taskTitle?: string;        // background/scheduled 用
  taskDescription?: string;
}

export interface AgentRefInput { kind: AgentRefKind; id: string; }

export interface AgentOptionDto {
  kind: AgentRefKind;
  id: string;
  name: string;
  description: string | null;
  flavor?: CliFlavorDto;
  providerName?: string | null;
  builtin: boolean;
  enabled: boolean;
}

export interface AttachmentDto {
  id: string;
  sessionId: string;
  seq: number | null;
  kind: "file" | "image" | "paste" | "text";
  name: string;
  mime: string;
  relPath: string;
  sizeBytes: number;
  sha256: string;
  createdAt: number;
}

export interface ScheduleDto {
  id: string;
  name: string;
  cronExpr: string;
  enabled: boolean;
  targetKind: "task" | "chat" | "group";
  agent: AgentRefDto | null;
  teamId: string | null;
  sessionMode: "per_trigger" | "reuse";
  sessionId: string | null;
  autoDispatch: boolean;
  taskTitle: string;
  taskDescription: string;
  lastTriggeredAt: number | null;
  nextTriggerAt: number | null;
}

export interface ScheduleInput {
  name: string;
  cronExpr: string;
  targetKind: "task" | "chat" | "group";
  agent?: AgentRefInput | null;
  teamId?: string | null;
  sessionMode: "per_trigger" | "reuse";
  sessionId?: string | null;
  autoDispatch: boolean;
  taskTitle: string;
  taskDescription: string;
}
```
> 变更：`SessionDto` 可保留（CLI 兼容），前端主用 `ConversationDto`；`RunDto` 增加 `kind?: "single"|"team"`、`cancelable: boolean`。

### 4.3 错误码（`ipc_error.rs::codes` + i18n `errors.*`）

| code | 触发 |
|---|---|
| `conversation.not_found` | 会话不存在 |
| `conversation.invalid_kind` | kind 非法 / group 缺 team |
| `conversation.busy` | 同会话已有 Run 在跑（若采用「同会话串行」） |
| `run.not_active` | stop/cancel 时无活跃 Run |
| `attachment.too_large` | 超过上限（建议 25 MB） |
| `attachment.invalid_mime` | 非白名单 MIME |
| `attachment.not_found` | 删除不存在的附件 |
| `schedule.invalid_target` | chat/group 目标缺 agent/team |

`describeError` 会自动映射为 `errors.conversation_not_found` 等（`src/i18n/index.ts:25-33`），i18n 需同步加键。

---

## 5. 实时事件地基（P0，先于一切 UI）

### 5.1 缺口回顾

| 写入点 | 落库 | bus publish | 后果 |
|---|---|---|---|
| `append_domain_event`（task/schedule/workspace） | ✅ | ❌ | `event://domain` 收不到 task/schedule 变更 |
| `transition_run_with_detail` | ✅ | ❌ | 后台 Run 状态不变更 UI |
| `whiteboard.record_turn`（群聊发言） | ✅ | ❌ | 群聊发言不实时（只有黑板 note 实时且不含 role） |
| `team_former` `team.formed` | ✅ | ✅ | 唯一有效的 domain 事件 |
| `scheduler_service::tick` | ✅（events 表） | ❌ | 调度触发不实时 |

### 5.2 方案 A：CDC tail → domain 事件（`src-tauri/src/events.rs` 新增 loop）

```rust
/// 轮询 change_log，把行级变更映射为 UI 可订阅的 domain 事件。
/// 退避 500ms；游标持久化于内存（进程重启从 0 重放一次，前端幂等 invalidate）。
pub fn spawn_change_stream(app: AppHandle, db_path: Arc<str>) {
    tauri::async_runtime::spawn(async move {
        let mut cursor: i64 = 0;
        loop {
            let path = db_path.clone();
            let page = tokio::task::spawn_blocking(move || -> Result<Vec<ChangeEntry>, _> {
                let db = Db::open(&path)?; // 只读连接
                change_log::tail_after(&db.0, cursor, 200)
            }).await;
            // 对每条 entry 发 { "type": "change.<table>", "rowId", "op" } 到 event://domain
            // 空页 => sleep(500ms)，有页 => 立即继续（追平）
        }
    });
}
```
- `change.` 前缀需加入 `is_domain_topic`（`events.rs:34-40`）与 `notifier::is_domain_topic`（`notifier.rs:235-239`）。
- 前端：各视图订阅 `DOMAIN_CHANNEL`，按 `change.sessions/tasks/runs/teams` invalidate 对应 query。
- 价值：**同时修复现有 Board/Scheduler/Approvals 的死订阅**，并天然覆盖 CLI 端写入。

### 5.3 方案 B：关键 topic 精确 publish

1. `append_domain_event` 增加 `bus: &EventBus` 参数（或在 `AppState` 上包一层），写库后 `bus.publish(Event::new(topic, payload))`。调用点：`task.created/status_changed/deleted`、`schedule.*`、`workspace.switched`。
2. `transition_run_with_detail` 增加 `bus` 参数并 publish `run.state_changed`（payload 已含 `taskId/runId/sessionId/from/to`）；`dispatch_run`/`spawn_team_run` 传入 `state.kernel.context().bus()`。
3. `whiteboard::record_turn`：`record_turn` 已持 `WhiteBoardService`（含 `bus`），在 `events::append` 后补 `bus.publish(Event::new("session.message", {sessionId, role_id, role_name, content, seq}))`；**seq 用刚落库的 events.seq**（需让 `events::append` 返回 seq 或再查一次）。这样群聊发言带 role 名实时到达。
4. `SchedulerRunner::new(...).with_bus(bus)`：tick 完成后 publish `schedule.triggered`（§3.3 会话化后的 payload 含 `sessionId`/`taskId`）。

### 5.4 调度派发桥（`src-tauri`）

新增 `schedule_dispatcher`：订阅 bus 的 `schedule.triggered`，若 `auto_dispatch`：
- `target_kind=chat` → 用 D1 的会话级运行时启动 Run；
- `target_kind=group` → 复用 `spawn_team_run`；
- `target_kind=task` → 保持旧行为（Board 手动/批量运行）。

> 这样把「执行编排（需要 kernel）」留在 shell，「调度计时（纯逻辑）」留在 core，职责清晰且可测。

---

## 6. `/` 命令系统增强（R1）

### 6.1 registr y扩展模型（`src/lib/commands/registry.ts`）

```ts
export type CommandCategory = "session" | "agent" | "context" | "task" | "view" | "plugin";
export type ArgKind = "none" | "text" | "path" | "agent" | "team" | "file" | "schedule";

export interface SlashCommand {
  name: string;
  category: CommandCategory;
  descriptionI18nKey?: string;
  description?: string;          // 插件直供
  usage?: string;
  args: ArgKind;
  /** 触发词之外的模糊搜索关键词（中英） */
  keywords?: string[];
  /** 动态参数候选：如 /agent 的 agent 列表；返回空数组则退化为纯文本参数 */
  suggest?: (ctx: CommandContext) => Promise<CommandSuggestion[]> | CommandSuggestion[];
  run(args: string, ctx: CommandContext): Promise<void> | void;
  source?: "builtin" | "plugin";
}
```
- `parseInput` 升级：在命中命令且 `args !== "none"` 时，若当前 caret 处于「命令后第一个参数词」且 `suggest` 可用，则**把补全面板切到参数模式**（`/agent cl▮` 列出候选 Agent）。
- 排序：`startsWith` 优先，其次 `keywords`，最后 `includes`；同分按 `category` 固定权重（agent/context 靠前）。
- 保留 `registerCommand` 覆盖语义（后注册者胜），插件命令仍可注入。

### 6.2 命令清单（M1）

| 命令 | 分类 | 作用 | 新增 |
|---|---|---|---|
| `/agent [name]` | agent | 切换当前会话 Agent（无参→打开 Agent 选择器） | ✅ |
| `/attach <path>` | context | 以工作区相对路径添加附件 | ✅ |
| `/clear` | session | 清空输入框（保留既有语义） | 既有 |
| `/new [kind]` | session | 新建会话（可带类型：chat/group/background/scheduled） | 升级 |
| `/sessions` | view | 打开会话列表（可带类型过滤） | 既有（升级） |
| `/stop` | task | 停止当前会话活跃 Run | ✅ |
| `/background <title>` | task | 把当前输入转为后台任务会话 | ✅ |
| `/group` | session | 打开群聊新建向导 | ✅ |
| `/schedule <cron>` | task | 打开定时任务新建向导（预填 cron） | ✅ |
| `/workspace <path>` `/theme` `/help` | view | 既有 | 既有 |
| `/plugin` | plugin | 列出插件提供的命令 | 预留 |

> `/help` 改为**分组长列表**（不再挤在一个 toast 里）：M1 用 `Dialog` + 分组；`category` 作为分组标题。

### 6.3 补全面板 UI 规格（`src/features/conversation/composer/SlashMenu.tsx`）

- 结构：`absolute bottom-full` 面板，`role="listbox"`；顶部一行显示当前「模式」（命令模式 / 参数模式），列表项 = `图标 + /名称 + usage + 描述`，右侧 `category` 徽章。
- 键盘：`↑/↓` 移动、`Enter/Tab` 补全、`Esc` 关闭（沿用 `ChatInput.tsx:166-198` 已验证的契约）；参数模式补全后插入 `... ` 并保持焦点。
- 空态：输入 `/` 但无匹配 → 显示「无匹配命令」而非空面板。
- Recall：记录最近 5 条命令，空查询时置顶（localStorage `nuomi.commands.recent`）。
- 无障碍：`aria-activedescendant` 指向高亮项；面板与输入框用 `aria-controls`/`aria-expanded` 关联。

### 6.4 动态命令源

- **Agent**：`/agent` 的 `suggest` 调 `ipc.listAgentOptions()`（React Query 缓存 `["agentOptions"]`）。
- **Skill / MCP / 插件命令**：M1 预留 `hydratePluginCommands()`（启动时 `registerCommand` 注入，`source:"plugin"`），与 principle.md §4「开放注册表」一致；本次不实现技能发现，仅打通接口。
- **内置命令模块化**：`builtin.ts` 拆为 `sessionCommands.ts` / `agentCommands.ts` / `contextCommands.ts` / `taskCommands.ts`，在 `builtin.ts` 汇总注册（单文件超 200 行需拆分，见 typescript-react 规则）。

### 6.5 文件清单

- 改：`src/lib/commands/registry.ts`、`src/lib/commands/builtin.ts`
- 新：`src/lib/commands/sessionCommands.ts`、`agentCommands.ts`、`contextCommands.ts`、`taskCommands.ts`、`commandTypes.ts`
- 新：`src/features/conversation/composer/SlashMenu.tsx`、`useComposerTriggers.ts`

---

## 7. Agent 切换与显示（R1 核心）

### 7.1 Agent 解析优先级（`src/lib/conversation/agentResolve.ts` + Rust `conversation_service`）

```
会话显式绑定 (sessions.agent_kind/agent_ref_id)
  └─ 空 → 全局默认 Agent（app_settings: "conversation.default_agent"）
        └─ 空 → 首个 enabled 的 AgentProfile
              └─ 空 → 首个 builtin Role
                    └─ 空 → 内核默认 provider（现状行为）
```
- 前端只读 `ConversationDto.agent`（后端已解析成 `AgentRefDto` 带 `name`），不做二次 join，避免 UI 里散落解析逻辑。
- 后端 `conversation_service::resolve_agent(conn, session) -> Option<AgentRefDto>` 统一实现，供 `get/list_conversation` 复用。

### 7.2 会话级绑定

- `set_conversation_agent(sessionId, agent | null)` → 写 `sessions.agent_kind/agent_ref_id`，返回刷新后的 `ConversationDto`，并 `bus.publish("conversation.updated", {sessionId})`（走 §5.2/5.3）。
- 校验：`kind=cli` 必须命中 `agent_profiles` 且 `enabled`；`kind=role` 必须命中 `roles`。
- 正在运行的会话允许改绑定，但**下一条消息才生效**（不打断当前 Run）；UI 提示「下条消息生效」。

### 7.3 AgentChip（常显）

`src/features/conversation/composer/AgentChip.tsx`：
- 位置：Composer **左下角**（发送按钮对侧），始终可见；`kind=group` 时显示 Team 名 + 成员数（不可点切 Agent，点击打开成员面板）。
- 内容：`<Icon name="users"> 名称`，`cli` 显示 flavor 角标（`Claude Code`/`Codex`/`Plain`），`role` 显示 provider 名。
- 交互：点击 → `AgentPickerPopover`（搜索 + 分组 CLI Agents / Roles + 「设为默认」）；键盘可达；`tooltip` 显示完整来源。
- 状态：`conversation.busy` 时 chip 置灰但可查看；切换中显示 `Spinner`。
- 同时在 **ConversationHeader** 与 **SessionsList 行** 显示同源徽章（列表行右侧小图标）。

### 7.4 `/agent` 与 `@` 复用

- `/agent`：无参 → 打开 picker；有参 → 前缀匹配唯一则直接切。
- `@` 菜单的「Agent」命名空间（§8.1）复用同一 `AgentPicker` 数据与切换动作。

### 7.5 默认 Agent 设置

- 在 `SettingsView` 新增「对话」分区（或复用 `app_setting_get/set`）：`conversation.default_agent`（值 `cli:<id>` / `role:<id>`）。
- 键名常量放 `src/lib/conversation/kinds.ts`，避免散落魔法字符串。

---

## 8. `@` 附件系统（R2）

### 8.1 触发与命名空间（`useComposerTriggers.ts`）

`@` 在**词边界**（行首或空白后）触发，面板按命名空间分组：

| 命名空间 | 数据源 | 插入结果 |
|---|---|---|
| `@file` | 工作区索引 `src/lib/editor-ext/indexer/workspace-index.ts`（复用 QuickOpen 的 `scorePath`） | 以路径形式插入 `@src/foo.ts`，并入附件列表（`kind=file`） |
| `@agent` | `ipc.listAgentOptions()` | `@AgentName`，并入「消息级 Agent 覆盖」候选（仅下一条消息） |
| `@session` | `ipc.listConversations(null)` | `@会话标题`，作为上下文引用插入（M1 仅文本引用，不做跨会话内容注入） |
| `@board` | `ipc.listTasks(null)`（M1.5） | 引用任务 |

- 面板与 `/` 面板共用同一 popup 容器与键盘契约（`SlashMenu` 泛化为 `CompletionMenu`，`@`/`/` 只改数据源与过滤规则）。
- `Esc` 关闭；关闭后再次输入 `@` 或继续输入会重新打开。

### 8.2 附件数据流

```
用户选择文件 / 粘贴图片
   └─ (前端) 读取字节 → base64
        └─ ipc.save_attachment(sessionId, name, mime, base64)
             └─ (Rust) 校验 MIME/大小 → 计算 sha256 → 写入 .nuomi/attachments/<sid>/<sha>.<ext>
                  └─ 插 attachments 行 → 返回 AttachmentDto
   └─ (前端) 加入 composer 附件条（缩略图/文件名 + 删除）
提交时：ipc.submit_message(sessionId, text, attachmentIds)
   └─ (Rust) 把附件内容/路径注入 user message 上下文（见 8.4），并回填 attachments.seq
```

- **去重**：同 `sha256` 已存在则复用 rel_path，不重复落盘（内容寻址）。
- **大小/MIME**：上限 25 MB / 20 个附件；白名单 `image/*`、`text/*`、`application/pdf`、`application/json`、常见代码/文档；超限返回 `attachment.too_large` / `attachment.invalid_mime`，UI toast 且保留输入。
- **并发**：`spawn_blocking` 落盘（遵循 rust-core 规则）；大文件可用 `tauri::ipc::Channel` 分片（M2 优化，M1 先 base64）。

### 8.3 存储与沙箱安全

- 根目录 `.nuomi/attachments/` 纳入 `.gitignore`（检查现有 `.gitignore`，否则补一行）。
- 所有路径经 `WorkspaceService` 规范化并断言仍在工作区内（复用现有逃逸校验 `workspace_escape_denied`）。
- 文件名仅用于展示；落盘名用 `sha256`，防路径穿越/重名。
- 过期清理：`attachments` 随会话删除，或按 `created_at` + 保留期 GC（M2，复用 `Artifact` 保留期思路）。

### 8.4 上下文注入

- **文本类**（`text/*`、代码）：把内容以内联引用注入 user message（超阈值则截断并提示）。
- **图片**：多模态 provider 走 image block；非多模态 provider 降级为「文件已附上：<path>」文本 + 工具可读路径（Agent 可用 `read_file` 工具读取，因为落在工作区内）。
- **二进制/其他**：仅注入路径引用 + 元数据，交由工具处理。
- 注入格式由后端 `conversation_service::compose_user_message(text, attachments)` 统一产出，前端不拼接。

### 8.5 UI 规格

- **附件条**（`AttachmentShelf.tsx`）：输入框上方横向滚动；图片显缩略图（`URL.createObjectURL` + `revokeObjectURL` 清理），其他显文件图标 + 名称 + 大小；每项 hover 显示删除；键盘可达。
- **拖拽**：`getCurrentWebview().onDragDropEvent`（Tauri v2）接收文件路径 → 直接转附件（比粘贴更可靠，作为文件添加的**主路径**）。
- **点击回形针按钮** → Tauri `dialog.open`（`@tauri-apps/plugin-dialog` 已依赖）选择文件。
- 空/错误态：上传失败原地标红 + 重试；`AsyncBoundary` 不适用于此窄交互，用局部状态。

---

## 9. 剪贴板粘贴（R3）

### 9.1 优先级规则（`useClipboardPaste.ts`）

| 剪贴板内容 | 行为 |
|---|---|
| 纯文本（< 阈值，如 8 KB） | **默认插入**（不阻断原生粘贴） |
| 纯文本（≥ 阈值） | 存为附件（`kind=text`），输入框插入 `[大文本 12.3KB]` 占位，提交时展开注入 |
| 图片（`clipboardData.items` 含 image） | 调 `save_attachment`（`kind=paste`，mime=image/png），插入图片附件 + 缩略图 |
| 文件（`clipboardData.files`） | 逐个读 Blob → 附件（`kind=file`）；名称取 `File.name` |
| 其他 | 不拦截 |

- 实现基于 DOM `paste` 事件（`clipboardData.items`/`files`），**不新增 Tauri 插件依赖**；WebView2/WKWebView 支持 Blob 读取。
- 大文本阈值常量放 `src/lib/conversation/attachmentModel.ts`，可配。

### 9.2 处理细节

- 图片：`FileReader.readAsDataURL` → 去掉 dataURL 前缀得 base64 → IPC。
- 若粘贴的图片与已有附件同 hash → 只 toast「已存在」不重复添加。
- `preventDefault()` 仅在确认拦截时调用，避免吞掉正常文本粘贴。
- `isComposing`（输入法合成）期间不拦截。

### 9.3 Tauri 能力与权限

- 现有 `core:window:allow-start-dragging`、`dialog` 权限已具备；`onDragDropEvent` 需确认 `core:webview` 相关 capability（在 `src-tauri/capabilities/*.json` 补齐并注释）。
- 无需 `clipboard-manager` 插件（M1）；若 M2 要「粘贴按钮」，再评估 `@tauri-apps/plugin-clipboard-manager`。

### 9.4 UI 反馈

- 粘贴成功：附件条追加 + 轻微 `animate-draw-in`。
- 被拦截为大文本：输入框内 inline 提示条「已折叠为附件（可展开）」。
- 失败：toast（`describeError`）+ 输入不变。

---

## 10. 新会话类型的 UI/UX（R4/R5）

### 10.1 新建会话入口（`NewConversationMenu.tsx`）

- 触发点：`SessionsList` 的「新建」按钮改为 **split button**（左键=默认 chat，下拉=类型菜单）；`/new <kind>` 亦可。
- 菜单项：`普通对话`｜`多 Agent 群聊`｜`后台任务`｜`定时任务`。
- 选择后进入**类型化向导**（`Dialog`）：
  - chat：标题 + Agent 选择（默认链预选）。
  - group：Team 选择（或「去创建」）+ 首条任务。
  - background：任务标题/描述 + 执行体（单 Agent 或 Team）。
  - scheduled：`ScheduleForm` 升级版（cron + 目标类型 + 执行体 + 每触发新会话/复用会话）。

### 10.2 会话列表分组与徽章（`SessionsList` → `ConversationsList`）

- 顶部类型过滤 `Tabs`：`全部 / 对话 / 群聊 / 后台 / 定时`（查询键 `["conversations", kind]`）。
- 行内：`kind` 图标（`chat`/`users`/`play`/`scheduler` 复用现有 `IconName`）+ 标题 + **Agent/Team 徽章** + 相对时间 + 右端状态点（`running` 呼吸、`failed` 红）。
- `running` 会话置顶并显示活动指示（复用 `animate-pulse`）。

### 10.3 `group` 多 Agent 群聊会话 UI（`GroupConversationView.tsx`）

信息架构：**主区 = 发言时间线；右侧 = 黑板坞；顶部 = Round/收敛状态**。

- **发言者气泡** `SpeakerBubble.tsx`：左侧竖条 + 角色名 pill（颜色由 role id 稳定 hash 到 `cap-*`/语义色集合，保证同角色同色）；气泡内纯文本；`handoff` 用气泡间 **HandoffRibbon** 箭头 + `handoff_to_next` 参数展示。
- **RoundIndicator**：顶部显示 `Round 3 / 6`、当前发言者、Selector 模式（RR/LLM）；`max_rounds` 到达前显示进度。
- **WhiteboardDock**：右侧可折叠坞（复用 `WhiteBoardFlow` 的 `{seq, body}` 渲染，升级为 `author + noteType` 徽章）；`session.whiteboard` 实时追加。
- **GroupControls**：`停止`（cancel token）、`继续一轮`、`追加黑板笔记`、`导出转录`。
- **收敛态**：`converged=true` 显示 `Badge tone="ok"` + 最终输出卡；`HandoffLoopDetected` 显示告警卡（复用 `trace.cycleWarning` 语义）。
- **复用**：执行层完全复用 `run_team_session`/`team_runner`；展示层从 `useSessionStream` 扩展出 `kind:"speech"` entry（`roleName`+`roleColor`），并在 `traceModel.ts` 的 `buildHandoffChain` 基础上抽公共函数，避免 Trace 页与群聊页两套解析。

> 与 Trace 页的分工：群聊页负责**对话式**阅读与操作；Trace 页保留**审计式**时间线/Journal。二者共享解析函数。

### 10.4 `background` 后台任务会话 UI（`BackgroundConversationView.tsx`）

信息架构：**顶部 Run 状态条 + 中部工具/消息时间线 + 底部 Composer（可发补充指令若支持）**。

- **RunStatusBar**：状态机徽章（`queued/running/awaiting_approval/succeeded/failed/timed_out/cancelled/interrupted`）+ 已用时 + 心跳年龄（`heartbeat_at`）+ `停止`/`重试`；`awaiting_approval` 时内联 `Approve/Deny`（联动 `resolve_approval`）。
- **RunTimeline**：工具调用 `ToolCard` + 消息；运行中显示流式 caret（复用 `STREAM_ENTRY_ID`）。
- **全局 BackgroundTray**（`src/features/shell/BackgroundTray.tsx`）：从 `AreaNav` 右侧入口打开的抽屉，`list_active_runs()` + `change.runs` 实时刷新；每行 = 会话标题 + 状态 + 停止；空态「没有正在运行的后台任务」。这是「后台」心智的关键：即使切走也能看到。
- **脱离导航不中断**：Run 由 `tokio::spawn` 执行，切视图不影响；托盘提供全局可见性。
- **孤儿恢复（P2）**：补 `heartbeat()` 写入 + 后台扫描（`derive_status` 已就绪），超时 `running → interrupted`，UI 提供 `requeue`。

### 10.5 `scheduled` 定时任务会话 UI（`ScheduledConversationView.tsx`）

信息架构：**规则卡（顶部）+ 下次触发倒计时 + 历史触发 runs 时间条 + 结果列表**。

- **ScheduleRuleCard**：`cron_expr` 人类可读化（如 `0 9 * * *` → 「每天 09:00（UTC）」）+ 编辑（升级 `ScheduleForm`）+ 启停（`toggle_schedule`）。
- **NextRunCountdown**：`next_trigger_at` 实时倒计时（本地 tick，不必后端事件）。
- **RunHistoryStrip**：每次触发产生的会话/Run 卡片（`per_trigger` 模式每次一条，可点进对应 conversation）。
- **ScheduledList**：`SchedulerView` 升级为「定时任务」页，展示所有规则及其最近一次结果；`schedule.triggered` 实时插入新的运行卡。
- **向导**：`targetKind=chat/group` 时展示 Agent/Team 选择；`auto_dispatch` 开关（关闭则只造任务，等价旧行为）。
- **i18n**：所有 cron 人类可读文案走 i18n（zh/en），避免硬编码中文星期/单位。

### 10.6 统一 ConversationHeader / Composer 适配

- `ConversationHeader.tsx`：左侧 `kind` 图标 + 标题（可重命名，复用 `update_title` 思路）+ 右侧类型专属状态（group: Round；background: 状态；scheduled: 下次触发）+ Agent/Team 徽章。
- Composer 按 `kind` 微调：
  - `chat`：完整 `/`+`@`+粘贴+AgentChip。
  - `group`：`/`+`@`+粘贴；AgentChip 显示 Team；发送进入群聊 Run。
  - `background`：以「任务指令」为主；`/stop` 可用。
  - `scheduled`：Composer 用于编辑**触发时投喂的 prompt**（保存到 schedule），不直接执行。
- 统一 `ConversationView.tsx` 按 `kind` 分派到 4 个视图，`ChatView` 保留为 `kind=chat` 的实现（渐进迁移，避免一次性重写）。

### 10.7 布局：向三栏演进（可选但建议）

- pr.md §5 目标是三栏（左 `{git|sessions}` / 中 `{bubbles + input}` / 右 `{文件|其他功能}`）。当前是两栏互斥。
- M1 先不动全局布局，只在**会话主区**内实现「右坞」（group 黑板坞 / background 时间线侧栏），复用 `EditorArea` 的右侧弹性思路。
- M2 评估把 `LeftRail` 拆为「窄导航条 + 会话列」与右侧「上下文栏」，对齐 pr.md。此项单列 ADR，避免与本次会话类型耦合。

### 10.8 动效 / 主题 / i18n

- 动效：列表插入用 `animate-draw-in`；状态点 running 用 `animate-pulse`；Handoff 箭头用 `animate-stroke`（已有）。
- 主题：全部走语义 token；新增 `SpeakerBubble` 的角色色**必须**从既有 `cap-*` / `state-*` / `ink-accent` 派生，若确需新色，必须同步更新 `themeContrast.test.ts` 的 4 主题配对（否则测试红）。
- i18n：新增 `conversation.*`、`composer.*`、`group.*`、`background.*`、`schedule2.*`（沿用 `scheduler.*` 演进）、`agent.*`、`attachment.*`、`errors.*`。**必须 zh-CN + en 同步**；若脚本合并，遵循「内存构建 + 校验 + 原子写盘」（防止并行 agent 丢键）。

---

## 11. 组件与文件清单

### 11.1 新增文件

```
src/lib/conversation/kinds.ts               ConversationKind/AgentRef 常量与守卫
src/lib/conversation/agentResolve.ts        展示辅助（flavor 文案、颜色）
src/lib/conversation/attachmentModel.ts     大小/MIME/阈值/格式化
src/lib/commands/commandTypes.ts            CommandCategory/ArgKind/Suggestion
src/lib/commands/sessionCommands.ts
src/lib/commands/agentCommands.ts
src/lib/commands/contextCommands.ts
src/lib/commands/taskCommands.ts
src/lib/commands/pluginCommands.ts(预留)
src/features/conversation/ConversationView.tsx
src/features/conversation/ConversationHeader.tsx
src/features/conversation/composer/Composer.tsx
src/features/conversation/composer/CompletionMenu.tsx   (泛化 SlashMenu)
src/features/conversation/composer/AgentChip.tsx
src/features/conversation/composer/AgentPickerPopover.tsx
src/features/conversation/composer/AttachmentShelf.tsx
src/features/conversation/composer/useComposerTriggers.ts
src/features/conversation/composer/useClipboardPaste.ts
src/features/conversation/new/NewConversationMenu.tsx
src/features/conversation/new/NewConversationDialog.tsx
src/features/conversation/group/GroupConversationView.tsx
src/features/conversation/group/SpeakerBubble.tsx
src/features/conversation/group/HandoffRibbon.tsx
src/features/conversation/group/WhiteboardDock.tsx
src/features/conversation/group/RoundIndicator.tsx
src/features/conversation/group/GroupControls.tsx
src/features/conversation/background/BackgroundConversationView.tsx
src/features/conversation/background/RunStatusBar.tsx
src/features/conversation/background/RunTimeline.tsx
src/features/shell/BackgroundTray.tsx
src/features/conversation/scheduled/ScheduledConversationView.tsx
src/features/conversation/scheduled/ScheduleRuleCard.tsx
src/features/conversation/scheduled/NextRunCountdown.tsx
src/features/conversation/scheduled/RunHistoryStrip.tsx
src/features/shell/ConversationsList.tsx     (由 SessionsList 演进)
```
> 每个 >200 行的文件再拆；组件一文件一组件；`type XxxProps` 导出（typescript-react 规则）。

### 11.2 修改文件

| 文件 | 改动 |
|---|---|
| `src-tauri/src/events.rs` | `is_domain_topic` 加 `change.`/`conversation.`；新增 `spawn_change_stream` |
| `src-tauri/src/lib.rs` | 注册新命令；启动 CDC stream；`SchedulerRunner::with_bus` |
| `src-tauri/src/tauri_cmds.rs` | 新命令包装（4 点锁步） |
| `src-tauri/src/commands.rs` | `impl_create_conversation/list_conversations/set_conversation_agent/submit_message/...`；`append_domain_event`/`transition_run*` 接 bus |
| `src-tauri/src/notifier.rs` | `is_domain_topic` 加 `change.`/`conversation.` |
| `crates/nuomi-core/src/domain/entities.rs` | `ConversationKind`、`AgentRefKind`、`Session`/`Schedule` 扩展 |
| `crates/nuomi-core/src/store/repos/sessions.rs` | 新字段读写 + `list_by_kind` |
| `crates/nuomi-core/src/store/repos/attachments.rs` | 新增仓储 |
| `crates/nuomi-core/src/store/repos/mod.rs` | 导出 attachments |
| `crates/nuomi-core/src/store/migrations.rs` | 注册 0011/0012/0013 |
| `crates/nuomi-core/src/services/scheduler_service.rs` | 会话化 + `with_bus` + `schedule.triggered` |
| `crates/nuomi-core/src/services/conversation_service.rs` | 新增：新建/绑定/解析 Agent/组装 user message |
| `crates/nuomi-core/src/orchestrator/whiteboard.rs` | `record_turn` 补 `session.message` publish（含 role_name + seq） |
| `crates/nuomi-core/src/facade.rs` | `run_task_in_session`（D1）+ `Session::new_chat` |
| `src/lib/ipc/client.ts` / `test-double*.ts` | 新方法 + 替身 |
| `src/lib/store/uiStore.ts` | `View` 加 `conversation`?（保留 `chat`）+ `backgroundTrayOpen`/`whiteboardDockOpen` |
| `src/features/shell/Shell.tsx` / `LeftRail.tsx` / `AreaNav.tsx` | 视图与入口 |
| `src/features/scheduler/*` | 升级为会话化调度 |
| `src/features/chat/*` | 渐进迁移到 `conversation/`（`ChatView` 保留 chat 实现） |
| `src/i18n/locales/zh-CN.json` `en.json` | 新增键 |
| `.gitignore` | `.nuomi/attachments/` |

### 11.3 明确不做（Non-goals，防范围蔓延）

- 不实现 Markdown 富文本气泡（M2）。
- 不实现跨会话内容级 `@session` 注入（M1 仅文本引用）。
- 不实现 Skills 自动发现与 MCP 命令注入（仅打通注册接口）。
- 不重写全局三栏布局（单列 ADR）。
- 不引入向量记忆 / 附件语义检索。

---

## 12. 测试与验收

### 12.1 Rust（core-engineer / qa-guardian）

- `ConversationKind::parse` 表驱动（合法 4 + ≥2 非法）。
- `sessions` 仓储：新字段 round-trip；`list_by_kind` 排序。
- `attachments` 仓储：插入/按会话列出/去重（同 sha 复用路径）。
- `conversation_service::resolve_agent`：优先级链 5 档 + 无效 id 回退。
- `conversation_service::compose_user_message`：文本/图片/二进制/MIME 降级。
- `scheduler_service`：`target_kind` 三值各自产物（task/chat/group）；`session_mode=reuse` 复用同一 session；`@every`/cron 边界。
- `whiteboard::record_turn` publish `session.message`（断言 bus 收到且含 `role_name`）。
- `transition_run_with_detail` publish（断言 topic/payload）。
- 沙箱：附件路径逃逸被拒（`workspace_escape_denied`）。
- 状态机：`heartbeat`/`OrphanTimeout`/`Requeue` 表驱动（补齐未覆盖边）。
- 覆盖率：`domain/` 与 `orchestrator/` ≥ 80%（testing.md）。

### 12.2 前端（vitest + testing-library）

- `registry`：分类过滤、参数模式 `suggest`、`parseInput` 边界、`/agent` 参数补全。
- `useComposerTriggers`：`@` 词边界触发、`Esc` 关闭、命名空间切换。
- `useClipboardPaste`：文本直插、大文本转附件、图片转附件、`isComposing` 不拦截（构造 `ClipboardEvent`）。
- `AgentChip`：显示解析名/flavor；切换调用 `set_conversation_agent` 并失效 `["conversation", id]`。
- `AttachmentShelf`：缩略图、删除、失败态、objectURL 清理。
- `GroupConversationView`：发言者渲染、Handoff 边、收敛徽章、环路告警。
- `BackgroundConversationView`：状态条各态、approve/deny、停止。
- `ScheduledConversationView`：cron 人类可读、倒计时、历史条。
- `ConversationsList`：类型过滤 + 徽章 + running 置顶。
- 所有面：loading/empty/error（`AsyncBoundary`）；i18n 键存在性（zh 完整）。
- IPC 一律走 `test-double`，禁止真实 Tauri 运行时（testing.md）。

### 12.3 契约漂移

- `pnpm contracts:check`（在 CI/本地）必须零 diff。
- 新增命令若未同步 `test-double` → 测试失败（故意）。

### 12.4 手工验收脚本（摘）

1. 新建 chat → Composer 左下角显示默认 Agent 名。
2. `/agent` → 选 Claude Code → chip 更新；发消息后气泡无 role 名（单 Agent 不强制显示），列表行徽章变为该 Agent。
3. 输入 `@` → 面板出现「文件/Agent/会话」；选文件 → 附件条出现；发送后 `attachments.seq` 非空。
4. 截图后 Ctrl+V → 图片附件 + 缩略图；粘贴 200 行文本 → 折叠为附件。
5. 新建 group（含 3 成员）→ 发任务 → 发言者气泡轮流出现，Handoff 箭头、Round 进度、黑板坞实时更新；点停止可中断。
6. 新建 background → Run 状态条走 queued→running→succeeded；切到 board 再切回，状态仍实时；全局托盘能停止。
7. 新建 scheduled（`@every 30`，target=chat）→ 倒计时；30s 后自动出现新会话与运行历史卡。
8. 四主题切换，全部可读（对比度测试绿）。

### 12.5 验收标准（AC）

- **AC1** 四种会话类型可创建、可区分、可持久化、重启后仍在。
- **AC2** 当前会话 Agent 在三处（Composer/Header/列表）一致显示；`/agent` 与点击 chip 均可切换。
- **AC3** `@` 支持至少「文件/Agent/会话」三类；附件入库且随消息生效。
- **AC4** 粘贴文本/图片/文件均按规则处理，不破坏原生文本粘贴。
- **AC5** 后台任务脱离导航继续执行，全局托盘可见可控。
- **AC6** 群聊多种发言者实时渲染，含 Handoff/Round/黑板/收敛/环路。
- **AC7** 定时任务可自动派发并产生可回看的会话/运行历史。
- **AC8** 领域事件实时性修复后，Board/Scheduler/Approvals 的既有订阅真正生效（回归）。
- **AC9** 门禁全绿：`pnpm typecheck && pnpm lint && pnpm test`、`cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test`、`pnpm contracts:check`。
- **AC10** 无新增 `any`/`unwrap`/`@ts-ignore`；无魔法色；i18n zh/en 同步。

---

## 13. 任务拆分与建议顺序

> Owner 依据 AGENTS.md §8。跨层契约变更由 conductor 协调；**同一文件禁止两名 agent 同时改**。

### P0 地基（必须先做，否则后面对话类型「不刷新/不并发」）
| # | 任务 | Owner | 依赖 |
|---|---|---|---|
| P0.1 | 迁移 0011/0012/0013 + `migrations.rs` 注册 | core-engineer | — |
| P0.2 | `ConversationKind` / `AgentRefKind` / Session+Schedule 扩展 + 构造器 | core-engineer | P0.1 |
| P0.3 | `conversation_service`（新建/绑定/解析/组装消息） | core-engineer | P0.2 |
| P0.4 | 实时地基：CDC tail loop + 关键 topic publish（§5） | bridge-engineer + core-engineer | — |
| P0.5 | IPC 命令/DTO + contracts:gen + test-double | bridge-engineer | P0.3/P0.4 |
| P0.6 | D1 `run_task_in_session`（若选 A） | core-engineer | P0.2 |
| P0.7 | 契约漂移测试 + 门禁跑通 | qa-guardian | P0.5 |

### P1 Composer（R1/R2/R3）
| # | 任务 | Owner |
|---|---|---|
| P1.1 | registry 升级（分类/参数/动态源）+ 命令模块拆分 | ui-engineer |
| P1.2 | `Composer` + `CompletionMenu`（`/` 与 `@` 共用）+ 键盘/无障碍 | ui-engineer |
| P1.3 | `AgentChip` + `AgentPickerPopover` + `useComposerTriggers` | ui-engineer |
| P1.4 | `AttachmentShelf` + `useClipboardPaste` + 拖拽 + 文件选择 | ui-engineer |
| P1.5 | 后端附件命令 + 沙箱 + 注入 | bridge-engineer + core-engineer |
| P1.6 | 组件测试 | qa-guardian |

### P2 会话类型 UI（R4/R5）
| # | 任务 | Owner | 依赖 |
|---|---|---|---|
| P2.1 | `ConversationView` 分派 + `ConversationHeader` + `ConversationsList` | ui-engineer | P1.2 |
| P2.2 | `NewConversationMenu/Dialog` + 类型向导 | ui-engineer | P2.1 |
| P2.3 | group UI（SpeakerBubble/HandoffRibbon/WhiteboardDock/Round/Controls） | ui-engineer | P2.1；P0.4（发言实时） |
| P2.4 | background UI（RunStatusBar/RunTimeline）+ 全局 BackgroundTray | ui-engineer | P2.1；P0.5 |
| P2.5 | scheduled UI（RuleCard/Countdown/RunHistory）+ Scheduler 页升级 | ui-engineer | P2.1；P0.3 |
| P2.6 | 调度派发桥 + 会话化 tick | core-engineer + bridge-engineer | P2.5 |
| P2.7 | 类型专属测试 | qa-guardian | P2.3/P2.4/P2.5 |

### P3 打磨与增强
- 心跳写入 + 孤儿扫描 + requeue（P2 语义落地）。
- `.nuomi/attachments` GC 保留期。
- Markdown 气泡（M2，单列 spec）。
- 三栏布局 ADR。
- `principle.md` 追加「对话类型与 Composer 实现原理」（AGENTS.md §7.8）。

---

## 14. 风险、权衡与未决问题

| 风险/问题 | 影响 | 缓解 |
|---|---|---|
| D1 改动 core facade 并发模型 | 高（会话隔离回归面大） | 先实现 A 的最小闭环 + 会话隔离测试；用 `maxConcurrency` 限流；B 作为降级 |
| 领域事件量大（CDC 全表轮询） | 中（CPU/唤醒） | 200/批 + 空页退避 500ms + 只映射白名单表；必要时改 SQLite `update_hook` 或共享内存通知 |
| 附件 base64 走 JSON | 中（大文件内存） | 25 MB 上限 + M2 分片 Channel；图片先压缩可选 |
| 群聊 role 颜色与对比度 | 低（测试门禁） | 只用既有语义色 + hash 到有限色环；改色必更 `themeContrast.test.ts` |
| 会话类型语义蔓延 | 中（范围失控） | §11.3 明确 Non-goals；新类型须先更新本方案 |
| 迁移 `0009` 空缺 | 低 | 沿用 runner 跳过；不补历史版本（append-only） |
| 单文件 `commands.rs` 已很大（>3000 行） | 中（维护） | 新命令实现放 `commands/conversation.rs`、`commands/attachments.rs` 子模块，`mod.rs` 汇总 |

**未决（需用户/评审拍板）**
1. D1 是否立即采用方案 A（并发），还是先 B 降级？建议 A。
2. 图片附件在**非多模态** provider 下：失败提示 vs 静默降级为路径引用？（建议：降级 + 明确提示）
3. 定时任务默认 `auto_dispatch` 开还是关？（建议：开，但首次创建时显式确认）
4. 群聊页与 Trace 页是否合并入口？（建议：保留两个入口，共享解析层）

---

## 15. 附录

### A. 命令清单全表（M1 目标）

`/agent` `/attach` `/clear` `/new [kind]` `/sessions [kind]` `/stop` `/background` `/group` `/schedule` `/workspace` `/theme` `/help`（`/plugin` 预留）。

### B. i18n 键草案（zh-CN / en 同步）

```
composer.agentChipLabel = "当前 Agent"
composer.attach = "添加附件"
composer.pastedImage = "已粘贴图片"
composer.foldedText = "已折叠为附件（{{size}}）"
agent.switch = "切换 Agent"
agent.setDefault = "设为默认"
agent.boundNotice = "已切换，下条消息生效"
group.round = "第 {{n}}/{{max}} 轮"
group.converged = "已收敛"
group.stop = "停止群聊"
group.whiteboard = "共享黑板"
background.status_queued|running|... 
background.stop = "停止"
background.retry = "重试"
background.tray = "后台任务"
scheduled.nextRun = "下次触发：{{time}}"
scheduled.reuseSession = "复用同一会话"
attachment.tooLarge = "附件过大（上限 {{max}}）"
errors.conversation_not_found 等（由 describeError 自动映射）
```

### C. 事件 topic 表（本方案后）

| topic | 通道 | 来源 | 触发 |
|---|---|---|---|
| `session.delta` | session | facade | 流式增量（既有） |
| `session.message` | session | facade / **whiteboard.record_turn（新）** | 落库消息 |
| `tool.call` / `tool.result` | session | plugins/tools | 工具 |
| `session.whiteboard` | session | whiteboard | 黑板 note（既有） |
| `run.state_changed` | domain | **transition_run*（新）** | 状态机迁移 |
| `task.created` / `task.status_changed` / `task.deleted` | domain | **append_domain_event + bus（新）** | 任务 |
| `schedule.triggered` | domain | **scheduler（新）** | 触发 |
| `team.formed` | domain | team_former | 组队（既有） |
| `conversation.updated` | domain | **conversation_service（新）** | Agent/绑定变更 |
| `change.<table>` | domain | **CDC tail（新）** | 行级变更兜底 |

### D. 术语表

- **Conversation 对话**：`Session` + `kind` + 绑定的统一称呼（UI 概念）。
- **AgentRef**：`{kind: cli|role, id}`，指向 `agent_profiles` 或 `roles`。
- **Session 会话**：持久化转录实体（既有）。
- **Run 运行**：一次执行的状态机实例（既有）。
- **Round 轮次**：群聊中一轮完整发言循环。
- **Handoff**：群聊中把发言权显式交给下一成员的工具调用。

---

> 评审通过后：拆分 `docs/specs/conversation-ux-m1.md`（正式 SPEC）+ 4 篇 ADR（D1 并发、D2 实时、D3 附件、D4 Markdown 延后），并按 §13 派单。
