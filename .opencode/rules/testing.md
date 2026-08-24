# Rules — 测试策略 / Testing Strategy

> 适用范围 Scope: 全仓库 whole repo。金字塔：Rust 单测/集成为主干，前端组件测试保关键流，E2E 后置。
> Pyramid: Rust unit/integration as backbone; component tests guard key flows; E2E deferred to M2+.

## Rust（src-tauri）
| 层 Layer | 要求 Requirement |
|---|---|
| Unit 单测 | 状态机表驱动：每条合法迁移 + 每类非法迁移各至少 1 例。Table-driven state-machine tests: every legal transition + each illegal class. |
| Repository | happy path + 错误路径（约束冲突、迁移失败）；临时目录 SQLite。Happy + error paths on tempdir sqlite. |
| Integration 集成 | orchestrator 用 **fake adapter** 端到端跑通：入队 → 运行 → 事件落库 → 终态；含取消与心跳超时恢复两场景。End-to-end with fake adapter incl. cancel + orphan-recovery scenarios. |

- `adapters/` 必须内置一个 `fake` adapter：确定性输出、可注入延迟与失败——供集成测试与演示模式共用。A deterministic fake adapter is mandatory (tests + demo mode).
- 单测不得访问网络。No network in unit tests — mock providers.

## Frontend（src）
- 关键用户流必须有组件测试（Vitest + Testing Library）：创建任务、审批操作、查看实时日志。Key flows tested: create task, approve, view live logs.
- IPC 一律 mock `src/lib/ipc/test-double.ts`，测试不触真实 Tauri runtime。Mock via the shared test double; never the real runtime.
- hooks 单测覆盖 `useDomainEvents` 的订阅/清理/批量刷新逻辑。

## 覆盖率预期 / Coverage expectations
- `domain/` 与 `orchestrator/` ≥ 80% 行覆盖；UI 不追百分比，以关键流为准。≥80% lines on domain+orchestrator; UI pragmatic.
- 修 bug 先写复现该 bug 的失败测试，再修复。Reproduce with a failing test first, then fix.

## 质量门 / Quality gates（交付前全部必绿 all green before handoff）
```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test   # in src-tauri/
pnpm typecheck && pnpm lint && pnpm test
```
