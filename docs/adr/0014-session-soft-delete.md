# ADR 0014: Session 软删除

- **状态**: Accepted
- **日期**: 2026-09-19
- **决策者**: user + conductor

## 背景

ADR 0013 落地后，新增"删除会话"与"清空工作区会话"功能（`delete_conversation` / `clear_conversations` IPC）。初版实现尝试级联物理删除 `events` 表记录，触发 `trg_events_no_delete` 触发器报错 `events is append-only`。

`events` 表的 append-only 是 AGENTS.md §4 铁律："EventRecord 事件 - 只追加的事件日志"，由 `migrations/0001_init.sql` 的 `trg_events_no_delete` / `trg_events_no_update` 触发器强制。物理删除 events 违背此铁律。

## 决策

**对 `sessions` 表采用软删除（tombstone）模式，不删除任何关联数据。**

1. `sessions` 表新增 `deleted_at INTEGER` 列（NULL = 未删除，非 NULL = 软删除时间戳，unix-ms）
2. `delete(session_id)` 改为 `UPDATE sessions SET deleted_at = ? WHERE id = ? AND deleted_at IS NULL`
3. `delete_all_for_workspace` 循环对未删除会话调用 `delete`
4. 所有读取会话的查询加 `deleted_at IS NULL` 过滤：`get` / `list` / 孤儿会话查询
5. 关联数据（events / conversation_participants / session_cli_handles / conversation_todos）全部保留，不物理删除
   - events 保留符合 append-only 铁律
   - 参与者/cli handles/todos 保留使软删除可恢复

## 影响范围

### Migration
- `migrations/0022_session_soft_delete.sql`: `ALTER TABLE sessions ADD COLUMN deleted_at INTEGER`

### 实体
- `Session` struct 加 `deleted_at: Option<i64>` 字段
- `row_to_session` 读取新列
- `insert` / `new_chat` 不写 deleted_at（默认 NULL）

### 查询适配（加 `deleted_at IS NULL`）
- `sessions::get` — 已删除的 get 返回 NotFound
- `sessions::list` — 已删除的不在列表
- `sessions::delete_all_for_workspace` 内部 SELECT — 只软删未删除的
- `commands.rs` 孤儿会话查询 — 已删除的孤儿不显示

### 删除函数改造
- `sessions::delete` — 改为 UPDATE 软删除
- `sessions::delete_all_for_workspace` — 逻辑不变（循环调用 delete）

### 不变
- `ConversationDto` 不加 `deleted_at`（已删除的不返回，DTO 永远是未删除的）
- `touch` / `update_title` / `update_meta` / `update_kind` / `set_workspace_id` / `cache_scope` — 按 id 更新，不影响已删除的（且已删除的不会被查到）
- `journal.rs` 测试辅助函数 — 测试代码，不适配
- IPC bindings — `Session` 无 `specta::Type`，`ConversationDto` 结构不变，无需重新生成

## 权衡

- **优点**: 不违背 append-only 铁律；可恢复；最小改动（1 migration + 1 实体字段 + 4 查询过滤）
- **代价**: 软删除的会话行及关联数据留存数据库，占用存储空间；未来如需物理清理需单独 GC 机制
- **替代方案**: (a) 留孤儿 events（最小改动但不可恢复）(b) 临时禁用触发器硬删（违背铁律）— 均否决

## 实施计划

1. migration 0022
2. Session 实体 + row_to_session + insert 适配
3. sessions.rs 查询适配 + delete 改软删除
4. commands.rs 孤儿查询适配
5. 编译 + 测试验证
