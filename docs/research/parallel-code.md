# parallel-code — 组件级研究与可吸收点

> 基于 johannesjo/parallel-code v1.14.5 源码验证（前人笔记 `references-analysis.md` 作地图，本文所有 file:line 均已在源码中核实）。

## 1. 定位与架构骨架

Electron 40 + SolidJS 桌面应用：在多个 **git worktree** 中并行驱动 6+ 种 AI CLI agent（Claude Code/Codex/Gemini/Copilot/OpenCode），核心创新是 **MCP 协调器模式**——让一个协调器 agent 通过 MCP 工具自动编排并行子任务。无独立后端服务，协调器、PTY、git 全在 Electron 主进程内存态。

关键目录：`electron/mcp/`（协调器核心，coordinator.ts 2561 行）、`electron/ipc/`（pty.ts 1132 / git.ts 2293）、`electron/remote/`（HTTP/WS 监控）、`src/store/coordinator-preamble.ts`（调度 prompt）。

## 2. 差异化机制

### 2.1 MCP 协调器：任务自动编排成多并行子任务
- **位置**: `electron/mcp/coordinator.ts:782`（createTask：worktree+分支 → preamble 注入 → per-task MCP 配置 → spawn → 提示符检测后投递 prompt）；MCP 工具权限分离 `mcp/mcp-tool-list.ts`（协调器 vs 子任务各见不同工具集）。
- **解决**: 把"一个大任务拆成 N 个并行 PR"从人工操作变成 agent 自主循环。跨 CLI 统一靠 MCP stdio + per-agent 启动参数适配（`mcp/agent-args.ts`）。
- **代价**: 状态全在内存 Map（coordinator.ts:143），协调器崩溃依赖 hydrateTask（:1879）重建，复杂度高。

### 2.2 滑动窗口调度——纯 prompt 工程
- **位置**: `src/store/coordinator-preamble.ts:40-72`：backlog/inFlight/landed/blocked 四态、`{{MAX_CONCURRENT}}` 上限、10s 最小轮询间隔、"spawn 替补立即补位"规则；硬性并发上限不存在于代码，**全靠 LLM 遵守系统提示词**。
- **解决**: 限制并发爆炸，且不写调度器代码。
- **边界**: 提示词约束可被违反；上下文随轮询膨胀。nuomi 应在 orchestrator 侧加硬闸门，此 prompt 仅作补充纪律。

### 2.3 自我着陆（self-landing）+ 结构化验证
- **位置**: `coordinator.ts:1586` landSelf：校验 `verification.checks` 全 passed（:110 isPassedVerification）→ 清理注入的 preamble（stripPreambleFromBranch）→ git merge → 清 worktree。五态落地状态机 `mcp/types.ts:113`（landing_escalated/failed/pending_review/cleanup_failed/reviewed）；失败可升级为协调器 merge_task 逃生门（:1748 assertTaskCanBeMerged）。
- **解决**: 子任务自验自合，协调器只处理异常，通知量与 token 大降。
- **批判**: verification 是子任务**自报**结果，后端不重跑命令——诚实但可伪造；适合信任内环境。

### 2.4 通知批处理 + 幂等信号等待
- **位置**: stageBatch `coordinator.ts:699`（有活跃 wait waiter 时抑制通知防打断；非零退出缩短延迟 :725）；`wait_for_signal_done` :2401 + `mcp/replay-cache.ts:7`（requestId 幂等缓存，TTL 120s，防 HTTP 重试导致信号重复消费）。
- **解决**: 多子任务完成时合并通知 + 网络重试不重复消费，长轮询模型的关键可靠性设计。

### 2.5 PTY 提示符检测与程序化 prompt 投递
- **位置**: `electron/shared/prompt-detect.ts`（ANSI 剥离 + per-agent 模式）；双重确认 `coordinator.ts:325`（markAgentPromptReady，50ms 稳定期）；写入 :1127（bracketed-paste 自动探测 :297，按行数动态延迟 ：93，body/Enter 分相错误恢复）。echo 抑制 ：364 防止自己的 prompt 被误判为 idle。
- **解决**: **程序化驱动交互式 CLI agent** 的根本难题——这是"CLI 与原生 Agent 沟通协作"的工程答案，与 nuomi 的创新点直接对应。
- **代价**: 模式匹配脆弱，agent CLI 换版本即需更新 pattern。

### 2.6 Git worktree 文件系统级隔离
- **位置**: `electron/ipc/git.ts:887` createWorktree；symlink node_modules 等缓存目录 :136/:958；`.git/info/exclude` 注册 :1117；`.claude/` 必须真目录（bwrap 不接受 symlink）:992；`withWorktreeLock` :118 串行化同 worktree 并发 git 操作。
- **解决**: 零冲突真并行；symlink 省去重复装依赖。
- **边界**: Windows symlink 需特权；仅适用于 repo 型任务。

### 2.7 Diff 基线智能选择
- **位置**: `git.ts:372` pickMergeBase（本地 vs origin 两个 merge-base，祖先判断取更近者）；`git.ts:445` refineDiffBaseWithCherryPick（`--cherry-pick --right-only` patch-id 检测：全合并→折叠空 diff、唯一提交连续→精确到最老唯一提交之父、交错→保留基线）；TTL 缓存（主分支 60s / diff 基线 30s）。
- **解决**: rebase 后 diff 出现大量 patch-equivalent 噪声的经典问题。
- **代价**: 三种情况分支逻辑 + 多次 git 调用，但都失败安全（回退原基线）。

### 2.8 多层安全
- **位置**: IPC 白名单 `electron/preload.cjs:5`（~140 通道字面量）；ENV_BLOCK_LIST `ipc/pty.ts:83`（35+ 变量，每条有威胁模型注释：LD_PRELOAD/GIT_CONFIG_COUNT/NODE_EXTRA_CA_CERTS 等，防 rc 文件注入与 API 流量 MITM）；Docker 资源限制 `pty.ts:334`（--memory 8g --pids-limit 512）+ per-task 独立 HOME :349；per-task doneToken `mcp/config.ts:60` + MCP 配置路径白名单 config.ts:87；token 四分类（coordinator/subtask/mobile/paired）timing-safe 比较 `remote/server.ts:708-713`；分支名保守校验 `mcp/validation.ts:4`。
- **解决**: agent 是"会执行任意代码的不可信进程"——纵深防御。
- **批判**: `--network host`（pty.ts:332）放弃网络隔离，是显著弱点。

### 2.9 原子文件写入
- **位置**: `mcp/atomic.ts:74`：temp 文件 + fsync + rename + **目录 fsync** + umask 修正保留原 mode；写队列序列化同路径并发（coordinator.ts:177 preambleWriteQueue）。
- **解决**: 并发写 AGENTS.md/MCP 配置的撕裂。

### 2.10 内置 HTTP/WS 远程监控
- **位置**: `electron/remote/server.ts`（REST + WS 推送 + ring buffer）；PIN 配对 ：44-45（5min TTL、5 次尝试上限）换 paired token 才能建任务；token 分级最小权限。
- **解决**: 手机远程盯多 agent 进度、远程派任务。

## 3. 组件评分表

| 维度 | 分 | 理由 |
|---|---|---|
| 编排 | 5 | 滑动窗口+自我着陆+幂等信号+逃生门，多 agent 编排闭环最完整 |
| 沙箱 | 4 | worktree+Docker+env 过滤纵深强，但 --network host 破功 |
| 持久化 | 2 | 内存 Map + JSON 文件，hydrate 恢复复杂且无事件溯源 |
| 扩展性 | 4 | agent-args/preamble/mcp-launch 三层适配 6+ CLI，新 CLI 成本低 |
| 上下文 | 3 | preamble 注入+diff 50KB 截断，无记忆/压缩/缓存命中意识 |
| 路由 | 2 | 无 router/pipeline 抽象，协调器即路由 |
| 可观测 | 4 | PTY 全流+MCP ring buffer+WS 推送，但无结构化事件库 |

## 4. 可吸收清单

| 机制 | 对 nuomi 的收益 | 难度 | 实现要点 | 冲突点 |
|---|---|---|---|---|
| self-landing + 结构化验证 | Team Run 的"自验自合"完成协议，减协调轮次 | 中 | landSelf 五态状态机进 `domain/run_state.rs`；验证 checks 进 EventRecord | 与 approval_gate 对齐：landing_escalated ≈ awaiting_approval |
| 滑动窗口 prompt + 通知批处理 | group_chat/selector 的调度纪律模板 | 低 | 改写为 nuomi 系统提示词插件；批处理挂 Bot sink | nuomi 走代码侧编排，需在 orchestrator 加硬并发闸门兜底 |
| ReplayCache 幂等信号 | run 信号/审批回调防重复消费 | 低 | requestId+TTL 缓存，置于 event bus 消费侧 | 无；补 SQLite 持久化更好 |
| PTY 提示符检测+prompt 投递 | adapters/cli.rs 驱动交互式 CLI 的核心，直击第三创新点 | 中 | ANSI 剥离+双重确认+bracketed paste+echo 抑制，移植为 Rust | nuomi CLI adapter 现为一次性进程模型，需加 PTY 长会话态 |
| diff 基线 cherry-pick 精化 | git_service.rs 的 diff 去噪 | 中 | patch-id 用 `git log --cherry-pick` 移植，失败安全回退 | 无 |
| ENV_BLOCK_LIST | CLI agent spawn 环境消毒 | 低 | 静态 Set + 每条威胁注释进代码 | 无 |
| worktree 隔离 | 并行 Team 的文件系统隔离选项 | 中 | workspace.rs 加 worktree 模式；Windows symlink 需降级复制 | nuomi 是通用 Harness，worktree 仅对 repo 型任务有意义，做成可选插件 |
| doneToken per-task | 子任务回调最小权限 | 低 | Run 级一次性 token | nuomi 本地单机，优先级低 |

## 5. 明确不建议吸收

- **Electron preload IPC 白名单**：nuomi 用 tauri-specta 生成类型安全 bindings，白名单机制已被覆盖。
- **远程监控 HTTP/WS+QR 配对**：M-BOT1 飞书 webhook 已覆盖通知场景；自建移动端投入大收益小，除非未来产品需要。
- **JSON 状态持久化 + hydrate 恢复**：nuomi 的 SQLite WAL + append-only EventRecord + 状态机铁律（先落库后副作用）严格更强，恰是 parallel-code 的最大短板（评分 2），无需反向学习。
- **前端布局/主题/键盘导航**：技术栈不兼容，无架构参考价值。
- **协调器 prompt 作为唯一调度器**：违反 nuomi"逻辑在 orchestrator 不在 prompt"的边界规则，只可借其措辞，不可借其架构。
