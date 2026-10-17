# ADR 0022: 插件化代码运行沙箱（内置插件壳 + SandboxExecutor trait + CubeSandbox 默认执行器）

- **状态**: Proposed
- **日期**: 2026-10-17
- **决策者**: user + conductor
- **前置设计**: `.codeartsdoer/specs/steward_workbench_v2/design.md` §2.1.3.4、§2.3.2.2
- **依赖**: ADR-0021（授权目录收敛依赖工作区两级模型的"当前工作区工作目录"语义）

## 背景

用户需求（steward_workbench_v2 spec 5.7）：支持代码运行沙箱与应用自身在沙箱内构建，**插件化实现**，参考 `CubeSandbox`（容器化隔离、多语言运行时、资源限额的沙箱运行时）。

现状：
1. **无任何沙箱域能力**——代码执行空白，`crates/nuomi-core/src/sandbox/` 不存在；
2. **插件体系已有两种载体**：内置插件（`crates/nuomi-core/src/plugins/*.rs`，Rust 原生实现 `harness/plugin.rs:10` 的 `Plugin` trait：init/start/dispose）与 NPP 第三方插件（ADR-0009：子进程 + JSON-RPC，manifest 声明 `[permissions]`，**v1 声明展示不强制执行**）；
3. **Run 状态机铁律**（AGENTS.md §4）：每次状态迁移先落 append-only 事件再产生外部副作用——沙箱任务生命周期须对齐；
4. **密钥安全规则**（AGENTS.md §7-4）：API key 只经 OS keyring——构建签名密钥不得进入沙箱。

## 决策

### 设计决策

| # | 决策点 | 选择 |
|---|---|---|
| 1 | 沙箱载体 | **内置插件壳 + `SandboxExecutor` trait + 默认 CubeSandbox 执行器**（否决 NPP 第三方子进程插件方案） |
| 2 | 隔离边界保证 | **执行器自身强制**（容器/强隔离进程），不依赖 manifest `[permissions]` 声明 |
| 3 | 授权目录范围 | **内核强制收敛**：提交时校验授权目录 ⊆ 当前工作区工作目录（ADR-0021 语义） |
| 4 | 任务生命周期 | **落库 `sandbox_tasks`（0037）+ 状态机事件先于副作用**（对齐 Run 铁律） |
| 5 | 构建签名密钥 | **留在宿主 keyring，不进沙箱**；如需签名，产物回传后由宿主工序完成 |
| 6 | 可替换性 | 替换 = 注册另一个 `SandboxExecutor` 实现；禁用 = 插件不 start（入口隐藏），**均不改内核** |

### 1. 三层架构

```
SandboxPlugin（内置插件壳，实现 Plugin trait）
  ├─ 生命周期与能力注册：init 注册执行器与命令、start 启用入口、dispose 终止在途任务并回收资源
  ├─ 委派 → SandboxExecutor trait（契约层）
  │          submit / poll / cancel / fetch_output / fetch_artifact
  └─ 默认实现 → CubeSandboxExecutor（对接 CubeSandbox 类运行时：
                独立执行器进程/服务、容器或强隔离进程边界、多语言运行时、
                资源限额 CPU/内存/时长/网络开关、超时中止、文件系统边界隔离）
```

**否决"NPP 第三方子进程插件"方案的四条理由**：
1. NPP v1 的 `[permissions]` 声明**不强制执行**（ADR-0009 §5 约束），而沙箱的核心诉求恰是**强制**边界隔离——隔离必须由执行器自身（容器/强隔离进程）保证；
2. NPP 工具贡献面向"LLM 工具调用"（请求-响应），沙箱需要**长任务生命周期管理**（流式输出/取消/产物回传/审计），协议不匹配；
3. trait + 插件壳组合已满足 spec 5.7.1-1"禁用/启用/替换实现不改内核"（替换=注册另一 Executor 实现，禁用=插件不 start）；
4. 默认执行器以**独立执行器进程/服务**对接 CubeSandbox 类运行时，宿主崩溃隔离天然成立（执行器崩溃/超时不影响主应用稳定性）。

### 2. 隔离边界：执行器强制，不依赖声明

沙箱的文件系统边界（授权目录外宿主路径读写被拒绝并返回越权错误）、资源限额（CPU/内存/时长/网络开关）、进程隔离均由执行器自身的隔离机制（容器/强隔离进程）强制。manifest 声明仅作展示（对齐 ADR-0009 v1 约束），**不作为安全边界**。

### 3. 授权目录内核收敛

任务提交时内核校验 `authorized_dirs ⊆ 当前工作区的工作目录集`（依赖 ADR-0021 两级模型）。越权目录直接拒绝（`sandbox.dir_not_authorized`）。这保证沙箱内的文件访问范围与用户当前工作上下文一致，不因执行器实现差异而放宽。

### 4. 任务生命周期（对齐 Run 铁律）

状态机：`queued → running → succeeded / failed / timed_out / cancelled`。

**每次状态迁移先落 `sandbox.task_state_changed` 审计事件（含任务 id/状态/输出游标/资源用量快照），再产生外部副作用**——与 Run 状态机铁律同构。超时/资源超限中止并保留已产生输出；产物回传失败 → 任务态 succeeded + `artifact_retryable`（沙箱内保留产物可重试）。输出增量经 `sandbox.<task_id>` 事件通道推送（ADR-0002 多通道事件模式）。

### 5. 密钥安全

构建任务的签名密钥留在宿主 OS keyring，**不进入沙箱**。沙箱内构建仅产出未签名产物；如需签名，回传后由宿主签名工序完成。API key 等敏感信息不出现在沙箱输出/日志（任务 stdout/stderr 落库前不脱敏改造，但提交参数与配置不携带密钥）。

### 6. 数据面：迁移 0037

`migrations/0037_sandbox_tasks.sql`：`sandbox_tasks` 表（id/task_type CHECK('run_code','build_app')/runtime/status CHECK 六态/source/authorized_dirs JSON/limits JSON/stdout/stderr/artifact_ref/artifact_retryable/exit_info/duration_ms/created_at/finished_at + 索引 idx_sandbox_tasks_created）。超大输出落 `.nuomi/sandbox/<task_id>/` 产物目录、行内只存摘要与定位（截断策略由执行器控制）。

### 7. IPC 命令组（6 个）

`sandbox_run_code`（前置校验+异常映射 sandbox.unavailable / dir_not_authorized / runtime_unknown / limit_invalid）、`sandbox_build_app`（构建目标=当前工作区 nuomi 仓库目录）、`sandbox_list_tasks` / `sandbox_get_task`（含输出与产物定位）/ `sandbox_cancel_task`（CancellationToken）/ `sandbox_list_runtimes`（插件声明的运行时清单，含隔离机制说明）。

## 向后兼容

- 全新能力域：无存量行为变更；`sandbox_tasks` 新表 + 新命令组 + 新前端入口，既有消费端零影响。
- 迁移 0037 只增不改；含 0036 的存量库追加应用成功、重复应用幂等、既有表零变更。
- 插件默认启用；禁用后应用内沙箱入口隐藏且其余功能不受影响（启停不改内核）。

## 权衡

- **优点**：trait 契约使执行器可替换（CubeSandbox 之外可接 Firecracker/Docker/其他）；内置插件壳复用 harness 生命周期（无新生命周期机制）；执行器独立进程天然崩溃隔离；任务审计对齐 Run 铁律（可回放）。
- **代价**：新域全套（trait + 默认执行器 + 任务管理 + 仓储 + 6 命令 + 前端界面）；默认执行器需对接外部沙箱运行时（环境依赖，CI 无环境时集成测试跳过）。
- **替代方案**：
  - (a) NPP 第三方子进程插件——否决。理由见决策 1（权限不强制、协议不匹配、无生命周期管理）。
  - (b) 直接硬编码进内核（不走插件）——否决。违反 spec 5.7.1-1"禁用/启用/替换不改内核"。
  - (c) 宿主内线程池模拟沙箱（无进程隔离）——否决。无真实隔离边界，越权/资源限制形同虚设。
  - (d) 任务输出落 events 表（复用事件流）——否决。输出体积大（stdout/stderr 可达 MB 级），events 表面向轻量审计；独立表 + 产物目录分层更合理。

## 风险与缓解

| 风险 | 缓解 |
|---|---|
| 执行器对接外部运行时的环境依赖（CI 无沙箱环境） | 集成测试显式开关标记（无环境跳过）；单测以 mock 执行器服务覆盖提交/轮询/取消/输出游标协议 |
| 执行器崩溃影响主应用 | 独立进程边界 + dispose 终止在途任务 + 宿主崩溃隔离验证（单测） |
| 越权路径访问 | 内核提交校验（⊆ 当前工作区目录）+ 执行器文件系统边界双重防线；单测越权拒绝 |
| 任务失控（死循环/资源耗尽） | `sandbox_default_timeout_secs` 默认 120s 超时中止 + 资源限额（CPU/内存）；保留已产生输出 |
| 产物回传失败丢产物 | succeeded + artifact_retryable，沙箱内保留产物可重试 |
| 审计缺失（副作用先于事件） | 状态机单测断言"事件先于副作用"顺序（对齐 Run 铁律测试范式） |
| 密钥泄漏进沙箱 | 签名密钥留宿主 keyring；提交参数不携带密钥；安全抽测验证 |
| tauri-specta bindings 需 regen | 契约先行：命令提交内 `pnpm contracts:gen` + `pnpm contracts:check` |

## 影响范围

### Migration
- **新增** `migrations/0037_sandbox_tasks.sql` — `sandbox_tasks` 表 + 索引

### 后端（新域）
- **新增** `crates/nuomi-core/src/sandbox/`（mod.rs / types.rs / executor.rs / task_manager.rs / cube_executor.rs）
  - types.rs：`SandboxTaskSpec` / `SandboxTaskResult` / `SandboxLimits { timeout_secs, cpu_percent, memory_mb, network_enabled }`
  - executor.rs：`SandboxExecutor` trait（submit/poll/cancel/fetch_output/fetch_artifact）
  - task_manager.rs：状态机 + 审计事件 + 提交校验（依赖 ADR-0021 workspace registry 的当前工作区目录查询）
  - cube_executor.rs：默认执行器（独立进程对接 CubeSandbox 类运行时）
- **新增** `crates/nuomi-core/src/store/repos/sandbox_tasks.rs`
- **新增** `crates/nuomi-core/src/plugins/sandbox.rs`（内置插件壳，实现 `harness/plugin.rs:10` Plugin trait）
- 事件类型：`sandbox.task_state_changed`（append-only events，无 DDL）

### IPC 契约
- `src-tauri/src/commands.rs` — 6 命令薄转发（业务逻辑在 sandbox 域）
- `src-tauri/src/tauri_cmds.rs` / `lib.rs` — 注册
- `src/lib/ipc/bindings.gen.ts` — 重新生成（camelCase）；新 DTO：`SandboxTaskDto` / `SandboxRunRequestDto` / `SandboxLimitsDto` 等

### 前端
- **新增** `src/features/sandbox/` — 代码运行界面、任务列表与详情、应用构建入口、取消、不可用指引

### 不涉及
- NPP 协议与 sideload 机制——零改动（沙箱不走 NPP）
- Run 状态机 / tasks_runs——沙箱任务独立聚合，不混入 Run 生命周期
- Provider/编排引擎——沙箱不经 LLM 工具调用链（用户直触 UI 入口）

## 实施计划

任务组 7（tasks.md）七步：7.1 迁移 0037 → 7.2 Executor trait 与类型 → 7.3 任务生命周期管理（依赖 6.3 授权目录收敛）→ 7.4 默认执行器 → 7.5 插件壳 → 7.6 IPC 命令组【契约】 → 7.7 前端入口。配套文档：`docs/plugins/` 沙箱插件文档（manifest 与能力声明、隔离机制、运行时清单、资源限额配置、入门教程）。

## 验证门禁

- `cargo test -p nuomi-core`（状态机全路径含取消/超时/资源超限、越权拒绝、事件先于副作用顺序断言、迁移幂等）
- mock 执行器覆盖提交/轮询/取消/输出游标协议；真实沙箱集成测试显式开关（CI 无环境跳过）
- `cargo clippy --all-targets -- -D warnings`（新代码零警告）
- `pnpm contracts:gen` 无未提交 diff + `pnpm contracts:check`
- `pnpm typecheck && pnpm lint && pnpm test -- --no-file-parallelism`
- 安全抽测：越权路径访问被拒、密钥不出现在沙箱输出/日志、构建密钥不进沙箱、默认超时 120s 中止回报
