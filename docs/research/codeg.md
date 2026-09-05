# codeg — 组件级研究与可吸收点

## 1. 定位与架构骨架
Tauri 2 多智能体编码工作台：用 ACP 协议（sacp 11.0）统一接入 15+ CLI Agent（Claude Code/Codex/Gemini 等），核心是异步委托 Broker + Git Worktree 任务隔离。单一 Rust 库经 feature flags 编译三个二进制：桌面 `codeg` / 无头 `codeg-server`（Axum+WS）/ MCP 伴生 `codeg-mcp`（`src-tauri/Cargo.toml:40-53`）。关键目录：`src-tauri/src/acp/`（连接+委托，35 文件）、`work_task/`（任务引擎+git）、`web/`（事件桥）、`parsers/`（各 Agent 会话逆向解析）。

## 2. 差异化机制
- **ACP 统一接入 + 空闲回收**：`acp/registry.rs:226` 内置 Agent 注册（npx/binary/uvx 分发），`acp/connection.rs`（19K 行）管生命周期；`acp/idle_sweep.rs:19,23` 60s 扫描、180s 无活动断连。解决 N 个 CLI 各有私协议的接入碎片化。代价：connection.rs 巨石化，Agent 方言差异仍需特判。
- **非阻塞委托 Broker**：`acp/delegation/broker.rs:2131` `start_delegation` 立即返回 task_id；`:3196` `get_task_status` long-poll（StatusWait 挂起等待，完成即唤醒）；`:3029,3054` `cancel_by_parent(_turn)` 取消级联；`:148-168` depth_limit（默认 1）防递归爆炸；`:2751` `complete_call` 写 FIFO 完成缓存。父 Agent 不被子任务阻塞，LLM 用 MCP 工具轮询。代价：broker.rs 8.5K 行、大量竞态防御（PreCanceled、ToolCallTracker），状态机复杂度极高。
- **MCP 伴生进程**：`acp/delegation/companion.rs` — codeg-mcp 随 Agent 启动注入，stdio 收 MCP 调用，经 UDS/named pipe + token 认证（`:159,221`）转发给 Broker。让"任何 CLI Agent"无需改造即获得委托能力。代价：进程编排+传输层+token 生命周期。
- **run_seq 代际 CAS**：`work_task/engine.rs:7-8` 事件按 `(connection_id, run_seq)` 匹配、CAS 结算；`:1765-1773` 取消只作用于当代。解决取消/重启与迟到事件的竞态。代价：几乎零，纯编号纪律。
- **Worktree 隔离 + 两阶段合并 + 意图持久化**：`work_task/git.rs` Stage A 将 base 合入 worktree（冲突必落 worktree，`merge_base_into_worktree`），Stage B 在项目目录 git mutex 下落回 base；合并意图先持久化到任务行（`engine.rs:2812-2826` queue_merge），崩溃后 `recover_merging`（`engine.rs:4436`）从 git 真相重放结算（`settle_merge_generation` :2931：landed ⟺ base HEAD 含该提交）。多任务并行互不干扰且崩溃安全。代价：git CLI 包装细节多（scratch index、detached 防御）。
- **双事件总线**：进程内类型化 `InternalEventBus`（`acp/internal_bus.rs:41`，broadcast `Arc<EventEnvelope>`，容量 4096）与 WS 侧 `WebEventBroadcaster`（`web/event_bridge.rs:28`，`Arc<serde_json::Value>`）分离，`EventEmitter` 枚举（`event_bridge.rs:90`）统一 Tauri/Web/Noop 三出口。动机（internal_bus.rs:8-20）：消除每订阅者 JSON 反序列化 + 前端去重。代价：低，但两总线发送顺序需对齐。
- **三二进制统一**：feature flag `tauri-runtime` + `_core` 后缀函数模式，业务逻辑零重复。代价：条件编译遍布，`cfg_attr(tauri::command)` 约定需纪律。

## 3. 组件评分表
| 维度 | 分 | 理由 |
|---|---|---|
| 编排 | 5 | 委托 Broker 竞态处理教科书级，但无 Pipeline/Router 拓扑 |
| 沙箱 | 4 | Worktree 隔离扎实，无容器级隔离 |
| 持久化 | 4 | 意图持久化+git 真相恢复优秀；SeaORM 常规 CRUD，无事件溯源纪律 |
| 扩展性 | 4 | ConnectionSpawner trait（spawner.rs:52）预留远程 Agent，三二进制灵活 |
| 上下文 | 3 | 19 个解析器逆向私有会话存储，广但脆，无长期记忆 |
| 路由 | 2 | 仅扁平委托，无角色路由/群聊 |
| 可观测 | 4 | EventBusMetrics 精细（lagged/replay/snapshot_fallback），transcript append-only |

## 4. 可吸收清单
| 机制 | 对 nuomi 的收益 | 难度 | 实现要点 | 冲突点 |
|---|---|---|---|---|
| 非阻塞委托+long-poll+取消级联+depth_limit | 直击"CLI 与原生 Agent 协作"创新点；群聊/Run 内子任务不阻塞父循环 | 高 | broker 收敛为独立模块；MCP 工具暴露 delegate/status/cancel；long-poll 用 tokio::sync::Notify | nuomi 无 MCP 伴生进程形态；需定义 Run 与委托任务的生命周期归属 |
| run_seq 代际 CAS | Run resume/cancel 与迟到事件的竞态防护，成本低收益高 | 低 | Run 行加 run_seq，事件结算按 (run_id, run_seq) CAS，状态机迁移前置校验 | 与"先落 state_changed 事件"铁律兼容，需统一进 EventRecord |
| Worktree 隔离+两阶段合并+意图持久化 | 多 Agent 并行任务文件隔离；git_service 增强；崩溃恢复有 git 真相兜底 | 中 | git.rs 思路：Stage A 冲突留 worktree；合并意图写 SQLite 再执行；recover 从 HEAD 重放 | nuomi 现为单工作区运行，需引入任务→worktree 映射与清理策略 |
| ACP 统一接入 | 替代/补充 adapters/cli.rs 白名单方言，接入面从 N 方言收敛为 1 协议+注册表 | 中高 | registry 元数据（分发方式/版本/能力标签）映射到 AgentProfile；sacp crate 或自实现 JSON-RPC | AgentAdapter trait 需新增 acp 方言；现有解析器路径保留为降级 |
| 双事件总线分离 | Tauri 壳性能：内核消费者拿类型化事件，IPC 只走 JSON 一条路 | 低 | nuomi-core bus 出类型化订阅；Tauri emit 层单独序列化一次复用 Arc<Value> | 与 tauri-specta 契约需对齐事件 envelope 类型 |

## 5. 明确不建议吸收的部分
- **19 个私有会话解析器**（parsers/*.rs）：逆向各 CLI 私有存储格式，vendor 每次升级即碎，维护黑洞；nuomi 用 ACP/适配器直连后无需此路径。
- **connection.rs 单体 19K 行**：所有连接逻辑塞一处的反模式，nuomi 应按 kernel 插件拆分吸收其机制而非其组织。
- **supervise PID 1 / 自更新 / 升级回滚**：服务器产品化才需要，nuomi 当前 CLI+桌面定位用不上。
- **聊天频道/办公文档/10 语言 i18n**：产品面功能，与内核无关（飞书 webhook nuomi 已有 M-BOT1）。
