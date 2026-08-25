# ADR 0002: 领域事件多通道推送（修订 ipc-contract 唯一通道规则）

- 状态：Accepted（依据已批准 SPEC `docs/specs/ui-m1.md` D7）
- 日期：2026-08-24

## 背景 / Context

现行 `ipc-contract.md` 规定唯一事件通道 `event://domain`。M-UI1 要求三栏主界面同时呈现：会话内流式 token、看板状态流转、审批到达提醒。单通道下所有会话的高频 token 混入同一广播流，前端需按 sessionId 过滤，且任一会话的洪峰（群聊多 Agent 并发）会稀释其他会话的推送时效。

## 决策 / Decision

将「唯一通道」修订为**两类命名空间的多通道**：

1. **会话通道** `event://session/{session_id}`：该会话的 thought / tool_call / tool_result / message 增量小包；每通道携带单调递增 `seq`（沿用 events 表 per-aggregate seq），前端断线重连按 seq 调 `listEvents(sessionId, afterSeq)` 补拉。
2. **全局通道** `event://domain`：跨会话的结构化低频事件——task 状态迁移、run 状态迁移、approval_requested/resolved、schedule_triggered。

不变式（继续强制）：
- 高频流只推增量小包；历史一律命令 + 游标分页查询；
- 每条事件先落 EventRecord 再 emit（先落库后副作用铁律）；
- payload 为 `DomainEvent` 判别联合 `{ type, taskId?, runId?, sessionId?, seq?, payload }`；
- 只加不改删的版本化策略照旧。

## 后果 / Consequences

- 正面：会话间洪峰隔离；前端按面板订阅所需通道，减少无效过滤与渲染；seq 补拉语义在会话粒度更自然。
- 负面：Tauri 监听器数量随打开会话数增长（上限：仅当前活跃会话 + 看板全局通道订阅）；契约规则复杂度略增。
- 缓解：前端 `useDomainEvents` 统一管理订阅生命周期（切换会话即换订阅）；内核侧 emit 失败不影响已落库事实。
