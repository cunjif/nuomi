# ADR 0015: 对话消息队列

- **状态**: Proposed
- **日期**: 2026-09-19
- **决策者**: user + conductor

## 背景

当前对话系统一次只能处理一条消息：`submitTask` IPC 同步等待 `run_conversation_turn` 完成，前端 `Composer` 在 `pending` 时禁用输入框。用户无法连续发送多条消息。

需求：
- **单聊**：用户连续发送多条消息，RoleAgent 处理上一条期间剩余消息排队，处理完自动取下一条
- **群聊**：Selector 逐条处理，处理完一条后从队列取下一条
- 队列消息列表显示在输入框上方

## 决策

### 1. 队列存储：后端 SQLite `message_queue` 表

```sql
CREATE TABLE message_queue (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL,
    text        TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'queued',  -- queued | processing | done
    seq         INTEGER NOT NULL,                 -- 入队顺序
    created_at  INTEGER NOT NULL
);
CREATE INDEX idx_queue_session ON message_queue (session_id, status, seq);
```

### 2. 忙碌状态：`sessions.agent_busy` 列

```sql
ALTER TABLE sessions ADD COLUMN agent_busy INTEGER NOT NULL DEFAULT 0;
```

turn 开始时 `UPDATE sessions SET agent_busy = 1`，完成时 `SET agent_busy = 0`。

### 3. IPC 命令设计

- **`submitTask`**：保持不变（直接跑 turn，用于队列空时直接发送）。开始时设 busy=1，完成时设 busy=0 + 发 `session.turn_end` 事件。
- **`enqueueMessage(sessionId, text)`**：入队 `message_queue` 表。如果 `agent_busy = 0`，spawn 后台处理循环。
- **`listMessageQueue(sessionId)`**：返回 `status = 'queued'` 的队列消息列表。
- **`cancelMessageQueueItem(id)`**：删除单条队列消息。
- **`clearMessageQueue(sessionId)`**：清空 session 的所有 queued 消息。

### 4. 后台处理循环（后端驱动）

`enqueueMessage` 入队后，如果 `agent_busy = 0`，spawn async 任务：
```
while queue has 'queued' item for session:
    dequeue oldest → set status='processing', agent_busy=1
    run_conversation_turn(session, text)
    set status='done', agent_busy=0
    emit session.turn_end { sessionId, queueRemaining }
```

前端不驱动 dequeue，后端自洽。IPC `enqueueMessage` 立即返回（不等处理完成）。

### 5. turn 完成事件：新增 `session.turn_end`

当前 `session.end` 事件 payload 缺 `sessionId`，被 `partition_event` 丢弃。新增 `session.turn_end` 事件，payload `{ sessionId, queueRemaining }`，路由到 `event://session/{sid}`。前端监听后刷新队列列表。

### 6. 前端交互

- `Composer`：移除 `pending` 时 disabled 逻辑，允许 busy 时继续输入发送
- `ChatView.onSubmit`：busy 时调 `enqueueMessage`，idle 时调 `submitTask`
- `QueueList` 组件：渲染在 `Composer` 的 `topSlot`（textarea 上方），显示 queued 消息列表 + 单条取消
- `useSessionStream`：加 `session.turn_end` 分支，触发 `listMessageQueue` 刷新

### 7. 群聊队列

群聊保留 Selector 模式（ADR 0013）：Selector 逐条处理队列消息，每条消息仍由 Selector 选择谁回复。队列消费逻辑与单聊相同（busy=1 → 入队，turn 完成后取下一条）。

## 影响范围

### Migration
- `migrations/0023_message_queue.sql`：`message_queue` 表 + `sessions.agent_busy` 列

### 后端
- `crates/nuomi-core/src/domain/`：`MessageQueueEntry` 实体 + `QueueStatus` 枚举
- `crates/nuomi-core/src/store/repos/message_queue.rs`：enqueue/dequeue/list/cancel/clear
- `crates/nuomi-core/src/store/repos/sessions.rs`：`set_busy` / `get_busy`
- `src-tauri/src/commands.rs`：`impl_enqueue_message` / `impl_list_message_queue` / `impl_cancel_message_queue_item` / `impl_clear_message_queue`；`run_conversation_turn` 加 busy 落库 + turn_end 事件
- `src-tauri/src/tauri_cmds.rs` + `lib.rs`：注册 4 个新 IPC 命令
- `src-tauri/src/events.rs`：`session.turn_end` 路由（已有 `session.*` 匹配，无需改）

### 前端
- `src/lib/ipc/bindings.gen.ts`：重新生成
- `src/lib/ipc/client.ts`：4 个新 wrapper
- `src/features/chat/ChatView.tsx`：onSubmit 区分 busy/idle
- `src/features/conversation/composer/Composer.tsx`：移除 pending disabled
- `src/features/conversation/composer/QueueList.tsx`：新建，渲染在 topSlot
- `src/features/chat/useSessionStream.ts`：加 `session.turn_end` 分支
- i18n：队列相关 keys

## 权衡

- **优点**：后端自洽（不依赖前端驱动 dequeue）；持久化（刷新不丢队列）；busy 状态可观测
- **代价**：新表 + 新列 + 4 个 IPC 命令 + 后台处理循环；`submitTask` 与 `enqueueMessage` 双入口需协调
- **替代方案**：前端驱动 dequeue（改动小但依赖前端在线）— 否决，因用户关闭页面后队列不处理

## 实施计划

1. migration 0023 + 实体 + repo
2. `run_conversation_turn` 加 busy 落库 + turn_end 事件
3. `enqueueMessage` + 后台处理循环
4. IPC 命令注册 + bindings 生成
5. 前端 `QueueList` + `ChatView` 改造 + `useSessionStream` 适配
6. i18n + 测试
