# ADR 0020: 消息回退（Rollback/Undo）

- **状态**: Proposed
- **日期**: 2026-10-07
- **决策者**: user + conductor

## 背景

当前对话不支持消息回退（rollback/undo）。具体缺口：

1. `events` 表（`migrations/0001_init.sql:12-32`）有 `trg_events_no_update/no_delete` 触发器**物理强制 append-only**，无法 UPDATE/DELETE。
2. 前端**完全无消息级操作入口**：`Bubble.tsx`（`src/features/chat/Bubble.tsx:13-40`）纯渲染，无右键菜单/hover 按钮；`MessageList.tsx` 无删除/编辑入口；`Composer.tsx` 不支持编辑已发送消息。
3. **工具副作用撤销机制从零开始**：`Tool` trait（`plugins/tools.rs:22-25`）无 `undo()` 方法，`ToolDef`（`providers/types.rs:76-81`）无 `reversible` 标记；`Context::effect`/`Disposer` 机制（`harness/context.rs:150-212`）存在但生产代码零调用。
4. 唯一通用读取入口 `events::list_by_aggregate`（`events.rs:48-65`）**无任何回退过滤**；session 聚合有 8 处调用。
5. Run 状态机（`run_state.rs:26-35`）与 session 事件流是分离的聚合（`aggregate_type="run"` vs `"session"`），回退消息不直接动 Run。

现有 rollback 范式（可参考）：`journal_rollback`（`commands.rs:6915`）和 `steward_rollback_config_change`（`commands.rs:7699`）都是**回滚本身是 append-only 的前向操作**（追加 `RolledBack` 审计条目 / 写回 before 快照 + publish 事件），从不 DELETE/UPDATE 历史。

需求：用户可把对话回退到某个 user message 之前的状态，被回退的消息从视图隐藏（不物理删除，保留审计）。

## 决策

### 设计决策

| # | 决策点 | 选择 |
|---|---|---|
| 1 | 回退持久化机制 | **独立标记表 `event_rollbacks`**（不污染 events 表，查询友好） |
| 2 | Run 状态同步回退 | **不动**（Run 审计独立保留） |
| 3 | 工具副作用撤销 | **v1 不撤销 + UI 警告**"工具副作用已保留"（方案 a+d） |
| 4 | busy session 回退 | **先 cancel in-flight 再回退**（参考 `session_cancels.cancel`） |
| 5 | 回退粒度 | **只能回退到 user message 边界**（避免切断 tool_call/tool_result 配对） |
| 6 | 队列消息处理 | **保留**（回退只影响已落库事件，不影响待处理队列） |
| 7 | 脱敏/快照是否含已回退事件 | **包含**（审计完整性优先，脱敏/快照需看到全部历史） |

### 1. 回退持久化：独立标记表

**迁移** `migrations/0033_event_rollbacks.sql`：

```sql
CREATE TABLE event_rollbacks (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
    aggregate_type  TEXT NOT NULL,
    aggregate_id    TEXT NOT NULL,
    from_seq        INTEGER NOT NULL,   -- 被回退段起始 seq（含）
    to_seq          INTEGER NOT NULL,   -- 被回退段结束 seq（含）
    rolled_back_at  INTEGER NOT NULL,
    reason          TEXT
);
CREATE INDEX idx_event_rollbacks_agg ON event_rollbacks (aggregate_type, aggregate_id, from_seq, to_seq);
```

**不污染 events 表**：`events_rolled_back` 元事件方案（塞 events 表）会让业务事件混入元事件，语义不清；独立表更清晰、可索引、可审计。

**查询过滤**（`events::list_by_aggregate` 加子查询）：

```sql
SELECT ... FROM events e
WHERE e.aggregate_type = ?1 AND e.aggregate_id = ?2 AND e.seq > ?3
  AND NOT EXISTS (
    SELECT 1 FROM event_rollbacks r
    WHERE r.aggregate_type = e.aggregate_type
      AND r.aggregate_id = e.aggregate_id
      AND e.seq BETWEEN r.from_seq AND r.to_seq
  )
ORDER BY e.seq ASC
```

### 2. 回退粒度：user message 边界

回退只能以 user message 为边界（"回退到此处"按钮仅 `kind="message" && payload.role="user"` 显示）。回退到 user message seq=N 时，回退段为 `(N, 当前最大 seq]`——即该 user message 之后的所有事件。

**理由**：
- 避免切断 tool_call/tool_result 配对（回退到 tool_call 之前但 tool_result 之后会留下孤儿）
- 语义清晰（"撤销我刚才的提问和 agent 的回复"）
- UI 简单（只在 user message 显示回退入口）

### 3. 工具副作用：v1 不撤销 + UI 警告

回退只软隐藏事件，工具已产生的外部副作用（文件写入、shell 执行、git 操作）保留。

**UI 警告**：若回退段含 `tool_call` 事件，回退确认 Dialog 显式提示"⚠️ 此回退段含 N 次工具调用，工具副作用（文件/shell/git）已保留且不会撤销"。

**v2 方向**（不在本 ADR 范围）：
- Tool trait 加 `fn undo(&self, args, result) -> Option<Disposer>`，工具可选实现
- 或独立 `tool_side_effects` 表登记副作用，回退时按清单补偿
- 或只读工具白名单 `reversible=true`，写类工具回退时拒绝

### 4. busy session 回退：先 cancel in-flight

回退时若 `sessions.agent_busy = 1`，先 `state.session_cancels.cancel(&session_id)`（参考 `commands.rs:4934` `impl_delete_conversation` 模式）等 in-flight turn 结束，再执行回退。

**不拒绝回退**：用户期望回退能即时生效，cancel in-flight 后回退比拒绝更友好。

### 5. Run 状态：不动

回退 session 事件不回退对应 Run 行状态。Run 审计记录（`tasks_runs` 表 + run 聚合事件）独立保留。

**权衡**：Run 详情抽屉可能与对话视图不一致（对话回退了但 Run 显示 succeeded），但 Run 审计完整性优先——回退是用户对话操作，不应篡改运行审计。

### 6. 队列消息：保留

回退不清空 `message_queue`。回退只影响已落库事件，待处理队列消息保留（用户可手动 `clearMessageQueue` 清空）。

### 7. IPC 命令

| 命令 | 签名 | 说明 |
|---|---|---|
| `rollbackSessionEvents` | `(sessionId, toSeq)` | 回退到 `toSeq`（含）之前，即回退段 `(toSeq, maxSeq]`；`toSeq` 必须是 user message seq |
| `listRollbackPoints` | `(sessionId)` | 列出可回退点（user message seq 列表），供前端 UI |
| `restoreRolledbackEvents` | `(sessionId, rollbackId)` | 恢复某次回退（可选，进阶） |

`rollbackSessionEvents` 流程：
1. 校验 `toSeq` 是 user message seq（否则报错）
2. 检查 `agent_busy`，busy 则 `session_cancels.cancel` 等 in-flight 结束
3. 查 `events::list_by_aggregate` 取 `maxSeq`
4. INSERT `event_rollbacks`（from_seq=toSeq+1, to_seq=maxSeq）
5. 发布 `session.events_rolled_back` 事件（前端监听后刷新）
6. 返回 `{ rolledBackCount, containsToolCalls }`

## 向后兼容

- `event_rollbacks` 新表，既有数据无回退记录，`list_by_aggregate` 子查询 NOT EXISTS 命中零行，行为与现状完全一致。
- IPC 命令新增，既有前端不调用则无影响。
- `events` 表零改动，append-only 触发器无冲突。
- Run 状态机零改动。

## 权衡

- **优点**：独立标记表查询友好（SQL 子查询/索引）；events 表零改动不破坏 append-only 铁律；回退本身是 append-only 前向操作（INSERT 标记表），符合现有 rollback 范式；Run 审计完整保留。
- **代价**：1 迁移（新表）+ store 层 `list_by_aggregate` 加子查询 + 3 IPC 命令 + 前端 Bubble 加交互入口 + 回退确认 Dialog；工具副作用不撤销（v1 接受，UI 警告）。
- **替代方案**：
  - (a) 追加 `events_rolled_back` 元事件（塞 events 表）——否决。查询时需应用层展开 from_seq~to_seq 范围过滤，SQL 不友好；元事件混入业务事件语义不清。
  - (b) 直接 UPDATE events 加 `rolled_back_at` 列——否决。events 表 `trg_events_no_update` 触发器禁止 UPDATE。
  - (c) 工具副作用 v1 即撤销——否决。Tool trait 无 undo，需改所有内置工具，复杂度高；v1 先不撤销 + UI 警告，v2 再加。
  - (d) 回退时同步回退 Run 状态——否决。Run 审计完整性优先；回退是用户对话操作，不应篡改运行审计。
  - (e) 回退粒度任意 seq——否决。可能切断 tool_call/tool_result 配对，语义混乱。
  - (f) busy session 拒绝回退——否决。cancel in-flight 后回退比拒绝更友好。
  - (g) 回退清空队列——否决。回退只影响已落库事件，队列消息保留（用户可手动清空）。

## 风险与缓解

| 风险 | 缓解 |
|---|---|
| `list_by_aggregate` 子查询性能 | `event_rollbacks` 表加索引；回退是低频操作，标记表行数远小于 events |
| 工具副作用不撤销导致语义不一致 | UI 显式警告"工具副作用已保留"；v2 加 undo 机制 |
| 回退后新事件 seq 续接 | events 表 seq 单调递增，回退不重置 seq 计数器，新消息 seq > maxSeq，无冲突 |
| cancel in-flight 后回退竞态 | `session_cancels.cancel` 等待 in-flight 完成后再 INSERT 标记表；标记表 INSERT 在事务内 |
| 脱敏/快照包含已回退事件 | 决策 7 明确包含（审计完整性）；`steward/cleanse.rs` 和 `steward/snapshot.rs` 的独立 SELECT 不加回退过滤 |
| 恢复回退（restore）复杂度 | v1 可不实现 restore；`restoreRolledbackEvents` 标为可选进阶 |
| tauri-specta bindings 需 regen | 实施计划阶段 3 显式 `pnpm contracts:gen` |
| 8 处 `list_by_aggregate` 调用点行为变更 | 子查询对所有调用方透明生效；session 聚合 8 处自动过滤；steward 独立 SELECT 不受影响 |

## 影响范围

### Migration
- **新增** `migrations/0033_event_rollbacks.sql` — `event_rollbacks` 表 + 索引

### 后端实体
- `crates/nuomi-core/src/domain/entities.rs` — 新增 `EventRollback` struct
- `crates/nuomi-core/src/domain/entities.rs:182-195` — `EventRecord` 无需改（回退标记在独立表）

### Store 层
- **新增** `crates/nuomi-core/src/store/repos/event_rollbacks.rs` — `insert`/`list`/`exists_for_seq`
- `crates/nuomi-core/src/store/repos/events.rs:48-65` — `list_by_aggregate` 加 NOT EXISTS 子查询过滤
- `crates/nuomi-core/src/store/repos/journal.rs:75,81,96` — 审查 journal sink session 读取（自动继承过滤）

### IPC 契约
- `src-tauri/src/commands.rs` — `impl_rollback_session_events`/`impl_list_rollback_points`/`impl_restore_rolledback_events`；`RollbackResultDto { rolledBackCount, containsToolCalls }`
- `src-tauri/src/tauri_cmds.rs` — 3 命令薄壳
- `src-tauri/src/lib.rs` — `collect_commands!` 注册
- `src/lib/ipc/bindings.gen.ts` — 重新生成
- `src/lib/ipc/client.ts` — 3 个 wrapper
- `src/lib/ipc/test-double-commands.ts` — test double stub

### 前端
- `src/features/chat/Bubble.tsx` — 加 hover 按钮"回退到此处"（仅 user message 显示）
- `src/features/chat/ChatView.tsx` — 加 rollback mutation + onSuccess invalidate `["sessionEvents", sessionId]` + 回退确认 Dialog（含工具副作用警告）
- `src/features/chat/useSessionStream.ts` — 加 `session.events_rolled_back` 分支，触发刷新
- `src/features/chat/MessageList.tsx` — 顶部"已回退 N 条 [恢复]"横幅（可选）
- `src/lib/ipc/client.ts` — `rollbackSessionEvents`/`listRollbackPoints`/`restoreRolledbackEvents` wrapper
- i18n `src/i18n/locales/{en,zh-CN}.json` — `chat.rollbackToHere`/`chat.rollbackConfirm`/`chat.toolSideEffectsRetained`/`chat.rolledBackN` 文案
- 审查 `src/features/trace/JournalView.tsx:40`、`TraceView.tsx:57`、`RunHistoryStrip.tsx:15`、`GroupConversationView.tsx:47`、`BackgroundConversationView.tsx:18` — trace/群聊/调度/后台视图的 listEvents 是否需反映回退（trace/Run 审计保留全部，群聊/后台对话视图应反映回退）

### 不涉及
- `domain/run_state.rs` — Run 状态机零改动（决策 2）
- `steward/events.rs:184-222`、`steward/cleanse.rs:285-290`、`steward/snapshot.rs:178` — steward 独立 SELECT 不加回退过滤（决策 7，审计完整）
- `store/repos/message_queue.rs` — 队列保留（决策 6）

## 实施计划

分 5 阶段，每阶段独立可验证、可提交。

### 阶段 1：迁移 + 实体 + store 层

1. `migrations/0033_event_rollbacks.sql`：`event_rollbacks` 表 + 索引
2. `entities.rs`：`EventRollback` struct
3. 新 `store/repos/event_rollbacks.rs`：`insert`/`list`/`exists_for_seq`
4. `store/repos/events.rs:48-65`：`list_by_aggregate` 加 NOT EXISTS 子查询
5. `cargo test -p nuomi-core` + `cargo clippy` 验证
6. 提交："feat(store): event_rollbacks 标记表 + list_by_aggregate 回退过滤"

### 阶段 2：IPC 命令 + busy session 处理

1. `commands.rs`：`impl_rollback_session_events`（校验 user message seq + cancel in-flight + INSERT 标记表 + publish 事件）+ `impl_list_rollback_points` + `RollbackResultDto`
2. `tauri_cmds.rs`：3 命令薄壳
3. `lib.rs`：`collect_commands!` 注册
4. `cargo test` 验证
5. 提交："feat(ipc): rollbackSessionEvents 命令 + busy session cancel 处理"

### 阶段 3：重生 bindings + 前端 client

1. `pnpm contracts:gen`
2. `src/lib/ipc/client.ts`：3 个 wrapper
3. `src/lib/ipc/test-double-commands.ts`：stub
4. `pnpm typecheck` 验证
5. 提交："feat(ipc): 前端 client rollback 命令 wrapper"

### 阶段 4：前端交互入口 + 确认 Dialog

1. `Bubble.tsx`：加 hover 按钮"回退到此处"（仅 user message，通过 `onRollback` 回调）
2. `ChatView.tsx`：rollback mutation + 回退确认 Dialog（含工具副作用警告）+ onSuccess invalidate
3. `MessageList.tsx`：透传 `onRollback` 给 Bubble
4. i18n 文案
5. `pnpm typecheck && pnpm test` 验证
6. 提交："feat(chat): 消息回退交互入口 + 确认 Dialog"

### 阶段 5：回退后刷新 + 群聊适配

1. `useSessionStream.ts`：加 `session.events_rolled_back` 分支，触发 listEvents 刷新
2. `GroupConversationView.tsx`：回退入口 + 刷新
3. `BackgroundConversationView.tsx`：刷新（审查）
4. `pnpm typecheck && pnpm test` 验证
5. 提交："feat(chat): 回退后事件刷新 + 群聊适配"

## 验证门禁

每阶段必须通过：
- `cargo test -p nuomi-core` + `cargo clippy`（涉及 Rust 时）
- `pnpm typecheck && pnpm lint`
- `pnpm test`（单线程模式，memory `feedback-test-suite-single-thread`）
- 人工验证：`pnpm tauri dev` 启动后单聊中回退到某 user message，被回退消息隐藏，新消息 seq 续接；含工具调用的回退段显示警告；trace 视图仍显示全部历史
