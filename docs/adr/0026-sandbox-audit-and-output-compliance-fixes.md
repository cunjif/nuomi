# ADR 0026: 沙箱机制四项差距修复（提交人审计 + 密钥强制兜底 + watchdog 资源用量 + 大输出产物目录分流）

- **状态**: Proposed（待用户确认）
- **日期**: 2026-11-02
- **决策者**: user + conductor
- **前置设计**: ADR-0022（插件化代码沙箱）、ADR-0025（沙箱重定位与自进化）、spec 5.7（代码运行沙箱）
- **依赖**: ADR-0022 §4/§5/§6、ADR-0025 §4.1（Profile）、迁移 0037/0038（sandbox_tasks）

## 背景

对当前沙箱机制（ADR-0022 落地实现）做深度对照分析后，发现四处与设计意图/spec 的差距：

1. **spec 5.7.1-5 提交人缺失**：spec 要求"记录提交人、目标、状态变迁、输出与资源用量"，但 `sandbox_tasks`（迁移 0037）无 submitter 列。多来源（工作台 vs 自进化引擎 vs 未来多用户）提交时无法区分。
2. **spec 5.7.1-7 密钥安全约定式**：spec 要求"禁止沙箱任务访问 OS 密钥库与应用配置中的密钥"，ADR-0022 决策 5 仅约束"提交参数不携带密钥"为设计约定，内核侧无校验。若未来接入非容器执行器（宿主内强隔离进程），约定可能被绕过。
3. **spec 5.7.3-3 watchdog 不记 usage**：spec 要求"资源超限→终止并记录资源用量"，但 `watchdog_time_out`（`task_manager.rs:689`）硬编码 `usage: None`。执行器侧超时会带 usage，manager watchdog 接管时不带，审计事件中该转移的 usage 为空。
4. **ADR-0022 §6 大输出产物目录分流未兑现**：ADR 意图"超大输出落 `.nuomi/sandbox/<task_id>/` 产物目录、行内只存摘要与定位"，当前 `apply_outputs` 全量追加到 DB 行，无截断/产物目录分流。长输出任务（构建日志 MB 级）DB 行膨胀。

## 决策

### 设计决策

| # | 决策点 | 选择 |
|---|---|---|
| 1 | 提交人审计 | **迁移 0048 加 `submitter` 列 + submit 链路写入 + 审计事件 payload 携带** |
| 2 | 密钥强制兜底 | **submit_check 增加启发式密钥嗅探，拒绝已知泄漏模式**（约定 + 内核兜底双重防线） |
| 3 | watchdog usage | **watchdog_time_out 采用触发时最后一次 poll 的 snapshot.usage**（已有参数被忽略） |
| 4 | 大输出分流 | **INLINE_OUTPUT_CAP 截断 + 产物目录落盘 + 行内摘要 + output_ref 定位列** |

### 1. 提交人审计（spec 5.7.1-5）

**迁移 0048**：`ALTER TABLE sandbox_tasks ADD COLUMN submitter TEXT;`（NULL 兼容存量行）。

**submitter 取值约定**（字符串枚举，不强制 CHECK 以保留扩展空间）：
- `"workbench"` — 工作台 UI 入口提交（`sandbox_run_code` / `sandbox_build_app`）
- `"selfeval"` — 自进化引擎提交（ADR-0025 场景 B2）
- `"repl"` — headless CLI REPL 提交
- 未来可扩展用户 id（多用户场景）

**链路改动**：
- `SandboxTaskSpec` 加 `submitter: String` 字段
- `SandboxTaskRecord` 加 `submitter` 字段，`insert` 写入
- `submit()` 将 spec.submitter 传入 record
- 审计事件 payload 加 `"submitter"` 字段
- IPC 层 `impl_sandbox_run_code` / `impl_sandbox_build_app` 从调用上下文填 `"workbench"`；自进化引擎调用填 `"selfeval"`

### 2. 密钥强制兜底校验（spec 5.7.1-7）

**设计哲学**：从纯"约定式"升级为"约定 + 内核启发式兜底"。不试图做完整密钥扫描（不可能完备），而是兜底常见泄漏模式，使无意泄漏在内核侧即被拦截，不依赖执行器实现差异。

**submit_check 新增校验**（`task_manager.rs`）：扫描 `spec.source` 与 `spec.authorized_dirs` 拼接文本，命中以下模式则返回 `SandboxError::LimitInvalid`（复用现有错误码，新增 `sandbox.secret_in_payload` 更精确但需 IPC 契约同步——权衡后用 `LimitInvalid` 携带明确消息，避免新增错误码扩散）：

| 模式 | 正则 | 理由 |
|---|---|---|
| keyring 引用 | `keyring://` | 显式 keyring 路径引用 |
| keyring API | `os\.keyring` / `keyring\.get` | keyring 编程式访问 |
| Bearer token | `Bearer\s+[A-Za-z0-9\-_]{20,}` | 常见 API token 内联 |
| 环境变量赋值 | `(API_KEY\|SECRET\|TOKEN\|PASSWORD)\s*=` | 密钥环境变量内联 |
| AWS key | `AKIA[0-9A-Z]{16}` | AWS access key id 格式 |
| 私钥头 | `-----BEGIN (RSA\|EC\|OPENSSH) PRIVATE KEY-----` | PEM 私钥块 |

**权衡**：
- 启发式有假阴性（无法覆盖所有密钥形态）——可接受，目的是兜底而非完备
- 假阳性风险（代码片段恰好含 `Bearer xxx`）——返回明确消息，用户可调整；不阻断合法构建（构建 source 是路径/目标描述，不含密钥）
- 不扫描 `app_temp_dirs`（路径不含密钥）

### 3. watchdog 资源用量（spec 5.7.3-3）

**现状**：`watchdog_time_out` 签名已有 `_snapshot: &SandboxTaskSnapshot`（`task_manager.rs:666`），但下划线前缀忽略不用，Transition 硬编码 `usage: None`（`task_manager.rs:689`）。

**修复**：去掉下划线，Transition 的 `usage` 字段填 `snapshot.usage.as_ref()`。这是最小改动——watchdog 触发时刚做过 `executor.poll`（`task_manager.rs:370`），poll 返回的 snapshot 已携带执行器报告的 usage，复用即可。

**边界**：执行器在 watchdog 触发瞬间可能未报告 usage（`snapshot.usage == None`），此时 Transition.usage 仍为 None——与执行器侧超时路径行为一致，可接受。

### 4. 大输出产物目录分流（ADR-0022 §6）

**迁移 0048** 加四列：
```sql
ALTER TABLE sandbox_tasks ADD COLUMN stdout_ref TEXT;
ALTER TABLE sandbox_tasks ADD COLUMN stderr_ref TEXT;
ALTER TABLE sandbox_tasks ADD COLUMN stdout_truncated INTEGER NOT NULL DEFAULT 0;
ALTER TABLE sandbox_tasks ADD COLUMN stderr_truncated INTEGER NOT NULL DEFAULT 0;
```

**截断策略**（`task_manager.rs::apply_outputs`）：
- 引入 `const INLINE_OUTPUT_CAP: usize = 64 * 1024;`（64KB，与执行器侧截断独立）
- 累计追加后若 `record.stdout.len() > INLINE_OUTPUT_CAP`：
  1. 将全量 stdout 写入 `.nuomi/sandbox/<task_id>/stdout.log`
  2. 行内 `stdout` 截断为前 `INLINE_OUTPUT_CAP` 字节
  3. `stdout_ref` 设为产物文件路径，`stdout_truncated = 1`
- stderr 同理
- 产物目录创建：首次截断时 `fs::create_dir_all(.nuomi/sandbox/<task_id>/)`

**SandboxTaskRecord** 加四字段；`SandboxTaskResult` 加 `stdout_ref` / `stderr_ref` / `stdout_truncated` / `stderr_truncated`，IPC DTO 同步。

**get_task 返回**：前端据 `stdout_truncated` 判断是否需读取 `stdout_ref` 完整输出；未截断时行内即完整。

**与执行器侧截断的关系**：执行器侧 `stdout_truncated`（snapshot）表示执行器已截断其缓冲；manager 侧 `stdout_truncated`（record）表示行内已截断、全量在产物目录。两者独立，不冲突——执行器截断时 fetch_output 只能拿到截断后内容，manager 落盘的就是截断后内容；manager 截断是行内存储优化，不影响数据完整性。

## 向后兼容

- **迁移 0048 只增不改**：四列均有默认值（NULL / 0），存量行零变更；submitter NULL 兼容存量。
- **submitter 字段**：SandboxTaskSpec 加非可选字段会破坏现有调用方——采用 `String` 直接字段，所有调用方必须显式填值（IPC 层填 "workbench"，自进化填 "selfeval"，测试填 "test"）。这是破坏性构造变更，但所有调用点在仓内可控。
- **密钥校验**：仅拒绝命中模式的提交，合法提交零影响；假阳性通过明确消息可识别。
- **watchdog usage**：纯填充原先为 None 的字段，无行为破坏。
- **大输出分流**：未超 cap 时行为不变（truncated=0, ref=NULL）；超 cap 时行内截断但数据完整保留在产物目录，可恢复。

## 权衡

- **优点**：
  - 提交人审计闭合 spec 5.7.1-5，多来源可区分
  - 密钥兜底校验从约定升级为强制，不依赖执行器实现
  - watchdog usage 闭合 spec 5.7.3-3，审计完整
  - 大输出分流闭合 ADR-0022 §6，DB 行体积有界
- **代价**：
  - 迁移 0048 四列 + SandboxTaskSpec 破坏性字段变更（所有调用点需改）
  - 密钥嗅探有假阳性风险（构建场景低，代码运行场景中）
  - 大输出分流引入产物目录 IO，首次截断有写盘开销
- **替代方案**：
  - (a) submitter 用 CHECK 约束枚举——否决，限制未来扩展（多用户 id）
  - (b) 密钥校验新增 `sandbox.secret_in_payload` 错误码——否决，IPC 契约扩散成本高于复用 LimitInvalid 的收益
  - (c) 大输出用 events 表复用——否决，ADR-0022 已论证 events 面向轻量审计
  - (d) 大输出不截断只加 ref——否决，不截断则 DB 行仍膨胀，违背 §6 意图

## 风险与缓解

| 风险 | 缓解 |
|---|---|
| submitter 字段破坏性变更漏改调用点 | 编译期保证（String 字段无默认值，所有构造点必须填）；cargo clippy 全量 |
| 密钥嗅探假阳性阻断合法代码运行 | 模式保守（需 20+ 字符 Bearer、PEM 完整头）；明确错误消息指引用户调整 |
| 产物目录写入失败丢输出 | 写盘失败时回退为行内全量存储（不截断），truncated=0；best-effort 不阻断任务生命周期 |
| 产物目录残留（任务删除后） | 复用 ADR-0025 ephemeral GC 范式：任务终态后由 dispose 清理；暂不实现自动 GC（手动清理 .nuomi/sandbox/） |
| IPC 契约变更需 regen | 契约先行：同一提交内 pnpm contracts:gen + check |

## 影响范围

### Migration
- **新增** `migrations/0048_sandbox_audit_columns.sql` — submitter + stdout_ref + stderr_ref + stdout_truncated + stderr_truncated 五列

### 后端
- `crates/nuomi-core/src/sandbox/types.rs` — SandboxTaskSpec 加 submitter；SandboxTaskResult 加四输出字段；新增 SECRET_PATTERNS 常量
- `crates/nuomi-core/src/sandbox/task_manager.rs` — submit_check 加密钥嗅探；watchdog_time_out 用 snapshot.usage；apply_outputs 加截断分流；SandboxTaskRecord 加字段
- `crates/nuomi-core/src/store/repos/sandbox_tasks.rs` — insert/list/find_by_id 携带新列；新增 set_output_ref 方法
- `src-tauri/src/commands.rs` — impl_sandbox_run_code / impl_sandbox_build_app 填 submitter
- 自进化引擎调用点（若有）— 填 submitter="selfeval"

### IPC 契约
- `src/lib/ipc/bindings.gen.ts` — 重新生成；SandboxTaskDto 加 submitter + stdout_ref + stderr_ref + stdout_truncated + stderr_truncated

### 测试
- task_manager 单测：submitter 写入与审计、密钥嗅探各模式命中与合法通过、watchdog usage 填充、大输出截断与产物目录落盘、未超 cap 不截断
- 迁移幂等测试

### 不涉及
- SandboxExecutor trait 契约——零改动（执行器侧不感知 submitter / 截断）
- CubeSandboxExecutor——零改动
- 前端——DTO 字段增加，前端按需消费（截断时显示"输出已截断，查看完整"入口）

## 实施计划

四项可并行后串行验证：
1. 迁移 0048（五列）
2. types.rs 字段 + SECRET_PATTERNS
3. task_manager.rs：submitter 写入 + 密钥嗅探 + watchdog usage + 截断分流
4. repos/sandbox_tasks.rs 新列读写
5. commands.rs submitter 填充
6. IPC 契约 regen
7. 单测全量
8. 验证门禁

## 验证门禁

- `cargo test -p nuomi-core`：submitter 审计、密钥嗅探六模式 + 假阴性、watchdog usage 填充、截断分流边界（cap-1 / cap / cap+1 / 多次追加触发）、产物目录落盘与回退
- `cargo clippy --all-targets -- -D warnings`
- `pnpm contracts:gen` 无未提交 diff + `pnpm contracts:check`
- `pnpm typecheck && pnpm lint && pnpm test`
- 安全抽测：六密钥模式各一例命中拒绝、合法代码运行不被误拒、构建 source（路径描述）通过
