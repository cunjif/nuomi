# TASKS: 消息引用 Quote/Reply（message-quote）

> 对应 SPEC：`docs/specs/message-quote/spec.md`（待批准）
> 对应 ADR：`docs/adr/0018-message-quote.md`（Proposed）
> 验收标准：AC1-AC14（详见 spec.md §4）
> 任务规划原则：垂直切割（按业务功能分组）· 契约先行（后端 DTO + bindings 先于前端消费）· 原子性 · 有序性

## 依赖关系与优先级总览

| 任务组 | 优先级 | 前置依赖 | 验收映射 | 提交 |
|--------|--------|----------|----------|------|
| 1. 后端实体 + 写入透传 | 高 | 无 | AC1-AC2 | feat(core) |
| 2. IPC 加参数 + excerpt 提取 | 高 | 任务 1 | AC3-AC4 | feat(ipc) |
| 3. bindings 再生 + client | 高 | 任务 2 | AC5 | feat(ipc) |
| 4. 前端类型 + 渲染 + 交互 | 中 | 任务 3 | AC6-AC8 | feat(chat) |
| 5. Composer 引用状态 + 装配 | 中 | 任务 4 | AC9-AC11 | feat(chat) |
| 6. i18n + 质量门 | — | 任务 1-5 | AC12-AC14 | chore |

**串行策略**：任务 1→2→3 严格串行（契约先行）；任务 4→5 串行（前端类型先于交互）；任务 6 收尾。

---

## 1. 后端实体 + 写入路径透传

> 目标：ChatMessage 支持引用字段，persist_transcript/record_turn 写入 payload。
> 范围：crates/nuomi-core（实体 + 内核写入路径）。

- [ ] 1.1 `crates/nuomi-core/src/providers/types.rs`：`ChatMessage` 加 `quoted_event_seq: Option<i64>` + `quoted_excerpt: Option<String>` 字段；`user()` 构造器透传
- [ ] 1.2 `crates/nuomi-core/src/facade.rs:685-740` `persist_transcript`：写 message event 时把 ChatMessage 的 quoted 字段塞进 payload JSON
- [ ] 1.3 `crates/nuomi-core/src/orchestrator/whiteboard.rs:138-178` `record_turn`：加 `quoted_event_seq`/`quoted_excerpt` 参数，写入 payload
- [ ] 1.4 `crates/nuomi-core/src/orchestrator/group_chat.rs:146`：调 record_turn 时透传引用
- [ ] 1.5 `cargo test -p nuomi-core` + `cargo clippy` 验证
- [ ] 1.6 提交："feat(core): ChatMessage 支持消息引用字段透传至 payload"

---

## 2. IPC 命令加参数 + excerpt 后端提取

> 目标：3 个写入命令加 quotedEventSeq 参数，后端查 events 取 excerpt。
> 范围：src-tauri（commands + tauri_cmds）。

- [ ] 2.1 `src-tauri/src/commands.rs:161` `run_conversation_turn`：加 `quoted_event_seq: Option<i64>` 参数，查 `events::list_by_aggregate` 取该 seq 的 message event content 截前 80 字符作为 excerpt（seq 不存在或非 message kind 时静默丢弃）
- [ ] 2.2 `src-tauri/src/commands.rs:568,704,5241` `impl_submit_task`/`impl_enqueue_message`/`impl_submit_message`：加 `quoted_event_seq: Option<i64>` 参数透传
- [ ] 2.3 `src-tauri/src/tauri_cmds.rs:117,128,1109`：3 命令加 `quoted_event_seq: Option<i64>` 参数（camelCase 生成 `quotedEventSeq`）
- [ ] 2.4 `cargo test` + `cargo clippy` 验证
- [ ] 2.5 提交："feat(ipc): submitTask/submitMessage/enqueueMessage 加 quotedEventSeq 参数"

---

## 3. 重生 bindings + 前端 client

> 目标：重新生成 TS bindings，前端 client 透传 quotedEventSeq。
> 范围：src/lib/ipc。

- [ ] 3.1 `pnpm contracts:gen` 重新生成 `src/lib/ipc/bindings.gen.ts`
- [ ] 3.2 `src/lib/ipc/client.ts:90-93,235`：`submitTask`/`enqueueMessage`/`submitMessage` 3 个 wrapper 加可选 `quotedEventSeq?: number`
- [ ] 3.3 `src/lib/ipc/test-double-commands.ts`：同步 3 个方法签名
- [ ] 3.4 `pnpm typecheck` 验证
- [ ] 3.5 提交："feat(ipc): 前端 client 透传 quotedEventSeq"

---

## 4. 前端类型 + 渲染 + 交互

> 目标：ChatEntry 加引用字段，Bubble 渲染引用块 + hover 按钮。
> 范围：src/features/chat。

- [ ] 4.1 `src/features/chat/useSessionStream.ts:14`：`ChatEntry` 加 `quotedEventSeq?: number` + `quotedExcerpt?: string`
- [ ] 4.2 `src/features/chat/useSessionStream.ts:27-52`：`toEntries` 读 `payload.quoted_event_seq`/`payload.quoted_excerpt` 填充 ChatEntry
- [ ] 4.3 `src/features/chat/useSessionStream.ts:111-142`：live `session.message` 分支同步读引用字段
- [ ] 4.4 新建 `src/features/chat/QuoteBlock.tsx`：引用块组件（摘录 + 点击跳转 via `data-seq` + `scrollIntoView`）
- [ ] 4.5 `src/features/chat/Bubble.tsx:13-40`：`entry.quotedExcerpt` 存在时在 `<p>` 之前渲染 `<QuoteBlock>`；加 hover"引用"按钮（仅 `kind==="message"` 显示，`onQuote` 回调）
- [ ] 4.6 `src/features/conversation/group/SpeakerBubble.tsx`：加 quoted props + 渲染 `QuoteBlock` + hover"引用"按钮
- [ ] 4.7 `src/features/chat/MessageList.tsx`：加 `data-seq` 属性 + 跳转能力；透传 `onQuote` 给 Bubble
- [ ] 4.8 `pnpm typecheck && pnpm test` 验证
- [ ] 4.9 提交："feat(chat): 消息引用块渲染 + Bubble 引用按钮"

---

## 5. Composer 引用状态 + 装配

> 目标：Composer 支持引用状态，ChatView/GroupConversationView 装配。
> 范围：src/features/conversation/composer + ChatView + GroupConversationView。

- [ ] 5.1 新建 `src/features/conversation/composer/QuotePreviewBar.tsx`：引用预览条（摘录 + 取消按钮）
- [ ] 5.2 `src/features/conversation/composer/Composer.tsx:35`：`onSubmit` 签名变 `(input: string, quotedEventSeq?: number) => Promise<void>`
- [ ] 5.3 `Composer.tsx`：加 `quotedEntry: { seq: number; excerpt: string } | null` state + 渲染 `QuotePreviewBar`（当 quotedEntry 非空）+ 提交后清空
- [ ] 5.4 `src/features/chat/ChatView.tsx:43-49,82-91,133`：`setQuotedEntry` 透传给 MessageList→Bubble；submitMut/enqueueMut mutationFn 透传 `quotedEventSeq`
- [ ] 5.5 `src/features/conversation/group/GroupConversationView.tsx:52-56,86-97`：submitMessage 透传 `quotedEventSeq`；`setQuotedEntry` 透传
- [ ] 5.6 群聊引用块取 speaker 名兼容 `role_name`/`role` 两种 payload 键（`asString(payload.role_name) || asString(payload.role) || "unknown"`）
- [ ] 5.7 `pnpm typecheck && pnpm test` 验证
- [ ] 5.8 提交："feat(chat): Composer 引用状态 + 引用预览条 + 装配"

---

## 6. i18n + 质量门

- [ ] 6.1 `src/i18n/locales/zh-CN.json` + `en.json`：新增 `chat.quote`/`chat.quoteCancel`/`chat.quotedMessageDeleted` 文案
- [ ] 6.2 `pnpm typecheck && pnpm lint && pnpm test`（单线程模式）全绿
- [ ] 6.3 `cargo test -p nuomi-core && cargo clippy` 全绿
- [ ] 6.4 人工验证：`pnpm tauri dev` 启动后单聊/群聊中引用消息，引用块渲染正确，点击跳转高亮原消息
- [ ] 6.5 提交："chore(i18n): 消息引用文案 + 质量门验证"
