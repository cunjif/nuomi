# ADR 0019: 会话记忆（Memory IPC 暴露 + RoleAgent 绑定）

- **状态**: Proposed
- **日期**: 2026-10-07
- **决策者**: user + conductor

## 背景

后端 `MemoryService` 已就绪（`MemoryEntry` 实体 + FTS5 全文索引 + remember/recall/user_profile 方法），但存在三层缺口：

1. **IPC 层零暴露**：`src-tauri/src/tauri_cmds.rs` 中无任何 `memory_*` IPC 命令（grep 确认）。`MemoryService` 仅在 `commands.rs` 内部用于 approval_gate / online_authorized / evolution settings 读写，前端无法主动 remember/recall。
2. **前端零消费**：`AppStateSnapshotDto.memoryEntries`（`commands.rs:7393`）只读、只 `user_profile=1`、只 `id+content`（`MemorySummaryDto` 丢失 tags/kind/source_session_id），且 `src/features/` 下无任何组件引用。
3. **RoleAgent 未绑定记忆**：`MemoryEntry`（`entities.rs:572-585`）无 `role_id` 字段，记忆是全局的，无法 per-agent 隔离。`loop_engine.rs:267-283` 已有全局记忆注入但无 role_id 过滤；`facade.rs:546-588` turn 结束后无自动提炼摘要钩子。

另外 `MemoryPolicy`（`entities.rs:1045-1059`）的 `retention_days` + `retrieval` 字段**零消费**（`plugins/memory.rs:17-20` 注释明示"stored but not yet consumed"）。

需求：(a) 前端可主动 remember/recall 记忆；(b) RoleAgent 拥有 per-agent 私有记忆；(c) turn 开始自动检索注入、turn 结束自动提炼写入；(d) 消费 MemoryPolicy（retention 清理 + retrieval 策略分派）。

## 决策

### 设计决策

| # | 决策点 | 选择 |
|---|---|---|
| 1 | IPC 暴露范围 | 4 命令：`rememberMemory` / `recallMemory` / `listMemory` / `deleteMemory` |
| 2 | per-agent 记忆隔离 | 迁移加 `memory_entries.role_id` 列（`Option<String>` FK→roles） |
| 3 | Role 记忆配置存储 | **复用 `params` JSON**（`params.memory = { enabled, tags, autoRemember }`），不加强类型列 |
| 4 | MemoryPolicy 消费 | recall 按 `retrieval` 分派 `search`/`search_fts`；新增 retention 清理任务 |
| 5 | 自动记忆注入 | turn 开始：`loop_engine` 加 role_id 过滤；turn 结束：`facade` 新增自动写入钩子 |
| 6 | auto_remember 模式 | v1 `user_input_only`（直接存 user input，无需 LLM）；`llm_summary` 后置 v2 |
| 7 | MemorySummaryDto | 补全 tags/kind/user_profile/role_id 字段 |

### 1. IPC 命令暴露（4 命令）

| 命令 | 签名 | 说明 |
|---|---|---|
| `rememberMemory` | `(content, tags, kind?, roleId?, userProfile?)` | 主动写入，`roleId` 缺省为全局记忆 |
| `recallMemory` | `(query?, tag?, roleId?, limit?)` | 关键词/标签检索，按 `retrieval` 策略分派 |
| `listMemory` | `(roleId?, kind?, limit?)` | 列表浏览 |
| `deleteMemory` | `(id)` | 软删除（`updated_at` 标记 + 物理删除二选一，见决策 8） |

参数 camelCase（对齐 tauri-specta 生成约定）。`roleId` 为 `Option<String>`，缺省视为全局记忆（向后兼容）。

### 2. per-agent 记忆隔离（迁移 + 实体 + store）

**迁移** `migrations/0032_memory_role_binding.sql`：

```sql
ALTER TABLE memory_entries ADD COLUMN role_id TEXT REFERENCES roles(id);
CREATE INDEX idx_memory_role ON memory_entries(role_id);
```

FTS5 触发器无需改动（external-content 自动同步）。

**实体** `MemoryEntry` 加 `role_id: Option<String>` 字段。

**store 层** `repos/memory.rs`：
- `insert`：写入 `role_id` 列
- `search` / `search_fts`：新增 `role_id` 过滤参数（`WHERE role_id = ? OR role_id IS NULL`——含全局记忆）
- `row_to_memory`：读 `role_id` 列

### 3. Role 记忆配置存 params JSON

不加强类型列（避免 `roles` 表列膨胀），复用 `params` JSON 命名空间（与现有 `agent_profile_id` 存法一致）：

```json
{
  "agent_profile_id": "...",
  "memory": {
    "enabled": true,
    "tags": ["project-context"],
    "auto_remember": "user_input_only"
  }
}
```

`auto_remember` 枚举：`"off"`（默认）| `"user_input_only"` | `"llm_summary"`（v2）。

**读取**：`facade.rs` `run_turn` 从 RoleOverlay 的 `params` 提取 memory 配置，传入 loop_engine。

### 4. MemoryPolicy 消费

**retrieval 策略分派**（`MemoryService::recall`）：

```rust
match self.memory_policy.retrieval {
    RetrievalStrategy::Keyword => repos::memory::search(conn, keyword, tag, role_id, limit),
    RetrievalStrategy::Semantic | RetrievalStrategy::Hybrid => 
        repos::memory::search_fts(conn, query, role_id, limit),  // FTS5 已就绪
}
```

`Semantic` 暂复用 FTS5（向量检索后置 v2）；`Hybrid` 同 FTS5 + 关键词 fallback。

**retention 清理**：新增 `MemoryService::enforce_retention(&self, retention_days: u32)`，删除 `created_at < now - retention_days*86400_000` 的行。接入点：`SchedulerRunner` 加 daily job，或 `remember` 内每次触发检查（低频记忆写入，可接受）。

### 5. 自动记忆注入（turn 开始 + turn 结束）

**turn 开始检索**（`loop_engine.rs:267-283` 已有，加 role_id 过滤）：

```rust
if let Some(mem) = memory {
    let role_id = ctx.role_id();  // 新增：从 ctx 提取当前 role_id
    let keyword = user_task.split_whitespace().next().map(str::to_string);
    if let Ok(hits) = mem.recall(keyword, self.config.memory_tag.clone(), role_id, 5).await {
        // 注入 system prompt（现有逻辑）
    }
}
```

**turn 结束自动写入**（`facade.rs:546-588` 之后新增钩子）：

```rust
// turn 结束后
if let Some(role_mem) = role.params().memory.as_ref().filter(|m| m.enabled) {
    match role_mem.auto_remember {
        AutoRemember::UserInputOnly => {
            if let Some(user_msg) = fresh.iter().find(|m| m.role == MessageRole::User) {
                self.memory.remember(
                    user_msg.content.clone(),
                    Some(session_id.to_string()),
                    role_mem.tags.clone(),
                    "turn_user_input",
                    false,
                    Some(role.id.clone()),  // per-agent
                ).await?;
            }
        }
        AutoRemember::LlmSummary => { /* v2: LLM 提炼摘要 */ }
        AutoRemember::Off => {}
    }
}
```

`TurnHook`（`loop_engine.rs:62`）是 per-turn 参数覆盖，不是生命周期钩子——不能复用做 on_turn_end，需在 `facade` 层直接调用。

### 6. MemorySummaryDto 补全

`commands.rs:7446-7451` `MemorySummaryDto` 补全字段：

```rust
pub struct MemorySummaryDto {
    pub id: String,
    pub content: String,
    pub tags: Vec<String>,
    pub kind: String,
    pub user_profile: bool,
    pub role_id: Option<String>,
    pub source_session_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}
```

或新增完整 `MemoryEntryDto`，保留 `MemorySummaryDto` 向后兼容。

## 向后兼容

- `memory_entries.role_id` 为 `Option<String>`，缺省 NULL 视为全局记忆，既有数据不破。
- IPC 命令 `roleId` 参数可选，缺省视为全局记忆。
- `Role.params` JSON 内 `memory` 键缺省视为 `enabled: false`，既有 Role 行为不变。
- `MemoryPolicy` 消费后，既有 `retrieval=Keyword` 行为与现状一致（`search` 方法）。
- `MemorySummaryDto` 补全字段为新增，既有前端消费方（若有）不受影响（新字段可选）。

## 权衡

- **优点**：后端 MemoryService/FTS5 复用，增量改动；per-agent 隔离让 RoleAgent 拥有私有记忆；MemoryPolicy 从占位符变可用配置；自动记忆注入降低用户手动操作。
- **代价**：1 迁移 + MemoryEntry 加字段 + store/plugins 全链路加 role_id + 4 IPC 命令 + loop_engine/facade 改造 + 前端新面板；retention 清理需接入 SchedulerRunner。
- **替代方案**：
  - (a) Role 记忆配置加强类型列（`memory_enabled`/`memory_tags`/`auto_remember`）——否决。`roles` 表已 15 列，避免膨胀；`params` JSON 与现有 `agent_profile_id` 存法一致。
  - (b) 全局记忆不做 per-agent 隔离——否决。用户明确要 RoleAgent 私有记忆。
  - (c) auto_remember v1 直接做 llm_summary——否决。需额外 LLM 调用，复杂度高；v1 先 user_input_only，v2 再加 LLM 提炼。
  - (d) MemoryPolicy 不消费，仅暴露 IPC——否决。既然要碰记忆层，一并消费已存储的 policy 字段，消除"存储但未消费"债务。
  - (e) Semantic 走向量检索——否决 v1。向量索引需额外基础设施（embedding 模型 + 向量存储），后置 v2；v1 Semantic 复用 FTS5。

## 风险与缓解

| 风险 | 缓解 |
|---|---|
| 自动记忆写入过多导致 memory_entries 膨胀 | `auto_remember` 默认 off；retention 清理任务按 `retention_days` 裁剪；per-agent 记忆量可配上限 |
| FTS5 trigram 索引性能（CJK） | 已就绪且经测试；记忆量级可控 |
| `params` JSON 无 schema 约束导致配置错误 | RoleForm 前端校验 + 后端反序列化兜底（解析失败视为 enabled=false） |
| loop_engine 注入 role_id 需 ctx 透传 | `ctx.role_id()` 新增方法，从 RoleOverlay 提取 |
| retention 清理任务接入 SchedulerRunner 复杂 | 简化：v1 在 `remember` 内每次触发检查（低频写入可接受），SchedulerRunner 接入后置 |
| tauri-specta bindings 需 regen | 实施计划阶段 3 显式 `pnpm contracts:gen` |

## 影响范围

### Migration
- **新增** `migrations/0032_memory_role_binding.sql` — `memory_entries.role_id` 列 + 索引

### 后端实体
- `crates/nuomi-core/src/domain/entities.rs:572-585` — `MemoryEntry` 加 `role_id: Option<String>`
- `crates/nuomi-core/src/domain/entities.rs:483-518` — `Role` 无需改（memory 配置存 `params` JSON）

### Store 层
- `crates/nuomi-core/src/store/repos/memory.rs:8-25` — `insert` 写 `role_id`
- `crates/nuomi-core/src/store/repos/memory.rs:29-67,80-103` — `search`/`search_fts` 加 `role_id` 过滤
- `crates/nuomi-core/src/store/repos/memory.rs:145-157` — `row_to_memory` 读 `role_id`

### Plugins 层
- `crates/nuomi-core/src/plugins/memory.rs:59-84` — `remember` 加 `role_id` 参数
- `crates/nuomi-core/src/plugins/memory.rs:87-95` — `recall` 加 `role_id` 参数 + 按 `retrieval` 分派
- `crates/nuomi-core/src/plugins/memory.rs` — 新增 `enforce_retention`

### Loop Engine / Facade
- `crates/nuomi-core/src/plugins/loop_engine.rs:267-283` — memory 注入加 role_id 过滤
- `crates/nuomi-core/src/facade.rs:546-588` — turn 结束后新增自动写入钩子
- `crates/nuomi-core/src/facade.rs:417-462` — `run_task_in_session` 从 overlay 提取 role_id + memory 配置传入 run_turn

### IPC 契约
- `src-tauri/src/commands.rs` — 新增 `MemoryEntryDto` + `impl_remember_memory`/`impl_recall_memory`/`impl_list_memory`/`impl_delete_memory`；`MemorySummaryDto` 补全字段
- `src-tauri/src/tauri_cmds.rs` — 4 个 `#[tauri::command] #[specta::specta]` 薄壳
- `src-tauri/src/lib.rs:37-198` — `collect_commands!` 注册 4 命令
- `src/lib/ipc/bindings.gen.ts` — 重新生成
- `src/lib/ipc/client.ts` — 4 个 wrapper
- `src/lib/ipc/test-double-commands.ts` — test double stub

### 前端
- **新增** `src/features/settings/MemorySection.tsx` — Memory 列表 + remember + recall 面板
- `src/features/settings/RoleForm.tsx` — Role 编辑器加"私有记忆"配置区（enabled/tags/auto_remember）
- i18n `src/i18n/locales/{en,zh-CN}.json` — memory 面板文案

## 实施计划

分 6 阶段，每阶段独立可验证、可提交。

### 阶段 1：迁移 + 实体 + store 层

1. `migrations/0032_memory_role_binding.sql`：`memory_entries.role_id` 列 + 索引
2. `entities.rs`：`MemoryEntry` 加 `role_id` 字段
3. `repos/memory.rs`：`insert`/`search`/`search_fts`/`row_to_memory` 加 role_id
4. `cargo test -p nuomi-core` + `cargo clippy` 验证
5. 提交："feat(store): memory_entries 加 role_id 列支持 per-agent 记忆"

### 阶段 2：MemoryService + MemoryPolicy 消费

1. `plugins/memory.rs`：`remember`/`recall` 加 role_id 参数；`recall` 按 `retrieval` 分派；新增 `enforce_retention`
2. `cargo test -p nuomi-core` 验证
3. 提交："feat(plugins): MemoryService 消费 MemoryPolicy + per-agent 检索"

### 阶段 3：IPC 命令暴露 + bindings

1. `commands.rs`：`MemoryEntryDto` + 4 个 `impl_*`；`MemorySummaryDto` 补全
2. `tauri_cmds.rs`：4 个命令薄壳
3. `lib.rs`：`collect_commands!` 注册
4. `pnpm contracts:gen`
5. `src/lib/ipc/client.ts` + `test-double-commands.ts`：4 个 wrapper
6. `cargo test` + `pnpm typecheck` 验证
7. 提交："feat(ipc): 暴露 memory remember/recall/list/delete 命令"

### 阶段 4：自动记忆注入（loop_engine + facade）

1. `loop_engine.rs:267-283`：memory 注入加 role_id 过滤（ctx 新增 `role_id()` 方法）
2. `facade.rs:546-588`：turn 结束后自动写入钩子（`user_input_only` 模式）
3. `facade.rs:417-462`：从 RoleOverlay 提取 role_id + memory 配置传入 run_turn
4. `cargo test -p nuomi-core` 验证
5. 提交："feat(core): turn 开始自动检索 + turn 结束自动写入 per-agent 记忆"

### 阶段 5：前端 Memory 面板

1. 新 `src/features/settings/MemorySection.tsx`：列表 + remember 表单 + recall 检索
2. `src/features/settings/SettingsView.tsx`：挂载 MemorySection
3. i18n 文案
4. `pnpm typecheck && pnpm test` 验证
5. 提交："feat(settings): Memory 管理面板"

### 阶段 6：RoleForm 记忆配置区

1. `src/features/settings/RoleForm.tsx`：加"私有记忆"配置区（enabled 开关 + tags 输入 + auto_remember 选择）
2. `pnpm typecheck && pnpm test` 验证
3. 提交："feat(settings): Role 编辑器加私有记忆配置"

## 验证门禁

每阶段必须通过：
- `cargo test -p nuomi-core` + `cargo clippy`（涉及 Rust 时）
- `pnpm typecheck && pnpm lint`
- `pnpm test`（单线程模式，memory `feedback-test-suite-single-thread`）
- 人工验证：`pnpm tauri dev` 启动后 Memory 面板 remember/recall；Role 配置私有记忆后对话中自动注入/写入
