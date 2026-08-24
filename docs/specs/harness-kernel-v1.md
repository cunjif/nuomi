# SPEC: Nuomi Harness 内核 v1（harness-kernel-v1）

> 状态：已批准（v1.1，含 Selector+Handoff 修订）
> 需求来源：`pr.md`（需求权威）；本 SPEC 与 `AGENTS.md` 冲突处按 pr.md 修订 AGENTS.md。

## 0. 决策记录摘要（来自 Interview）

| # | 决策 |
|---|---|
| D1 | `pr.md` 是需求权威；`AGENTS.md` 与其冲突处按 pr.md 修订（本 SPEC 含该修订任务） |
| D2 | Cordis 仅**借鉴理念**，Rust 自研插件内核（不引入 Node 运行时） |
| D3 | MVP = **Harness 内核优先**，验收形态为 cargo test + headless CLI |
| D4 | Master-Slave Provider：**路由+故障降级** 与 **子 Provider 即工具** 两种策略都支持 |
| D5 | CLI Agent（Claude Code/Codex 等）v1 **只留 `AgentAdapter` 接口占位**，不实现具体接入 |
| D6' | MultiAgent v1：串行 Pipeline、路由分发 Router、**群聊 = Selector 选人 + Handoff/Swarm 交接**（Round-Robin 仅作为降级基线与测试基准） |
| D7 | Role + Team 做**可运行**数据模型（Pipeline 可由 Team 驱动） |
| D8 | WhiteBoard = SQLite 结构化共享黑板（append-only 笔记 + 槽位） |
| D9 | 插件化核心 5 件：**Loop Engine、SystemPrompt、Memory、MCP、Hook**；ReAct 循环内置于 Loop Engine 首个实现 |
| D10 | Memory 用 SQLite 起步（关键词/结构化检索），sqlite-vec 向量检索后置 |
| D11 | Self-Evolution 借鉴 GEPA 思想自研；联网学习=白名单源抓取，产出报告，用户**永久授权后**全自动后台融合 |
| D12 | MCP 支持 **stdio + Streamable HTTP** |
| D13 | headless CLI：`nuomi run "<task>"` 单发 + REPL 双模式，会话可续传 |

## 1. 目标 / Goals

1. 构建 Rust 实现的插件化 Agent Harness 内核：所有 Harness 组件（Loop Engine、SystemPrompt、Memory、MCP、Hook）实现为统一 Plugin trait 体系之上的插件，借鉴 Cordis 的 Context/Service/生命周期模型。
2. Provider 层支持主流 Provider 及 OpenAICompatible / AnthropicCompatible 协议；支持 Master-Slave 配置——主 Provider 可按能力路由调用子 Provider（含失败自动降级 fallback），也可将子 Provider 封装为 tool 由主模型主动委派。
3. 可运行的 Role / Team / 多 Agent 编排：串行 Pipeline、路由分发 Router、以及群聊式协作——采用 **Selector + Handoff/Swarm 双机制路由**：当前发言 Agent 可显式 Handoff 指定下一发言者（Swarm 模式）；无显式交接时由 Selector 依据「角色相关性 × 话题匹配度 × 发言新鲜度（避免连续独占）」的接近人类讨论启发式加权选人。Selector 优先用主 Provider 的 LLM 判断，可插拔替换为自研启发式实现。
4. Long-term Memory（SQLite 存储、事件溯源 append-only）与 Self-Evolution 引擎：综合用户画像、长期记忆、历史轨迹与当前轨迹，迭代进化 SystemPrompt/策略版本；支持白名单源定期联网调研并经授权合入。
5. headless CLI（单发执行 + REPL 会话续传）作为内核的可运行验收入口。
6. 同步修订 `AGENTS.md` 使其与 pr.md 对齐（领域模型扩展 Provider/Role/Team/Harness 概念，里程碑重排为内核优先）。

## 2. 用户故事 / User Stories

- **US1** 作为开发者，我配置一个 OpenAICompatible Provider（base URL + key），在终端 `nuomi run "重构此模块"`，看到流式 ReAct 循环（thought/tool_call/tool_result）直到产出结果。
- **US2** 作为开发者，我配置一个主 Provider + 两个子 Provider（不同能力标签），发出复杂任务后主 Provider 自动把子任务路由给匹配的子 Provider；某子 Provider 失败时自动降级到备选。
- **US3** 作为开发者，我把子 Provider 注册为工具，主模型在对话中通过 function calling 主动委派它完成子任务。
- **US4** 作为开发者，我定义多个 Role 组成 Team 并选择「群聊」模式发起任务；Agent 间通过 Handoff 显式交接任务控制权，或在无人交接时由 Selector 自动选出最合适的下一位发言者，直至收敛产出结论。
- **US5** 作为开发者，我在 REPL 中中断会话后重新打开，`nuomi resume <session>` 续传上下文继续工作。
- **US6** 作为开发者，我给 Hook 插件注册 `on_tool_call` 钩子拦截/审计工具调用；给 Memory 插件写入的长期记忆会在后续新会话中被检索注入上下文。
- **US7** 作为开发者，我开启 Evolution：系统聚合我的历史轨迹与记忆做反思性总结，生成新版 SystemPrompt 候选并记录版本差异；我授权后，后台定期抓取白名单参考项目并产出演进摘要报告供合入。
- **US8** 作为开发者，我配置 stdio 或 HTTP MCP server，其 tools 自动出现在 Agent 可用工具集中。

## 3. 验收标准 / Acceptance Criteria

**内核架构**
- [ ] AC1 存在统一的 `Plugin` trait（注册、生命周期 init/start/dispose、事件订阅），Loop Engine/SystemPrompt/Memory/MCP/Hook 均为插件；新增组件无需修改内核调度代码。
- [ ] AC2 内核事件总线贯穿全部组件；每次状态/阶段变化产生 append-only EventRecord（先落库后副作用——沿用现有铁律）。
- [ ] AC3 `cargo clippy --all-targets -- -D warnings` 零警告；库路径零 `unwrap()/expect()`；domain/orchestrator 相关模块行覆盖 ≥ 80%。

**Provider**
- [ ] AC4 OpenAICompatible 与 AnthropicCompatible 两协议客户端可通过集成测试（mock HTTP，无真实网络）；流式输出解析正确。
- [ ] AC5 表驱动测试覆盖：能力路由选择、fallback 顺序、子 Provider 即 tool 的委派回路。
- [ ] AC6 API key 经 OS keyring 存储，日志 INFO 以上无密钥与完整 prompt 正文。

**多 Agent**
- [ ] AC7 fake adapter 端到端集成测试：串行 Pipeline（Team 驱动）入队→逐 Agent 执行→WhiteBoard 读写的消息可见→汇总终态落库。
- [ ] AC8 Router 测试：按能力标签/角色匹配选择 Agent；无匹配时有明确错误路径。
- [ ] AC9a **Handoff 测试**：Agent A 显式交接给 B 后控制权转移、WhiteBoard 与消息上下文随行；handoff 目标不存在/非法时有明确错误路径。
- [ ] AC9b **Selector 测试**：fake judge 下选人结果确定可复现；加权因子（相关性/话题/新鲜度）各有独立单测；同一 Agent 连续发言次数受上限约束。
- [ ] AC9c **终止性测试**：最大轮数上限 + 收敛条件 + handoff 环检测（A→B→A 循环超限强制收敛或报错），保证群聊必然终止。
- [ ] AC9d WhiteBoard 为 SQLite append-only 结构，事件同步到各 Agent 上下文。
- [ ] AC9e Round-Robin 作为降级模式保留（Provider 不可用时），并有对照测试。
- [ ] AC10 CLI Adapter 接口（`AgentAdapter` trait）已定义并有 fake 实现与编译期占位（Claude Code/Codex 等 TODO 注释占位）。

**Memory / Evolution**
- [ ] AC11 Memory 插件：写入→按关键词/结构化条件检索→注入新会话上下文，全链路测试；events 表只追加（禁止 UPDATE/DELETE 由约束保证）。
- [ ] AC12 Evolution：给定 ≥2 条历史轨迹 fixture，能产出带 diff 的新 SystemPrompt 候选版本（版本化存储，不直接覆盖生产 prompt）；候选需显式激活。
- [ ] AC13 联网学习：仅访问白名单域名；未获永久授权时不产生任何网络请求；授权开关持久化；抓取产物为报告条目而非直接改写 prompt。
- [ ] AC14 用户画像与跨会话记忆参与 evolution 输入（非仅当前会话），有对应单测。

**MCP / Hook**
- [ ] AC15 stdio MCP server 启动、tools/list 解析、tool 调用往返集成测试（用内置测试 server 进程）；HTTP transport 同等覆盖。
- [ ] AC16 Hook 在 `pre_tool_call` 可拒绝工具调用并有审计 EventRecord。

**CLI 验收入口**
- [ ] AC17 `nuomi run "<task>"` 流式输出到终端并以退出码反映成败；`nuomi resume <session-id>` 续传历史上下文；REPL 支持多轮对话与 `/` 命令（至少 `/new` `/sessions` `/exit`）。
- [ ] AC18 子进程/任务取消时资源被回收（kill on drop/cancel）。

**文档**
- [ ] AC19 `AGENTS.md` 已修订（Provider/Role/Team/Harness 领域概念 + 内核优先里程碑），冲突消除；本 SPEC 存于 `docs/specs/`。

## 4. 非目标 / Non-goals

**项目级后置**（后续独立 SPEC）：Telemetry 上报、飞书 Bot、QQ Bot。

**本期范围外但保留在路线图**（不在本 SPEC 任务拆分，随 UI 里程碑回归）：Board 任务看板、Approvals 审批门、Scheduler 定时任务、全部 Tauri 图形 UI（三栏界面/文件树/SubAgent Trace）、sqlite-vec 向量检索、ReAct 以外的推理范式、CLI Agent 具体接入实现、Skills/Command/Plugin-market 插件、GEPA 论文严格复现（Pareto 前沿 + LLM-as-judge）。

## 5. 技术约束 / Technical Constraints

- 锁定栈不变：Tauri 2 + Rust(edition 2021) + tokio + SQLite(rusqlite+WAL) + pnpm/React18/TS strict（UI 后置但栈保留）。Cordis 借鉴自研属架构决策，见 ADR `docs/adr/0001-rust-plugin-kernel.md`。
- 插件内核放 `src-tauri/src/harness/` 对应的 lib crate 路径（kernel、plugin registry、context、event bus）；Provider 层放 providers 模块；编排放 orchestrator 模块；存储延续 store 模块（编号迁移只增不改）；headless CLI 为 workspace 内新 crate `crates/nuomi-cli`，与 tauri core 共享 lib crate `crates/nuomi-core`。
- 错误处理每模块 thiserror 枚举；SQLite 操作 `spawn_blocking`；共享态优先 channel；取消用 CancellationToken。
- 参数绑定 SQL、append-only events、密钥进 keyring、tracing 按 session_id/team_id 建 span。
- 测试：状态机表驱动、fake provider/adapter、临时目录 SQLite、单测无网络（HTTP mock + 内置 stdio MCP 测试进程除外，后者为本地进程）。
- 质量门：`cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test` 全绿。

## 6. 任务拆分 / Task Breakdown

| # | 任务 | 内容 | 前置 |
|---|---|---|---|
| T0 | ADR + 文档对齐 | `0001-rust-plugin-kernel.md`（Cordis 理念映射、lib crate 抽取）；修订 `AGENTS.md`；落 `.gitignore` | – |
| T1 | 工作区脚手架 | cargo workspace：`crates/nuomi-core`（lib）、`crates/nuomi-cli`（bin）；迁移框架 + `PRAGMA user_version`；错误体系骨架 | T0 |
| T2 | 插件内核 | `Plugin` trait、Context、注册表、事件总线、生命周期管理 + 单测 | T1 |
| T3 | 存储层 | sessions/events/memory/whiteboard/prompt_versions/provider_configs/teams 迁移 SQL + repositories（参数绑定、tempdir 测试） | T1 |
| T4 | Provider 层 | ProviderConfig、OpenAICompatible/AnthropicCompatible 客户端（mock 流式测试）、keyring 集成 | T1 |
| T5 | Master-Slave 编排 | 能力路由 + fallback 链 + 子 Provider-as-tool 委派（表驱动测试） | T4 |
| T6 | Loop Engine + SystemPrompt 插件 | ReAct 循环、上下文组装、动态剪枝接口预留、SystemPrompt 插件 | T2,T3,T4 |
| T7 | Memory + Hook 插件 | 记忆读写/检索注入；Hook 点位（pre/post_tool_call 等）+ 审计 | T6 |
| T8 | MCP 插件 | stdio + Streamable HTTP transport、tools 发现与调用、测试 server | T6 |
| T9 | Role/Team + 三种编排 | Role 覆盖层模型、Team 拓扑配置；Pipeline/Router 执行器；群聊执行器 = Handoff 协议（含环检测与跳数上限）+ 可插拔 Selector trait（默认 LLM 实现 + 启发式自研实现）+ Round-Robin 降级；WhiteBoard 读写协议 | T6 |
| T10 | headless CLI | `nuomi run` / `nuomi resume` / REPL（`/new` `/sessions` `/exit`）、流式终端输出、取消回收 | T6–T9 |
| T11 | Self-Evolution | 轨迹聚合器、反思进化（prompt 候选版本 + diff + 显式激活）、白名单联网调研器 + 永久授权开关、定时任务 | T7,T10 |
| T12 | 收尾验收 | 覆盖率核对（≥80%）、端到端场景串联、AC 清单逐项核验、质量门全绿 | 全部 |
