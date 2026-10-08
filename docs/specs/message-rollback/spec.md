# SPEC: 消息回退 Rollback/Undo（message-rollback）

> 状态：待批准（属跨层变更，需先获用户确认 ADR 0020 再动工）
> 关联 ADR：`docs/adr/0020-message-rollback.md`（Proposed）
> 需求来源：用户请求「RoleAgent 支持消息回退」；需求权威仍为 `pr.md`。
> 事实基础：events 表 `trg_events_no_update/no_delete` 触发器物理强制 append-only；前端无消息级操作入口；`Tool` trait 无 `undo()` 方法；`events::list_by_aggregate`（`events.rs:48-65`）无回退过滤；Run 状态机与 session 事件流是分离聚合；现有 rollback 范式为"回滚=前向新版本"（journal_rollback/steward_rollback_config_change）。

## 1. 决策记录摘要 / Decisions

| # | 决策 |
|---|---|
| D1 | **独立标记表 `event_rollbacks`**（不污染 events 表，`list_by_aggregate` 加 NOT EXISTS 子查询过滤；否决"元事件塞 events 表"——SQL 不友好） |
| D2 | **Run 状态不动**（Run 审计独立保留，回退是用户对话操作不篡改运行审计） |
| D3 | **工具副作用 v1 不撤销 + UI 警告**"工具副作用已保留"（Tool trait 无 undo，v2 再加） |
| D4 | **busy session 先 cancel in-flight 再回退**（参考 `session_cancels.cancel`，不拒绝） |
| D5 | **回退粒度 = user message 边界**（避免切断 tool_call/tool_result 配对；"回退到此处"按钮仅 user message 显示） |
| D6 | **队列消息保留**（回退只影响已落库事件，不影响待处理队列） |
| D7 | **脱敏/快照包含已回退事件**（审计完整性优先，steward 独立 SELECT 不加回退过滤） |

## 2. 目标 / Goals

1. 用户可把对话回退到某个 user message 之前的状态，被回退消息从视图隐藏。
2. 回退不物理删除事件（append-only 铁律），用独立标记表软隐藏。
3. 回退本身是 append-only 前向操作（INSERT 标记表），符合现有 rollback 范式。
4. Run 审计完整保留；脱敏/快照含全部历史。
5. 不回归：无回退记录时 `list_by_aggregate` 行为与现状完全一致。

## 3. 用户故事 / User Stories

- **US1** 用户 hover 某 user message 看到"回退到此处"按钮，点击后弹确认 Dialog；若回退段含工具调用，Dialog 警告"工具副作用已保留"；确认后该消息之后的所有消息从视图隐藏。
- **US2** 用户回退后继续对话，新消息 seq 续接（大于回退前 maxSeq），不重置 seq 计数器。
- **US3** 用户回退时 agent 正 busy，系统先 cancel in-flight turn 再执行回退（不拒绝）。
- **US4** 回退后 message_queue 中的待处理消息保留（用户可手动 clearMessageQueue）。
- **US5** trace 视图/Journal 仍显示全部历史事件（含已回退），审计完整。

## 4. 验收标准 / Acceptance Criteria

**阶段一：迁移 + 实体 + store**
- [ ] AC1 `migrations/0033_event_rollbacks.sql` 新增 `event_rollbacks` 表（id/aggregate_type/aggregate_id/from_seq/to_seq/rolled_back_at/reason）+ `idx_event_rollbacks_agg` 索引。
- [ ] AC2 `EventRollback` struct；新 `store/repos/event_rollbacks.rs`：`insert`/`list`/`exists_for_seq`。
- [ ] AC3 `events::list_by_aggregate`（`events.rs:48-65`）加 NOT EXISTS 子查询过滤被回退 seq。

**阶段二：IPC 命令 + busy session 处理**
- [ ] AC4 `impl_rollback_session_events`：校验 `toSeq` 是 user message seq → 检查 `agent_busy`，busy 则 `session_cancels.cancel` 等 in-flight → 查 maxSeq → INSERT `event_rollbacks`(from_seq=toSeq+1, to_seq=maxSeq) → publish `session.events_rolled_back` 事件 → 返回 `RollbackResultDto { rolledBackCount, containsToolCalls }`。
- [ ] AC5 `impl_list_rollback_points`：返回 user message seq 列表供前端 UI。
- [ ] AC6 `tauri_cmds.rs` 3 命令薄壳 + `lib.rs` 注册。

**阶段三：bindings 再生 + 前端 client**
- [ ] AC7 `pnpm contracts:gen` + `client.ts` 3 wrapper + `test-double-commands.ts` stub。

**阶段四：前端交互入口 + 确认 Dialog**
- [ ] AC8 `Bubble.tsx` 加 hover 按钮"回退到此处"（仅 `kind="message" && payload.role="user"` 显示，`onRollback` 回调）。
- [ ] AC9 `ChatView.tsx` rollback mutation + 回退确认 Dialog（含工具副作用警告"⚠️ 此回退段含 N 次工具调用，工具副作用已保留且不会撤销"）+ onSuccess invalidate `["sessionEvents", sessionId]`。
- [ ] AC10 `MessageList.tsx` 透传 `onRollback` 给 Bubble。

**阶段五：回退后刷新 + 群聊适配**
- [ ] AC11 `useSessionStream.ts` 加 `session.events_rolled_back` 分支，触发 listEvents 刷新。
- [ ] AC12 `GroupConversationView.tsx`/`BackgroundConversationView.tsx` 回退入口 + 刷新。

**横切**
- [ ] AC13 无回退记录时 `list_by_aggregate` 行为与现状完全一致（NOT EXISTS 子查询命中零行）。
- [ ] AC14 Run 状态机零改动；`steward/events.rs`/`steward/cleanse.rs`/`steward/snapshot.rs` 独立 SELECT 不加回退过滤（审计完整）。
- [ ] AC15 `message_queue` 保留（回退不清空队列）。
- [ ] AC16 i18n 文案（`chat.rollbackToHere`/`chat.rollbackConfirm`/`chat.toolSideEffectsRetained`/`chat.rolledBackN`）中英双语。
- [ ] AC17 双端质量门全绿。

## 5. 影响范围 / Impact

| 层 | 文件 | 改动 |
|---|---|---|
| migration | `0033_event_rollbacks.sql` | event_rollbacks 表 + 索引 |
| domain | `entities.rs` | EventRollback struct |
| store | `repos/event_rollbacks.rs`（新）/`repos/events.rs` | 标记表 CRUD + list_by_aggregate 加子查询 |
| IPC | `commands.rs`/`tauri_cmds.rs`/`lib.rs` | 3 命令 + RollbackResultDto |
| bindings | `bindings.gen.ts`/`client.ts`/`test-double-commands.ts` | 重新生成 + 3 wrapper |
| 前端 | `Bubble.tsx`/`ChatView.tsx`/`useSessionStream.ts`/`MessageList.tsx`/`GroupConversationView.tsx`/`BackgroundConversationView.tsx` | 交互入口 + 确认 Dialog + 刷新 |
| i18n | `en.json`/`zh-CN.json` | 文案 |
| 不涉及 | `run_state.rs`/`steward/*`/`message_queue.rs` | Run 状态机零改动 + steward 审计完整 + 队列保留 |
