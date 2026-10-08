# ADR 0018: 消息引用（Quote/Reply）

- **状态**: Proposed
- **日期**: 2026-10-07
- **决策者**: user + conductor

## 背景

当前对话消息不支持引用（quote/reply）已有消息。具体缺口：

1. `EventRecord`（`crates/nuomi-core/src/domain/entities.rs:182-195`）只有 7 个字段（`id, aggregate_type, aggregate_id, kind, payload, seq, created_at`），**无任何 `quote`/`reply_to`/`in_reply_to`/`parent_message_id` 字段**。
2. `events` 表（`migrations/0001_init.sql:12-32`）自 0001 创建后**零 ALTER**，且有 `trg_events_no_update/no_delete` 触发器物理强制 append-only。
3. 消息写入有 3 个并行入口（`submitTask` / `submitMessage` / `enqueueMessage`），入参 `text` 均为纯 `String`，无引用参数。
4. 前端 `Bubble.tsx`（`src/features/chat/Bubble.tsx:13-40`）纯渲染，无引用块；`ChatEntry`（`useSessionStream.ts:14-23`）无引用字段；`Composer.tsx` `onSubmit: (input: string) => Promise<void>` 无引用状态。
5. 现存 payload 结构不统一：单 Role 路径写 `{"role","content"}`（`facade.rs:708`），群聊路径写 `{"role_id","role_name","content"}`（`whiteboard.rs:155`）——引用块取被引用消息 speaker 名时需兼容两种 payload。

需求：用户在对话中可引用某条已有消息，被引用消息的 seq + 摘录随新消息一起存储，前端渲染引用块（可点击跳转/高亮原消息）。适用于单聊与群聊（含跨 agent 引用）。

## 决策

### 设计决策

| # | 决策点 | 选择 |
|---|---|---|
| 1 | 引用信息存储位置 | **payload JSON 内**（零迁移、EventDto 零改） |
| 2 | payload 键命名 | `quoted_event_seq` + `quoted_excerpt`（snake_case，与现有 `role_name`/`delta_to` 一致） |
| 3 | excerpt 来源 | **后端从被引用消息自动提取**（前端只传 `quotedEventSeq`，后端查 events 取 content 截取前 80 字符） |
| 4 | append-only 兼容 | 引用关系随新消息一次性写入 payload，不事后补建——符合 append-only 铁律 |
| 5 | payload 结构不统一 | 引用块渲染时兼容 `role`/`role_name` 两种键取 speaker 名；不在本 ADR 统一 payload（独立问题） |
| 6 | 群聊跨 agent 引用 | 被引用消息 seq 来自同一 session 的 events 表，机制上无需特殊处理 |

### 1. payload 路线（零迁移）

不新增 `events` 表列，引用信息塞入 message event 的 `payload` JSON：

```json
{
  "role": "user",
  "content": "回复内容",
  "quoted_event_seq": 42,
  "quoted_excerpt": "被引用消息的前 80 字符..."
}
```

无引用时省略 `quoted_event_seq`/`quoted_excerpt` 键（向后兼容，旧消息 payload 无此键）。

**选择 payload 路线的理由**：
- 零迁移（events 表 append-only，ALTER 加列虽不破坏铁律但增加复杂度）
- `EventDto.payload` 是 `serde_json::Value`（透明 JSON），DTO 零改动
- 引用是低频展示需求，无需对 `quoted_event_seq` 建索引或反查引用关系
- store 层 `events::append` 签名不变（引用信息在调用方拼入 payload）

### 2. IPC 命令参数扩展

3 个写入入口加可选 `quoted_event_seq: Option<i64>` 参数（camelCase，对齐 tauri-specta 生成约定）：

| 命令 | 现签名 | 新签名 |
|---|---|---|
| `submitTask` | `(sessionId, input)` | `(sessionId, input, quotedEventSeq?)` |
| `enqueueMessage` | `(sessionId, input)` | `(sessionId, input, quotedEventSeq?)` |
| `submitMessage` | `(sessionId, text, attachmentIds, routeTargetAgentIds, contextInjectionIds)` | 末尾加 `quotedEventSeq?` |

参数可选（`Option<i64>`），缺省视为无引用，旧调用方不破。

**excerpt 后端提取**：`run_conversation_turn` 收到 `quoted_event_seq` 后，查 `events::list_by_aggregate` 取该 seq 的 message event，从 payload 提取 `content` 截取前 80 字符作为 `quoted_excerpt`。若 seq 不存在或非 message kind，引用静默丢弃（不阻断发送）。

### 3. 写入路径透传

引用信息沿调用链透传至 `persist_transcript` / `record_turn`，写入 message event payload：

```
submitTask/submitMessage/enqueueMessage (quotedEventSeq?)
  → run_conversation_turn (quotedEventSeq?)
    → 查 events 取 excerpt
    → materialize_single_role / run_selector_turn / core_run_team
      → persist_transcript (quotedEventSeq, quotedExcerpt)
        → events::append (payload 内含 quoted_event_seq + quoted_excerpt)
      → record_turn (quotedEventSeq, quotedExcerpt)  [群聊]
        → events::append (payload 内含 quoted_event_seq + quoted_excerpt)
```

涉及修改：
- `ChatMessage`（`providers/types.rs:28-37`）加 `quoted_event_seq: Option<i64>` + `quoted_excerpt: Option<String>` 字段，`user()` 构造器透传
- `facade.rs:685-740` `persist_transcript` 写 message event 时把 ChatMessage 的 quoted 字段塞进 payload
- `orchestrator/whiteboard.rs:138-178` `record_turn` 加 quoted 参数，写入 payload
- `orchestrator/group_chat.rs:146` 调 record_turn 时透传引用

### 4. 读取路径

`EventDto`（`commands.rs:1187-1205`）**无需改动**——`payload` 是 `serde_json::Value`，引用信息随 payload 透传到前端。

前端 `toEntries`（`useSessionStream.ts:27-52`）扩展：读 `payload.quoted_event_seq` / `payload.quoted_excerpt` 填充 `ChatEntry`。

### 5. 前端类型与渲染

```ts
// useSessionStream.ts
export interface ChatEntry {
  // ...现有字段
  quotedEventSeq?: number;
  quotedExcerpt?: string;
}
```

**新组件**：
- `src/features/chat/QuoteBlock.tsx`——引用块（摘录 + 点击跳转/高亮原消息）
- `src/features/conversation/composer/QuotePreviewBar.tsx`——Composer 内引用预览条（显示被引用摘录 + 取消按钮）

**渲染改造**：
- `Bubble.tsx`：`entry.quotedExcerpt` 存在时在 `<p>` 之前渲染 `<QuoteBlock>`
- `SpeakerBubble.tsx`（群聊）：同上
- `MessageList.tsx`：无需改（透传 entry）；加 seq → DOM 跳转能力（用 `data-seq` 属性 + `scrollIntoView`）

**交互入口**：
- `Bubble.tsx` / `SpeakerBubble.tsx` 加 hover 按钮"引用"（仅 `kind === "message"` 显示）
- 点击后通过回调把该 entry 设入 `Composer` 的 `quotedEntry` state
- `ChatView` / `GroupConversationView` 把 `setQuotedEntry` 透传给 `MessageList` → `Bubble`

**Composer 改造**：
- 加 `quotedEntry: { seq: number; excerpt: string } | null` state
- `onSubmit` 签名变 `(input: string, quotedEventSeq?: number) => Promise<void>`
- 渲染 `QuotePreviewBar`（当 `quotedEntry` 非空时，显示在 textarea 上方）
- 提交后清空 `quotedEntry`

### 6. 群聊兼容

群聊消息 payload 用 `role_id`/`role_name`（`whiteboard.rs:155`），单 Role 用 `role`（`facade.rs:708`）。引用块取被引用消息 speaker 名时：

```ts
const speaker = asString(payload.role_name) || asString(payload.role) || "unknown";
```

兼容两种 payload，不在本 ADR 统一 payload 结构（独立问题，另立 ADR）。

## 向后兼容

- IPC 参数 `quotedEventSeq` 为可选（`Option<i64>`），缺省视为无引用，旧前端不破。
- payload 内 `quoted_event_seq`/`quoted_excerpt` 键省略时视为无引用，旧消息渲染不受影响。
- `EventDto` 结构不变，既有前端 `toEntries` 对无引用消息行为不变（新字段 `undefined`）。
- `events` 表零迁移，既有 append-only 触发器无冲突。

## 权衡

- **优点**：零迁移、EventDto 零改、store 层 `append` 签名不变；引用信息随 payload 透传，读取链改动最小；append-only 铁律无冲突。
- **代价**：3 个 IPC 命令加参数 + ChatMessage 加 2 字段 + persist_transcript/record_turn 透传 + 前端 2 新组件 + Bubble/Composer 改造；payload 内引用关系无法索引反查（可接受，引用是低频展示需求）。
- **替代方案**：
  - (a) 独立列路线（`ALTER TABLE events ADD COLUMN quoted_event_seq INTEGER`）——否决。需改 EventRecord/EventDto/append/row_to_event/SELECT 全链，且引用无需索引查询，收益不抵成本。
  - (b) 前端传 excerpt——否决。前端可伪造 excerpt、可能与实际消息不一致；后端提取更安全。
  - (c) 独立 `message_quotes` 表——否决。过度设计，引用关系 1:1 随新消息，无需独立表。
  - (d) 统一 payload 结构后再做引用——否决。payload 统一是独立问题，不应阻塞引用功能；引用块兼容两种 payload 即可。

## 风险与缓解

| 风险 | 缓解 |
|---|---|
| 被引用消息被回退（若未来实现 ADR 消息回退）后引用悬空 | 引用块渲染时检查 seq 是否存在，不存在则显示"原消息已删除" |
| excerpt 与被引用消息后续编辑不一致 | 消息当前不支持编辑（无 edit IPC）；若未来加编辑，excerpt 是历史快照，符合引用语义 |
| 群聊 payload 结构不统一导致引用块 speaker 名缺失 | 兼容 `role_name`/`role` 两种键，fallback "unknown" |
| tauri-specta bindings 需 regen | 实施计划阶段 2 显式 `pnpm contracts:gen` |
| `submitTask`/`submitMessage`/`enqueueMessage` 三入口签名变更遗漏一处 | 阶段 1 集中改 Rust 三入口 + 阶段 3 集中改前端三 wrapper，typecheck 兜底 |

## 影响范围

### Migration
- **无**（payload 路线零迁移）

### 后端实体/内核
- `crates/nuomi-core/src/providers/types.rs:28-37` — `ChatMessage` 加 `quoted_event_seq` + `quoted_excerpt`
- `crates/nuomi-core/src/facade.rs:685-740` — `persist_transcript` 写 payload 时塞引用
- `crates/nuomi-core/src/orchestrator/whiteboard.rs:138-178` — `record_turn` 加 quoted 参数
- `crates/nuomi-core/src/orchestrator/group_chat.rs:146` — 透传引用

### IPC 契约
- `src-tauri/src/tauri_cmds.rs:117,128,1109` — 3 命令加 `quoted_event_seq: Option<i64>`
- `src-tauri/src/commands.rs:161,568,704,5241` — `run_conversation_turn` + 3 个 impl_* 透传 + 查 events 取 excerpt
- `src-tauri/src/lib.rs` — `collect_commands!` 无需改（命令名不变，仅加参数）
- `src/lib/ipc/bindings.gen.ts` — 重新生成（`pnpm contracts:gen`）
- `src/lib/ipc/client.ts:90-93,235` — 3 个 wrapper 加可选 `quotedEventSeq`
- `src/lib/ipc/test-double-commands.ts` — test double 同步签名

### 前端
- `src/features/chat/useSessionStream.ts:14,27-52,111-142` — `ChatEntry` 加字段 + `toEntries` 读引用 + live `session.message` 分支同步
- `src/features/chat/Bubble.tsx:13-40` — 条件渲染 `QuoteBlock` + hover"引用"按钮
- `src/features/conversation/composer/Composer.tsx:35,53,219` — `onSubmit` 签名 + `quotedEntry` state + `QuotePreviewBar`
- `src/features/chat/ChatView.tsx:43-49,82-91,133` — submitMut/enqueueMut 透传 quoted + `setQuotedEntry` 透传给列表
- `src/features/conversation/group/GroupConversationView.tsx:52-56,86-97` — submitMessage 透传 + SpeakerBubble 渲染引用
- `src/features/conversation/group/SpeakerBubble.tsx` — 加 quoted props + 渲染
- **新增** `src/features/chat/QuoteBlock.tsx` — 引用块组件
- **新增** `src/features/conversation/composer/QuotePreviewBar.tsx` — 引用预览条
- i18n `src/i18n/locales/{en,zh-CN}.json` — `chat.quote` / `chat.quoteCancel` / `chat.quotedMessageDeleted` 文案

## 实施计划

分 5 阶段，每阶段独立可验证、可提交（对齐 memory `feedback-commit-stage-by-logical-group`）。

### 阶段 1：后端实体 + 写入路径透传

1. `providers/types.rs`：`ChatMessage` 加 `quoted_event_seq: Option<i64>` + `quoted_excerpt: Option<String>`，`user()` 构造器透传
2. `facade.rs` `persist_transcript`：写 message event 时把 quoted 字段塞进 payload JSON
3. `orchestrator/whiteboard.rs` `record_turn`：加 `quoted_event_seq`/`quoted_excerpt` 参数，写入 payload
4. `orchestrator/group_chat.rs`：调 record_turn 时透传
5. `cargo test -p nuomi-core` + `cargo clippy` 验证
6. 提交："feat(core): ChatMessage 支持消息引用字段透传至 payload"

### 阶段 2：IPC 命令加参数 + excerpt 后端提取

1. `commands.rs` `run_conversation_turn`：加 `quoted_event_seq: Option<i64>` 参数，查 events 取 excerpt
2. `commands.rs` `impl_submit_task`/`impl_enqueue_message`/`impl_submit_message`：加参数透传
3. `tauri_cmds.rs`：3 命令加 `quoted_event_seq: Option<i64>` 参数
4. `cargo test` + `cargo clippy` 验证
5. 提交："feat(ipc): submitTask/submitMessage/enqueueMessage 加 quotedEventSeq 参数"

### 阶段 3：重生 bindings + 前端 client

1. `pnpm contracts:gen`
2. `src/lib/ipc/client.ts`：3 个 wrapper 加可选 `quotedEventSeq`
3. `src/lib/ipc/test-double-commands.ts`：同步签名
4. `pnpm typecheck` 验证
5. 提交："feat(ipc): 前端 client 透传 quotedEventSeq"

### 阶段 4：前端类型 + 渲染 + 交互

1. `useSessionStream.ts`：`ChatEntry` 加 `quotedEventSeq?`/`quotedExcerpt?`，`toEntries` 读 payload 填充，live 分支同步
2. 新 `QuoteBlock.tsx`：引用块（摘录 + 点击跳转）
3. `Bubble.tsx`：条件渲染 `QuoteBlock` + hover"引用"按钮（通过 `onQuote` 回调）
4. `SpeakerBubble.tsx`：同上
5. `MessageList.tsx`：加 `data-seq` 属性 + 跳转能力
6. `pnpm typecheck && pnpm test` 验证
7. 提交："feat(chat): 消息引用块渲染 + Bubble 引用按钮"

### 阶段 5：Composer 引用状态 + 装配

1. 新 `QuotePreviewBar.tsx`：引用预览条（摘录 + 取消按钮）
2. `Composer.tsx`：加 `quotedEntry` state + `onSubmit` 签名变 `(input, quotedEventSeq?)` + 渲染 `QuotePreviewBar`
3. `ChatView.tsx`：`setQuotedEntry` 透传给 `MessageList` → `Bubble`；submitMut/enqueueMut 透传 `quotedEventSeq`
4. `GroupConversationView.tsx`：同上 + submitMessage 透传
5. i18n 文案
6. `pnpm typecheck && pnpm test` 验证
7. 提交："feat(chat): Composer 引用状态 + 引用预览条 + 装配"

## 验证门禁

每阶段必须通过：
- `cargo test -p nuomi-core` + `cargo clippy`（涉及 Rust 时）
- `pnpm typecheck && pnpm lint`
- `pnpm test`（单线程模式，memory `feedback-test-suite-single-thread`）
- 人工验证：`pnpm tauri dev` 启动后单聊/群聊中引用消息，引用块渲染正确，点击跳转高亮原消息
