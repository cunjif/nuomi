# ADR 0024: 工作区用户命名模型 + 内置终端架构

- **状态**: Proposed（待用户确认）
- **日期**: 2026-10-25
- **决策者**: user + conductor
- **修订**: ADR-0021 决策③（标识派生不落库 → 用户命名落库 + 派生兜底）

## 背景

ADR-0021 确立了工作区两级模型（单开工作区 ⊃ 多工作目录），决策③规定工作区标识"派生不落库"——由各工作目录 display_name 以 `-` 连接派生。用户现要求：

1. **工作区由用户创建和命名**，不再通过打开某个具体目录自动创建；创建好的工作区可以添加目录。
2. **新增内置 Terminal**：在"对话"、"工作台"顶部导航右侧新增"终端"导航按钮，绑定 `Alt+`` 快捷 toggle，架构参考工作台——右侧同样具备工作区目录面板，但编辑页面替换为终端页面。

现状痛点：工作区无人类可读命名（派生标识在目录增减时变化、同目录名消歧后冗长）；终端能力缺失（项目有 portable-pty 依赖用于 CLI agent，但无面向用户的交互式终端）。

## 决策

### Part A: 工作区用户命名模型（修订 ADR-0021 决策③）

| # | 决策点 | 选择 |
|---|---|---|
| A1 | name 列 | **workspaces 表新增 `name TEXT` 可空列**（迁移 0041） |
| A2 | 展示优先级 | **name > 派生 label > id**（前端展示与 DTO 派发均按此序） |
| A3 | 存量迁移 | **name 列默认 NULL，零数据搬移**；存量工作区展示回退派生 label |
| A4 | 创建命令 | **新增 `workspace_create(name, initial_path?)`**：用户命名创建工作区，可选初始目录（不传则创建空工作区） |
| A5 | add_directory 语义变更 | **无打开工作区时不再自动创建**，返回 `NoOpenWorkspace` 错误；前端捕获后弹"创建工作区"对话框 |
| A6 | DTO 扩展 | **WorkspaceEntryDto 追加可选 `name` 字段**（serde 追加不破坏消费端） |
| A7 | ADR-0021 决策③修订 | **"派生不落库" → "用户命名落库 + 派生兜底"** |

### Part B: 内置终端架构

| # | 决策点 | 选择 |
|---|---|---|
| B1 | area 定位 | **ActiveArea 加 `"terminal"`**，与 chat/workbench 并列的 area 级 |
| B2 | 页面布局 | **仿工作台**：TerminalTabs（多终端）+ 终端列(xterm.js, flex-1) + 右侧 aside(EditorToolbar + FileTree + WorkspaceBottomBar) |
| B3 | 工作区绑定 | **终端绑定当前工作区**，cwd = 活动工作目录；无工作区时显示空态提示 |
| B4 | 后端实现 | **Rust TerminalManager**：portable-pty spawn shell + stdin/stdout/stderr pipe + resize + kill |
| B5 | IPC 命令 | **terminal_create(workspace_id, cwd?) / terminal_write(id, data) / terminal_resize(id, cols, rows) / terminal_kill(id)** |
| B6 | 事件通道 | **`terminal-output` 事件**（payload: `{ id, data }`），Tauri emit 桥接 |
| B7 | 前端依赖 | **xterm.js + xterm-addon-fit**（@xterm/xterm + @xterm/addon-fit 新包名） |
| B8 | 快捷键 | **`Alt+`` toggle 终端 ↔ 上一个 area**（再按切回，VSCode ctrl+` 习惯） |
| B9 | 多终端状态 | **uiStore.terminalByWorkspace 分桶**（仿 editorByWorkspace，per-workspace 独立终端集） |

### A1-A3. name 列与迁移

`migrations/0041_workspace_name.sql`：`ALTER TABLE workspaces ADD COLUMN name TEXT`（NULL 允许，零数据搬移）。append-only 铁律遵守。

展示优先级（前端 + DTO 派发）：`name ?? label ?? id`。`label` / `labelFull` 仍由 `workspace_label::derive_label` 派生（保留既有逻辑作兜底）。

### A4. workspace_create 命令

`workspace_create(name: String, initial_path: Option<String>) -> WorkspaceView`：
- name 必填，非空 trim 后校验（空串返回 `InvalidName`）
- initial_path 可选：传入时校验路径有效 + 加入 work_directories；不传时创建空工作区（无工作目录，open_state 不打开）
- 创建后不自动打开（用户在 WorkspaceListDialog 里点击切换）——与既有"创建后手动切换"语义一致

### A5. add_directory 语义变更

`workspace_add_directory(path)` 现有语义：无打开工作区 → 自动创建含该目录的新工作区并打开。**变更为**：无打开工作区 → 返回 `NoOpenWorkspace` 错误（前端捕获后弹"创建工作区"对话框，用户命名 + 可选将该目录作为初始目录）。

有打开工作区的分支不变（目录加入当前工作区）。

### B2. 终端页布局

```
┌─ AreaNav (顶部) ─────────────────────────────────────────────┐
│ [N] nuomi·糯米      [对话][工作台][终端]              — ▢ ✕  │
├──────┬───────────────────────────────────────────────────────┤
│Left- │  TerminalTabs (多终端 tab + [+] 新建)                  │
│Rail  ├───────────────────────────────────────────────────────┤
│      │ ┌─ 终端区 (renderTerminalContent) ───────────────────┐│
│      │ │ ┌──────────────────────┬──────────────────┐       ││
│      │ │ │ xterm.js             │ aside (w-56)     │       ││
│      │ │ │ (终端列, flex-1)     │ ┌EditorToolbar┐  │       ││
│      │ │ │                      │ │[切换工作区]  │  │       ││
│      │ │ │                      │ ├──────────────┤  │       ││
│      │ │ │                      │ │ FileTree     │  │       ││
│      │ │ │                      │ │ (滚动)       │  │       ││
│      │ │ │                      │ ├──────────────┤  │       ││
│      │ │ │                      │ │WorkspaceBot-│  │       ││
│      │ │ │                      │ │tomBar        │  │       ││
│      │ │ └──────────────────────┴─┴──────────────┘       ││
│      │ └──────────────────────────────────────────────────┘│
└──────┴───────────────────────────────────────────────────────┘
```

### B4-B6. TerminalManager 与 IPC

**Rust TerminalManager**（`crates/nuomi-core/src/services/terminal_manager.rs`）：
- `TerminalHandle`：持有 `MasterPty` + `PtyPair` + spawn 后的 reader/writer
- `create(cwd: &Path) -> String`：spawn 默认 shell（Windows: PowerShell，Unix: $SHELL 或 bash），返回 terminal id
- `write(id, data: &[u8])`：写入 master pty stdin
- `resize(id, cols, rows)`：pty resize
- `kill(id)`：终止 pty 进程
- reader 线程：循环读 pty stdout → 通过 `TerminalEventEmitter` trait 发事件（shell 侧实现 emit Tauri event）

**IPC 命令**（`src-tauri/src/commands.rs`）：
- `terminal_create(workspace_id: String, cwd: Option<String>) -> String`（返回 terminal id）
- `terminal_write(id: String, data: String) -> ()`（data 为 base64 或 UTF-8 字符串）
- `terminal_resize(id: String, cols: u16, rows: u16) -> ()`
- `terminal_kill(id: String) -> ()`

**事件**：`terminal-output`，payload `{ id: String, data: String }`（data 为终端输出字节，UTF-8 传递给 xterm.js write）

### B8. alt+` 快捷键

在 `Shell.tsx` 的 `useGlobalNavShortcuts` 加 `Alt+`` 监听：
- 当前 area 非 terminal → 记录上一个 area，切到 terminal
- 当前 area 为 terminal → 切回上一个 area（chat 或 workbench）
- 与既有 `Alt+H`（chat ↔ workbench）独立，不冲突

### B9. 多终端状态分桶

`uiStore.terminalByWorkspace: Record<string, { terminals: TerminalInstance[], activeTerminalId: string | null }>`，仿 `editorByWorkspace` 模式。`TerminalInstance` = `{ id, title, createdAt }`（id 来自后端，title 默认 shell 名，可改）。切工作区时终端保留（不销毁），切 area 时终端保留。

## 向后兼容

- **迁移幂等**：0041 仅 ALTER TABLE ADD COLUMN，重复应用零副作用（SQLite ADD COLUMN 幂等性由迁移 runner NOT EXISTS 守卫保证）。
- **name 列可空**：存量工作区 name=NULL，展示回退派生 label，零回归。
- **DTO 只增不改**：WorkspaceEntryDto.name 可选追加，既有消费端不更新也不破坏。
- **add_directory 语义变更**：前端所有"打开目录"入口（拖拽、命令面板、WorkspaceBottomBar +）改为先检查有无当前工作区，无则弹创建对话框——后端 NoOpenWorkspace 错误是兜底。
- **终端零侵入**：新增 area/命令/事件，不改动既有 chat/workbench 链路。

## 权衡

- **优点**：工作区有人类可读稳定命名（解决派生标识在目录增减时变化）；终端能力补齐（项目已有 portable-pty 依赖，复用既有 pty 基础设施）；area 级终端与工作台对称（右侧 aside 复用 FileTree/WorkspaceBottomBar）。
- **代价**：1 迁移（加列）+ workspace_registry_v2 加 create 方法 + add_directory 语义变更 + 新终端后端（TerminalManager + 4 命令 + 事件）+ 前端终端 area（xterm.js 依赖 + TerminalArea + TerminalTabs + 快捷键）。
- **替代方案**：
  - (a) 工作区标识完全用户命名、移除派生 label —— 否决。存量工作区迁移需回填 name，且派生 label 作兜底有诊断价值（name 为空时仍可识别）。
  - (b) 终端作为 view 而非 area —— 否决。用户明确要求"顶部导航右侧新增"，area 级与 chat/workbench 对称。
  - (c) 终端用 tauri-plugin-shell 简化版（非真 pty）—— 否决。不支持交互式 vim/less，能力受限。
  - (d) 终端单实例（无多 tab）—— 否决。用户选定多终端 tab 方案，与工作台 FileTabs 对称。

## 风险与缓解

| 风险 | 缓解 |
|---|---|
| 存量工作区 name=NULL 展示回退 | 派生 label 逻辑保留，回退路径单测覆盖 |
| add_directory 语义变更破坏既有调用 | 前端所有入口统一改造 + 后端 NoOpenWorkspace 错误兜底 + 集成测试更新 |
| portable-pty 跨平台 shell 检测 | 默认 shell 走 env（Windows: $env:ProgramFiles\PowerShell\pwsh.exe 回退 cmd.exe；Unix: $SHELL 回退 /bin/bash）+ 单测 mock |
| xterm.js 依赖体积 | xterm.js ~150KB gzip，可接受；按需 import 不入主 bundle |
| 终端 pty 进程泄漏 | TerminalManager kill + workspace close 时清理该工作区全部终端 + Shell unmount cleanup |
| tauri-specta bindings 需 regen | 契约先行：命令变更提交内 `pnpm contracts:gen` + `pnpm contracts:check` |

## 影响范围

### Migration
- **新增** `migrations/0041_workspace_name.sql` — `ALTER TABLE workspaces ADD COLUMN name TEXT`

### 后端 (nuomi-core)
- `crates/nuomi-core/src/store/repos/workspaces.rs` — WorkspaceEntry 加 name 字段 + SQL 读写
- `crates/nuomi-core/src/services/workspace_registry_v2.rs` — 加 `create(name, initial_path?)` + add_directory 语义变更 + NoOpenWorkspace 错误
- `crates/nuomi-core/src/services/workspace_label.rs` — 展示优先级 name > label > id（derive_display 逻辑保留）
- **新增** `crates/nuomi-core/src/services/terminal_manager.rs` — TerminalManager (portable-pty)

### IPC 契约 (nuomi-shell)
- `src-tauri/src/commands.rs` — 新增 workspace_create + 4 terminal 命令 + WorkspaceEntryDto 扩 name
- `src-tauri/src/tauri_cmds.rs` / `lib.rs` — 注册与薄壳
- `src/lib/ipc/bindings.gen.ts` — 重新生成

### 前端
- `src/lib/store/uiStore.ts` — ActiveArea 加 "terminal" + terminalByWorkspace 分桶 + 上一个 area 记忆
- `src/features/shell/AreaNav.tsx` — AREA_TABS 加终端
- `src/features/shell/Shell.tsx` — renderMainContent 加 terminal 分支 + alt+` 快捷键
- **新增** `src/features/terminal/TerminalArea.tsx` / `TerminalTabs.tsx` / `TerminalView.tsx`
- **新增** `src/features/shell/WorkspaceCreateDialog.tsx`
- `src/features/shell/WorkspaceListDialog.tsx` — 改造为创建工作区入口
- `src/features/shell/WorkspaceBottomBar.tsx` — + 按钮改造（无工作区时弹创建对话框）
- `src/i18n/locales/zh-CN.json` / `en.json` — 双语词条

### 不涉及
- ADR-0021 其他决策（单开、目录唯一归属、split 退役、会话归属等）——零变更
- 派生 label 逻辑——保留作兜底
- CLI agent pty（harness 既有）——独立链路，不共享 TerminalManager

## 验证门禁

- `cargo test -p nuomi-core`（含 workspace_create + add_directory 新语义 + TerminalManager 单测）
- `cargo clippy --all-targets -- -D warnings`
- `pnpm contracts:gen` 无未提交 diff + `pnpm contracts:check`
- `pnpm typecheck && pnpm lint && pnpm test -- --no-file-parallelism`
- 人工验证：`pnpm tauri dev` 下创建命名工作区、添加目录、alt+` toggle 终端、多终端 tab、终端 cwd 跟随工作目录
