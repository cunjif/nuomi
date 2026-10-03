# ADR 0016: 工作区文件操作 IPC 契约 + 文件树多选右键菜单

- **状态**: Accepted
- **日期**: 2026-10-03
- **决策者**: user + conductor

## 背景

工作台右侧工作区目录树（`FileTree.tsx`）当前仅支持单击展开目录 / 单击打开文件，无任何文件操作（新建/重命名/删除/复制/粘贴）入口，也无右键菜单与多选能力。用户要求参考 VSCode，增加：

1. 文件操作的"右键"功能（新建/重命名/删除/复制/粘贴/复制路径）
2. `Ctrl` + 鼠标左单击：toggle 多选
3. `Shift` + 鼠标左单击：范围批量选择

当前 `WorkspaceService`（`nuomi-core/src/services/workspace.rs`）仅暴露 `list_dir` / `read_file` / `write_file_atomic` 三个方法，无 create/delete/rename/copy。前端 `src/components/ui/` 无 ContextMenu 组件，`uiStore` 无"选中节点"概念（只有"打开的文件" `openFiles` / `activeFile`）。

## 决策

### 1. Rust 端：`WorkspaceService` 新增 5 个文件操作方法

全部复用现有 `resolve()` 沙箱校验（防 `..`/绝对路径/symlink 逃逸，已由 ADR 0007 / 现有测试覆盖）。

| 方法 | 签名 | 语义 |
|---|---|---|
| `create_file` | `(rel, content)` | 写空文件（content 默认 ""）；父目录不存在则 `create_dir_all` |
| `create_dir` | `(rel)` | `create_dir_all`，幂等 |
| `delete` | `(rel)` | 文件→`remove_file`，目录→`remove_dir_all`；不存在视为错误 |
| `rename` | `(from, to)` | `rename`，from/to 都走 resolve；to 已存在则先 remove（对齐 `write_file_atomic` 的 Windows 处理） |
| `copy` | `(from, to)` | 文件→`std::fs::copy`，目录→递归 copy；to 已存在则错误（不覆盖，避免误合并） |

**安全约束**：
- 所有路径经 `resolve()` 校验，逃逸返回 `WorkspaceError::Escape`
- `delete` 不做"删除到回收站"——工作区是用户自己的目录，物理删除对齐 VSCode 行为；前端弹 Dialog 二次确认
- `copy` 目录递归时每条子路径都走 `resolve()` 校验（防止复制过程中逃逸）

### 2. IPC 契约：5 个新命令

`src-tauri` 三处注册（对齐现有 `list_dir`/`read_file`/`write_file` 模式）：
- `commands.rs`：`impl_create_file` / `impl_create_dir` / `impl_delete` / `impl_rename` / `impl_copy`
- `tauri_cmds.rs`：`#[tauri::command] #[specta::specta]` 包装
- `lib.rs`：`collect_commands!` 宏注册

跑 `pnpm contracts:gen` 重新生成 `bindings.gen.ts`（camelCase，对齐现有约定）。

### 3. 前端：新建 `ContextMenu` 组件

`src/components/ui/ContextMenu.tsx`：轻量右键菜单组件。
- portal 到 `document.body`（避免被父容器 overflow 裁剪）
- 定位：右键坐标 + 视口边界自适应（超出右/下边则左/上翻转）
- 关闭：Esc / 外部点击 / 滚动
- 菜单项：`{ id, label, icon?, disabled?, onSelect }[]`
- 分隔线：`{ type: "separator" }`
- 样式对齐手绘纸质风（`sketch-btn` / `border-ink-muted/30` / `bg-surface-raised`）

### 4. 前端：`uiStore` 新增多选状态

```ts
selectedPaths: string[]        // 当前选中的节点路径（多选时多个）
lastSelectedPath: string | null // shift 范围选择的锚点
clipboardPaths: string[]        // 复制/剪切的剪贴板（工作区内）
clipboardMode: "copy" | "cut" | null
setSelectedPaths: (paths) => void
toggleSelected: (path) => void
selectRange: (from, to, visiblePaths) => void
clearSelection: () => void
setClipboard: (paths, mode) => void
```

**与 `openFiles`/`activeFile` 的关系**：`selectedPaths` 是文件树面板内的"选中"概念（可多选、可含目录），`openFiles` 是"已打开的编辑器 tab"（仅文件、单例）。二者独立——选中不等于打开。

### 5. 前端：`FileTree` 改造

#### 节点点击语义（参考 VSCode）

| 操作 | 行为 |
|---|---|
| 单击文件 | `clearSelection` → 选中该节点 → `openFile(path)` |
| 单击目录 | `clearSelection` → 选中该节点 → 切换展开/折叠 |
| `Ctrl`+单击 | `toggleSelected(path)`（不打开、不折叠） |
| `Shift`+单击 | `selectRange(lastSelectedPath, path, visiblePaths)`（按当前可见渲染顺序） |
| 右键 | 若点中节点不在 `selectedPaths` → `setSelectedPaths([path])`；再弹 ContextMenu |

**Shift 范围选择**：`visiblePaths` 是当前 DOM 中已渲染（即已展开目录内）的节点路径按顺序排列的数组；`selectRange` 取 from/to 在该数组中的索引，选中两者之间（含端点）的所有节点。未展开目录的子节点不参与（对齐 VSCode）。

#### 选中态高亮

- 单选/多选节点：`bg-surface-overlay text-ink-accent`（复用现有 activeFile 高亮色）
- `activeFile`（当前编辑的文件）额外加左侧 2px 强调条（`border-l-2 border-ink-accent`）以区分"选中"与"正在编辑"

#### Inline 重命名

重命名菜单项触发时，目标节点原地变为 `<input>`：
- 初始值=当前名，全选文件名（不含扩展名）
- Enter 确认 → 调 `rename` → invalidate 父目录 query
- Esc 取消
- 失焦确认（对齐 VSCode）
- 校验非法字符（`/ \ : * ? " < > |`）——非法时 input 红框 + 不确认

### 6. 右键菜单项

| 菜单项 | 可用条件 | 行为 |
|---|---|---|
| 新建文件 | 单选目录 / 单选文件（在其父目录） | 弹 input 输入名 → `createFile` |
| 新建文件夹 | 同上 | 弹 input 输入名 → `createDir` |
| 重命名 | 单选 | 节点变 inline input |
| 删除 | 单选或多选 | 弹 Dialog 列出待删项确认 → 批量 `delete` |
| 复制 | 单选或多选 | `setClipboard(paths, "copy")` |
| 剪切 | 单选或多选 | `setClipboard(paths, "cut")` |
| 粘贴 | 单选目录 / 单选文件（在其父目录）且有剪贴板 | 逐项 `copy` 或 `rename`（cut）→ invalidate |
| 复制路径 | 单选或多选 | 写入系统剪贴板（`@tauri-apps/api/clipboard`） |

**多选时**：仅"删除/复制/剪切/复制路径"可用，"新建/重命名/粘贴"禁用（灰显）。

### 7. 删除确认 Dialog

复用现有 `src/components/ui/Dialog.tsx`：
- 标题："确认删除" / "确认删除 N 项"
- 内容：列出待删路径（多选时最多显示 10 条 + "等 N 项"）
- 按钮：取消 / 删除（danger 色）
- 删除目录时额外提示"该文件夹及其所有内容将被永久删除"

### 8. 文件操作 hooks

在 `FileTree.tsx` 旁或新建 `fileOps.ts`：每个操作一个 `useMutation`，成功后 `queryClient.invalidateQueries({ queryKey: ["dir", parentPath] })`。批量操作用 `Promise.all` 或串行（删除串行避免并发冲突，复制可并发）。

## 影响范围

### Rust
- `crates/nuomi-core/src/services/workspace.rs`：+5 方法 + 测试
- `src-tauri/src/commands.rs`：+5 impl 函数
- `src-tauri/src/tauri_cmds.rs`：+5 命令包装
- `src-tauri/src/lib.rs`：collect_commands! 注册 5 个

### 前端
- `src/lib/ipc/bindings.gen.ts`：重生（5 个新命令）
- `src/lib/ipc/client.ts`：+5 个 ipc 方法
- `src/lib/store/uiStore.ts`：+多选/剪贴板状态 + setter
- `src/components/ui/ContextMenu.tsx`：新建
- `src/features/shell/FileTree.tsx`：改造 onClick/onContextMenu/高亮/inline 重命名
- `src/features/shell/fileOps.ts`（新建）：文件操作 hooks
- `src/features/shell/DeleteConfirmDialog.tsx`（新建）：删除确认
- i18n key 新增（`files.newFile` / `files.newFolder` / `files.rename` / `files.delete` / `files.copy` / `files.cut` / `files.paste` / `files.copyPath` / `files.deleteConfirm` 等）

### 不变
- `WorkspaceService::resolve` 沙箱校验逻辑不变（复用）
- `openFiles` / `activeFile` 语义不变（"选中"是新增独立概念）
- `FileTabs` / `MonacoTab` 不变
- 现有 `list_dir` / `read_file` / `write_file` IPC 不变

## 权衡

- **优点**：复用现有沙箱校验，安全边界不变；多选/右键对齐 VSCode 直觉；inline 重命名体验好；删除有确认兜底
- **代价**：5 个新 IPC 命令 + ContextMenu 新组件 + uiStore 状态膨胀；inline 重命名需处理光标/失焦/校验，实现复杂度中等
- **替代方案**：
  - (a) 重命名用 Dialog 而非 inline——否决，用户选 inline
  - (b) 删除不确认直接删——否决，用户选 Dialog 确认
  - (c) 多选状态放 TanStack Query 而非 uiStore——否决，选中是纯 UI 瞬态，不持久化，属 uiStore 职责
  - (d) ContextMenu 用第三方库（radix-ui 等）——否决，项目无此依赖，自建轻量组件对齐手绘风

## 实施计划

1. Rust: `WorkspaceService` +5 方法 + 单测
2. src-tauri: commands + tauri_cmds + lib 注册
3. `pnpm contracts:gen` 重生 bindings
4. 前端: client.ts +5 ipc 方法
5. 前端: uiStore 多选/剪贴板状态
6. 前端: ContextMenu 组件
7. 前端: fileOps hooks + DeleteConfirmDialog
8. 前端: FileTree 改造（onClick/onContextMenu/高亮/inline 重命名/菜单项）
9. i18n key 补全
10. 验证: cargo test + clippy + pnpm typecheck/lint/test
