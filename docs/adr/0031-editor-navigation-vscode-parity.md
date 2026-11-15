# ADR 0031: 编辑器导航体验对标 VSCode

- **状态**: Accepted
- **日期**: 2026-11-14
- **决策者**: user + ui-engineer/bridge-engineer
- **依赖**: ADR-0027（LSP 代码智能提示）、ADR-0028（LSP 自动检测）

## 背景

ADR-0027/0028 已交付完整 LSP 集成（hover/completion/definition/references 等 provider 已注册进 Monaco，语义能力与 VSCode 同源——跑同样的 pyright/gopls/rust-analyzer 等 LS）。2026-11-14 勘察确认导航**体验**层与 VSCode 尚有六处差距：

1. **符号导航断线**：后端 `documentSymbol` 已实现（`client.rs` `textDocument/documentSymbol`）且 IPC `lspDocumentSymbols` 已生成（`bindings.gen.ts`），但前端零调用——无 `DocumentSymbolProvider` 注册，Monaco 原生 Ctrl+Shift+O 符号跳转不可用；符号视图仅有 regex 版 `outline-symbols`。
2. **导航历史完全缺失**：全仓无 `navigationHistory`/`jumpBack` 实现，跨文件跳转（`requestOpenSymbol` → `openFile` + `pendingJump`）是单向的，无法"后退"回原位置（VSCode Ctrl+- / Ctrl+Shift+-）。
3. **跨文件跳转 opener 未接线**：全仓 `openCodeEditor`/opener 相关配置零命中。LSP `DefinitionProvider` 返回的 `file://` Location 在 Monaco standalone 模式下，对**未加载**文件的处理没有自定义 handler——Ctrl+Click 跨文件跳转大概率静默失败；当前仅 codebase-index 的自绘 F12/Alt+F1 链路（regex index）可跨文件。
4. **`gotoLocation.*` 未配置**：`MonacoTab.tsx` editor options 未设置任何 gotoLocation 项，多定义/多引用行为全走 Monaco 默认值，peek 体验未验证。
5. **能力面缺口**：`client.rs` 无 rename/prepareRename、workspaceSymbol、typeDefinition、implementation、codeAction 请求。
6. **超时硬编码**：`lsp-bridge.tsx` `COMPLETION_TIMEOUT_MS = 500` / `REQUEST_TIMEOUT_MS = 2000`——LS 冷启动（jdtls 可达 10s+）期间请求静默返回空，降级 regex 提示。

## 决策

### 1. 跨文件跳转 opener 接线（导航基座，最高优先）

把 LSP Location（`file:///...` uri + range）映射到既有跨文件打开链路 `uiStore.openFile` + `pendingJump`（复用 `MonacoTab` 的 `consumePendingJump` → `revealLineInCenter` + jump-flash）。

- 机制优先采用 Monaco 为 standalone 场景提供的 editor opener / `openCodeEditor` 能力（官方 cross-editor 场景的标准做法）；若当前 Monaco 版本该机制受限，则降级为在 provider 层拦截（definitionProvider 返回前直接改走自建跳转）。**具体 API 以实施时运行验证定稿，不在此锁死**。
- 打通后 Monaco 原生 Ctrl+Click / Shift+F12 peek 内的"跳转"按钮对 LSP 结果跨文件可用，与 codebase-index 的 Ctrl+Click 设计意图（`codebase-index.tsx` 文件头注释）对齐。

### 2. DocumentSymbolProvider 接线（Ctrl+Shift+O）

- `lsp-bridge.tsx` 新增 `registerDocumentSymbolProvider`：LSP DocumentSymbol 树 → Monaco DocumentSymbol 映射（含 kind、range、selectionRange、children）。
- 双轨策略对齐 hover 既有模式：LSP 结果优先，为空/超时时回退 regex 版 `outline-symbols` 数据（不删既有 provider，Monaco 对同名 provider 结果合并/竞争行为按返回时序，实施时验证取其一）。
- 后端 `lspDocumentSymbols` IPC 已就绪，纯前端接线。

### 3. 导航历史（跨 tab 后退/前进）

Monaco 内置 `navigationHistory` 是 per-editor 实例，跨 tab（`openFile` 换 model）不覆盖，故**自建轻量跨 tab 跳转栈**：

- `uiStore` 新增 jump history：`{ entries: [{workspaceId, fileId, line, col}], index }`，容量上限 50，`push`/`back`/`forward` 语义对齐 VSCode navigateBack/Forward（后退不压新栈，前进位置随新跳转截断）。
- **跳转入口统一压栈**：opener 跳转、NavPanel 选择跳转、peek 内跳转、`consumePendingJump`（跳转前记录 origin）。
- 后退执行：openFile + pendingJump(restore) 复用既有链路。
- 键位：**Alt+← / Alt+→**（不占用 Ctrl+- ——webview 环境下浏览器缩放键冲突），右键菜单补"后退/前进"项。

### 4. `gotoLocation` 显式配置 + peek 验证

- `MonacoTab.tsx` editor options 显式设置 `gotoLocation.multipleDefinitions: "peek"`、`gotoLocation.multipleReferences: "peek"`（Monaco 默认即 peek，显式声明防版本漂移）。
- 验证 `editor.action.peekReferences`（Shift+F12）随 ReferenceProvider 自动可用，及 peek 内嵌内容对未加载文件的展示行为（依赖决策 1 的 opener）。

### 5. 能力面扩展（分里程碑）

| 里程碑 | 内容 | 落点 |
|---|---|---|
| **M1 体验基座** | 决策 1-4 + 超时可配置 | 前端为主 |
| **M2 语义增强** | **rename**（client.rs 加 `textDocument/rename` + `prepareRename`；Monaco `registerRenameProvider`；LSP WorkspaceEdit 应用走既有文件写 IPC，多文件改动后刷新脏状态）+ **workspaceSymbol**（client.rs 加 `workspace/symbol`，接入 QuickOpen 符号模式） | core + IPC + 前端 |
| **M3 可选后置** | typeDefinition / implementation（跳转变体）、codeAction（quickfix） | 同上 |

新 IPC 命令须走契约先行：Rust 命令 + specta 收集器 + bindings 再生成 + client/test-double 消费端对齐（同一批次）。

### 实现进展

- 2026-11-14：M1 收官（opener/documentSymbol/导航历史/gotoLocation/超时三档，13 新用例，纯前端零契约）
- 2026-11-14：M2 收官（rename 自绘管线——Monaco standalone bulkEditService 对未打开 model 抛错故弃内置流程；workspaceSymbol 接 QuickOpen symbols 模式 + Ctrl+T；契约三命令 +39 行；11 新用例）
- 2026-11-14：M3 收官（typeDefinition/implementation 走内置 action + M1 opener 复用，codeAction 走 Monaco 内置灯泡 + LSP→Monaco 转换纯函数；契约三命令累计 +63 行；6 新用例）。M2 实施修正一处设计：rename 未注册 Monaco RenameProvider 而是自绘（standalone bulkEditService 限制）。

### 6. 超时策略

- 超时常量提为按 provider 类型区分：completion 保持 500ms；hover/navigation 类提升至 3000ms 且集中定义（`lsp-bridge.tsx` 顶部常量区），不做用户级配置。
- LS 非 running 状态时前端直接快速失败返回 null（fallback regex 接管），不空等 2s；status=running 才发请求。实施时确认 `ensure_started` 启动中状态的 IPC 请求行为（阻塞 or 立即失败），以后端语义为准。

## 实现范围

- **前端**：`lsp-bridge.tsx`（documentSymbol provider、超时常量、非 running 快速失败）、`MonacoTab.tsx`（gotoLocation options、opener 接线、后退/前进命令）、`workspace-index.ts`/`codebase-index.tsx`（跳转入口压栈）、`uiStore`（jump history）、i18n 双语词条
- **Rust core**（M2）：`client.rs` 加 rename/prepareRename/workspaceSymbol 请求
- **IPC**（M2）：新命令 + bindings 再生成 + 消费端对齐
- **测试**：client.rs 单测（M2）、lsp-bridge provider 映射测试、jump history store 测试（纯 store 测试须副作用 import test/helpers）、MonacoTab 命令/选项测试

## 不做

- 不做 LSP 崩溃自动重启（ADR-0027 设计遗留议题，独立推进）
- 不做 semanticTokens / callHierarchy / inlayHint（semanticTokens 涉及语法高亮体系重构，另立议题）
- 不删 hover-docs/codebase-index 双轨 fallback（LSP 不可用时的体验保底）
- 不做用户级超时配置 UI

## 测试策略

- DocumentSymbol 树映射单测（LSP 嵌套 DocumentSymbol → Monaco DocumentSymbol）
- jump history store 测试：push/back/forward/截断/容量上限
- 跳转入口压栈集成测试（NavPanel/opener → history）
- gotoLocation options 存在性测试（对齐 monacoThemes 守护范式）
- M2：client.rs rename/workspaceSymbol 单测（FakeLSP mock）、WorkspaceEdit 应用测试、QuickOpen 符号模式测试

## 影响

- **正面**：Ctrl+Shift+O / Ctrl+Click 跨文件 / Alt+←→ 后退前进 / peek 多定义选择——补齐 VSCode 四项核心导航体验；rename 是编辑器高频操作的最大缺口
- **负面**：jump history 与 pendingJump 状态在 uiStore 的联动复杂度；M2 rename 的多文件 WorkspaceEdit 应用需处理并发写
- **风险**：Monaco standalone opener 机制的版本差异（决策 1 已预留降级路径）；peek 对未加载文件的展示行为依赖 opener 打通——M1 先验证此链路，若受阻则 NavPanel 自绘面板承担兜底职责
