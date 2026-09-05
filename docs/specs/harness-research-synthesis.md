# Harness 框架横向研究综合报告 — nuomi 演进路线

> 汇总 7 份单框架研究（`docs/research/*.md`，全部经源码 file:line 验证）+ `references/references-comparison.md`。
> 生成：2026-09-05。方法：每框架由独立研究 agent 读 `references-analysis.md` 作地图、深挖源码作证据，再批判性评估可吸收性。

## 1. 七框架组件评分总表（1-5）

| 维度 | codeg | parallel-code | agent-orch | deepseek | hermes | pi | prime | nuomi 现状 |
|---|---|---|---|---|---|---|---|---|
| 编排 | 5 | 5 | 5 | 4 | 4 | 2 | 4 | **4**（Pipeline/Router/群聊拓扑最全，缺子任务委托） |
| 沙箱 | 4 | 4 | 3 | 5 | 4 | 3 | 3 | **2**（仅 workspace 约束，无系统级隔离） |
| 持久化 | 4 | 2 | 5 | 5 | 5 | 5 | 4 | **4**（事件溯源+状态机铁律强，缺 CDC/FTS/派生状态） |
| 扩展性 | 4 | 4 | 5 | 5 | 5 | 5 | 5 | **3**（插件内核尚浅：无 effect 可逆、无 waterfall） |
| 上下文 | 3 | 3 | 4 | 4 | 5 | 5 | 4 | **2**（无压缩器、无缓存意识——最大短板） |
| 路由 | 2 | 2 | 3 | 3 | 4 | 2 | 3 | **4**（Master-Slave 独有优势） |
| 可观测 | 4 | 4 | 5 | 4 | 4 | 3 | 4 | **3**（事件落库全，缺实时推送与派生视图） |

**结论**：nuomi 的编排/路由/持久化纪律已是第一梯队；**上下文管理与缓存命中是全场最大空白**，也是 hermes/pi 两家最强的领域——优先补齐即是最大杠杆。扩展性内核（Cordis 反应式）是 deepseek 的护城河，nuomi 已选此路线但只实现了皮层。

## 2. 分维度对比与差距分析

### 2.1 上下文管理（nuomi 最大短板 → P0 主攻）
- **hermes 三段式压缩**（context_compressor.py, 8454 行）：①零 LLM 廉价预裁剪——MD5 去重重复工具输出 + 带语义 1 行摘要（`[terminal] ran npm test -> exit 0`）；②token 预算尾切点——`effective_window=(context_length−max_tokens)×pct`，小窗口抬升至 75-85%，切点永不落进 tool_call/result 组，单调锚定尾部 user/assistant；③结构化模板迭代更新——`PREVIOUS SUMMARY + NEW TURNS`，Goal/Completed/Active State 分区。
- **pi 截断安全**：`stop_reason==length` 时全部 toolCall 标记失败——流式参数可能"通过校验但语义残缺"。
- **nuomi 差距**：无任何压缩/裁剪层；消息直发。

### 2.2 缓存命中（pr.md 创新点 → P0）
- **pi 统一字段**：`CacheRetention("none|short|long")` + `Usage.cacheRead/cacheWrite/cacheWrite1h`；Anthropic 在 system prompt、末位 tool 定义、末条消息打 `cache_control`。
- **hermes 硬约束**：system prompt 会话内字节稳定；Skill 以 user 消息按需注入不进 system；工具集会话内固定；发送前 `repair_message_sequence` 修角色交替；failover 原位改写保持字节结构。
- **hermes cache scope**：压缩轮换 session 后 `prompt_cache_key` 仍指向 compression-lineage 根——牺牲一次性重建但不破坏 bucket 隔离。
- **nuomi 差距**：Usage 无缓存字段、无 alternation repair、无 lineage 概念。

### 2.3 编排与 CLI 协作（nuomi 第三创新点）
- **codeg 非阻塞委托 Broker**：start_delegation 立即返回 task_id、long-poll StatusWait、取消级联、depth_limit 防递归爆炸；MCP 伴生进程让任意 CLI Agent 零改造获得委托能力。
- **parallel-code PTY 提示符检测**：ANSI 剥离 + 双重确认（50ms 稳定期）+ bracketed-paste + echo 抑制——程序化驱动交互式 CLI 的工程答案；`agent-orchestrator` 的 `WaitForMessageDeliveryReady`（150ms 轮询 + 750ms 稳定窗 + 5s 兜底）补"投递不丢"。
- **codeg run_seq 代际 CAS**：事件按 `(run_id, run_seq)` CAS 结算，取消只作用当代——零成本消灭迟到事件竞态。
- **agent-orchestrator 保守终止**："失败探测 ≠ 死亡证明"，三重确认 + 乐观锁复核。
- **parallel-code self-landing**：五态落地状态机 + verification checks 自验自合（注意：自报结果可伪造，只适合信任内环境）。
- **nuomi 差距**：CLI adapter 是一次性进程模型（无 PTY 长会话/无投递就绪）；无子任务委托；无代际 CAS。

### 2.4 内核扩展性（Cordis 对标）
- **deepseek Cordis 三大机制**：①`ctx.effect()` 一切注册可逆、卸载逆序 disposer；②epoch 字符串反应式依赖——服务 provide/注销自动驱动 Fiber reload/unload；③waterfall 事件分发（洋葱中间件，不调 next() 即否决）。
- **Rust 映射判断**：effect/disposer ≈ `Vec<BoxAsyncFn>` 直接可做；waterfall ≈ trait 回调；epoch 反应式 ≈ watch 通道（中难度，最后做）；JS Proxy 动态查找**不模仿**（Rust TypeId 静态注册更优）。
- **nuomi 差距**：harness/ 仅 456 行，init/start 一次性，注册无 disposer 链，bus 只有 broadcast。

### 2.5 持久化与可观测
- **agent-orchestrator 派生状态**：DeriveStatus 纯函数，展示状态读时计算永不落库——与 nuomi 事件溯源天然契合。
- **agent-orchestrator 触发器 CDC**：业务表 AFTER 触发器同事务写 change_log → 100ms tail → SSE；应用层零手工发事件。
- **hermes FTS5**：trigram + CJK 专用索引 + 后台分块回填，记忆/轨迹跨会话可搜。
- **nuomi 差距**：无 change_log、前端靠轮询、memory 仅 LIKE 关键词。

### 2.6 Self-Evolution（prime-agent 最强对标）
- **六层稳定性组合**：base system prompt 永不可变（prompt 类仅是补充笔记）；触发前 LLM 审查门 + 25 轮间隔 + 20 分钟冷却；apply 前对比规划期 baselineState 乐观并发拒绝；before/after 快照逆序回滚（全局 JSONL 跨会话可回滚）；create 冲突拒绝 + local/global scope 隔离；branchVersion 守卫丢弃过期提案。
- **nuomi 差距**：PromptVersion 已有候选→激活状态机（比 prime 裸改更稳），但无审查门/冷却/回滚快照/基线检测。

### 2.7 沙箱（全场 deepseek 最佳，nuomi 从零）
- 链式 runner（Linux bwrap→Landlock、macOS Seatbelt、Windows 受限令牌 ACL），**每级功能探测（跑 `true`），失败 fail-closed**，partial 上报。
- parallel-code 纵深补充：35+ ENV_BLOCK_LIST（LD_PRELOAD/GIT_CONFIG_COUNT 等，每条带威胁模型注释）、Docker 资源限制、per-task token。

## 3. 吸收优先级路线

### P0 — 低难度 × 高收益（直接支撑三大创新点，建议本轮实施）
| # | 机制 | 来源 | nuomi 落点 |
|---|---|---|---|
| 1 | 上下文预裁剪：MD5 去重 + 语义 1 行摘要 + token 预算尾切点 + tool 组对齐 | hermes | 新建 `providers/context.rs`（只作用于发送视图，EventRecord 不动） |
| 2 | 缓存字段抽象：CacheRetention + Usage.cache_read/write + Anthropic cache_control 三点位 | pi | `providers/types.rs` + `anthropic.rs` |
| 3 | system prompt 字节稳定 + 工具集会话固定 + 发送前 repair_message_sequence | hermes | `providers/client.rs` + `plugins/system_prompt.rs` |
| 4 | 截断安全：stop_reason==length 全 toolCall 标失败 | pi | `plugins/loop_engine.rs` |
| 5 | effect 可逆注册 + waterfall 事件分发 | deepseek | `harness/context.rs` + `harness/bus.rs` |
| 6 | run_seq 代际 CAS | codeg | `domain/run_state.rs` + EventRecord |
| 7 | 派生状态纯函数 derive_status（展示状态不落库） | agent-orchestrator | `domain/` 新纯函数 + 查询侧 |
| 8 | 演进稳定性包：base prompt 不可变 + 审查门 + 冷却 + before/after 回滚快照 + 基线冲突检测 | prime-agent | `evolution/{versioning,scheduler,reflection}.rs` |
| 9 | ENV_BLOCK_LIST spawn 环境消毒 | parallel-code | `adapters/cli.rs` |

### P1 — 中难度 × 战略价值（下轮）
- cache scope = compression-lineage root（hermes）→ sessions 表加 lineage
- FTS5 + CJK 全文检索替换关键词（hermes）→ `store/repos/memory.rs`
- steering/follow-up 双队列 + prepareNextTurn 热切换模型（pi）→ loop_engine
- SQLite 触发器 CDC → tauri event 推送（agent-orchestrator）→ `store/migrations`
- PTY 提示符检测 + 投递就绪窗口（parallel-code + agent-orchestrator）→ `adapters/cli.rs` PTY 长会话态
- 非阻塞委托 Broker + depth_limit + long-poll（codeg）→ orchestrator 子任务模块
- worktree 隔离 + 两阶段合并 + 意图持久化（codeg/parallel-code）→ `services/workspace.rs`
- self-landing 五态（parallel-code）→ run_state
- 窄端口 AgentAdapter 拆分（agent-orchestrator，6 必需方法 + 可选能力接口）
- 契约 drift 测试（CI 跑 contracts:gen 后 git diff --exit-code）

### P2 — 高难度 × 远期
- epoch 反应式依赖装卸（deepseek）；ACP 统一接入协议（codeg）；compat 标志位 Provider 目录（pi）；内核级沙箱 fail-closed（deepseek）；Orchestrator-Worker 项目级拓扑 + PR 事实回注（agent-orchestrator）；滑动窗口调度 prompt 纪律（parallel-code，必须配 orchestrator 硬闸门）。

## 4. 创新突破组合设计（nuomi 独有合成）

pr.md 三大创新点在吸收后的合成形态：

1. **Token 消耗**：`预裁剪（零 LLM，回收大头）→ token 预算尾切 → 结构化迭代摘要` 三级流水全部作用于"发送视图"，append-only EventRecord 不动（可重放/可审计/可换策略重算）——这是 nuomi 相对 hermes 的结构性优势：hermes 压缩不可逆，nuomi 压缩可重算。
2. **缓存命中**：统一 CacheRetention 字段 + system prompt 字节稳定 + 工具集会话固定 + alternation repair + lineage-root cache scope。四项中三项是纯纪律（成本近零），组合后 prompt cache 命中率可作为一等公民指标进 Usage 遥测与看板。
3. **CLI 协作**：`PTY 提示符检测（能投）+ 投递就绪窗（投得准）+ 非阻塞委托 Broker（能并行）+ run_seq CAS（不竞态）+ 保守终止（不死锁）`——五件套是目前所有参考框架都没凑齐的组合：codeg 有委托无 PTY 细节、parallel-code 有 PTY 无代际 CAS、agent-orchestrator 有投递就绪无委托 broker。

## 5. 明确不吸收清单（跨框架汇总）

- **私有会话逆向解析器**（codeg 19 个）——vendor 升级即碎
- **巨型单体**（hermes cli.py 21K / codeg connection.rs 19K / prime AgentSession 11.7K）——与插件内核哲学相悖，取机制不取结构
- **JS Proxy 动态 ctx / 声明合并**（deepseek）——Rust 无反射，TypeId 注册更优
- **20+ 平台 Gateway / Honcho SaaS / IPython 内核 / CBOR 远程栈 / 自研 TUI**——生态绑定重、与本地 SQLite 优先原则冲突
- **prompt 作为唯一调度器**（parallel-code 滑动窗口）——违反"逻辑在 orchestrator 不在 prompt"边界规则
- **JSON 文件态持久化 + hydrate 恢复**（parallel-code/prime）——nuomi 的 SQLite WAL + 事件溯源严格更强
