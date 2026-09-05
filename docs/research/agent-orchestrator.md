# agent-orchestrator — 组件级研究与可吸收点

> Go 1.25 守护进程 + Electron 桌面端，多 coding agent 并行编排平台。源码：`C:\Users\james\Codehub\references\agent-orchestrator`。

## 1. 定位与架构骨架

项目级 AI 编排平台：每项目一个 Orchestrator agent（持久规划）+ N 个 Worker（一会话一 worktree 一 PR）。六边形架构：`backend/internal/ports/`（窄接口）← `adapters/`（26+ agent、tmux/conpty、GitHub SCM）；`lifecycle/` 归约器只持久化事实；`cdc/` + `httpd/` 负责事件与 SSE。核心管道 OBSERVE → UPDATE → DERIVE。

## 2. 差异化机制

**2.1 持久化事实最小主义**：只存 `activity_state`、`is_terminated`、PR 事实，展示状态从不落库。`pkg/contract/status.go:102` `DeriveStatus()` 纯函数：SessionFacts+PRFacts+now → 状态（terminated/merged 优先，再 activity，再 PR 管道，最后 no_signal/idle）。重算时机：每次 API 读取 + CDC 事件推送后前端刷新。收益：状态永不与事实矛盾、零迁移负担；代价：每次读都要 join PR 表，状态逻辑集中在纯函数中且需测试覆盖（`status_test.go` 很厚）。

**2.2 SQLite CDC 管道**：`migrations/0001_init.sql:105-128` —— `change_log` append-only 表 + 每张业务表的 AFTER INSERT/UPDATE 触发器，事件与业务变更同事务原子写入，应用层永不手工发事件。`cdc/poller.go:13` 100ms tail 轮询（batch 512，seq 单调序），重启 `SeekToHead` 不回放历史，客户端用 SSE Last-Event-ID 自行补齐。注意它仍是轮询，只是把"轮询 DB"藏在 100ms + 触发器后面，换来了零遗漏和原子性。

**2.3 保守终止**：`lifecycle/manager.go:372-448` `ApplyRuntimeObservation` —— 探测失败/liveness 歧义一律忽略；终止需 `runtimeClearlyDead` + 近期活动守卫 + 双段 mutate 用 `UpdatedAt` 作乐观锁复核；launch 生成号围栏防旧进程误报。加 SCM 观察（`observe/scm/observer.go:29` 30s 轮询 PR/CI，review 2min）。核心原则"失败探测 ≠ 死亡证明"值得直接抄。

**2.4 端口-适配器 + agentbase**：`ports/agent.go:37-60` 仅 6 个必需方法（launch argv / prompt 投递策略 / hooks 安装 / restore / session info），能力用可选接口扩展（AuthChecker、BinaryResolver、InterfaceHandoff 等）。`agentbase/agentbase.go:38` `Base` 嵌入补默认值，新 agent 只写差异。活动检测靠 `ao hooks <agent> <event>`：适配器向 agent 原生 hook 配置注入回调，`activitydispatch/dispatch.go` 统一分发为 activity state——比轮询终端画面可靠。

**2.5 Orchestrator-Worker 分层**：`domain/session.go:16-22` SessionKind 区分 worker/orchestrator；orchestrator 有项目级持久对话（`domain/conversation.go:28`，outlives 单次会话）+ `projectconfig.go:40` OrchestratorRules 常驻指令，上下文含活跃 worker/PR/CI 实时事实。

**2.6 Code-first API 契约**：`httpd/apispec/` —— Go 内注册 dto 与操作生成 openapi.yaml，`parity_test.go` CI 保证提交的 spec 与生成一致，前端 `openapi-typescript` 生成类型。与 tauri-specta 同思路，但多了"spec drift 测试"这一环。

**2.7 跨平台终端与投递就绪**：`adapters/runtime/{tmux,conptyptyexec,runtimeselect}`；`session_manager/message_delivery.go` `WaitForMessageDeliveryReady`（150ms 轮询 + 750ms 稳定窗 + 5s 兜底）确保注入消息不丢——对 CLI agent 协作（nuomi 第三创新点）高度相关。

## 3. 组件评分表

| 维度 | 分 | 理由 |
|---|---|---|
| 编排 | 5 | Orchestrator-Worker 分层 + 项目级持久上下文，同类少见 |
| 沙箱 | 3 | git worktree 隔离 + 容器 reap，无强沙箱 |
| 持久化 | 5 | 事实最小主义 + 触发器 CDC，教科书级 |
| 扩展性 | 5 | 6 方法窄接口 + Base 默认值，26+ 适配器实证 |
| 上下文 | 4 | orchestrator 项目上下文强，worker 上下文传递较朴素 |
| 路由 | 3 | 无智能路由，PR 反馈回注（mode-aware messenger）是亮点 |
| 可观测 | 5 | CDC→SSE 全链路实时 + Kanban 派生视图 |

## 4. 可吸收清单

| 机制 | 对 nuomi 的收益 | 难度 | 实现要点 | 冲突点 |
|---|---|---|---|---|
| 状态派生纯函数（DeriveStatus） | Run/Worker 看板状态不落库、永不失一致 | 低 | Rust 侧纯函数 `derive_status(facts, now)`，SQL 读时调用；事件日志已具备事实源 | nuomi 已有 state_changed 落库的状态机，需区分"机内状态"与"展示状态"两层 |
| 触发器 CDC → SSE | 替代前端轮询 IPC，实时事件流 | 中 | SQLite AFTER 触发器写 change_log（含 payload JSON），tokio 100ms tail + broadcast；tauri event 或 SSE 推前端 | nuomi EventRecord 面向对话转录，需新增独立 change_log（面向行级变更）；WAL 下触发器开销需测 |
| 保守终止（探测≠死亡） | 崩溃恢复 orphan 判定更稳 | 低 | reaper 仅在"明确死亡+活动超窗+乐观锁复核"三重满足后置 interrupted | 与现有心跳超时机制融合，避免缩短超时 |
| 窄 Agent 端口 + hooks 活动上报 | AgentProfile/CLI 适配器瘦身；活动检测不用截图轮询 | 中 | trait 拆成必需小接口 + 默认实现；为 claude code 等 CLI 注入 hook 回调上报 busy/idle/waiting | adapters/cli.rs 现为单一 trait，需重构；CLI 方言 hook 支持度不齐 |
| Orchestrator-Worker + worktree | 自发组队/多 worker 并行的顶层蓝图 | 高 | 项目级 orchestrator 会话 + 每 worker git worktree + PR 事实回注 | nuomi 群聊（Selector+Handoff）拓扑不同，可作为 Team 拓扑新增项而非替换 |
| 契约 drift 测试 | tauri-specta 已够，补 CI 校验生成物最新 | 低 | CI 跑 `pnpm contracts:gen` 后 git diff --exit-code | 无 |

## 5. 明确不建议吸收

- **Go/Electron 双守护进程 + HTTP loopback + LAN bearer 认证**：nuomi 是 Tauri 进程内内核，IPC 直连，引入 HTTP 层纯属负担。
- **100ms CDC 轮询原样照搬**：Tauri 单机进程内可直接在写路径后广播，触发器 CDC 保留原子性优点即可，poller 可省。
- **PR 管道重度 GitHub 化**（SCM observer、review threads、stack 派生）：nuomi 未承诺 GitHub 工作流，吸收其"PR 事实外置观察"思想即可，不建议引入 GitHub 适配器。
- **controller 代次/接口热切换全套机制**（drain/interrupt、outbox、generation fence）：复杂度极高，只在 TUI↔Chat 双界面共存时有意义，nuomi 无此场景。
