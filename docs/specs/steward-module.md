# Steward Module SPEC

> 应用管家 AI（App Steward AI）模块规格。独立于对话 Agent，管理应用自身的配置、进化、任务调度与数据清洗。
> 创建：2026-10-06（K-Steward 全阶段交付）。

## 1. 模块边界

- **源码**：`crates/nuomi-core/src/steward/` + `crates/nuomi-core/src/domain/steward{,_enums}.rs` + `crates/nuomi-core/src/store/repos/steward.rs`
- **迁移**：`migrations/0027_steward_sessions.sql` ~ `migrations/0031_steward_builtin_roles.sql`（只增不改）
- **IPC 命令**：25 个 `steward_*` Tauri 命令（camelCase bindings 自动生成）
- **前端**：`src/features/steward/`（6 个组件）+ `src/lib/store/use{StewardSessions,EvolutionCycles,GatePending}.ts` + `src/lib/stewardHooks.ts`
- **零侵入**：不修改 `evolution::*` API、`team_runner::run_team` 签名、`ConversationKind` 枚举、`events` 表结构、已发布 migration 0001-0026

## 2. 数据模型（10 张表）

| 表 | 用途 |
|---|---|
| `steward_sessions` | 管家会话（独立于 sessions 表，1:1 引用 sessions(id) ON DELETE CASCADE） |
| `steward_ai` | 管家单例（1 行/workspace） |
| `steward_dev_team` | 研发团队单例（1:1 引用 steward_ai） |
| `steward_dev_role_bindings` | 5 个研发角色绑定（PK=role_kind，CHECK 约束 5×2） |
| `evolution_cycles` | 进化周期（phase/status CHECK 约束） |
| `evolution_tasks` | 进化任务（cycle_id 外键 + depends_on_json + status CHECK） |
| `evolution_artifacts` | 进化产物（task_id 外键 + artifact_type/status CHECK + diff_preview + rollback_plan_json） |
| `evolution_gate_decisions` | 验收门决策（UNIQUE(artifact_id) 保证每产物至多一条决策） |
| `evolution_data_pools` | 数据池 |
| `steward_change_snapshots` | 配置变更快照（proposal_id 外键 + before/after JSON） |

## 3. 事件类型（17 种 StewardEventKind）

`session_created` / `message_received` / `intent_recognized` / `config_proposal_created` / `config_change_confirmed` / `config_change_rolled_back` / `dev_role_binding_changed` / `cycle_triggered` / `cycle_phase_changed` / `cycle_cancelled` / `cycle_failed` / `task_dispatched` / `task_completed` / `task_failed` / `artifact_produced` / `gate_decision_made` / `online_authorization_changed`

所有事件 aggregate_type 统一为 `"steward"`，通过 `events::append` 写入 events 表（复用现有事件总线），可选 `mirror_to_journal` 镜像到 EvolutionJournal。

## 4. IPC 命令清单（25 个）

### 会话管理
- `steward_create_session(title)` → `StewardSessionDto`
- `steward_list_sessions()` → `StewardSessionDto[]`
- `steward_send_message(session_id, text)` → `StewardReplyDto`（5 种回复类型：text/config_proposal/evolution_accepted/clarify/refused）
- `steward_get_app_snapshot()` → `AppStateSnapshotDto`

### 配置变更
- `steward_propose_config_change(target, intent)` → `ConfigProposalDto`
- `steward_confirm_config_change(proposal_id)` → `ConfigChangeResultDto`
- `steward_rollback_config_change(snapshot_id)` → `null`

### 研发团队
- `steward_get_dev_team()` → `DevTeamDto`
- `steward_set_dev_role_binding(binding)` → `null`

### 进化周期
- `steward_trigger_evolution(instruction)` → `EvolutionCycleDto`
- `steward_list_cycles(status)` → `EvolutionCycleDto[]`
- `steward_get_cycle(cycle_id)` → `CycleDetailDto`
- `steward_cancel_cycle(cycle_id)` → `null`
- `steward_list_cycle_tasks(cycle_id)` → `EvolutionTaskDto[]`

### 数据清洗
- `steward_cleanse_data(scope, rules)` → `CleanseReportDto`
- `steward_list_data_pools()` → `EvolutionDataPoolDto[]`

### 验收门
- `steward_list_artifacts(cycle_id, status)` → `EvolutionArtifactDto[]`
- `steward_get_artifact(artifact_id)` → `EvolutionArtifactDto`
- `steward_resolve_gate(artifact_id, decision)` → `ResolveOutcomeDto`
- `steward_resolve_gate_batch(cycle_id, decision)` → `ResolveOutcomeDto[]`
- `steward_list_gate_pending()` → `EvolutionArtifactDto[]`

### 事件与授权
- `steward_list_events(aggregate_id, kind_prefix, limit)` → `StewardEventDto[]`
- `steward_set_online_authorization(authorized)` → `null`

## 5. 状态机

### Run 生命周期
```
queued ─▶ running ⇄ awaiting_approval ─▶ succeeded
             │   │
             │   └──▶ cancelled
             └──────▶ failed | timed_out
```

### Cycle 阶段流转
```
cleanse → research → design → develop → test → verify → gate → merge
```

### Artifact 状态
```
pending_review → approved → merged
                → rejected
                → needs_revision → (退回研发团队)
```

## 6. 迁移清单

| 编号 | 文件 | 用途 |
|---|---|---|
| 0027 | `steward_sessions.sql` | 管家会话表 |
| 0028 | `steward_ai.sql` | 管家单例 + 研发团队 + 角色绑定 |
| 0029 | `evolution_cycles.sql` | 进化周期 + 任务 + 产物 |
| 0030 | `evolution_gate_and_pool.sql` | 验收门决策 + 数据池 + 变更快照 |
| 0031 | `steward_builtin_roles.sql` | 5 个内置研发角色（INSERT OR IGNORE） |
