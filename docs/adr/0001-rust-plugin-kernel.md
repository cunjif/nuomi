# ADR 0001: Rust 自研插件内核（Cordis 理念借鉴）

- 状态：Accepted（依据已批准 SPEC `docs/specs/harness-kernel-v1.md` D2）
- 日期：2026-08-24
- 决策人：james（经 Interview 批准）

## 背景 / Context

pr.md 要求「以 Cordis 为核心构建」，将 Command、Hook、Skills、MCP、Plugins、ReAct、Loop-Run、Memory、SystemPrompt、Loop Engine 等 Harness 组件全部插件化。

Cordis（cordis.moe）是 Koishi 生态的 **TypeScript** 插件框架，而本项目锁定栈为 Rust 核心 + tokio。引入 Node sidecar 运行时会带来跨进程 IPC、双运行时运维与打包复杂度。

## 决策 / Decision

**不引入 Cordis.js 运行时；在 Rust 侧自研一个借鉴 Cordis 理念的插件内核。**

### Cordis 理念 → Rust 实现映射

| Cordis 概念 | nuomi Rust 对应 |
|---|---|
| `Context`（生命周期作用域，可 fork/dispose） | `harness::Context`：持有插件依赖的共享服务句柄，支持子作用域与级联销毁 |
| `Plugin`（声明式组件单元） | `harness::Plugin` trait：`id()` + `init(ctx)` + `start()` + `dispose()`，异步 |
| Service 注入（ctx.[service]） | `ServiceMap`：插件向 Context 注册类型化服务，其他插件按 `TypeId + name` 解析 |
| 事件总线（ctx.on/emit） | `EventBus`（tokio broadcast + 持久化订阅器）：领域事件先落 EventRecord 再广播（先落库后副作用铁律） |
| 生命周期（start/reusable/fork） | 内核按依赖序 init→start，退出/错误时逆序 dispose；每个会话/团队运行创建子 Context |

### v1 插件化范围

核心 5 件为插件：**Loop Engine、SystemPrompt、Memory、MCP、Hook**。
后置为插件的：Command、Skills、Plugin-market、Telemetry（见 SPEC Non-goals）。
ReAct 循环是 Loop Engine 的首个内置实现，不是独立插件。

## 后果 / Consequences

- 正面：单语言单运行时；无 JS/Rust 边界序列化开销；与 SQLite/tokio 生态直接集成；测试在同一进程内完成。
- 负面：需自行维护插件协议演进（命名、版本、兼容性）；无法复用 Cordis 生态现有插件。
- 缓解：插件协议以 trait + 注册表为中心，新增组件不改内核调度代码（AC1）；协议变更走 ADR。

## 关联决策：workspace 结构抽取 lib crate

headless CLI 验收入口要求核心逻辑可在无 Tauri 壳的情况下运行：

```
crates/
├─ nuomi-core/   # 全部业务逻辑 lib：domain / harness / providers / orchestrator / store / evolution
└─ nuomi-cli/    # headless 入口 bin：run / resume / REPL
```

后续 UI 里程碑中 `src-tauri` 作为薄壳 bin 依赖 `nuomi-core`，Tauri commands 层只做参数校验与转发（沿用既有边界规则）。此结构保证内核优先路线与未来 Tauri UI 不产生分叉实现。
