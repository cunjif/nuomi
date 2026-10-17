# ADR 0021: 工作区两级模型重构（单开工作区 ⊃ 多工作目录）

- **状态**: Accepted（2026-10-17 用户确认全部 8 项决策）
- **日期**: 2026-10-17
- **决策者**: user + conductor
- **前置设计**: `.codeartsdoer/specs/steward_workbench_v2/design.md` §2.1.3.3、§2.3.2.2

## 背景

当前工作台是"多工作区并行打开"模型（ADR 0024 建立）：一个物理目录 = 一个工作区（`workspaces.root_path` UNIQUE，`impl_add_workspace` 每次注册新目录为新工作区，`commands.rs:6387`），`workspace_open_state` 维护多开集合（`services/workspace_open_set.rs:80-232`），前端支持 split 双栏（`uiStore.ts:99` `LayoutMode = "single" | "split" | "overview"`、`Shell.tsx:312-335`、`SplitView.tsx`）。

用户需求（steward_workbench_v2 spec 5.5/5.6）要求重构为两级模型：

1. **工作区向上抬一级**为唯一打开单位——一次只能打开一个工作区，"切换工作区"按钮切换；
2. 打开的**物理目录降为"工作目录"**——一个工作区可同时挂载多个工作目录；
3. 工作目录的目录名组成工作区标识（`{目录1}-{目录2}`，超过 4 个显示 `...`，hover 展示完整）；
4. 工作区面板底部导航"+"支持打开已加入工作区的工作目录；
5. 移除设置页工作区设置（启动恢复/最大同时开启数/最近列表容量——多开模型产物，单开下无意义）。

现状痛点：多开 + split 使"当前工作上下文"语义分散（哪个工作区是激活的、split 两侧各是什么）；最近使用列表以 UUID 展示工作区（因工作区无人类可读标识）；设置页工作区参数（最大同时开启数）在单开模型下完全失效。

## 决策

### 设计决策

| # | 决策点 | 选择 |
|---|---|---|
| 1 | 打开单位 | **单开工作区**（`workspace_open_state` 应用层收窄至至多一行，不改表结构） |
| 2 | 目录归属 | **同一物理路径全局唯一归属**（加入新工作区时从原工作区移除） |
| 3 | 工作区标识 | **派生不落库**（各目录 display_name 按 added_at 排序 join("-")，>4 截断 `...`） |
| 4 | 存量迁移 | **0036 幂等迁移**：每个存量工作区以 root_path 生成首个工作目录（SQL 占位 + Rust 修正 display_name） |
| 5 | 多开命令退役 | 五命令（open/close/focus/close_all/get_open_set）**保留一个版本周期兼容 shim 后移除** |
| 6 | split 双栏 | **退役**（单开下双栏语义失效）；`mode='split'` 值停用、读取兼容不报错 |
| 7 | 会话归属键 | **不变**（`sessions.workspace_id` 仍指向工作区 id，按工作区过滤天然覆盖其全部目录的会话） |
| 8 | 设置页工作区面板 | **移除编辑入口**；后端 `app_setting_get` 对相关键保留可读（不做破坏性清理） |

### 1. 单开模型：应用层收窄，不改表

`workspace_open_state` 表结构（0024）不动——PRIMARY KEY 仍是 workspace_id，多行能力物理保留——但 `WorkspaceOpenSetService` 收窄语义：`open` 前先关闭当前打开的工作区（保存其布局快照），保证全表至多一行有效打开记录。

**理由**：改表需新迁移且与 0024 的存量数据冲突；服务层收窄是纯逻辑变更，可测试、可回滚（恢复多开只需还原服务层语义）。

### 2. 同一物理路径全局唯一归属

目录 D 加入工作区 A 时若已属于工作区 B，则事务内从 B 移除（B 的其他目录与全部会话归属不变）。

**理由**：会话按 `workspace_id` 过滤。若一路径双属两工作区，用户在 B 中创建的会话切到 A 后归属语义不明；唯一归属使"按工作区过滤 = 该工作区全部目录的会话"无歧义。`work_directories` 以**规范化绝对路径**做全局唯一判断（UNIQUE(workspace_id, abs_path) 做库内兜底，跨工作区唯一性由 repo 层事务保证）。

### 3. 工作区标识：派生不落库

`label()` = 各工作目录 display_name（按 added_at 排序）以 `-` 连接；超过 4 个目录截断为前 4 名 + `...`（`labelFull` 供 tooltip）。同名目录消歧：追加父目录名（禁止两个相同标识）。异常目录（路径解析失败）跳过并记警告。纯派生计算 ≤50ms，不阻塞界面。

**理由**：落库会引入"目录增减后标识过期"的一致性问题；目录集是唯一事实源，派生即得。

### 4. 迁移 0036 与 display_name 修正

`migrations/0036_work_directories.sql`：`work_directories` 表（id/workspace_id/abs_path/display_name/mounted/added_at，UNIQUE(workspace_id, abs_path)，索引 idx_work_directories_ws）+ 幂等存量迁移（每个 `workspaces.root_path` 非空行生成首个工作目录，NOT EXISTS 守卫，mounted=1、added_at=created_at）。

display_name 的精确提取（跨平台分隔符 `/\`、尾部斜杠、同工作区内重名消歧）由 Rust 端 `services/workspace_migration.rs` 在迁移 runner 执行后修正——对齐 0016 的"SQL 占位 + Rust 修正"既有模式，避免在 SQL 中实现路径语义。迁移异常（个别行）跳过 + 原始数据保留 + 落 `workspace.migration_report` 事件，异常工作区在最近使用中标记不可点击。

`workspaces.root_path` 列**保留不删**（append-only 铁律；退化为兼容读取字段，不再写入新值）。

### 5. 多开命令退役与 IPC 变更

新增命令组（详见 design §2.2.2 B 组）：`workspace_add_directory`（新语义：无打开工作区→自动创建仅含该目录的新工作区并打开；有→加入当前工作区并挂载）、`workspace_remove_directory`（返回 workspace_closed 标志）、`workspace_mount_directory` / `workspace_unmount_directory`、`workspace_switch`（保存被切走工作区布局→关旧→开新，≤1s 呈现）、`workspace_get_open`（空态返回 null）。

`list_workspaces` DTO 扩展：`WorkspaceEntryDto` 追加可选字段 `label` / `labelFull` / `directories`（serde 可选追加，不破坏既有消费端）。

退役五命令：`open_workspace / close_workspace / focus_workspace / close_all_workspaces / get_open_set`（`commands.rs:6518-6658`、`tauri_cmds.rs:619-659`、`lib.rs:98-102`）——保留一个版本周期的兼容 shim（内部转调新语义：open_workspace → workspace_switch）后移除。

### 6. split 双栏退役

`LayoutMode` 移除 `"split"`；`Shell.tsx` split 分支与 `SplitView.tsx` 删除；`uiStore.splitWorkspaceIds` 移除；`workspace_layout_snapshot.mode` CHECK 约束中的 `'split'` 值停用（既有快照含 split 时读取降级为 single，不报错——读取兼容，无迁移）。

### 7. 会话/任务归属零变更

`sessions.workspace_id` / `tasks_runs.workspace_id` / `schedules_workspace` 均指向工作区 id，两级模型不改变归属键。按工作区过滤会话 = 匹配该工作区全部工作目录下的会话（天然覆盖，无需目录级过滤条件）。

### 8. 设置页工作区面板移除

前端删除 `WorkspaceSettingsPanel.tsx` 与 `SettingsView.tsx` 的路由入口；后端 `app_setting_get` 对"启动恢复/最大同时开启数/最近列表容量"键保留可读（兼容性要求），仅不再提供编辑入口。

## 向后兼容

- **迁移幂等**：0036 NOT EXISTS 守卫，重复应用零副作用；存量库升级后每个工作区恰有 1 条工作目录，标识与升级前一致（单目录标识=目录名）。
- **会话归属不变**：全部存量 sessions/tasks/schedules 的 workspace_id 语义不变，零数据搬移。
- **DTO 只增不改**：`WorkspaceEntryDto` 可选字段追加，既有消费端不更新也不破坏。
- **退役命令有 shim 期**：前端切换完成前旧命令仍可用（转调新语义），一个版本周期后移除。
- **`workspaces.root_path` 保留**：旧版本二进制读新库仍能取到 root_path（降级为单目录视图）。

## 权衡

- **优点**：单开模型消除"激活上下文"歧义；工作区标识人类可读（解决 UUID 展示问题）；目录唯一归属使会话过滤无歧义；不改 0024 表结构（迁移风险最小化）；标识派生无一致性问题。
- **代价**：1 迁移（新表）+ workspace 域服务层重写（open_set → registry v2）+ 前端 Shell/uiStore/底部导航重构 + 5 命令退役排期 + split 功能下线。
- **替代方案**：
  - (a) 改 `workspace_open_state` 表加单开约束——否决。需新迁移且与 0024 存量数据冲突，服务层收窄已足够。
  - (b) 工作区标识落库（目录变更时 UPDATE）——否决。引入标识过期一致性问题，派生计算 ≤50ms 无性能诉求。
  - (c) 目录允许多工作区归属——否决。会话按工作区过滤产生歧义（见决策 2）。
  - (d) 会话归属改目录级（sessions.directory_id）——否决。大规模数据迁移 + 破坏既有过滤链路，收益仅是更细粒度过滤，无需求支撑。
  - (e) split 保留为"同工作区双目录分屏"——否决。超出本批需求范围，且编辑器分桶按 workspace 组织，重定义成本高；留待后续独立提案。

## 风险与缓解

| 风险 | 缓解 |
|---|---|
| 存量迁移丢数据/归属错乱 | cargo test 无损专项：0035 旧库（多工作区+会话+布局快照）→ 0036 → 全量比对（工作区↔目录一一对应、标识一致、sessions.workspace_id 零变更、幂等） |
| 目录唯一归属迁移（跨工作区移除）误删 | repo 层事务内完成"加入新 + 移除旧"，单测覆盖跨工作区唯一归属路径 |
| 退役命令的前端残留调用 | shim 期 + `pnpm typecheck` 捕获 bindings 移除后的类型错误 |
| 单开收窄与并发打开竞态 | 服务层 open 事务内"先关后开"，单测并发打开被拒（不变式：至多一行） |
| split 快照读取报错 | mode 值读取兼容（split 降级 single），不改 CHECK 约束 |
| 切换工作区 >1s | 布局保存失败继续切换 + toast 警告；数据全在本地库，正常路径 ≤1s |
| tauri-specta bindings 需 regen | 契约先行：命令变更提交内 `pnpm contracts:gen` + `pnpm contracts:check` |

## 影响范围

### Migration
- **新增** `migrations/0036_work_directories.sql` — `work_directories` 表 + 索引 + 幂等存量迁移

### 后端
- `crates/nuomi-core/src/services/workspace_migration.rs` — 迁移后 display_name 修正 + 异常报告
- **新增** `crates/nuomi-core/src/store/repos/work_directories.rs` — 目录增删查/挂载翻转/唯一归属/失效标记
- `crates/nuomi-core/src/services/workspace_registry.rs`（或新建 v2）— 单开约束、add_directory 新语义、switch、自动关闭
- **新增** 标识派生模块（label 逻辑）— label/labelFull/截断/消歧
- `crates/nuomi-core/src/services/workspace_open_set.rs` — 多开语义退役（被 registry v2 取代）

### IPC 契约
- `src-tauri/src/commands.rs` — 新增 6 命令（add_directory/remove_directory/mount/unmount/switch/get_open）+ `list_workspaces` DTO 扩展 + 5 命令 shim
- `src-tauri/src/tauri_cmds.rs` / `lib.rs` — 注册与薄壳
- `src/lib/ipc/bindings.gen.ts` — 重新生成（camelCase）

### 前端
- `src/lib/store/uiStore.ts` — `openWorkspaceIds` 多值 → 单开语义；`splitWorkspaceIds`/`LayoutMode "split"` 移除
- `src/features/shell/Shell.tsx` / `SplitView.tsx` — split 分支退役
- `src/features/shell/WorkspaceBottomBar.tsx` — 重构为"当前工作区的工作目录 chips + '+'"
- `src/features/shell/`（空态/OverviewGrid/WorkspaceListDialog）— UUID 标识替换为工作区标识 + tooltip
- `src/features/settings/SettingsView.tsx` / `WorkspaceSettingsPanel.tsx` — 面板移除

### 不涉及
- `sessions` / `tasks_runs` / `schedules_workspace` 归属键——零变更
- `workspace_recent` / `workspace_palette`（路径哈希取色）——机制保留
- `uiStore.editorByWorkspace` 编辑器内存分桶——验证而非重做

## 实施计划

任务组 6（tasks.md）九步：6.1 迁移+修正 → 6.2 目录仓储 → 6.3 registry v2 → 6.4 标识派生 → 6.5 IPC 命令组【契约】 → 6.6 前端单开化+底部导航 → 6.7 切换界面+标识替换 → 6.8 split 退役+设置面板移除 → 6.9 会话过滤回归。每步独立可验证、可提交。

## 验证门禁

- `cargo test -p nuomi-core`（含迁移无损专项：0035 旧库升级全量比对 + 幂等）
- `cargo clippy --all-targets -- -D warnings`（新代码零警告）
- `pnpm contracts:gen` 无未提交 diff + `pnpm contracts:check`
- `pnpm typecheck && pnpm lint && pnpm test -- --no-file-parallelism`
- 人工验证：`pnpm tauri dev` 下打开多目录、切换工作区 ≤1s、标识截断与 tooltip、移除最后目录自动关闭、设置页无工作区分区
