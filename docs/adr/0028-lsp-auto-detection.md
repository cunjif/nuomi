# ADR 0028: LSP 自动检测与持久化配置

- **状态**: Accepted
- **日期**: 2026-11-05
- **决策者**: user + core-engineer
- **依赖**: ADR-0027（LSP 代码智能提示）

## 背景

ADR-0027 §5 设计了"三层发现策略：user config → SDK config inference → PATH detection"，但实际只实现了两层（user config → 硬编码 defaults），且 user config 仅存内存 HashMap，重启丢失。导致：

1. Java 默认命令 `jdtls` 在多数环境不存在，用户需手动在 Settings → LSP 配置实际命令，配置重启后丢失；
2. 无 PATH 预检测——`LspManager::start` 直接 spawn，失败才报错，错误体验差；
3. 无 SDK inference——不检测 `JAVA_HOME`、vscode-java 扩展路径等常见安装位置。

用户需求：LSP 配置应自动检测、自动配置，首次打开某语言文件时懒触发。

## 决策

### 1. 新增 `detector.rs` 模块

在 `crates/nuomi-core/src/lsp/detector.rs` 实现自动检测，按语言定义候选命令优先级列表：

| 语言 | 候选命令（按优先级） | SDK inference |
|---|---|---|
| java | `jdtls` → `jdt-language-server` → 扫描 `~/.vscode/extensions/redhat.java-*/server/` 下的 `jdtls`/`jdtls.cmd` | 检测 `JAVA_HOME` 设置 `java.jdtls.java.home` env |
| python | `pyright-langserver` → `pylsp` → `ruff-lsp` | 检测 `VIRTUAL_ENV`/`CONDA_PREFIX` 推断 venv 内的 LSP |
| typescript/javascript | `typescript-language-server` → `tsserver` | 无 |
| go | `gopls` → `bingo` | 检测 `GOROOT` |
| rust | `rust-analyzer` | 无（通常随 rustup 安装） |
| c/cpp | `clangd` → `ccls` | 无 |

**PATH 探测**：复用现有 `which_in_path` 模式（`terminal_manager.rs:245`），在 `detector.rs` 内实现独立的 `is_in_path(cmd) -> bool`，用 `tokio::process::Command::new(cmd).arg("--version").output()` 探测。Windows 上通过 `spawn_candidates`（`adapters/mod.rs:62`）处理 PATHEXT 变体。

**vscode-java 扩展扫描**（仅 Java）：按平台扫描目录：
- Windows: `%USERPROFILE%\.vscode\extensions\redhat.java-*\server\`
- macOS/Linux: `~/.vscode/extensions/redhat.java-*/server/`
- 寻找 `jdtls`（Unix）或 `jdtls.cmd`（Windows）启动脚本

### 2. 配置持久化到 `app_settings` KV

复用现有 `app_settings` 表（migration 0008）和 `store::repos::settings` 接口，新增两个 key 命名空间：

| key 格式 | 含义 | 写入时机 |
|---|---|---|
| `lsp_config.{language}` | 用户手动配置（LspSection UI 编辑） | `set_user_config` 调用时 |
| `lsp_autodetect.{language}` | 自动检测到的配置 | 首次检测成功时 |

value 为 `LspServerConfig` 的 JSON 序列化。

**`LspManager` 改造**：
- 新增 `db: Option<Arc<Mutex<Connection>>>` 字段（`Option` 保留无 DB 测试场景）
- `new()` 保持无 DB（向后兼容）
- `with_db(db: Arc<Mutex<Connection>>)` 构造器——启动时从 DB 加载已保存的 user configs 和 auto-detected configs 到内存
- `set_user_config` / `set_autodetected_config` 落库
- `remove_user_config` 删除 DB key

### 3. 四层 `effective_config` 解析

```
effective_config(language):
  1. user_configs.get(language)        → 用户手动配置（最高优先级）
  2. autodetected_configs.get(language) → 已持久化的自动检测结果（避免每次重新探测）
  3. detector::detect(language)         → 实时 PATH + SDK 探测（探测成功后落库到 autodetected_configs）
  4. defaults::default_config(language) → 硬编码默认（最低优先级）
```

第 3 步仅在第 1、2 步都未命中时执行。探测成功后写入 `autodetected_configs` 内存 + DB，下次直接走第 2 步。

### 4. 懒触发时机

在 `LspManager::start(language)` 内调用 `effective_config` 时触发检测——即首次打开某语言文件经 `ensure_started` → `start` 路径自然触发。不在应用启动时全量探测。

### 5. 前端 LspSection UI 增强

- 配置输入框旁显示来源标签：`[自动检测]` / `[用户配置]` / `[默认]`
- 新增"重新检测"按钮——清除 `lsp_autodetect.{language}` 后重新触发探测
- 修复现有注释与实现不符的问题（`LspSection.tsx:5` 声称持久化但实际未落库）

## 实现范围

### Rust core（`crates/nuomi-core/src/lsp/`）
- **新增** `detector.rs`：`detect_config(language) -> Option<LspServerConfig>` + 各语言候选列表 + PATH 探测 + vscode-java 扫描
- **修改** `manager.rs`：加 `db` 字段、`with_db` 构造器、加载/落库逻辑、`effective_config` 四层解析
- **修改** `defaults.rs`：注释更新（不再是唯一发现手段）
- **新增** `detector.rs` 单元测试：mock PATH、候选优先级、Java 扩展扫描

### IPC 层（`src-tauri/src/`）
- **修改** `lib.rs`：`LspManager::with_db(state.db.clone())` 替代 `LspManager::new()`
- **修改** `lsp_commands.rs`：`impl_lsp_set_config` 落库（经 manager 内部完成，IPC 层不变）

### 前端（`src/features/settings/`）
- **修改** `LspSection.tsx`：显示配置来源标签 + "重新检测"按钮

## 不做

- 不引入 `which` crate（用现有 `tokio::process::Command` 探测，保持依赖最小）
- 不做 LSP 崩溃自动重启（ADR-0027 设计但未实现，属独立议题）
- 不做增量同步优化（didChange 仍发全量 text，属独立议题）

## 测试策略

- `detector.rs` 单元测试：各语言候选优先级、PATH 探测 mock、Java vscode 扩展目录扫描 mock
- `manager.rs` 测试：四层解析优先级、持久化加载/保存、自动检测落库
- 集成测试：`with_db` 构造器从 DB 加载配置、`set_user_config` 落库后重启可恢复
- 前端 `LspSection.test.tsx`：来源标签显示、"重新检测"按钮交互

## 影响

- **正面**：Java/Python/Go/Rust 等 LSP 零配置开箱即用；用户配置重启不丢失；vscode-java 用户自动发现 jdtls
- **负面**：首次打开某语言文件有 ~100ms 探测开销（仅一次，后续走缓存）；`LspManager` 新增 DB 依赖
- **风险**：vscode-java 扩展目录格式随版本变化，扫描逻辑可能失效——降级为 PATH 探测，不影响其他语言

## 实现进展

- 2026-11-06：PATH 探测 + Java vscode-java 扩展扫描已落地（`detector.rs` 初版）
- 2026-11-06：Python venv/conda inference 落地——`detect_python` 在 PATH 未命中时探测 `VIRTUAL_ENV`/`CONDA_PREFIX` 的 bin 目录（Windows `Scripts/`、Unix `bin/`），按 `pyright-langserver` → `pylsp` → `ruff-lsp` 优先级匹配，命中则用绝对路径启动
- 2026-11-06：Go SDK inference 落地——`detect_go` 在 PATH 未命中时按 `GOBIN` → `GOPATH/bin`（多路径逐个）→ `GOROOT/bin` 顺序探测 `gopls`/`bingo`，命中则用绝对路径启动
