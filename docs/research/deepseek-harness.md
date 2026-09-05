# deepseek-harness — 组件级研究与可吸收点

> 基于 v0.1.1-rc.2 源码验证，`references-analysis.md` 仅作地图。日期 2026-09-05。

## 1. 定位与架构骨架

TS monorepo（pnpm，50+ 包），**一切皆 Cordis 插件**：agent 循环、工具注册表、LLM 适配器、会话持久化、沙箱全是可替换插件，零特权核心。关键目录：`vendor/cordis/src`（内核 2693 行）、`packages/core/{session,tools,agent,agent-loop}`、`packages/subagent/*`（8+ provider）、`packages/sandbox/*`、`native/landlock-run`（Rust/C 启动器）。

## 2. Cordis 内核深挖（重点）

**核心数据结构**
- `Context`（context.ts:42）：**运行时 Proxy**，属性读取走服务解析器（reflect.ts:135 `handler.get`）；`extend()` 原型链派生子上下文（context.ts:99）、`isolate(name)` 为某服务开独立作用域（context.ts:121）、`intercept(name, config)` 注入服务配置（context.ts:141）。
- `Fiber`（fiber.ts:184）：插件运行实例 = 生命周期单元。状态机 `PENDING→LOADING→ACTIVE→UNLOADING→DISPOSED`（fiber.ts:147）。同一插件多处加载共享 `Plugin.Runtime`，各持一个 Fiber（registry.ts:316-336）。
- `Impl`（reflect.ts:116）：`{name, fiber, value, check}`——服务实现由提供它的 Fiber 持有，Fiber 卸载即注销。

**生命周期与依赖注入（最大亮点：反应式）**
- 插件声明 `inject: ['llm','tools']`，Fiber 逐项 `_checkImpl` 汇总成 epoch 字符串（fiber.ts:611-623）；依赖不齐 → PENDING；任何服务 `provide`/注销触发 `notify()` 重算所有相关 Fiber 的 epoch（reflect.ts:314-336）→ 自动 **reload（fiber.ts:646）或 unload（fiber.ts:675，逆序跑 disposer）**。依赖变化驱动插件热装卸，无需手工编排启动顺序。
- 副作用可逆：一切注册（服务、事件监听、工具、路由）经 `ctx.effect()` 安装（fiber.ts:418），disposer 存入 `_disposables`，卸载逆序执行、支持 async/generator（fiber.ts:83-93）。`update(config)` 走 `internal/update` waterfall 支持 HMR 与否决（fiber.ts:736）。
- 事件五种分发：`emit/parallel/serial/bail/waterfall`（events.ts:32）；waterfall 是洋葱中间件，不调 `next()` 即否决（events.ts:234）。

**代表性代码**（fiber.ts:625-639，epoch 驱动装卸）：
```ts
private _setEpoch(epoch: string) {
  const oldEpoch = this._runner.epoch
  if (epoch === oldEpoch) return
  this._runner.epoch = epoch
  if (this.inertia) return
  this._updateState(() => {
    if (epoch !== INACTIVE && oldEpoch === INACTIVE) {
      this.inertia = this._reload()
      return FiberState.LOADING
    } else {
      this.inertia = this._unload()
      return FiberState.UNLOADING
    }
  })
}
```

**映射到 Rust 的可行性**
- ✅ 可映射：effect/逆序 disposer ≈ `Vec<BoxAsyncFn>` + Drop；五种事件分发 ≈ trait 回调；epoch 反应式 ≈ watch 通道 + 依赖表；状态机枚举直接照搬。
- ⚠️ 需改造：JS Proxy 的 `ctx.tools` 动态属性访问 → Rust 无反射，nuomi 已用 `TypeId+qualifier` 静态注册（context.rs:15），保留即可；声明合并扩展事件类型 → Rust 用 enum + `#[non_exhaustive]`；跨 realm symbol 品牌 → 不需要。
- ❌ 不可照搬：`ctx` 无处不在的隐式 `this` 绑定与 Proxy trace（reflect.ts:398-417）——Rust 显式传 `&Context` 更好；await 任意 Fiber 的 PromiseLike 语义 → 用 `tokio::sync::watch`/JoinHandle。

## 3. 其它差异化机制

- **五阶段工具管道**：`tools/pre-execute` waterfall（钩子/权限/沙箱）→ 单调守卫 guard（只可拒不可放行，`AnonymousEntries`，tools/src/index.ts:212-490 区域）→ `tools/execute` waterfall（超时/重试/指标）→ 工具体 → `tools/post-execute` waterfall（可阻断/替换）。审批在 guard 前解析。
- **能力缝三角色**：Service Definition（如 `SubagentProvider`，extensions/tool-cordis/src/api-catalog.ts：`name/capabilities/start()`）+ Provider（subagent-acp/codex/claude-code/dsh-sdk/spawn-in-process/fork-in-process 等 8+）+ Consumer（tool-subagent）。换 provider 不动消费端；文件系统/子进程缝共享一个执行世界，指向远程沙箱即整体迁移。
- **跨平台沙箱**：链式选 runner——Linux bwrap→Landlock（native/landlock-run，Rust/C 预编译 launcher）、macOS Seatbelt（sandbox-exec -p）、Windows 受限令牌 ACL（sandbox-windows-acl/src/{ffi,grant}.ts，workspace SID + 每会话随机私有 temp SID）。**每级做功能探测（跑 `true` 验证），失败 fail-closed 而非裸跑**（sandbox-local/src/index.ts:2-21,141）；`SandboxEnforcement` 区分 full/partial（Windows 因 WRITE_RESTRICTED 永远 partial）。
- **Event-Sourced 会话**：append-only 日志唯一事实源（core/session/src/index.ts），LLM 消息历史由日志派生不单独存储；崩溃后用合成 `turn/end{reason:'interrupted'}` 修复（repair.ts `interruptedTurnClosers`）；持久化 JSONL（zstd）/SQLite 可插拔，write-behind 批量落盘（session-persistence/src/write-behind.ts）；"模型可见即可重建"是运行时不变量断言。

## 4. 组件评分表

| 维度 | 分 | 理由 |
|---|---|---|
| 编排 | 4 | step/turn 状态机严谨，但无 nuomi 式 Role/Team 群聊拓扑 |
| 沙箱 | 5 | 内核级三平台 + fail-closed + 完整性报告，同类最佳 |
| 持久化 | 5 | 事件溯源 + 派生历史 + 双后端 + write-behind |
| 扩展性 | 5 | Cordis 反应式装卸 + 能力缝，零特权核心 |
| 上下文 | 4 | 压缩/裁剪/摘要成熟，投影缓存但无长期记忆跨会话层 |
| 路由 | 3 | 多 provider 共存但路由策略薄，无 Master-Slave |
| 可观测 | 4 | 事件即日志 + otel，但依赖 TS 声明合并不易移植 |

## 5. 可吸收清单

| 机制 | 收益 | 难度 | 实现要点 | 冲突点 |
|---|---|---|---|---|
| 效果可逆注册（effect/disposer 树） | 插件卸载零泄漏，热重载基础 | 低 | `ctx.effect()` 收 `BoxAsyncFn`，Fiber 卸载逆序跑 | nuomi Plugin 只有 init/dispose 两段，需把注册全部改走 effect |
| epoch 反应式依赖装卸 | 服务就绪自动激活插件，消灭启动顺序 bug | 中 | watch 通道广播 service 变化，重算依赖表 | nuomi 是一次性 init/start，需引入 PENDING 态 |
| waterfall 事件分发 | 中间件式拦截（审批/脱敏/重试）统一入口 | 低 | bus.rs 加 waterfall 模式，监听器收 `next()` | 现有 broadcast 只支持观察 |
| 单调工具守卫 | 权限拒绝策略与工具解耦 | 低 | guard 列表只可 deny | 与 nuomi 审批流衔接点需定义 |
| 沙箱 fail-closed + 功能探测 | Windows ACL 降级策略直接可抄 | 中 | 每 runner 探测 `exit 0`，partial 上报 | nuomi 目前无沙箱层，从零建 |
| 合成事件修复崩溃 turn | resume 语义完备 | 低 | resume 时补 `state_changed(interrupted)` | nuomi 已有 interrupted 态，补合成事件落库即可 |

## 6. 明确不建议吸收

- **JS Proxy 动态 ctx 属性访问**：Rust 无反射，静态 TypeId 注册已足够，模仿只会引入运行时字符串查找与 panic 面。
- **声明合并扩展事件/类型**：TS 专属；Rust 用 enum + 非 exhaustive 匹配。
- **Profile/Bundle/cordis.yml 声明式组合层**：为多产品形态服务，nuomi 当前 CLI+桌面双表面用不到，先不背这套 loader 复杂度。
- **Code Mode（run_code 嵌套调度）**：需嵌入式 TS/Python 运行时，成本极高且与 nuomi 的 CLI Agent 适配器路线重叠。
- **Python SDK**：与产品定位无关。
