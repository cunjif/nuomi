# ADR 0007: 工具调用 I/O 交换文件系统（Exchange FS）

- 状态：Accepted（用户已在目标指令中明确批准实现——依据 pr.md「重大决策需 ADR + 用户确认」，本 ADR 记录该批准背景；机制参照 prime-agent 的 agent_message 思路并按用户要求做更完善的设计与性能创新）
- 日期：2026-09-06

## 背景 / Context

Agent 调用工具/MCP/插件/Skill 时，输出直接塞回模型上下文：大输出撑爆 context、无法审计、无法在执行前拦截、崩溃后调用轨迹全部丢失。prime-agent 的 agent_message 机制验证了「调用先落记录、结果延迟领取」的方向，但其实现是纯内存邮箱、无闸门、无持久化、无检索 API。

用户要求：在硬盘刷写次数、处理速度、性能容量上做大胆创新——即**不逐条 fsync 的分组提交日志 + 内存热层读取 + 内容寻址溢出去重**。

## 决策 / Decision

新增 `crates/nuomi-core/src/exchange`（`ExchangeJournal` + `Gate` trait + `DefaultGate`），接入 `plugins/tools.rs` 执行路径（`ExchangeConfig` 开关，默认关闭保持既有行为）。

**坐标与闸门**：每次调用先 `submit(run_id, handle, params)`，产出坐标 `Coordinate { run_id, seq, ulid }`（display `exch/<run_id>/<seq>-<ulid>`，seq 由 journal 单调分配，ulid 为 UUIDv7 重编码的 26 字符 Crockford 串，跨 run 唯一且按时间可排序）。输入闸门执行 验证（schema/大小）→ 审查（可插拔策略钩子）→ 拦截 / 拒绝 / 修复（RFC 7386 merge patch）/ 补全，四路结果 `Allowed | Fixed | Rejected | Intercepted`。执行后 `complete(coord, output)` 走同一套钩子写日志文件系统；Agent 只收到**执行回执**（坐标 + 状态 + 预览），随后用 `exchange_list / exchange_read / exchange_tail` 像翻文档一样分页检索输出。

**三层性能模型**（核心创新）：

1. **内存热层（Hot）**：运行中全部记录落入 per-run 内存索引（`RwLock<HashMap<seq, Arc<Record>>>`，per-run `io_lock` 串行化 submit/complete，多 run 并发安全）。刚执行完就读取——最高频路径——**纯内存命中，零磁盘 IO**（测试以目录快照 + mtime 对比证明）。
2. **持久层（Durable，分组提交）**：后台 writer 任务把积压记录按 run 分组，一次 `write_all` + 一次 `flush` 批量 append 到 per-run JSONL 段文件（`segments/<run_key>/<seg>.jsonl`，满 `flush_batch_min=32` 行轮转）。触发条件：积压 ≥32 条 **或** 50ms 定时窗口。**刻意不做逐条 fsync**：`flush` 只刷用户态缓冲到 OS 页缓存，进程崩溃不丢已 flush 数据；最坏丢失窗口 = 一个 50ms 周期内「已执行未落盘」的记录（副作用已发生，重启后 `recover()` 重放段文件重建索引，同 seq 最后一行胜出，seq 从 max 续接）。
   **量化**：1000 次调用的持久化成本——
   - 逐条 fsync 方案：1000 次 fsync × 5–15ms（机械盘）≈ **5–15s**（SSD 0.1–1ms 亦需 0.1–1s）；
   - 分组提交方案：fsync 次数 **0**（刻意省略，崩溃语义改为「最多丢 50ms」）；`write_all+flush` 系统调用从 1000 次降至 ⌈1000/32⌉ ≈ **32 次**（突发合并）或每 50ms 窗口 ≤1 次（稀疏调用，单次 ~10µs 级页缓存写入）。净收益约 **50–75×**（机械盘）/ 数量级消除延迟长尾。
3. **溢出层（Spill）**：输出序列化超过 8 KiB 时不进内存正文，索引只存 envelope（坐标/状态/长度/**前 256 字节预览**/**SHA-256 内容 hash**），payload 写入内容寻址 blob `blobs/<hash>`。**同 hash 复用同一文件（零拷贝引用）**——重复大输出（如多次读同一文件）磁盘只存一份；读取时按需加载 blob。
   **内存占用上界**：`M ≤ Σ_runs [ E_r × S_env + min(H, R_r) × S_spill ]`，其中 envelope `S_env ≈ 512 B`（元数据 + 256 B 预览）、热正文上限 `H = 256` 条/run（超限最旧终态记录降级为 envelope-only，`max_hot_payloads_per_run` 可调）、单正文 ≤ `S_spill = 8 KiB`。例：50 run × 1 万次调用 ≈ 50 × (10k×512 B) ≈ **250 MB 索引 + ≤100 MB 热正文**，且正文项恒定有界。
   **磁盘容量公式**：`C ≈ E × (S_meta ≈ 300 B + S_inline ≤ 8 KiB) + B_unique × S_payload`；默认 `DropPayloads` GC 后 `C → E × ~300 B`（仅 envelope 索引），blob 全部释放。

**容量治理**：per-run 分段 + GC 钩子 `RetainPolicy { KeepAll | KeepLastNRuns(n) | DropPayloads }`，默认 **DropPayloads**（run 结束后 envelope 留索引、blob 删除、段文件重写为 envelope-only 快照）。写放大补偿：仅在 run 段文件数 ≥4（过半数为完整段）时允许 `compact`——整段重写、同坐标仅保留最后一行、丢弃被 Fix/Reject/Complete 覆盖的 Pending 残留行，行数收敛到坐标数。

**与 prime-agent agent_message 对比**：agent_message 是会话内内存邮箱，消息正文随读随取塞回上下文、无闸门、无持久化、无跨会话恢复；Exchange FS 提供①坐标延迟领取（上下文只增 ~200 B 回执）②可插拔双向闸门（验证/修复/拦截可审计）③崩溃可重放的分组提交日志④内容寻址去重 blob⑤容量 GC 策略。代价是读取多一次往返——由热层零 IO 与坐标寻址摊薄。

## 后果 / Consequences

- 正面：大输出不再进入上下文（回执 + 按需检索）；全量调用轨迹可审计可回放；fsync 成本消除；重复大 payload 磁盘零冗余；GC 保证默认容量收敛。
- 负面/风险：①进程崩溃丢 50ms 内未落盘记录（可接受：副作用已发生，回执坐标可对照段文件对账）；②热正文逐出后 `read` 仅返回 envelope（payload 仍在磁盘，调大 `max_hot_payloads_per_run` 缓解）；③`compact`/`gc` 需在 run 静止时调用（与 writer 并发有理论竞态窗口）；④`run_key` 文件名清洗可能对仅含特殊字符差异的 run_id 碰撞（记录体内保存原始 run_id，可人工对账）。
- 回退：`ExchangeConfig` 开关默认关闭，注册表走原直连路径，行为与测试完全不变；关闭即回退，无迁移残留（exchange 目录为独立旁路数据，可整目录删除）。
