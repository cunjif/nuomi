# 沙箱插件（Sandbox Plugin）

> 决策依据：[ADR 0022](../adr/0022-pluggable-code-sandbox.md) · 任务来源：steward_workbench_v2 任务组 7
> 相关文档：[插件格式](plugin-format.md)（NPP 侧载插件）· [插件协议](plugin-protocol.md)

沙箱插件为 nuomi 提供插件化的代码执行与应用构建能力（spec 5.7）。它是一个**内置 Rust 插件**（`crates/nuomi-core/src/plugins/sandbox.rs`），不走 NPP 侧载协议——ADR-0022 否决了用 NPP 进程承载沙箱的方案，采用"内置插件壳 + `SandboxExecutor` trait"结构：插件壳负责生命周期与 SQLite 账本，执行器可替换（默认 `CubeSandboxExecutor`，HTTP 客户端形态）。

## 能力声明与运行时清单

沙箱能力表面由内核 `NuomiKernel::sandbox()` 暴露，经 IPC 七命令消费（`sandbox_run_code` / `sandbox_build_app` / `sandbox_list_tasks` / `sandbox_get_task` / `sandbox_cancel_task` / `sandbox_retry_artifact` / `sandbox_list_runtimes`）。

默认执行器（CubeSandbox）声明的语言运行时清单：

| runtime id | 名称 | 隔离机制 |
|---|---|---|
| `python` | Python 3 | CubeSandbox 容器隔离 |
| `node` | Node.js | CubeSandbox 容器隔离 |

运行时清单以执行器声明为准（`sandbox_list_runtimes` 实时查询）——替换执行器后清单随之变化，前端下拉与"沙箱不可用"判定均以该查询结果为准。

## 隔离机制

1. **容器级隔离**：代码在 CubeSandbox 类执行服务的隔离容器中运行，与宿主文件系统/进程隔离；执行服务是独立部署的进程（默认与本机内核通信）。
2. **授权目录白名单**：任务只能访问显式授权的目录——
   - `run_code`：提交时由用户勾选（激活工作区的已挂载目录，**默认全不选**，最小授权原则）；
   - `build_app`：构建目标由内核自动收敛为激活工作区的已挂载目录，无需选择。
   - 越权路径访问被拒绝并返回 `sandbox.dir_not_authorized`（spec 4.3-1）。
3. **网络默认关闭**：`limits.networkEnabled` 默认 `false`，显式开启才放行。
4. **签名密钥不进沙箱**：应用构建的签名密钥保留在宿主 OS keyring，沙箱内不可见；如需签名，产物回传后由宿主工序完成（spec 4.3-5）。

## 资源限额（SandboxLimits）

| 字段 | 必填 | 约束 | 默认 |
|---|---|---|---|
| `timeoutSecs` | 是 | ≥ 1 | 120 |
| `cpuPercent` | 否 | 1–100，空 = 不设限 | 空 |
| `memoryMb` | 否 | ≥ 16，空 = 不设限 | 空 |
| `networkEnabled` | 否 | — | `false` |

超时行为：任务超过 `timeoutSecs` 被中止、容器回收，落 `timed_out` 终态并保留已产生的部分输出（spec 5.7.3-2）；用户可查看部分输出并重新提交。

## 任务账本与产物回传

每次任务在 `sandbox_tasks` 表（迁移 0037）留档：状态机六态（queued/running/succeeded/failed/timed_out/cancelled，对齐 Run 状态机词表）、源码、授权目录、限额、stdout/stderr、退出信息、耗时。

产物回传是显式两段式：执行成功（Cube 侧 done）≠ 产物落盘（宿主侧 fetch）。回传失败时任务仍为 `succeeded` 但标记 `artifactRetryable`，详情页提供「重试回传」按钮（`sandbox_retry_artifact`，幂等）。

## 配置教程

### 1. 启动执行服务

部署一个 CubeSandbox 类执行服务。本机开发最简形态是监听回环端口的单容器执行服务。

### 2. 配置端点

端点解析优先级（`facade.rs` `resolve_sandbox_base_url`）：

```
环境变量 NUOMI_SANDBOX_CUBE_URL  >  app_settings 键 sandbox_cube_url  >  http://127.0.0.1:8819
```

- **环境变量**（优先级最高，适合 CI / 临时覆盖）：
  ```bash
  export NUOMI_SANDBOX_CUBE_URL="http://127.0.0.1:8819"
  ```
- **持久化设置**（写入 SQLite `app_settings` 表 `sandbox_cube_url` 键）。
- 未配置时使用默认回环端点 `http://127.0.0.1:8819`。

### 3. 使用入口

桌面壳左侧导航「沙箱」：

- **新建运行**：选择运行时 → （可选）勾选授权目录 → 调整限额 → 在 Monaco 编辑器输入代码 → 提交。任务出现在左侧列表，右侧详情实时展示状态/输出/产物。
- **构建应用**：选择运行时与限额，输入构建脚本；构建目标自动解析为激活工作区的已挂载目录。

### 4. 排障

- **「沙箱不可用：请检查沙箱插件配置」**：`sandbox_list_runtimes` 返回空或连接失败。依次检查执行服务是否启动、端点配置是否正确、环境变量拼写。
- **产物缺失但任务成功**：详情页出现「产物回传失败」标记，点击「重试回传」。
- **任务卡在 running**：等待超时看门狗兜底（manager 统一计时，保证任务一定有终态），或使用「取消任务」。

## 替换执行器（扩展开发）

执行器实现 `SandboxExecutor` trait（`crates/nuomi-core/src/sandbox/executor.rs`）：

```rust
#[async_trait]
pub trait SandboxExecutor: Send + Sync {
    /// 稳定执行器 id（诊断用，如 "cube"）。
    fn id(&self) -> &str;
    /// 声明的运行时清单 —— 驱动 sandbox_list_runtimes 与提交时的运行时校验。
    fn runtimes(&self) -> Vec<SandboxRuntimeInfo>;
    /// 以内核铸造的任务 id 接收任务（内核先落 queued 行与审计事件再调用 —— Run 铁律）。
    async fn submit(&self, task_id: &str, spec: SandboxTaskSpec) -> ...;
    /// 轮询任务快照（状态/输出总量/资源用量/终态详情）。
    async fn poll(&self, task_id: &str) -> ...;
    /// 请求取消；成功后 poll 必须报告 cancelled 终态。
    async fn cancel(&self, task_id: &str) -> ...;
    /// 按 cursor 拉取增量输出（stdout/stderr 游标协议，返回两条 chunk）。
    async fn fetch_output(&self, task_id: &str, stdout_from: u64, stderr_from: u64) -> ...;
    /// 取回产物（大产物落 .nuomi/sandbox/<task_id>/，DB 行只存 ref）。
    async fn fetch_artifact(&self, task_id: &str) -> ...;
}
```

在插件壳 `SandboxPlugin` 构造期替换默认执行器即可。实现方自行强制隔离边界（容器或强隔离进程）——清单声明只是展示信息，绝不是安全边界（ADR-0022 §2）；错误语义须映射到 `SandboxError` 稳定错误码（`sandbox.unavailable` / `sandbox.dir_not_authorized` / `sandbox.runtime_unknown` / `sandbox.limit_invalid` / `sandbox.task_not_found` / `sandbox.internal`），前端 i18n 与错误提示均按错误码渲染。
