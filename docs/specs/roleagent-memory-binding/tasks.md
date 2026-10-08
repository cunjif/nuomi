# TASKS: 会话记忆 IPC 暴露 + RoleAgent 绑定（roleagent-memory-binding）

> 对应 SPEC：`docs/specs/roleagent-memory-binding/spec.md`（待批准）
> 对应 ADR：`docs/adr/0019-roleagent-memory-binding.md`（Proposed）
> 验收标准：AC1-AC14（详见 spec.md §4）
> 任务规划原则：垂直切割 · 契约先行 · 原子性 · 有序性

## 依赖关系与优先级总览

| 任务组 | 优先级 | 前置依赖 | 验收映射 | 提交 |
|--------|--------|----------|----------|------|
| 1. 迁移 + 实体 + store | 高 | 无 | AC1-AC2 | feat(store) |
| 2. MemoryService + Policy 消费 | 高 | 任务 1 | AC3-AC4 | feat(plugins) |
| 3. IPC 命令暴露 + bindings | 高 | 任务 2 | AC5-AC6 | feat(ipc) |
| 4. 自动记忆注入 | 中 | 任务 1-2 | AC7-AC9 | feat(core) |
| 5. 前端 Memory 面板 | 中 | 任务 3 | AC10 | feat(settings) |
| 6. RoleForm 记忆配置 + 质量门 | 中 | 任务 3,5 | AC11-AC14 | feat(settings) |

**串行策略**：任务 1→2→3 严格串行（store→service→IPC 契约先行）；任务 4 依赖 1-2 但可与 3 并行；任务 5 依赖 3；任务 6 收尾。

---

## 1. 迁移 + 实体 + store 层

> 目标：memory_entries 加 role_id 列，支持 per-agent 记忆隔离。
> 范围：migrations + crates/nuomi-core（实体 + store）。

- [ ] 1.1 新建 `migrations/0032_memory_role_binding.sql`：`ALTER TABLE memory_entries ADD COLUMN role_id TEXT REFERENCES roles(id);` + `CREATE INDEX idx_memory_role ON memory_entries(role_id);`（FTS5 触发器无需改动）
- [ ] 1.2 `crates/nuomi-core/src/domain/entities.rs:572-585`：`MemoryEntry` 加 `role_id: Option<String>` 字段
- [ ] 1.3 `crates/nuomi-core/src/store/repos/memory.rs:8-25` `insert`：写入 `role_id` 列
- [ ] 1.4 `crates/nuomi-core/src/store/repos/memory.rs:29-67,80-103` `search`/`search_fts`：新增 `role_id: Option<String>` 过滤参数（`WHERE role_id = ? OR role_id IS NULL`——含全局记忆）
- [ ] 1.5 `crates/nuomi-core/src/store/repos/memory.rs:145-157` `row_to_memory`：读 `role_id` 列
- [ ] 1.6 `cargo test -p nuomi-core` + `cargo clippy` 验证
- [ ] 1.7 提交："feat(store): memory_entries 加 role_id 列支持 per-agent 记忆"

---

## 2. MemoryService + MemoryPolicy 消费

> 目标：MemoryService 加 role_id 参数，消费 retrieval 策略 + retention 清理。
> 范围：crates/nuomi-core/src/plugins/memory.rs。

- [ ] 2.1 `plugins/memory.rs:59-84` `MemoryService::remember`：加 `role_id: Option<String>` 参数
- [ ] 2.2 `plugins/memory.rs:87-95` `MemoryService::recall`：加 `role_id: Option<String>` 参数；按 `self.memory_policy.retrieval` 分派：`Keyword`→`search`，`Semantic`/`Hybrid`→`search_fts`（FTS5 已就绪）
- [ ] 2.3 `plugins/memory.rs` 新增 `enforce_retention(&self, retention_days: u32)`：删除 `created_at < now - retention_days*86400_000` 的行
- [ ] 2.4 `remember` 内触发 `enforce_retention` 检查（低频写入可接受）
- [ ] 2.5 `cargo test -p nuomi-core` 验证
- [ ] 2.6 提交："feat(plugins): MemoryService 消费 MemoryPolicy + per-agent 检索"

---

## 3. IPC 命令暴露 + bindings

> 目标：暴露 4 个 memory IPC 命令给前端。
> 范围：src-tauri + src/lib/ipc。

- [ ] 3.1 `src-tauri/src/commands.rs`：新增 `MemoryEntryDto`（完整字段）+ `impl_remember_memory`/`impl_recall_memory`/`impl_list_memory`/`impl_delete_memory`
- [ ] 3.2 `src-tauri/src/commands.rs:7446-7451` `MemorySummaryDto` 补全 tags/kind/user_profile/role_id/source_session_id/created_at/updated_at 字段
- [ ] 3.3 `src-tauri/src/tauri_cmds.rs`：4 个 `#[tauri::command] #[specta::specta]` 薄壳（`rememberMemory`/`recallMemory`/`listMemory`/`deleteMemory`）
- [ ] 3.4 `src-tauri/src/lib.rs:37-198` `collect_commands!`：注册 4 命令
- [ ] 3.5 `pnpm contracts:gen` 重新生成 bindings
- [ ] 3.6 `src/lib/ipc/client.ts`：4 个 wrapper
- [ ] 3.7 `src/lib/ipc/test-double-commands.ts`：4 个 stub
- [ ] 3.8 `cargo test` + `pnpm typecheck` 验证
- [ ] 3.9 提交："feat(ipc): 暴露 memory remember/recall/list/delete 命令"

---

## 4. 自动记忆注入（loop_engine + facade）

> 目标：turn 开始按 role_id 检索注入，turn 结束自动写入 per-agent 记忆。
> 范围：crates/nuomi-core（loop_engine + facade）。

- [ ] 4.1 `crates/nuomi-core/src/plugins/loop_engine.rs:267-283`：memory 注入加 role_id 过滤；ctx 新增 `role_id()` 方法从 RoleOverlay 提取
- [ ] 4.2 `crates/nuomi-core/src/facade.rs:546-588`：turn 结束后新增自动写入钩子——Role 配置 `auto_remember=user_input_only` 时，把 user input 存为 per-agent 记忆（`source_session_id`+`role_id`，kind="turn_user_input"）
- [ ] 4.3 `crates/nuomi-core/src/facade.rs:417-462` `run_task_in_session`：从 RoleOverlay `params` 提取 role_id + memory 配置（`params.memory = { enabled, tags, autoRemember }`）传入 run_turn
- [ ] 4.4 `cargo test -p nuomi-core` 验证
- [ ] 4.5 提交："feat(core): turn 开始自动检索 + turn 结束自动写入 per-agent 记忆"

---

## 5. 前端 Memory 面板

> 目标：Settings 加 Memory 管理面板。
> 范围：src/features/settings。

- [ ] 5.1 新建 `src/features/settings/MemorySection.tsx`：Memory 列表（id/content/tags/kind/role_id/created_at）+ remember 表单（content/tags/kind/roleId/userProfile）+ recall 检索框（query/tag/roleId/limit）
- [ ] 5.2 `src/features/settings/SettingsView.tsx`：挂载 MemorySection（新增 tab 或区域）
- [ ] 5.3 i18n：`settings.memory.*` 文案中英双语
- [ ] 5.4 `pnpm typecheck && pnpm test` 验证
- [ ] 5.5 提交："feat(settings): Memory 管理面板"

---

## 6. RoleForm 记忆配置 + 质量门

> 目标：Role 编辑器加私有记忆配置区，质量门全绿。
> 范围：src/features/settings + 质量门。

- [ ] 6.1 `src/features/settings/RoleForm.tsx`：加"私有记忆"配置区——`enabled` 开关 + `tags` 输入（复用 ToolTagInput）+ `autoRemember` 选择（off/user_input_only）；写入 `params.memory = { enabled, tags, autoRemember }`
- [ ] 6.2 i18n：`settings.roles.memory.*` 文案中英双语
- [ ] 6.3 `pnpm typecheck && pnpm lint && pnpm test`（单线程模式）全绿
- [ ] 6.4 `cargo test -p nuomi-core && cargo clippy` 全绿
- [ ] 6.5 人工验证：`pnpm tauri dev` 启动后 Memory 面板 remember/recall；Role 配置私有记忆后对话中自动注入/写入
- [ ] 6.6 提交："feat(settings): Role 编辑器加私有记忆配置 + 质量门验证"
