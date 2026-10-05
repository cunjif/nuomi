# ADR 0017: 多工作区文件编辑面板（工作区 tab 栏 + 可分屏 + 跨工作区编辑）

- **状态**: Proposed
- **日期**: 2026-10-04
- **决策者**: user + conductor

## 背景

当前"工作台"（`WorkbenchArea.tsx:5` 注释自述 `single-workspace file editor`）同一时刻只能显示一个工作区的文件编辑面板。核心限制：

1. `uiStore.openFiles / activeFile` 是全局单份 `string[]` / `string | null`，无 `workspaceId` 维度（`uiStore.ts:87-88`）。
2. `switchWorkspace` 整体替换 `openFiles` / `activeFile`，旧工作区 tab 存入 `workspaceTabs` 快照后从 UI 消失（`uiStore.ts:297-317`，测试 `uiStore.workbench.test.ts:77-99` 佐证）。
3. `SplitView` 已传 `workspaceId` 给 `renderWorkspace`（`SplitView.tsx:45,71`），但 `Shell.tsx:332-335` 的 `renderWorkspace` 写成 `() => ...` 忽略了它，导致两 pane 渲染同一 `WorkbenchArea`、消费同一全局 store。
4. IPC 文件操作 `listDir / readFile / writeFile / createFile / createDir / delete / rename / copy / gitStatus / ...` 只收 `path`，后端隐式用 active workspace 解析（`client.ts:106-122`）。
5. `MonacoTab` props 只有 `path`（`MonacoTab.tsx:32-34`），`FileTabs` 的 `key` 是纯路径（`FileTabs.tsx:29`）。
6. 跨工作区搜索结果无 `onClick`，无法点击打开（`CrossSearchPanel.tsx:74-84`）；`QuickOpen` 只搜当前工作区（`QuickOpen.tsx:202-207`）。

`pr.md` 与 `docs/specs/multi-workspace-ipc.md` 均未提出"同时打开多个工作区的文件编辑面板"需求，属新增能力。`multi-workspace-ipc.md` 已定义 `single | split | overview` 布局模式与 `splitWorkspaceIds` 持久化，本 ADR 复用该基础设施。

## 决策

### 9 项设计决策（用户确认）

| # | 决策点 | 选择 |
|---|---|---|
| 1 | 布局选型 | **C**：工作区 tab 栏 + 可分屏（single 默认；右键 tab "在右侧分屏打开" 进 split） |
| 2 | 跨工作区搜索结果点击行为 | 拉到当前工作区打开 |
| 3 | Ctrl+P 范围 | 加"当前/全部"模式切换 |
| 4 | 脏文件关闭确认 | 扩展到单文件级 |
| 5 | Monaco 实例数 | 接受多实例内存成本（每工作区独立 Monaco，split 下两实例并存） |
| 6 | FileTree 形态 | 每工作区独立一棵 FileTree |
| 7 | 跨工作区文件保存语义 | **回写源工作区原文件**（IPC 需 `sourceWorkspaceId` 参数） |
| 8 | 工作区 tab 与 chat tab 联动 | chat tab 跨工作区聚合不变（工作区 tab 与 chat tab 正交） |
| 9 | split 下 WorkspaceTabBar 形态 | 隐藏 tab 栏，用 pane 头代替 |

### 1. 后端 IPC：文件操作加 `workspaceId` 可选参数

给文件操作命令增加可选 `workspaceId?: string | null` 参数，缺省回退到 active workspace（保旧前端不破）。涉及命令（camelCase，对齐 tauri-specta 生成约定）：

```
listDir / readFile / writeFile / createFile / createDir / delete / rename / copy
gitStatus / gitLog / gitStage / gitCommit / gitPush / gitWorktrees / gitDiff / gitStagedDiff
```

后端 `WorkspaceService` 已有 `resolve()` 沙箱校验（ADR 0007/0016），新增 `resolve_for(workspace_id, rel)` 方法：按 `workspace_id` 查表得 `rootPath`，再走现有 `resolve()` 逻辑。`workspace_id` 不存在或目录缺失返回 `WorkspaceError::NotFound`。

**跨工作区编辑回写**（决策 7）：`writeFile(path, content, workspaceId?)` 的 `workspaceId` 即"源工作区 ID"，编辑器跨工作区打开文件后 Ctrl+S 时传源工作区 ID 回写原文件。

### 2. 前端 store：按 `workspaceId` 分桶

`uiStore` 的编辑器状态改为按 `workspaceId` 分桶的 `Record`：

```ts
interface WorkspaceEditorState {
  openFiles: string[];
  activeFile: string | null;
  selectedPaths: string[];
  lastSelectedPath: string | null;
  clipboardPaths: string[];
  clipboardMode: "copy" | "cut" | null;
  dirtyPaths: Record<string, boolean>;
  // 跨工作区只读引用：key=本地 tab 标识，value={ sourceWorkspaceId, sourcePath }
  crossRefs: Record<string, { sourceWorkspaceId: string; sourcePath: string }>;
}

interface UiState {
  // 删除：openFiles / activeFile / selectedPaths / lastSelectedPath /
  //       clipboardPaths / clipboardMode / dirtyPaths / workspaceTabs
  editorByWorkspace: Record<string, WorkspaceEditorState>;
  // 保留：activeWorkspaceId / openWorkspaceIds / focusedWorkspaceId /
  //       pinnedWorkspaceIds / layoutMode / splitWorkspaceIds
  // ...
  openFile: (path: string, workspaceId: string) => void;
  closeFile: (path: string, workspaceId: string) => void;
  // ...
}
```

`switchWorkspace` 简化为只切 `activeWorkspaceId`（各工作区 tab 状态本就独立保留在桶里，不再需要 `workspaceTabs` 快照存取）。`WorkspaceTabSnapshot` 与 `workspaceTabs` 字段废弃。

### 3. 前端组件：加 `workspaceId` prop

| 组件 | 改动 |
|---|---|
| `WorkbenchArea` | 加 `workspaceId: string` prop，透传给子组件 |
| `EditorArea` | 加 `workspaceId`，从 `editorByWorkspace[workspaceId]` 取状态 |
| `FileTabs` | 加 `workspaceId`，`key` 改为 `${workspaceId}:${path}` |
| `MonacoTab` | 加 `workspaceId`，`queryKey` 加维度 `["file", workspaceId, path]`，IPC 传 `workspaceId` |
| `FileTree` | 加 `workspaceId`，`listDir("")` 传 `workspaceId`，点击 `openFile(path, workspaceId)` |

### 4. 新组件 `WorkspaceTabBar`

位于主内容区顶部（ChatTabBar 下方，或与 ChatTabBar 同行右侧——见实施计划阶段 3 确认）。N 个工作区 tab + `[+]` 添加 + `[⬚]` 分屏按钮。tab 显示工作区名 + 路径哈希自动取色圆点（已有决策，memory `project-multi-workspace-architecture-decisions`）。右键菜单："在右侧分屏打开" / "关闭" / "固定"。

split 模式下隐藏 `WorkspaceTabBar`，每个 pane 头部显示工作区名 + 颜色 + "关闭/最大化"按钮（复用 `SplitView` 现有 pane 头，扩展显示工作区名）。

### 5. Shell 装配修复

```tsx
// Shell.tsx renderMainContent 改为：
if (layoutMode === "split") {
  return (
    <SplitView
      renderWorkspace={(wsId) =>
        activeArea === "workbench" ? <WorkbenchArea workspaceId={wsId} /> : renderView(view, wsId)
      }
    />
  );
}
// single 模式：
return (
  <>
    <WorkspaceTabBar />
    {activeArea === "workbench"
      ? <WorkbenchArea workspaceId={activeWorkspaceId} />
      : renderView(view, activeWorkspaceId)}
  </>
);
```

`renderView` 加 `workspaceId` 透传参数（决策 8：chat tab 跨工作区聚合不变，故 `ConversationView` 可忽略 `workspaceId`；`BoardView / TraceView / GitView / SchedulerView` 按 `workspaceId` 过滤数据）。

### 6. 跨工作区打开入口

**CrossSearchPanel**（决策 2 + 7）：match 项加 `onClick`：
- `focusWorkspace(当前 activeWorkspaceId)`（确保当前工作区聚焦）
- `openFile(match.relativePath, 当前 activeWorkspaceId)`
- 在当前工作区桶的 `crossRefs` 记录 `{ sourceWorkspaceId: match.workspaceId, sourcePath: match.relativePath }`
- `MonacoTab` 检测 `crossRefs` 命中时，`readFile` 用 `sourceWorkspaceId` 读取、`writeFile` 用 `sourceWorkspaceId` 回写；编辑器顶部标注"来自 ws-B · 跨工作区编辑"badge

**QuickOpen**（决策 3）：加"当前工作区 / 全部工作区"模式切换（Tab 键或下拉）。"全部"模式复用 `crossWorkspaceSearch`，结果项带工作区颜色标记，点击行为同 CrossSearchPanel。

### 7. 脏文件关闭确认扩展（决策 4）

- `closeFile(path, workspaceId)`：若 `editorByWorkspace[workspaceId].dirtyPaths[path]` 为 true，弹确认 Dialog（"保存 / 不保存 / 取消"）。
- `closeWorkspace(id, force=false)`：聚合该工作区桶内所有 `dirtyPaths` 为 true 的文件列表，弹确认 Dialog 列出脏文件清单。
- split 下关闭某 pane：同 `closeWorkspace` 流程。

## UI 布局

### single 模式（默认）

```
┌─────────┬──────────────────────────────────────────────────────┐
│ LeftRail│ [ws-A 蓝●] [ws-B 绿] [ws-C 橙] [+]            ⬚分屏  │  WorkspaceTabBar
│ 会话列表│ ┌─FileTabs (ws-A 桶)─────────────────────────────────┐ │
│ (跨ws   │ │ a.ts │ b.rs │ ×                                │ │
│  聚合)  │ └────────────────────────────────────────────────────┘ │
│         │ ┌─FileTree(ws-A)──┬─Monaco(ws-A, a.ts)──────────────┐  │
│         │ │ src/            │ a.ts                            │  │
│         │ │  a.ts           │ ...                             │  │
│         │ └─────────────────┴─────────────────────────────────┘  │
└─────────┴──────────────────────────────────────────────────────┘
```

### split 模式（右键 ws-B tab "在右侧分屏打开"）

```
┌─────────┬────────────────────────┬────────────────────────────┐
│ LeftRail│   ws-A (蓝) ●  ⤢关闭   │   ws-B (绿)    ⤢关闭       │  pane 头（无 WorkspaceTabBar）
│         │ ┌─FileTabs──────────┐  │ ┌─FileTabs──────────────┐  │
│         │ │ a.ts │ b.rs │ ×   │  │ │ c.ts │ ×             │  │
│         │ └────────────────────┘  │ └────────────────────────┘  │
│         │ ┌─FileTree─┬─Monaco─┐   │ ┌─FileTree─┬─Monaco────┐   │
│         │ │ src/     │ a.ts   │   │ │ lib/     │ c.ts      │   │
│         │ └──────────┴────────┘   │ └──────────┴───────────┘   │
└─────────┴────────────────────────┴────────────────────────────┘
```

### 跨工作区编辑 badge

```
┌─Monaco(ws-A, 编辑 ws-B 的 c.ts)──────────────────────────┐
│ ◆ 来自 ws-B · 跨工作区编辑                    [保存回 ws-B] │
│ ...                                                    │
└────────────────────────────────────────────────────────┘
```

## 向后兼容

- IPC 文件操作 `workspaceId` 参数为可选（`Option<String>`），缺省回退 active workspace，旧前端不破。
- `editorByWorkspace` 桶初始化时从旧 `openFiles / activeFile / workspaceTabs` 迁移一次（启动时若 `editorByWorkspace` 空且 `openFiles` 非空，写入 `editorByWorkspace[activeWorkspaceId]`）。
- `multi-workspace-ipc.md` spec 补一节"带 workspaceId 的文件操作"，对齐现有命令族格式。

## 权衡

- **优点**：复用已有 `splitWorkspaceIds` / `layoutMode` / `SplitView` 框架，增量改动；工作区 tab 栏与 ChatTabBar 风格一致；跨工作区编辑真实生效（非只读）。
- **代价**：~15 IPC 命令加可选参数 + store 中段重写 + 6 组件加 prop + 新 `WorkspaceTabBar` 组件 + `crossRefs` 跨工作区引用追踪；多 Monaco 实例内存成本（决策 5 接受）。
- **替代方案**：
  - (a) 方案 A 纯分屏（同时只 2 工作区）——否决，用户选 C 更灵活
  - (b) 方案 B 纯 tab 栏（无分屏对照）——否决，用户要对照能力
  - (c) 跨工作区文件只读快照——否决，用户要真实编辑回写
  - (d) chat tab 按工作区过滤——否决，用户要 chat tab 跨工作区聚合不变
  - (e) split 下保留 WorkspaceTabBar——否决，用户选 pane 头代替更简洁

## 风险与缓解

| 风险 | 缓解 |
|---|---|
| 多 Monaco 实例内存（split 下 2x） | 接受（决策 5）；非聚焦工作区 Monaco 可考虑 `keepAlive` 卸载内容保留 undo stack |
| `editorByWorkspace` 桶无上限 | 工作区关闭时清桶；pinned 工作区桶保留 |
| 跨工作区编辑回写冲突（两工作区同时编辑同一文件） | `crossRefs` 按 `sourceWorkspaceId+sourcePath` 去重；写入时重新 `readFile` 比对 mtime，冲突弹 Dialog |
| 既有 `uiStore.workbench.test.ts` 断言失效 | 重写为按桶断言 |
| `EditorArea` 预存 i18n 失败测试（memory） | 不在本 ADR 范围，沿用既有绕过策略 |
| tauri-specta bindings 需 regen | 实施计划阶段 4 显式 `pnpm contracts:gen` |

## 实施计划

分 7 阶段，每阶段独立可验证、可提交（对齐 memory `feedback-commit-stage-by-logical-group`）。

### 阶段 1：后端 IPC 加 workspaceId 参数

1. `nuomi-core/src/services/workspace.rs`：`resolve_for(workspace_id, rel)` 方法 + 单测
2. `src-tauri/commands.rs` + `tauri_cmds.rs`：~15 命令加 `workspace_id: Option<String>` 参数，委托 `resolve_for` 或回退 active
3. `cargo test -p nuomi-core` + `cargo clippy` 验证
4. 提交："feat(ipc): 文件操作加 workspaceId 可选参数"

### 阶段 2：重生 bindings + client.ts

1. `pnpm contracts:gen`
2. `src/lib/ipc/client.ts`：文件操作方法签名加可选 `workspaceId`
3. `pnpm typecheck` 验证
4. 提交："feat(ipc): 前端 client 文件操作透传 workspaceId"

### 阶段 3：store 分桶

1. `src/lib/store/uiStore.ts`：`editorByWorkspace` 桶 + `crossRefs` + `openFile/closeFile/setActiveFile/...` 改签名
2. 删除 `workspaceTabs` / `WorkspaceTabSnapshot`，`switchWorkspace` 简化
3. 旧字段迁移逻辑（启动时一次性）
4. 重写 `src/lib/store/uiStore.workbench.test.ts` 按桶断言
5. `pnpm test` 验证
6. 提交："refactor(store): 编辑器状态按 workspaceId 分桶"

### 阶段 4：组件加 workspaceId prop

1. `WorkbenchArea / EditorArea / FileTabs / MonacoTab / FileTree` 加 prop + 内部选择器改按桶
2. `MonacoTab` queryKey 加 workspaceId 维度
3. 现有调用点（Shell single 模式）传 `activeWorkspaceId`
4. `pnpm typecheck && pnpm test` 验证
5. 提交："refactor(shell): 编辑器组件加 workspaceId prop"

### 阶段 5：WorkspaceTabBar + Shell 装配

1. 新 `src/features/shell/WorkspaceTabBar.tsx`：N tab + [+] + 分屏按钮 + 右键菜单
2. `Shell.tsx`：single 模式渲染 `WorkspaceTabBar`，split 模式 `renderWorkspace` 用 wsId
3. `SplitView` pane 头扩展显示工作区名 + 颜色
4. `pnpm test` 验证
5. 提交："feat(shell): WorkspaceTabBar + split 装配修复"

### 阶段 6：跨工作区打开入口

1. `CrossSearchPanel`：match 加 `onClick` + `crossRefs` 记录
2. `MonacoTab`：检测 `crossRefs` 命中时读写用 `sourceWorkspaceId` + badge
3. `QuickOpen`：加"当前/全部"模式切换
4. `pnpm test` 验证
5. 提交："feat(shell): 跨工作区文件打开与编辑回写"

### 阶段 7：脏文件关闭确认扩展 + spec

1. `closeFile / closeWorkspace` 脏文件确认 Dialog（单文件级 + 工作区聚合清单）
2. `docs/specs/multi-workspace-ipc.md` 补"带 workspaceId 的文件操作"节
3. `pnpm test` + `cargo test` 验证
4. 提交："feat(shell): 跨工作区脏文件关闭确认 + spec 补全"

## 验证门禁

每阶段必须通过：
- `cargo test -p nuomi-core` + `cargo clippy`（涉及 Rust 时）
- `pnpm typecheck && pnpm lint`
- `pnpm test`（单线程模式，memory `feedback-test-suite-single-thread`）
- 人工验证：`pnpm tauri dev` 启动后多工作区 tab 切换、分屏、跨工作区编辑回写
