# SPEC: 会话记忆 IPC 暴露 + RoleAgent 绑定（roleagent-memory-binding）

> 状态：待批准（属跨层变更，需先获用户确认 ADR 0019 再动工）
> 关联 ADR：`docs/adr/0019-roleagent-memory-binding.md`（Proposed）
> 需求来源：用户请求「RoleAgent 支持会话记忆」；需求权威仍为 `pr.md`。
> 事实基础：`MemoryService` 后端就绪（remember/recall/user_profile + FTS5）但 IPC 层零暴露 memory_* 命令；前端零消费 `AppStateSnapshotDto.memoryEntries`；`MemoryEntry`（`entities.rs:572-585`）无 `role_id` 字段（记忆全局）；`MemoryPolicy`（`entities.rs:1045-1059`）的 `retention_days`+`retrieval` 零消费；`loop_engine.rs:267-283` 已有全局记忆注入但无 role_id 过滤；`facade.rs:546-588` turn 结束无自动写入钩子。

## 1. 决策记录摘要 / Decisions

| # | 决策 |
|---|---|
| D1 | **4 IPC 命令**：`rememberMemory`/`recallMemory`/`listMemory`/`deleteMemory` |
| D2 | **per-agent 隔离**：迁移加 `memory_entries.role_id` 列（`Option<String>` FK→roles，NULL=全局） |
| D3 | **Role 记忆配置存 `params` JSON**（`params.memory = { enabled, tags, autoRemember }`），不加强类型列 |
| D4 | **消费 MemoryPolicy**：recall 按 `retrieval` 分派 search/search_fts；新增 retention 清理任务 |
| D5 | **自动记忆注入**：turn 开始加 role_id 过滤（loop_engine）；turn 结束新增自动写入钩子（facade） |
| D6 | **auto_remember v1 = `user_input_only`**（直接存 user input，无需 LLM）；`llm_summary` 后置 v2 |
| D7 | **MemorySummaryDto 补全**字段（tags/kind/user_profile/role_id/source_session_id/created_at/updated_at） |

## 2. 目标 / Goals

1. 前端可主动 remember/recall 记忆（4 IPC 命令）。
2. RoleAgent 拥有 per-agent 私有记忆（`role_id` 隔离）。
3. turn 开始自动检索注入、turn 结束自动提炼写入（`user_input_only` 模式）。
4. 消费 MemoryPolicy（retention 清理 + retrieval 策略分派）。
5. 不回归：全局记忆（role_id=NULL）行为不变；既有 Role 无 memory 配置时行为不变。

## 3. 用户故事 / User Stories

- **US1** 用户在 Settings Memory 面板手动 remember 一条记忆，可指定 tags/kind/roleId；recall 按关键词检索返回匹配条目。
- **US2** 用户为某 RoleAgent 配置私有记忆（enabled=true, tags=["project-context"], autoRemember=user_input_only）；对话中该 RoleAgent turn 开始时自动检索注入相关记忆，turn 结束后自动把 user input 存为 per-agent 记忆。
- **US3** 用户在 Evolution Settings 配置 MemoryPolicy（retentionDays=30, retrieval=semantic）；recall 走 FTS5 检索；超过 30 天的记忆被清理任务删除。
- **US4** 未配置 memory 的 RoleAgent 对话行为与现状完全一致（无自动注入/写入）。

## 4. 验收标准 / Acceptance Criteria

**阶段一：迁移 + 实体 + store**
- [ ] AC1 `migrations/0032_memory_role_binding.sql` 新增 `memory_entries.role_id` 列 + `idx_memory_role` 索引。
- [ ] AC2 `MemoryEntry` 加 `role_id: Option<String>`；`repos/memory.rs` 的 `insert`/`search`/`search_fts`/`row_to_memory` 加 role_id。

**阶段二：MemoryService + MemoryPolicy 消费**
- [ ] AC3 `MemoryService::remember`/`recall` 加 `role_id` 参数；`recall` 按 `self.memory_policy.retrieval` 分派 `search`/`search_fts`（Semantic/Hybrid 复用 FTS5）。
- [ ] AC4 新增 `MemoryService::enforce_retention(retention_days)`，删除过期记忆；接入 `remember` 内触发检查。

**阶段三：IPC 命令暴露 + bindings**
- [ ] AC5 `commands.rs` 新增 `MemoryEntryDto` + `impl_remember_memory`/`impl_recall_memory`/`impl_list_memory`/`impl_delete_memory`；`MemorySummaryDto` 补全字段。
- [ ] AC6 `tauri_cmds.rs` 4 命令薄壳 + `lib.rs` `collect_commands!` 注册 + `pnpm contracts:gen` + `client.ts`/`test-double-commands.ts` 4 wrapper。

**阶段四：自动记忆注入**
- [ ] AC7 `loop_engine.rs:267-283` memory 注入加 role_id 过滤（ctx 新增 `role_id()` 方法）。
- [ ] AC8 `facade.rs:546-588` turn 结束后自动写入钩子：Role 配置 `auto_remember=user_input_only` 时，把 user input 存为 per-agent 记忆（`source_session_id`+`role_id`）。
- [ ] AC9 `facade.rs:417-462` 从 RoleOverlay `params` 提取 role_id + memory 配置传入 run_turn。

**阶段五：前端 Memory 面板**
- [ ] AC10 新 `src/features/settings/MemorySection.tsx`：列表 + remember 表单 + recall 检索；挂载到 SettingsView。

**阶段六：RoleForm 记忆配置区**
- [ ] AC11 `RoleForm.tsx` 加"私有记忆"配置区（enabled 开关 + tags 输入 + auto_remember 选择），写入 `params.memory`。

**横切**
- [ ] AC12 既有全局记忆（role_id=NULL）行为不变；既有 Role 无 `params.memory` 时行为不变（视为 enabled=false）。
- [ ] AC13 i18n 文案中英双语。
- [ ] AC14 双端质量门全绿。

## 5. 影响范围 / Impact

| 层 | 文件 | 改动 |
|---|---|---|
| migration | `0032_memory_role_binding.sql` | memory_entries.role_id 列 + 索引 |
| domain | `entities.rs` | MemoryEntry 加 role_id |
| store | `repos/memory.rs` | insert/search/search_fts/row_to_memory 加 role_id |
| plugins | `plugins/memory.rs` | remember/recall 加 role_id + retrieval 分派 + enforce_retention |
| 内核 | `loop_engine.rs`/`facade.rs` | turn 开始 role_id 过滤 + turn 结束自动写入 |
| IPC | `commands.rs`/`tauri_cmds.rs`/`lib.rs` | 4 命令 + MemoryEntryDto + MemorySummaryDto 补全 |
| bindings | `bindings.gen.ts`/`client.ts`/`test-double-commands.ts` | 重新生成 + 4 wrapper |
| 前端 | `MemorySection.tsx`（新）/`RoleForm.tsx`/`SettingsView.tsx` | Memory 面板 + Role 记忆配置 |
| i18n | `en.json`/`zh-CN.json` | 文案 |
