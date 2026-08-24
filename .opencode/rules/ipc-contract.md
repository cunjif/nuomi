# Rules — IPC 契约 / IPC Contract（TS ⇄ Rust 单一事实源）

> 适用范围 Scope: `src-tauri/src/commands/`、`src-tauri/src/adapters/` 对外类型、`src/lib/ipc/`。
> 契约漂移是本项目第一号事故源，本规则零容忍。Contract drift is incident #1 — zero tolerance.

## 单一来源 / Single source of truth
- 所有跨 IPC 类型在 Rust 端定义并标注 tauri-specta；TS 绑定生成到 `src/lib/ipc/bindings.gen.ts`。
- **生成文件禁止手改**；发现绑定过期先跑 `pnpm contracts:gen`，不要绕过它手写类型。
- Never hand-edit generated files; regenerate instead (`pnpm contracts:gen`).

## 序列化约定 / Serialization conventions
- 结构体一律 `#[serde(rename_all = "camelCase")]`。All structs camelCase.
- 枚举/联合用内部标记：`#[serde(tag = "type", rename_all = "camelCase")]`，前端得到可判别联合。Internally-tagged enums → TS discriminated unions.
- 时间戳统一 `i64` Unix 毫秒（字段名 `*At`）；ID 统一 `String`（uuid v7）。Epoch-ms `*At`; uuid-v7 string ids.

## 命令规范 / Command conventions
- 命名动词短语 snake_case：`list_tasks` / `create_task` / `get_run` / `approve_run` / `cancel_run` / `subscribe_events`。
- 返回一律 `Result<T, IpcError>`；`IpcError { code, message, details? }`，code 用稳定字符串常量（如 `"run.not_found"`），前端按 code 映射 i18n 文案。Structured errors; frontend maps codes to i18n.
- 每个新命令四步走（缺一即违约）：
  1. Rust handler 写进 `commands/` 薄层；
  2. 注册到唯一的 builder 函数（specta 收集处）;
  3. `pnpm contracts:gen` 再生绑定；
  4. 同提交内更新前端消费端与 test-double。

## 事件通道 / Event channel
- 唯一通道 `event://domain`，payload 为 `DomainEvent` 判别联合：`{ type, taskId?, runId?, seq?, payload }`。
- 高频流（日志 token、tool_result）只推增量小包；全量历史用命令 + 游标分页查询（`listEvents(runId, cursor)`）。Stream deltas only; history via cursor pagination.
- 事件必须携带单调递增 `seq`（每 Run 内），前端断线重连按 seq 补拉。Monotonic per-run seq for gap recovery.

## 版本化 / Versioning
- 只加不改删：新增字段必须给默认值；重命名/删除视为破坏性变更走 ADR。Additive only; renames/removals need an ADR.
- 契约变更的提交必须同时包含：Rust 定义 + 再生绑定 + 消费端 + test-double 四处一致。
