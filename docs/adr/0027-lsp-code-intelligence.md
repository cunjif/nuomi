# ADR 0027: LSP 代码智能提示（Language Server Protocol 全语言深度集成）

- **状态**: Proposed
- **日期**: 2026-11-04
- **决策者**: user + conductor
- **依赖**: ADR-0002（事件通道模式）、ADR-0010（编辑器插件桥接）

## 背景

用户需求：在配置了 SDK & 运行环境之后，实现类似 VSCode 的代码文档提示效果——包括代码补全（Completion）、签名帮助（Signature Help）、诊断（Diagnostics）、Hover 文档增强等 IntelliSense 能力。

现状：
1. **编辑器扩展框架已有 4 种 Provider**：hover、definition、reference、formatting（`registry.ts` QueuedMonacoProvider.kind），但 **不支持 completion 和 signatureHelp**；
2. **代码提示基于正则+索引**：`outline-symbols.ts` 正则提取符号 → `workspace-index.ts` 构建定义/引用 Map → `hover-docs.tsx` 提供 hover 信息。无类型信息、无深度文档、无实时诊断；
3. **Monaco 内置 TS/JS Worker**：`monacoSetup.ts` 配置了 `tsWorker`，理论上 TypeScript/JavaScript 已有基础补全能力，但其他 6 种语言（Java/Python/Go/Rust/C/C++）无任何语言服务；
4. **无 LSP 实现**：项目中不存在 LSP 协议、Language Server 客户端或 `lsp-types` 依赖；
5. **SDK 配置已存储语言命令**：`sdkConfigs.ts` 中 `SdkLanguageConfig.command` 字段存储了 `python`/`java`/`node` 等命令路径，可用于发现和启动 Language Server。

## 决策

### 设计决策

| # | 决策点 | 选择 |
|---|---|---|
| 1 | LSP 客户端位置 | **Rust 后端**（`crates/nuomi-core/src/lsp/`）——进程管理与 stdio 通信在 WebView 中不可能 |
| 2 | LSP 类型与传输 | **`lsp-types` crate + 自实现 JSON-RPC 2.0 over stdio**——不引入完整 LSP 框架，保持轻量 |
| 3 | IPC 通信模式 | **请求-响应 via Tauri invoke + 通知 via Tauri event**（对齐 ADR-0002 事件通道模式） |
| 4 | Language Server 发现 | **自动检测 PATH + SDK 配置集成 + 用户自定义**——三层发现策略 |
| 5 | Monaco 适配 | **扩展 EditorExtContext + 新建 LSP 编辑器扩展**——作为 `builtin.lsp-bridge` 注册 |
| 6 | 文件同步 | **增量同步**（didOpen/didChange/didClose），前端编辑器内容变化经 IPC 通知 Rust 转发 LS |
| 7 | 多语言支持 | **每语言一个 LS 进程**，`LspManager` 管理注册表与生命周期 |
| 8 | 降级策略 | **LS 不可用时回退到现有正则+索引方案**——hover/definition/reference 不中断 |

### 1. 四层架构

```
┌──────────────────────────────────────────────────────────┐
│ Frontend (React/Monaco)                                   │
│                                                            │
│  Monaco Editor ──→ LspBridgeExtension (EditorExtension)  │
│       ↑↓                    ↑↓                            │
│  Provider calls        LSP IPC commands/events            │
│  (completion, hover,   (lspRequest, lspEvent)             │
│   signatureHelp,                                        │
│   diagnostics)                                          │
└──────────────────────────┬───────────────────────────────┘
                           │ Tauri IPC (invoke + event)
┌──────────────────────────┴───────────────────────────────┐
│ Rust Backend (src-tauri)                                  │
│                                                            │
│  LspCommands ──→ LspManager ──→ LspClient ──→ LS Process │
│  (IPC layer)    (registry +     (JSON-RPC     (jdtls,     │
│                  lifecycle)      over stdio)   pylsp, ...) │
└──────────────────────────────────────────────────────────┘
```

### 2. Rust LSP 客户端（`crates/nuomi-core/src/lsp/`）

**模块结构**：
```
crates/nuomi-core/src/lsp/
├── mod.rs              # 模块导出
├── types.rs            # LSP 配置类型（LspServerConfig, LspLanguageId）
├── client.rs           # LspClient：JSON-RPC 2.0 over stdio 传输 + 请求/响应/通知
├── manager.rs          # LspManager：多语言 LS 注册表 + 进程生命周期管理
├── transport.rs        # stdio 读写 + Content-Length 头解析 + 消息帧化
└── defaults.rs         # 每语言默认 LS 命令映射
```

**LspClient** 核心职责：
- 启动 LS 子进程（`tokio::process::Command`）
- 通过 stdin/stdout 收发 JSON-RPC 2.0 消息（LSP Header 格式：`Content-Length: N\r\n\r\n{json}`）
- 维护请求 ID → pending response 映射
- 处理 LS → client 方向的通知（`textDocument/publishDiagnostics` 等）
- `initialize` → `initialized` → `textDocument/didOpen` 启动握手

**LspManager** 核心职责：
- 语言 → LspClient 映射（同时最多 8 个 LS 进程）
- LS 发现：PATH 检测 → SDK 配置 command → 用户自定义
- 生命周期：启动（lazy，首次打开该语言文件时）→ 重启（LS 崩溃时自动重启 1 次）→ 关闭（应用退出或用户禁用）
- 健康检查：`initialize` 超时 → 标记不可用 → 通知前端降级

**依赖**：新增 `lsp-types = "0.95"`（workspace 级依赖），提供 LSP 协议类型定义（`CompletionItem`, `Hover`, `Location`, `Diagnostic` 等）。不引入 `tower-lsp` 或其他完整 LSP 框架。

### 3. IPC 通信设计

**请求-响应命令**（前端 → Rust → LS → Rust → 前端）：

| IPC 命令 | LSP 方法 | 触发时机 |
|---|---|---|
| `lsp_completion` | `textDocument/completion` | Monaco CompletionItemProvider.provideCompletionItems |
| `lsp_hover` | `textDocument/hover` | Monaco HoverProvider.provideHover（增强现有 hover-docs） |
| `lsp_definition` | `textDocument/definition` | Monaco DefinitionProvider.provideDefinition（增强现有 codebase-index） |
| `lsp_signature_help` | `textDocument/signatureHelp` | Monaco SignatureHelpProvider.provideSignatureHelp |
| `lsp_references` | `textDocument/references` | Monaco ReferenceProvider.provideReferences |
| `lsp_formatting` | `textDocument/formatting` | Monaco DocumentFormattingEditProvider.provideDocumentFormattingEdits |
| `lsp_document_symbols` | `textDocument/documentSymbol` | SymbolProvider.provideSymbols（增强现有 outline-symbols） |

**通知命令**（前端 → Rust → LS，单向）：

| IPC 命令 | LSP 方法 | 触发时机 |
|---|---|---|
| `lsp_did_open` | `textDocument/didOpen` | Monaco 编辑器打开文件 |
| `lsp_did_change` | `textDocument/didChange` | Monaco 编辑器内容变化（增量同步） |
| `lsp_did_close` | `textDocument/didClose` | Monaco 编辑器关闭文件 |
| `lsp_did_save` | `textDocument/didSave` | Ctrl+S 保存 |

**事件通知**（LS → Rust → 前端，via Tauri event）：

| Event 名 | LSP 方法 | 前端消费 |
|---|---|---|
| `lsp://diagnostics` | `textDocument/publishDiagnostics` | Monaco `setModelMarkers` 设置错误/警告标记 |
| `lsp://log` | `window/logMessage` | DevTools console 输出 |
| `lsp://status` | 自定义 | LS 启动/就绪/崩溃/降级状态通知 |

###@### 4. 前端 LSP 桥接扩展

**扩展 EditorExtContext**：
```typescript
// types.ts 新增
registerCompletionItemProvider(language: string, provider: Monaco.languages.CompletionItemProvider): void;
registerSignatureHelpProvider(language: string, provider: Monaco.languages.SignatureHelpProvider): void;
```

**QueuedMonacoProvider.kind 扩展**：
```typescript
kind: "hover" | "definition" | "reference" | "formatting" | "completion" | "signatureHelp";
```

**`attachMonacoProviders` 新增 flush 分支**：
```typescript
if (q.kind === "completion") monaco.languages.registerCompletionItemProvider(q.language, ...);
else if (q.kind === "signatureHelp") monaco.languages.registerSignatureHelpProvider(q.language, ...);
```

**新建 `builtin/lsp-bridge.tsx`**：
- 作为 `builtinEditorExtensions` 成员注册
- 为每种已启用 LS 的语言注册 CompletionItemProvider、SignatureHelpProvider、增强 HoverProvider、增强 DefinitionProvider
- Provider 调用 → IPC 请求 → 等待响应 → 返回 Monaco 格式结果
- LS 不可用时 Provider 返回空结果（降级到现有正则+索引方案）

### 5. Language Server 发现与配置

**三层发现策略**（按优先级）：

1. **用户自定义**（最高优先级）：SDK 配置页新增 "Language Server" 配置区域，用户可指定 LS 命令和参数
2. **SDK 配置推断**：从 `SdkLanguageConfig.command` 推断——如 `python` → 检测 `pylsp` 或 `pyright-langserver` 是否在同一 PATH
3. **自动检测 PATH**：扫描 PATH 中常见 LS 命令

**默认 LS 命令映射**：

| 语言 | LS 命令 | 备注 |
|---|---|---|
| typescript | `typescript-language-server --stdio` | Monaco 内置 tsWorker 作为备选 |
| javascript | `typescript-language-server --stdio` | 同上 |
| python | `pylsp` 或 `pyright-langserver --stdio` | 优先 pyright（更流行） |
| java | `jdtls` | 需 Java 17+，配置较复杂 |
| go | `gopls serve` | Go 官方 LS |
| rust | `rust-analyzer` | Rust 官方 LS |
| c | `clangd` | LLVM/Clang LS |
| cpp | `clangd` | 同上 |

**LS 配置存储**：扩展 `app_settings` KV，key = `lsp_configs`，结构：
```json
{
  "java": { "command": "jdtls", "args": [], "enabled": true },
  "python": { "command": "pyright-langserver", "args": ["--stdio"], "enabled": true }
}
```

### 6. 降级策略

LSP 不可用时（LS 未安装、启动失败、崩溃），系统自动降级：

| 能力 | LSP 可用 | LSP 不可用（降级） |
|---|---|---|
| 补全 | LSP 类型补全 + 文档 | **无补全**（返回空数组） |
| Hover | LSP 深度文档 + 类型信息 | 现有正则+索引 hover（`hover-docs.tsx`） |
| 定义跳转 | LSP 精准跳转 | 现有工作区索引跳转（`codebase-index.tsx`） |
| 签名帮助 | LSP 参数签名 | **无签名帮助**（返回空） |
| 诊断 | LSP 实时诊断 | **无诊断** |
| 大纲 | LSP 精准符号 | 现有正则符号提取（`outline-symbols.ts`） |

降级对用户透明：Provider 返回空结果时 Monaco 不显示弹窗，不影响编辑体验。

### 7. 文件同步协议

**增量同步**（`textDocument/didChange` 使用 `TextDocumentContentChangeEvent`）：
- Monaco `onChange` → 计算增量变化（range + text）→ IPC `lsp_did_change` → Rust 转发 LS
- 避免每次全量发送文件内容（大文件性能优化）

**文件打开/关闭**：
- Monaco `onMount` → IPC `lsp_did_open`（含文件 URI + 语言 ID + 初始内容）
- Monaco `onDidDispose` → IPC `lsp_did_close`

**URI 格式**：`file:///{workspace_root}/{relative_path}`（对齐现有 `codebase-index.tsx` 的 URI 构造方式）

### 8. 与现有编辑器扩展的关系

| 现有扩展 | LSP 接入后行为 |
|---|---|
| `builtin.hover-docs` | **保留**，作为 LSP 不可用时的降级 fallback。LSP 可用时 hover provider 优先级更高 |
| `builtin.codebase-index` | **保留**，definition/reference 降级 fallback。LSP 可用时优先使用 LSP 结果 |
| `builtin.outline-symbols` | **保留**，正则符号提取作为降级 fallback |
| `builtin.lsp-bridge` | **新增**，LSP 桥接扩展，注册高优先级 Provider |

**优先级机制**：Monaco 多个 Provider 的结果合并——LSP Provider 返回非空结果时优先展示，正则+索引 Provider 作为补充。Hover Provider 的 `provideHover` 可返回合并内容（LSP 文档 + 正则索引的引用计数）。

## 实施路线

| 阶段 | 内容 | 依赖 |
|---|---|---|
| **L1** | Rust LSP 客户端核心：`lsp-types` 依赖 + `LspClient`（JSON-RPC over stdio）+ `LspManager`（进程管理）+ 单元测试 | 无 |
| **L2** | IPC 命令层：`lsp_*` Tauri 命令 + 事件通知 + bindings 再生成 | L1 |
| **L3** | 前端框架扩展：`EditorExtContext` 新增 completion/signatureHelp 注册 + `QueuedMonacoProvider.kind` 扩展 + `attachMonacoProviders` flush 分支 | L2 |
| **L4** | `builtin.lsp-bridge` 扩展：CompletionItemProvider + SignatureHelpProvider + 增强 HoverProvider + 文件同步（didOpen/didChange/didClose） | L3 |
| **L5** | 诊断显示：`lsp://diagnostics` 事件 → Monaco `setModelMarkers` | L4 |
| **L6** | SDK 配置集成：Settings 页新增 LS 配置区域 + `lsp_configs` app_settings 存储 + LS 发现策略 | L4 |
| **L7** | 降级策略完善：LS 不可用时无缝回退 + 状态指示 UI | L5, L6 |

## 风险与缓解

| 风险 | 缓解 |
|---|---|
| LS 启动慢（如 jdtls 需 10s+） | lazy 启动（首次打开该语言文件时才启动）+ 启动期间降级到正则方案 |
| LS 占用内存大 | 限制同时运行 LS 数量 + 用户可禁用不需要的语言 |
| jdtls 配置复杂（需 Java 17+、workspace path 等） | 首期可跳过 Java，优先支持 Python/Go/Rust（配置简单） |
| 增量同步复杂度 | 首期可先实现全量同步（didChange 发送完整内容），后续优化为增量 |
| IPC 延迟影响补全体验 | 补全请求设置超时（500ms），超时返回空结果降级 |
