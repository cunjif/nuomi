# 多工作区 IPC 契约

> 本文档记录多工作区并行管理新增的 IPC 命令族签名与用法。
> bindings 由 tauri-specta 自动生成（`src/lib/ipc/bindings.gen.ts`），禁止手改。

## 命令族总览

| 族 | 命令 | 用途 |
|---|---|---|
| open/close | `openWorkspace` / `closeWorkspace` / `closeAllWorkspaces` | 开启与关闭工作区 |
| focus | `focusWorkspace` | 切换聚焦工作区 |
| pin | `pinWorkspace` / `unpinWorkspace` | 固定/取消固定工作区 |
| layout | `getLayoutSnapshot` / `setLayoutSnapshot` | 布局快照持久化与恢复 |
| open-set | `getOpenSet` | 查询当前开启集合 + 聚焦 + 固定 + 未读指示器 |
| recent | `getRecentWorkspaces` | 查询最近使用工作区列表 |
| cross | `crossWorkspaceSearch` / `crossWorkspaceReference` / `crossWorkspaceCompare` | 跨工作区搜索、引用、比较 |
| audit | `detectIsolationViolations` | 隔离违规检测 |

## 签名与 DTO

### open/close 族

```typescript
openWorkspace(id: string): Promise<OpenWorkspaceResult>
// OpenWorkspaceResult = { workspaceId: string }

closeWorkspace(id: string, force: boolean): Promise<CloseWorkspaceResult>
// CloseWorkspaceResult = { closedId: string; newFocusedId: string | null }

closeAllWorkspaces(excludePinned: boolean): Promise<CloseWorkspaceResult[]>
```

- `force = true` 跳过脏文件/运行中任务确认，直接关闭。
- `closeAllWorkspaces` 批量关闭，`excludePinned = true` 时保留固定工作区。

### focus 族

```typescript
focusWorkspace(id: string): Promise<FocusWorkspaceResult>
// FocusWorkspaceResult = { workspaceId: string }
```

- 切换聚焦到指定工作区，更新 `lastFocusedAt` 时间戳，清零未读指示器。

### pin 族

```typescript
pinWorkspace(id: string): Promise<null>
unpinWorkspace(id: string): Promise<null>
```

- 固定工作区在启动时自动恢复，不会被 `closeAllWorkspaces(excludePinned: true)` 关闭。

### layout 族

```typescript
getLayoutSnapshot(): Promise<LayoutSnapshotDto | null>
setLayoutSnapshot(mode: string, splitWorkspaceIds: [string, string] | null): Promise<null>
// LayoutSnapshotDto = {
//   mode: string;                          // "single" | "split" | "overview"
//   splitWorkspaceIds: [string, string] | null;
//   focusedWorkspaceId: string | null;
//   capturedAt: number;
// }
```

- 启动时自动调用 `getLayoutSnapshot` 恢复上次布局。
- `setLayoutSnapshot` 在布局模式切换或分屏工作区变更时持久化。

### open-set 族

```typescript
getOpenSet(): Promise<OpenSetDto>
// OpenSetDto = {
//   openWorkspaces: OpenWorkspaceDto[];
//   focusedWorkspaceId: string | null;
//   pinnedWorkspaceIds: string[];
//   unreadIndicators: UnreadIndicatorDto[];
// }
// OpenWorkspaceDto = {
//   workspaceId: string;
//   openedAt: number;
//   lastFocusedAt: number;
//   isFocused: boolean;
// }
// UnreadIndicatorDto = { workspaceId: string; count: number }
```

- 前端启动时调用 `getOpenSet` 全量重建本地镜像（`uiStore.syncFromOpenSet()`）。
- `unreadIndicators` 用于非聚焦工作区标签的未读提示（任务完成/失败/待审批）。

### recent 族

```typescript
getRecentWorkspaces(limit: number): Promise<RecentWorkspaceDto[]>
// RecentWorkspaceDto = {
//   workspaceId: string;
//   lastUsedAt: number;
//   isPinned: boolean;
// }
```

- 容量可配置（`RECENT_LIST_CAPACITY`，默认 20）。

### cross 族

```typescript
crossWorkspaceSearch(query: string, matchContent: boolean): Promise<CrossSearchOutcomeDto>
// CrossSearchOutcomeDto = {
//   groups: CrossSearchGroupDto[];
//   skippedWorkspaceIds: string[];   // 目录缺失的工作区
// }

crossWorkspaceReference(sourceWorkspaceId: string, filePath: string): Promise<FileReferenceDto>
// FileReferenceDto = {
//   sourceWorkspaceId: string;
//   sourceRelativePath: string;
//   contentSnapshot: string;
// }

crossWorkspaceCompare(
  workspaceA: string, fileA: string,
  workspaceB: string, fileB: string,
): Promise<DiffResultDto>
// DiffResultDto = {
//   workspaceAId: string; workspaceBId: string;
//   fileAPath: string; fileBPath: string;
//   contentA: string; contentB: string;
//   isIdentical: boolean;
// }
```

- 目录缺失的工作区自动跳过并在 `skippedWorkspaceIds` 中标注。

### audit 族

```typescript
detectIsolationViolations(): Promise<IsolationViolationDto[]>
// IsolationViolationDto = {
//   runId: string;
//   taskId: string;
//   runWorkspaceId: string | null;
//   taskWorkspaceId: string | null;
// }
```

- 检测运行的 `workspace_id` 与其父任务的 `workspace_id` 不一致的记录（跨工作区数据串扰）。
- 返回空数组表示所有运行均正确隔离。

## 向后兼容

- 所有新增字段在 TS bindings 中为可选（`?:`）或 `| null`，支持旧前端降级。
- 存量命令（`getWorkspace` / `setWorkspace` / `activateWorkspace` / `getActiveWorkspace`）保持不变，内部委托到多工作区模型。

## 带 workspaceId 的文件操作（ADR 0017）

多工作区文件编辑面板要求文件操作能定位到具体工作区。以下命令新增可选参数 `workspace_id: Option<String>`：

- `None` 时回退到 `active_workspace_id`（向后兼容单工作区调用）。
- `Some(id)` 时通过 `AppState::workspace_for(workspace_id)` 解析对应 `WorkspaceEntry` 的 `root_path` 作为沙箱根。

### 文件操作族

```typescript
listDir(path: string, workspaceId?: string): Promise<DirEntry[]>
readFile(path: string, workspaceId?: string): Promise<string>
writeFile(path: string, content: string, workspaceId?: string): Promise<null>
createFile(path: string, content: string, workspaceId?: string): Promise<null>
createDir(path: string, workspaceId?: string): Promise<null>
delete(path: string, workspaceId?: string): Promise<null>
rename(oldPath: string, newPath: string, workspaceId?: string): Promise<null>
copy(oldPath: string, newPath: string, workspaceId?: string): Promise<null>
```

### Git 操作族

```typescript
gitStatus(workspaceId?: string): Promise<GitStatusDto>
gitLog(limit: number, workspaceId?: string): Promise<GitLogEntry[]>
gitStage(paths: string[], workspaceId?: string): Promise<null>
gitCommit(message: string, workspaceId?: string): Promise<null>
gitPush(remote: string, branch: string, workspaceId?: string): Promise<null>
gitWorktrees(workspaceId?: string): Promise<WorktreeDto[]>
gitDiff(path: string, workspaceId?: string): Promise<string>
gitStagedDiff(workspaceId?: string): Promise<string>
```

### 跨工作区编辑回写

当用户在 A 工作区打开了来自 B 工作区的文件（经 `crossWorkspaceSearch` 拉到当前打开），`uiStore.registerCrossRef(workspaceId=A, localPath, sourceWorkspaceId=B, sourcePath)` 记录映射。MonacoTab 读取/保存时使用 `effectiveWsId` / `effectivePath`（优先 crossRef 的 source 值），确保内容回写到源工作区 B 的原文件，而非 A 工作区沙箱。

### 脏文件关闭确认

- **单文件级**：`FileTabs` 的 × 按钮两步确认（第一次点 × 显示"确认关闭/取消"，第二次确认才执行 `closeFile`）。
- **工作区级**：`WorkspaceBottomBar` 的关闭按钮先检查 `editorByWorkspace[id].dirtyPaths`，若有脏文件则弹 Dialog 列出文件名，用户"强制关闭"后调 `closeWorkspace(id, force=true)`，否则直接 `closeWorkspace(id, force=false)`。
