# pi — 组件级研究与可吸收点

## 1. 定位与架构骨架
pi 是 TypeScript 严格模式的单 Agent 编程 CLI（v0.84.3），9 包 monorepo，单向依赖链：`telemetry → ai → agent → coding-agent`；`tui`、`session-backends/sqlite-node` 服务于 agent 层；`protocol → client → server` 是独立的 CBOR 远程会话栈。边界由 npm workspaces + 顶层 import 约束，每包可独立发布——对应 nuomi 即 crate 间禁止反向依赖。

## 2. 双层循环深挖
`packages/agent/src/agent-loop.ts:170-274`：**外层**是 follow-up 循环——Agent 自然停止后检查 `getFollowUpMessages()` 队列，非空则重进内层；**内层**是 tool-call + steering 循环——流式获取 Assistant 响应、执行工具、每轮末注入 `getSteeringMessages()`（用户中途插话）。摘录（agent-loop.ts:170-189）：

```ts
// Outer loop: continues when queued follow-up messages arrive after agent would stop
while (true) {
  let hasMoreToolCalls = true;
  // Inner loop: process tool calls and steering messages
  while (hasMoreToolCalls || pendingMessages.length > 0) {
    // Process pending messages (inject before next assistant response)
    if (pendingMessages.length > 0) {
      for (const message of pendingMessages) { ...currentContext.messages.push(message); }
      pendingMessages = [];
    }
```

相对单层 ReAct 的增量：① 非阻塞人机协作（steering/follow-up 双队列，`agent.ts:231-232`，模式可配 one-at-a-time/all）；② 每轮 `prepareNextTurn` 钩子可热切换模型与 thinking level（:232-245）；③ `shouldStopAfterTurn` 外部可控停。无状态循环 + `Agent` 有状态包装（队列/事件）+ `AgentHarness`（lane 分支/compaction）三层职责正交。nuomi 的 loop_engine 缺 steering 队列与 per-turn 换模型钩子。

## 3. Provider 统一抽象收敛点
收敛在两层：**API 协议层**（`ai/types.ts:17-29` 仅 10 个 `KnownApi` 实现）× **Provider 目录层**（`types.ts:35-75` 39 个 `KnownProvider`）。关键洞察：Provider 多 ≠ 适配器多——绝大多数 provider 复用 `openai-completions` 一个适配器，靠模型目录里的 `compat` 标志位抹平差异（`types.ts:557-625` `OpenAICompletionsCompat`：`thinkingFormat` 11 种变体、`maxTokensField`、`supportsStrictMode` 等）。摘录（types.ts:821-849）：

```ts
export interface Model<TApi extends Api> {
  id: string; name: string; api: TApi; provider: ProviderId;
  baseUrl: string; reasoning: boolean;
  thinkingLevelMap?: ThinkingLevelMap;
  input: ("text" | "image")[]; cost: ModelCost;
  contextWindow: number; maxTokens: number;
  compat?: TApi extends "openai-completions" ? OpenAICompletionsCompat : ...;
}
```

缓存处理：统一 `CacheRetention = "none"|"short"|"long"`（types.ts:108）+ `sessionId` 会话亲和；Anthropic 侧在 system prompt、最后一个 tool 定义、最后一条消息打 `cache_control`（`api/anthropic-messages.ts:1295-1317,1360`）；`Usage` 统一含 `cacheRead/cacheWrite/cacheWrite1h`（types.ts:382-403）。**这正是 nuomi 缓存命中创新点可直接照抄的字段设计**。

## 4. 其它差异化机制
- **截断安全**：`agent-loop.ts:208-214,381-406` —— `stopReason==="length"` 时所有 toolCall 标记失败并要求重发。因流式参数经"尽力 JSON 抢救"后可能通过校验但语义残缺，执行有风险；代价是一次额外往返 token。
- **文件变异队列**：`harness/tools/file-mutation-queue.ts:29-56` —— canonical path 为 key、Promise 链串行化（`WeakMap<ExecutionEnv, ...>`），注册本身也串行防 key 竞态。解决 edit/write/bash 并发写同一文件；代价是解析 symlink 的开销。
- **JSONL+SQLite 双存储**：默认 `JsonlSessionStorage`（`session/jsonl/storage.ts:24-48`）——临时文件+rename 原子发布、torn-tail 自动修复，会话=单文件记录日志（reducer 重建 + 12 种 corruption 检测）；SQLite（`session-backends/sqlite-node`）是可选 `SessionStorage` 后端，coding-agent 默认不引用。分工：JSONL 管 append-only 事实，SQLite 管查询检索。
- **自研 TUI 差分渲染**：`tui.ts` 组件 `render(width)→string[]`；`tui-main-screen.ts:180,295-315` 计算 firstChanged/lastChanged 只重绘变化行，`requestRender` 用 nextTick+timer 合帧。

## 5. 组件评分表
| 维度 | 分 | 理由 |
|---|---|---|
| 编排 | 2 | 单 Agent 专注，lane 分支而非多 Agent 拓扑 |
| 沙箱 | 3 | 扩展示例有 sandbox/Gondolin，非内核内建 |
| 持久化 | 5 | 原子发布+torn-tail+reducer 校验+双后端 |
| 扩展性 | 5 | 扩展可注册工具/命令/Provider/UI 原语 |
| 上下文 | 5 | 文件感知 compaction + 分支摘要 + 阈值/溢出触发 |
| 路由 | 2 | 仅透传 OpenRouter/Vercel 路由偏好 |
| 可观测 | 3 | telemetry 契约 + diagnostics，非全链路 |

## 6. 可吸收清单
| 机制 | 对 nuomi 的收益 | 难度 | 实现要点 | 冲突点 |
|---|---|---|---|---|
| compat 标志位模式 | 补 Provider 生态：一份 OpenAICompatible 适配器覆盖几十家 | 中 | 模型目录里放 `compat` 结构体，按 baseUrl 自动检测+覆盖 | 现有 ProviderConfig 需增 compat 字段（迁移只增不改） |
| CacheRetention+Usage 缓存字段 | 直接支撑"缓存命中"创新点 | 低 | StreamOptions 加 cache_retention，Usage 加 cache_read/write | Master-Slave 需决定缓存策略归属（模型级 or 请求级） |
| 截断安全 | loop_engine 可靠性 | 低 | stop_reason==length 时全部标失败 | 无 |
| steering/follow-up 队列 | 人机协作/群聊中断注入 | 中 | 双 PendingMessageQueue + 循环钩子 | 需接入事件溯源（消息注入也要落 EventRecord） |
| JSONL 原子发布+torn-tail | 事件日志崩溃安全 | 中 | temp+rename、加载时截断有效前缀 | nuomi 用 SQLite WAL，思路可移植到事件文件导出 |
| 文件变异队列 | CLI Agent 工具安全 | 低 | canonical path + tokio Mutex 链 | 无 |

## 7. 明确不建议吸收
- **自研 TUI 差分渲染**：nuomi 产品面是 Tauri+React，终端渲染无场景。
- **CBOR 远程协议 + client/server**：nuomi 共享 SQLite 即实现跨端续传，多一套远程栈是负担。
- **39 家 Provider 目录自动生成**：pi 靠脚本+在线水合维护；nuomi 用户自配 endpoint，手写 compat 枚举即可，避免引入目录生成管线。
- **Bun 双运行时/独立二进制**：Rust 侧无此问题。
