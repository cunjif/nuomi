# SPEC: 消息引用 Quote/Reply（message-quote）

> 状态：待批准（属跨层变更，需先获用户确认 ADR 0018 再动工）
> 关联 ADR：`docs/adr/0018-message-quote.md`（Proposed）
> 需求来源：用户请求「RoleAgent 支持消息引用」；需求权威仍为 `pr.md`。
> 事实基础：`EventRecord`（`entities.rs:182-195`）无引用字段；events 表 append-only 触发器禁止 UPDATE/DELETE；消息写入 3 入口（submitTask/submitMessage/enqueueMessage）入参均纯 String；`EventDto.payload` 是 `serde_json::Value`（透明 JSON）；前端 `Bubble.tsx` 纯渲染无引用块。

## 1. 决策记录摘要 / Decisions

| # | 决策 |
|---|---|
| D1 | **引用信息存 payload JSON**（零迁移、EventDto 零改），不新增 events 表列 |
| D2 | **payload 键 `quoted_event_seq` + `quoted_excerpt`**（snake_case，对齐现有 `role_name`/`delta_to`） |
| D3 | **excerpt 后端提取**：前端只传 `quotedEventSeq`，后端查 events 取 content 截前 80 字符（防伪造） |
| D4 | **append-only 兼容**：引用随新消息一次性写入 payload，不事后补建 |
| D5 | **payload 结构不统一兼容**：引用块取 speaker 名时兼容 `role`/`role_name` 两种键；不在本 SPEC 统一 payload |
| D6 | **群聊跨 agent 引用**：被引用消息 seq 来自同一 session events 表，机制无需特殊处理 |

## 2. 目标 / Goals

1. 用户在对话中可引用某条已有消息，新消息携带被引用 seq + 摘录。
2. 前端渲染引用块（摘录 + 点击跳转/高亮原消息）。
3. 单聊与群聊均支持（含跨 agent 引用）。
4. 零迁移、EventDto 零改、append-only 铁律无冲突。
5. 不回归：无引用消息渲染行为不变。

## 3. 用户故事 / User Stories

- **US1** 用户 hover 某条消息看到"引用"按钮，点击后 Composer 顶部出现引用预览条（被引用消息摘录 + 取消按钮）；输入回复后发送，新消息渲染引用块。
- **US2** 用户点击引用块，滚动到原消息并高亮。
- **US3** 群聊中用户引用另一 agent 的消息，引用块显示该 agent 的 speaker 名 + 摘录。
- **US4** 被引用消息的 excerpt 由后端从实际消息提取（前端不可伪造）。

## 4. 验收标准 / Acceptance Criteria

**阶段一：后端实体 + 写入路径透传**
- [ ] AC1 `ChatMessage`（`providers/types.rs:28-37`）加 `quoted_event_seq: Option<i64>` + `quoted_excerpt: Option<String>` 字段，`user()` 构造器透传。
- [ ] AC2 `persist_transcript`（`facade.rs:685-740`）写 message event 时把 quoted 字段塞进 payload；`record_turn`（`whiteboard.rs:138-178`）加 quoted 参数写入 payload；`group_chat.rs:146` 透传。

**阶段二：IPC 命令 + excerpt 后端提取**
- [ ] AC3 `run_conversation_turn` 加 `quoted_event_seq: Option<i64>` 参数，查 events 取 excerpt（seq 不存在或非 message kind 时静默丢弃）。
- [ ] AC4 `submitTask`/`submitMessage`/`enqueueMessage` 3 命令加可选 `quotedEventSeq: Option<i64>` 参数（camelCase），透传至 `run_conversation_turn`。

**阶段三：bindings 再生 + 前端 client**
- [ ] AC5 `pnpm contracts:gen` 重新生成 bindings；`client.ts` 3 个 wrapper 加可选 `quotedEventSeq`；`test-double-commands.ts` 同步。

**阶段四：前端类型 + 渲染 + 交互**
- [ ] AC6 `ChatEntry` 加 `quotedEventSeq?`/`quotedExcerpt?`；`toEntries` 读 `payload.quoted_event_seq`/`quoted_excerpt` 填充；live `session.message` 分支同步。
- [ ] AC7 新 `QuoteBlock.tsx` 组件（摘录 + 点击跳转）；`Bubble.tsx` 条件渲染 `QuoteBlock` + hover"引用"按钮（`onQuote` 回调）；`SpeakerBubble.tsx` 同上。
- [ ] AC8 `MessageList.tsx` 加 `data-seq` 属性 + `scrollIntoView` 跳转能力。

**阶段五：Composer 引用状态 + 装配**
- [ ] AC9 新 `QuotePreviewBar.tsx`（摘录 + 取消按钮）；`Composer.tsx` 加 `quotedEntry` state + `onSubmit` 签名变 `(input, quotedEventSeq?)` + 渲染 `QuotePreviewBar`。
- [ ] AC10 `ChatView`/`GroupConversationView` 透传 `setQuotedEntry` 给列表→Bubble；submitMut/enqueueMut/submitMessage 透传 `quotedEventSeq`。
- [ ] AC11 群聊引用块取 speaker 名兼容 `role_name`/`role` 两种 payload 键。

**横切**
- [ ] AC12 既有无引用消息渲染行为不变（payload 无 `quoted_*` 键时 `ChatEntry.quotedEventSeq` 为 `undefined`）。
- [ ] AC13 i18n 文案（`chat.quote`/`chat.quoteCancel`/`chat.quotedMessageDeleted`）中英双语。
- [ ] AC14 双端质量门全绿：`cargo test -p nuomi-core && cargo clippy` 与 `pnpm typecheck && pnpm lint && pnpm test`。

## 5. 影响范围 / Impact

| 层 | 文件 | 改动 |
|---|---|---|
| migration | **无** | payload 路线零迁移 |
| domain | `providers/types.rs` | ChatMessage 加 quoted 字段 |
| 内核 | `facade.rs`/`orchestrator/whiteboard.rs`/`orchestrator/group_chat.rs` | persist_transcript/record_turn 写 payload |
| IPC | `tauri_cmds.rs`/`commands.rs`/`lib.rs` | 3 命令加 quotedEventSeq 参数 + excerpt 提取 |
| bindings | `bindings.gen.ts`/`client.ts`/`test-double-commands.ts` | 重新生成 + wrapper |
| 前端 | `useSessionStream.ts`/`Bubble.tsx`/`Composer.tsx`/`ChatView.tsx`/`GroupConversationView.tsx`/`SpeakerBubble.tsx` | 类型 + 渲染 + 交互 |
| 新组件 | `QuoteBlock.tsx`/`QuotePreviewBar.tsx` | 引用块 + 预览条 |
| i18n | `en.json`/`zh-CN.json` | 文案 |
