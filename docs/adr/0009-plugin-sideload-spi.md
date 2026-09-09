# ADR 0009: 插件 SPI 与第三方插件侧载（NPP）

- 状态：Accepted（用户批准方案：进程外 JSON-RPC + facade 接入真 Kernel）
- 日期：2026-09-09
- 决策人：james（经 plan-mode 批准）
- 参考：Zed Extensions（WASM + WIT）、VSCode Extensions（manifest + contributes + extension host）

## 背景 / Context

SPEC D9 将插件化核心 5 件（Loop Engine、SystemPrompt、Memory、MCP、Hook）定义为编译进二进制的 Rust `Plugin` trait 实现；Command/Skills/Plugin-market 列为 Non-goal。但 pr.md §3 要求「把 Command、Hook、Skills、MCP、Plugins…均插件化」，且实际存在第三方扩展需求：用户需要在不重编 nuomi 的情况下为内核加装工具、钩子与事件订阅。

现状缺口（探索核实）：
1. **生产启动绕过 Kernel**：`facade.rs::NuomiKernel::boot` 手工注册 `system_prompt`/`memory` 服务（qualifier 还是 `""`，与内置包装器的 `"system_prompt"` 不一致），`ToolRegistry`/`HookRegistry` 是裸结构体字段；5 个内置 `Plugin` 包装器只在测试里运行。
2. **零侧载机制**：无 manifest 格式、无磁盘扫描、无第三方协议（全仓 grep 确认）。

## 决策 / Decision

### 1. SPI 载体 = 进程外 JSON-RPC over stdio（NPP），不是 WASM、不是 dylib

第三方插件 = 独立进程 + `plugin.toml` 清单 + Nuomi Plugin Protocol（JSON-RPC 2.0，ndjson 帧）。内核以 `SideloadedPlugin`（实现既有 `Plugin` trait）托管其生命周期。

| 备选 | 否决理由 |
|---|---|
| WASM 组件（Zed 风格） | wasmtime 依赖与工具链重量（对当前插件密度不成比例）；内核服务是 `Arc<dyn Any>` 类型化注册，跨 WASM 边界需另建一整套 WIT API——等于两套 SPI。**列入 roadmap**：当第三方插件数量证明需要进程内沙箱时再评估 |
| dylib / libloading | Rust 无稳定 ABI，跨编译器版本即 UB；与「第三方分发」天然冲突 |
| 纯声明式（无第三方代码） | 只能映射既有扩展点，表达力不足以构成 SPI |

进程方案的额外红利：任意语言可写（Python/Node/Rust/shell 包装皆可）、崩溃隔离（插件挂死不拖垮内核）、与既有 MCP stdio 传输（`plugins/mcp.rs::StdioTransport`）同构。

### 2. NPP 演进策略（协议即契约）

- `api_version: u32` 握手；host 常量 `NPP_API_VERSION = 1`，接受 `plugin.api_version <= NPP_API_VERSION`，拒绝更高。
- **同一 major 内只加不改删**（对齐 `.opencode/rules/ipc-contract.md`）；破坏性变更 = major+1，同时保留上一 major 的 host 兼容层一个版本周期。
- 方法命名空间：host→plugin 用裸方法名（`initialize`/`tools/list`/`tools/call`/`hook/handle`/`event`/`shutdown`），plugin→host 用 `nuomi/` 前缀（`nuomi/log`），为未来 plugin→host 能力扩容留位。

### 3. 第三方插件格式：`plugin.toml`（v1）

插件目录根放 `plugin.toml`（serde+toml 解析）。**未知键 = 警告不报错**（前向兼容）。必填：`id`（kebab）、`name`、`version`、`api_version`、`entry`（argv 数组，禁 shell 字符串拼接——注入面收口）。可选贡献表：`[[tools]]`、`[[hooks]]`、`[[events]]`、`[permissions]`。`[mcp_servers]` 声明式接入 v1.1；Command/Skills 维持 SPEC Non-goal。

工具注册名强制 `"<plugin_id>.<name>"` 前缀——`ToolRegistry` 按名查重（tools.rs:42），前缀使跨插件冲突不可能。

### 4. 侧载目录与冲突语义

扫描顺序：`NUOMI_PLUGIN_PATH`（PATH 风格分隔）→ `dirs::config_dir()/nuomi/plugins` → `<cwd>/.nuomi/plugins`。**同 id 先到先得**（env > user > workspace），后者报 `Skipped{duplicate}`。清单解析失败**永不 crash**：进 `BootReport.failed`，boot 继续——侧载失败非致命是硬性设计约束。

### 5. 权限模型 v1 = 声明 + 展示，不建新沙箱

`[permissions]`（fs.read/fs.write globs、network hosts、shell）在 boot report 与 `nuomi plugin list` 中**展示**；执行治理仍由既有机制承担（PreToolCall 钩子可 deny、ApprovalGate 敏感工具名单、Exchange gate）。进程边界本身就是隔离层。v1.1 roadmap 再评估权限强制执行。

### 6. Kernel 成为唯一生产启动路径（配套重构）

`NuomiKernel::boot` 改为：`Kernel::new` → 注册内置插件（ToolsPlugin→HooksPlugin→SystemPromptPlugin→MemoryPlugin）→ 注册侧载插件 → `kernel.boot()`。facade 字段改为 boot 后从 `ctx.service::<T>(qualifier)` 解析。provider 保持 facade 字段（per-boot 配置非插件服务，保 FakeLlm 测试路径）。LoopEnginePlugin 暂不注册（facade 需同步拿 run 结果与 delta 回调），留作 roadmap。这同时修正 facade qualifier `""` 与包装器 `"system_prompt"` 的潜在不一致。

### 7. 新依赖

`toml = "0.8"`（清单解析）、`dirs = "5"`（配置目录）。均为轻量 serde 生态包，符合 AGENTS.md §2 选型纪律。

## 后果 / Consequences

- 正面：第三方插件零编译接入；语言无关；崩溃隔离；内置与侧载同一生命周期；协议 add-only 演进有据。
- 负面：每插件一个进程（内存/启动开销）；钩子走进程往返（毫秒级延迟，钩子路径已设 5s 超时兜底）；协议兼容性需自行维护（ADR-0001 已列此负面并给出「协议变更走 ADR」缓解）。
- 缓解：NPP_API_VERSION 握手 + add-only 规则；BootReport 让失败可见；集成测试覆盖 mock 插件全链路。

## 关联

- ADR-0001（Plugin trait / ServiceMap / 协议演进走 ADR）、ADR-0002（事件版本化只加不改）、ADR-0007（Exchange FS，侧载工具 I/O 可选路径）。
- 实现：`crates/nuomi-core/src/harness/sideload/`、`docs/plugins/`、`examples/plugins/`。
