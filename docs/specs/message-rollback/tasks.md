# TASKS: 消息回退 Rollback/Undo（message-rollback）

> 对应 SPEC：`docs/specs/message-rollback/spec.md`（待批准）
> 对应 ADR：`docs/adr/0020-message-rollback.md`（Proposed）
> 验收标准：AC1-AC17（详见 spec.md §4）
> 任务规划原则：垂直切割 · 契约先行 · 原子性 · 有序性

## 依赖关系与优先级总览

| 任务组 | 优先级 | 前置依赖 | 验收映射 | 提交 |
|--------|--------|----------|----------|------|
| 1. 迁移 + 实体 + store | 高 | 无 | AC1-AC3 | feat(store) |
| 2. IPC 命令 + busy session | 高 | 任务 1 | AC4-AC6 | feat(ipc) |
| 3. bindings 再生 + client | 高 | 任务 2 | AC7 | feat(ipc) |
| 4. 前端交互 + 确认 Dialog | 中 | 任务 3 | AC8-AC10 | feat(chat) |
| 5. 回退后刷新 + 群聊适配 | 中 | 任务 4 | AC11-AC12 | feat(chat) |
| 6. i18n + 质量门 | — | 任务 1-5 | AC13-AC17 | chore |

**串行策略**：任务 1→2→3 严格串行（store→IPC→bindings 契约先行）；任务 4→5 串行（交互先于刷新）；任务 6 收尾。

---

## 1. 迁移 + 实体 + store 层

> 目标：event_rollbacks 标记表 + list_by_aggregate 回退过滤。
> 范围：migrations + crates/nuomi-core（实体 + store）。

- [ ] 1.1 新建 `migrations/0033_event_rollbacks.sql`：`CREATE TABLE event_rollbacks (id INTEGER PRIMARY KEY AUTOINCREMENT, aggregate_type TEXT NOT NULL, aggregate_id TEXT NOT NULL, from_seq INTEGER NOT NULL, to_seq INTEGER NOT NULL, rolled_back_at INTEGER NOT NULL, reason TEXT);` + `CREATE INDEX idx_event_rollbacks_agg ON event_rollbacks (aggregate_type, aggregate_id, from_seq, to_seq);`
- [ ] 1.2 `crates/nuomi-core/src/domain/entities.rs`：新增 `EventRollback` struct（id/aggregate_type/aggregate_id/from_seq/to_seq/rolled_back_at/reason）
- [ ] 1.3 新建 `crates/nuomi-core/src/store/repos/event_rollbacks.rs`：`insert(conn, rollback)`/`list(conn, aggregate_type, aggregate_id)`/`exists_for_seq(conn, aggregate_type, aggregate_id, seq)`
- [ ] 1.4 `crates/nuomi-core/src/store/repos/events.rs:48-65` `list_by_aggregate`：加 NOT EXISTS 子查询过滤——`AND NOT EXISTS (SELECT 1 FROM event_rollbacks r WHERE r.aggregate_type = e.aggregate_type AND r.aggregate_id = e.aggregate_id AND e.seq BETWEEN r.from_seq AND r.to_seq)`
- [ ] 1.5 审查 `crates/nuomi-core/src/store/repos/journal.rs:75,81,96` journal sink session 读取（自动继承 list_by_aggregate 过滤）
- [ ] 1.6 `cargo test -p nuomi-core` + `cargo clippy` 验证
- [ ] 1.7 提交："feat(store): event_rollbacks 标记表 + list_by_aggregate 回退过滤"

---

## 2. IPC 命令 + busy session 处理

> 目标：rollbackSessionEvents 命令 + busy session cancel 处理。
> 范围：src-tauri（commands + tauri_cmds）。

- [ ] 2.1 `src-tauri/src/commands.rs` `impl_rollback_session_events(state, session_id, to_seq)`：
  - 校验 `to_seq` 是 user message seq（查 events 确认 kind="message" && payload.role="user"，否则报错）
  - 检查 `sessions.agent_busy`，busy 则 `state.session_cancels.cancel(&session_id)` 等 in-flight 结束
  - 查 `events::list_by_aggregate` 取 `maxSeq`
  - INSERT `event_rollbacks`(aggregate_type="session", aggregate_id=session_id, from_seq=to_seq+1, to_seq=maxSeq, rolled_back_at=now)
  - 发布 `session.events_rolled_back` 事件（payload: { sessionId, rolledBackCount, containsToolCalls }）
  - 返回 `RollbackResultDto { rolledBackCount, containsToolCalls }`
- [ ] 2.2 `src-tauri/src/commands.rs` `impl_list_rollback_points(state, session_id)`：返回 user message seq 列表（查 events kind="message" && payload.role="user"）
- [ ] 2.3 `src-tauri/src/commands.rs`：新增 `RollbackResultDto { rolledBackCount: i64, containsToolCalls: bool }`
- [ ] 2.4 `src-tauri/src/tauri_cmds.rs`：`rollback_session_events`/`list_rollback_points`/`restore_rolledback_events`（可选）3 命令薄壳
- [ ] 2.5 `src-tauri/src/lib.rs` `collect_commands!`：注册命令
- [ ] 2.6 `cargo test` 验证
- [ ] 2.7 提交："feat(ipc): rollbackSessionEvents 命令 + busy session cancel 处理"

---

## 3. 重生 bindings + 前端 client

> 目标：重新生成 TS bindings，前端 client 加 rollback wrapper。
> 范围：src/lib/ipc。

- [ ] 3.1 `pnpm contracts:gen` 重新生成 `src/lib/ipc/bindings.gen.ts`
- [ ] 3.2 `src/lib/ipc/client.ts`：`rollbackSessionEvents`/`listRollbackPoints`/`restoreRolledbackEvents` 3 个 wrapper
- [ ] 3.3 `src/lib/ipc/test-double-commands.ts`：3 个 stub
- [ ] 3.4 `pnpm typecheck` 验证
- [ ] 3.5 提交："feat(ipc): 前端 client rollback 命令 wrapper"

---

## 4. 前端交互入口 + 确认 Dialog

> 目标：Bubble 加"回退到此处"按钮，ChatView 回退确认 Dialog。
> 范围：src/features/chat。

- [ ] 4.1 `src/features/chat/Bubble.tsx`：加 hover 按钮"回退到此处"（仅 `kind==="message" && role==="user"` 显示，`onRollback(seq)` 回调）
- [ ] 4.2 `src/features/chat/MessageList.tsx`：透传 `onRollback` 给 Bubble
- [ ] 4.3 `src/features/chat/ChatView.tsx`：加 rollback mutation（调 `rollbackSessionEvents`）+ 回退确认 Dialog
  - Dialog 显示"回退到此处？将隐藏此消息之后的 N 条消息"
  - 若 `RollbackResultDto.containsToolCalls === true`，Dialog 加警告"⚠️ 此回退段含工具调用，工具副作用（文件/shell/git）已保留且不会撤销"
  - onSuccess invalidate `["sessionEvents", sessionId]` query
- [ ] 4.4 `pnpm typecheck && pnpm test` 验证
- [ ] 4.5 提交："feat(chat): 消息回退交互入口 + 确认 Dialog"

---

## 5. 回退后刷新 + 群聊适配

> 目标：回退后事件刷新，群聊/后台视图适配。
> 范围：src/features/chat + conversation/group + conversation/background。

- [ ] 5.1 `src/features/chat/useSessionStream.ts`：加 `session.events_rolled_back` 事件分支，触发 listEvents 刷新
- [ ] 5.2 `src/features/conversation/group/GroupConversationView.tsx`：回退入口（SpeakerBubble 加"回退到此处"）+ 刷新
- [ ] 5.3 审查 `src/features/conversation/background/BackgroundConversationView.tsx`：回退后刷新
- [ ] 5.4 审查 `src/features/trace/JournalView.tsx`/`TraceView.tsx`/`RunHistoryStrip.tsx`：trace/Run 审计保留全部历史（不加回退过滤，决策 D7）
- [ ] 5.5 `pnpm typecheck && pnpm test` 验证
- [ ] 5.6 提交："feat(chat): 回退后事件刷新 + 群聊适配"

---

## 6. i18n + 质量门

- [ ] 6.1 `src/i18n/locales/zh-CN.json` + `en.json`：新增 `chat.rollbackToHere`/`chat.rollbackConfirm`/`chat.toolSideEffectsRetained`/`chat.rolledBackN` 文案
- [ ] 6.2 `pnpm typecheck && pnpm lint && pnpm test`（单线程模式）全绿
- [ ] 6.3 `cargo test -p nuomi-core && cargo clippy` 全绿
- [ ] 6.4 人工验证：`pnpm tauri dev` 启动后单聊中回退到某 user message，被回退消息隐藏，新消息 seq 续接；含工具调用的回退段显示警告；trace 视图仍显示全部历史
- [ ] 6.5 提交："chore(i18n): 消息回退文案 + 质量门验证"
