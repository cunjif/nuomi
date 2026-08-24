# Rules — Rust 核心（Tauri Backend）

> 适用范围 Scope: `src-tauri/**`。与 AGENTS.md 冲突时以 AGENTS.md 为准。
> Applies to `src-tauri/**`. AGENTS.md wins on conflict.

## 错误处理 / Errors
- 每个模块一个 `thiserror` 错误枚举（如 `domain::DomainError`）；库路径零 panic：禁 `unwrap()/expect()`（测试与有注释证明的不变式除外）。One thiserror enum per module; no unwrap/expect outside tests & documented invariants.
- 对外（IPC 层）错误统一映射为 `IpcError { code, message, details? }`，见 ipc-contract.md。Map outward errors to IpcError.
- `Result` 贯穿到底，不用 bool 表达失败原因。

## 异步 / Async
- tokio 运行时；SQLite 操作用 `spawn_blocking` 包裹，绝不阻塞 async 线程。Wrap sqlite calls in spawn_blocking.
- 共享状态优先用消息通道（`mpsc`/`watch`）而非共享锁；必须加锁时不持有锁跨越 `.await`。Prefer channels over shared locks; never hold a lock across `.await`.
- Run 执行器：每个 Run 一个受监督任务，注册 JoinHandle；取消用 `CancellationToken`；心跳定期上报。Supervised tasks with handles, CancellationToken for cancel, periodic heartbeats.

## 状态机 / State machine（产品心脏 product heart）
- `domain/run_state.rs` 中实现为枚举 + 全函数：
  `fn transition(&self, ev: RunEvent) -> Result<RunState, TransitionError>`
- match 必须穷尽（编译器保证新增状态即报错）。Exhaustive matches only.
- 迁移前先写 EventRecord，再执行副作用（AGENTS.md §4 铁律）。Persist event before side effects.

## 存储 / Persistence
- rusqlite + WAL；所有 SQL 收敛到 `store/` 的 repository 结构体，业务代码不写裸 SQL。SQL lives in repositories only.
- 一律参数绑定查询，禁止 `format!` 拼 SQL。Parameterized queries only — no string-built SQL.
- 迁移：`migrations/NNN__name.sql` 按字典序执行，`PRAGMA user_version` 记录版本；已发布文件永不修改。Numbered append-only migrations tracked via user_version.

## 事件溯源 / Event sourcing
- `events` 表只追加：`(id, aggregate_type, aggregate_id, kind, payload JSON, created_at)`；禁止 UPDATE/DELETE。Append-only events table; no updates/deletes.
- 视图/统计由事件推导（projection），不反向修改事件。

## 安全 / Security
- 子进程：参数数组传递，禁止 shell 字符串拼接；启动的可执行文件须白名单校验（来自 AgentProfile 配置时尤其）。Spawn with arg arrays, never shell strings; allowlist executables.
- 取消/超时时必须回收子进程（kill on drop/cancel）。Always reap child processes.
- 密钥只进 OS keyring / 加密存储；`tracing` 日志 INFO 以上不得出现密钥与完整 prompt 正文。Keys to keyring; never log secrets or full prompt bodies above DEBUG.

## 日志 / Logging
- `tracing` crate；以 `run_id`/`task_id` 建 span，便于按运行聚合日志。Spans keyed by run_id/task_id.

## 测试 / Testing
- 单测紧邻代码；状态机必须表驱动测试覆盖**每一条合法迁移与至少一条非法迁移**。Table-driven tests covering every legal transition + illegal ones.
- repository 测试用临时目录 SQLite（`tempfile`）。
- 完成前必跑 / gates before done:
```bash
cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test
```
