# ADR 0030: RoleAgent 对话内 Background Task 创建与派发

- **状态**: Proposed（待用户确认）
- **日期**: 2026-11-11
- **决策者**: user + conductor
- **前置**: ADR-0029（管家任务派发）、ADR-0012（CLI Agent 会话）、LoopEngine（plugins/loop_engine.rs）、ToolRegistry（plugins/tools.rs）、team_runner / orchestrator

## 背景

ADR-0029 为管家 Agent（Steward）交付了 Background Task 立项与双路径派发能力。用户现要求将此能力**扩展到任意 RoleAgent**——RoleAgent 在对话中也能创建 Background Tasks，分两种类型：

1. **类型1（自派分身）**：创建与当前 RoleAgent 配置一致的 subagent 来完成 Task——无需用户确认，自动执行
2. **类型2（跨角色派发）**：调用一个或多个其他 RoleAgent 作为 subagent 来完成 Task——需要弹窗和用户确认

现状缺口：

1. **RoleAgent 对话走 LoopEngine，无 Suspend 范式**——LoopEngine 的工具调用流程为 `tool_call → ToolRegistry::execute → Result<String, HarnessError>`，只有成功/失败两种结果，没有"挂起等待用户确认"的机制。管家的 `StewardToolRegistry` 有 `ToolOutcome::Suspend`，但 LoopEngine 不支持。
2. **RoleAgent 无任务派发工具**——`ToolRegistry` 中只有通用插件工具（文件操作、MCP 工具等），没有 `create_background_task` / `dispatch_task_now` / `propose_team_dispatch` 等任务派发工具。
3. **单成员 Team 被 validate_plan 禁止**——`team_former.rs` 的 `validate_plan` 要求 `plan.members.len() >= 2`，类型1"自派分身"需要单成员 Team，现有管线不允许。
4. **前端 ChatView 无确认弹窗**——StewardChat 有E有 `TaskDispatchBubble` 确认组件，ChatView（RoleAgent 对话）没有。

**硬事实（决策依据）**：
- `team_former.rs:582`：`validate_plan` 要求 `plan.members.len() >= 2`——单成员 Team 在现有管线下无法通过校验
- `loop_engine.rs:512`：工具执行结果为 `Result<String, HarnessError>`——无 Suspend 变体
- `tools.rs:22-25`：`Tool` trait 只有 `async fn execute(&self, arguments: &serde_json::Value) -> Result<String, HarnessError>`——无挂起声明
- ADR-0029 决策1已否决"放宽 validate_plan 至最小 1 成员"——不为绕校验而改核心

## 决策

### 设计决策

| # | 决策点 | 选择 |
|---|---|---|
| 1 | 类型1 subagent 执行方式 | **后台 Team 成员**：创建单成员 Team（克隆当前 Role 配置），通过 Team 编排执行任务，结果回传父对话。走特殊路径绕过 `validate_plan` 最小 2 成员约束（决策 2） |
| 2 | 单成员 Team 约束处理 | **为自派分身走 Team 简化创建路径**：不经 `team_former::draft_team_plan` / `validate_plan`，直接在 services 层创建 Team 记录（单成员 = 当前 Role 克隆）+ 运行。不修改 `validate_plan` 本体（ADR-0029 决策1先例：不改核心校验） |
| 3 | 类型2 确认方式 | **通用审批收件箱**：走 `propose_team_dispatch` 路径，提案进审批收件箱（approvals），用户在收件箱中确认。复用 ADR-0029 的 proposals 管线零改动 |
| 4 | LoopEngine Suspend 机制 | **在 LoopEngine 中引入 Suspend**：扩展 `Tool` trait 增加 `suspend_on_call` 声明 + `ToolOutcome` 枚举（`Result(String)` / `Suspend(SuspendRequest)`）；LoopEngine 检测到 Suspend 后暂停循环，发布 `tool.suspended` 事件，等待异步确认后恢复 |
| 5 | 类型1 是否需要确认 | **不需要用户确认**：自派分身是当前 RoleAgent 的自主行为（克隆自身配置执行任务），与"用户自己在对话中做事"信任等价。立项即时落 backlog 卡（同 ADR-0029 决策2），执行自动启动 |
| 6 | 类型2 是否需要确认 | **需要用户确认**：跨角色派发涉及调动其他 RoleAgent，属于"消耗配额的执行"，须经审批收件箱确认（同 ADR-0029 决策3b/4 信任模型） |
| 7 | 任务结果回传 | **事件总线 + 下轮注入**：subagent 完成任务后发布 `task.completed` 事件，父 RoleAgent 下轮对话通过 steering queue 注入结果摘要（复用 LoopEngine 既有 dual-queue 机制） |
| 8 | 新增工具 | **2 个工具**：`create_self_task`（类型1，Result 型，立项+自动启动单成员 Team）、`propose_team_dispatch`（类型2，Suspend 型，提案进审批收件箱）。`list_tasks` / `create_background_task` 不重复注册（RoleAgent 可通过 Board 查看） |
| 9 | Task source 新增取值 | **`source='agent'`**：RoleAgent 创建的任务标记为 `source='agent'`（区别于 `manual`/`schedule`/`steward`），`created_by` 为父 RoleAgent 的 session_id |

### 1. 类型1：自派分身——单成员 Team 简化创建

RoleAgent 调用 `create_self_task(title, description?)` 工具时：

1. **立项**：写 tasks 行（status='backlog', source='agent', created_by=父 session_id, workspace_id=当前 workspace）
2. **克隆 Role 配置**：从 DB 读取当前 Role 的 `provider_id`、`system_prompt_override`、`tool_allowlist`、`temperature`，构造一个"影子 Role"（name = `{原Role名}-subagent`）
3. **创建单成员 Team**：直接在 services 层 `create_team`（不经 `team_former` / `validate_plan`），Team 拓扑 = `pipeline`（单成员顺序执行），成员 = 影子 Role
4. **自动启动**：`run_team_on_task(task_id, team_id)` —— 任务从 backlog → queued → running
5. **结果回传**：任务完成后发布 `task.completed` 事件（含 result_summary），父对话下轮通过 steering queue 注入

**为什么不走 `validate_plan`**：`validate_plan` 的最小 2 成员约束是针对**规划器选人**场景（LLM 规划团队拓扑需要至少 2 个角色才有协作意义）。自派分身是**用户/Agent 明确指定单角色**——与规划器选人语义不同，走简化创建路径不修改核心校验（ADR-0029 决策1 先例）。

**为什么用 Team 编排而非独立 LoopEngine**：复用现有 `team_runner` 链路（Run 生命周期、事件落库、状态机、CLI handle 持久化全部继承），避免新建并行执行通道。单成员 pipeline 拓扑 = 顺序执行一个 Role，语义清晰。

### 2. 类型2：跨角色派发——审批收件箱

RoleAgent 调用 `propose_team_dispatch(task_ids, preferences?)` 工具时：

1. **立项**（若 task_ids 为空则先创建）：写 tasks 行（source='agent'）
2. **规划**：`draft_team_plan` 调用 LLM 规划团队（≥2 成员，现有校验无障碍）
3. **落提案**：kind='team_plan', source='agent', status='pending' —— 进审批收件箱
4. **Suspend**：工具返回 `Suspend` 结果，LoopEngine 暂停循环，发布 `tool.suspended` 事件
5. **用户确认**：用户在审批收件箱中 approve/deny
6. **恢复**：审批通过后 `resolve_proposal`（零改动复用 ADR-0029 管线）→ `form_team_from_plan` → `run_team_on_task` → 回填 assignee_team_id
7. **LoopEngine 恢复**：审批结果通过 steering queue 注入父对话，LoopEngine 恢复循环

### 3. LoopEngine Suspend 机制

扩展 `Tool` trait 和 LoopEngine 核心循环：

```rust
// tools.rs — Tool trait 扩展
#[async_trait]
pub trait Tool: Send + Sync {
    fn def(&self) -> ToolDef;
    /// 声明此工具是否在调用后挂起循环等待异步确认。
    /// 默认 false — 大多数工具是 Result 型。
    fn suspend_on_call(&self) -> bool { false }
    async fn execute(&self, arguments: &serde_json::Value) -> Result<String, HarnessError>;
}

// 工具执行结果枚举（LoopEngine 内部）
pub enum ToolOutcome {
    /// 正常结果 — 回喂模型继续循环
    Result(String),
    /// 挂起 — 发布事件等待异步确认，LoopEngine 暂停
    Suspend(SuspendRequest),
}

pub struct SuspendRequest {
    /// 确认 handle（如 "resolve_proposal"），前端据此路由到对应确认 UI
    pub confirm_handle: String,
    /// 挂起原因摘要（回喂模型，让模型知道已提交审批等待确认）
    pub summary: String,
    /// 关联实体 id（proposal_id / task_id 等）
    pub entity_id: Option<String>,
}
```

LoopEngine 核心循环改动（`loop_engine.rs:465-540` 区域）：

```rust
// 现状：工具执行后直接回喂 result
let result = match denied {
    Some(ref reason) => format!("[denied by hook] {reason}"),
    None => match tools.execute(&call.name, &call.arguments).await {
        Ok(out) => out,
        Err(e) => format!("[tool error] {eB}"),
    },
};
transcript.push(ChatMessage::tool_result(call.id, result));

// 改后：检测 suspend_on_call，分流 Result / Suspend
let outcome = if denied.is_some() {
    ToolOutcome::Result(format!("[denied by hook] {}", denied.unwrap()))
} else {
    let tool = tools.get(&call.name).await;
    let is_suspend = tool.map(|t| t.suspend_on_call()).unwrap_or(false);
    match tools.execute(&call.name, &call.arguments).await {
        Ok(out) => {
            if is_suspend {
                // 工具已执行完毕（落提案），返回 SuspendRequest
                // 工具自身在 execute 中已把提案写入 DB，
                // execute 的返回值是 SuspendRequest 的 JSON 序列化
                ToolOutcome::Suspend(serde_json::from_str(&out)
                    .unwrap_or(SuspendRequest {
                        confirm_handle: "unknown".into(),
                        summary: out,
                        entity_id: None,
                    }))
            } else {
                ToolOutcome::Result(out)
            }
        }
        Err(e) => ToolOutcome::Result(format!("[tool error] {e}")),
    }
};

match outcome {
    ToolOutcome::Result(r) => {
        transcript.push(ChatMessage::tool_result(call.id, r));
    }
    ToolOutcome::Suspend(req) => {
        // 1. 回喂模型一个 tool_result（让模型知道已提交审批）
        transcript.push(ChatMessage::tool_result(call.id, req.summary));
        // 2. 发布 tool.suspended 事件（前端监听，刷新审批收件箱）
        ctx.publish(Event::new("tool.suspended", serde_json::json!({
            "confirm_handle": req.confirm_handle,
            "entity_id": req.entity_id,
            "summary": req.summary,
        })));
        // 3. 返回 LoopRunResult（truncated=false, suspended=true）
        return Ok(LoopRunResult {
            transcript,
            final_text: String::new(),
            steps,
            truncated: false,
            suspended: true,  // 新字段
            suspend_handle: req.confirm_handle,  // 新字段
            cli_session_id: last_cli_session_id,
            sources: drain_sources(&self.sources),
        });
    }
}
```

**LoopRunResult 新增字段**：
```rust
pub struct LoopRunResult {
    // ... 既有字段 ...
    /// True if the loop was suspended waiting for async confirmation.
    pub suspended: bool,
    /// The confirm handle when suspended (e.g. "resolve_proposal").
    pub suspend_handle: Option<String>,
}
```

**恢复机制**：审批通过后，shell 层通过 steering queue 注入审批结果摘要，LoopEngine 在下一次 `run_with_history` 调用时自然恢复（复用既有 dual-queue 机制，无需新增恢复路径）。

### 4. 新增工具

| 工具 | 类型 | suspend_on_call | 参数 | 语义 |
|---|---|---|---|---|
| `create_self_task` | Result | false | `{ title: string, description?: string }` | 立项 + 自动创建单成员 Team + 启动运行；返回 `{ task_id, team_id, run_id, status: "running" }` |
| `propose_team_dispatch` | Suspend | true | `{ task_ids: string[], preferences?: string }` | 立项（若需要）+ LLM 规划团队 + 落提案进审批收件箱；返回 SuspendRequest |

**工具注册**：在 `single_role_materializer.rs` 物化 RoleAgent 时，将这两个工具注册进 `ToolRegistry`（通过 Role 的 `tool_allowlist` 控制是否暴露——默认暴露，Role 可通过 allowlist 关闭）。

### 5. Task source 新增取值

`source` 字段新增 `'agent'` 取值（区别于 `'manual'`/`'schedule'`/`'steward'`）：
- `'agent'` — RoleAgent 在对话中创建的任务
- `created_by` — 父 RoleAgent 的 session_id

Board 卡片显示 source 徽标时新增 `agent` 类型（i18n: "Agent" / "智能体"）。

### 6. 结果回传机制

subagent 完成任务后，结果通过**事件总线 + steering queue** 回传父对话：

1. `team_runner` 在 Run 完成时发布 `run.completed` 事件（含 result_summary）
2. shell 层监听该事件，若 Run 关联的 Task 的 `created_by` 是某个 session_id，则将 result_summary 注入该 session 的 steering queue
3. 父 RoleAgent 下次 `run_with_history` 时，steering queue 中的 result_summary 作为 user message 注入 transcript，模型自然看到 subagent 的执行结果

**复用既有机制**：LoopEngine 的 dual-queue（steering + follow_ups）已支持外部消息注入（`loop_engine.rs:431-443`），无需新建回传通道。

## 向后兼容

- **LoopEngine Suspend 是纯增量**：`Tool::suspend_on_call()` 默认 `false`，既有工具行为不变；`LoopRunResult` 新增 `suspended` / `suspend_handle` 字段默认 `false` / `None`，既有调用方零影响。
- **ToolRegistry 不变**：`Tool` trait 增加 `suspend_on_call` 方法带默认实现，既有工具实现零改动。
- **validate_plan 不变**：自派分身走简化创建路径，不修改 `team_former.rs` 核心校验。
- **proposals 管线零改动**：类型2 复用 ADR-0029 的 `resolve_proposal` / `form_team_from_plan` / `run_team_on_task` 全链路。
- **Task source 扩展**：`source='agent'` 是新增取值，旧行默认 `'manual'` 不受影响；Board 卡片 source 徽标新增一种类型。
- **契约变更**：`LoopRunResult` 新增字段、`TaskDto` source 新增取值 → 契约先行（同一提交 regen bindings + 更新消费端）。

## 权衡

- **优点**：
  - 复用现有 Team 编排链路（team_runner / Run 生命周期 / 事件落库 / CLI handle 持久化）
  - 复用 ADR-0029 proposals 管线零改动
  - LoopEngine Suspend 是正交扩展（默认不挂起，既有工具零影响）
  - 结果回传复用 dual-queue 机制，无需新建通道
  - 自派分身走简化路径不改核心校验

- **代价**：
  - LoopEngine 核心循环改动（工具执行分流 Result/Suspend）——改动面集中在一处（`loop_engine.rs:465-540`）
  - `Tool` trait 增加方法（带默认实现，既有工具零改动）
  - 单成员 Team 简化创建路径——新增 services 层函数（不经 team_former）
  - `LoopRunResult` 新增字段——契约变更

- **替代方案**：
  - (a) **类型1 走独立 LoopEngine 实例**（不走 Team 编排）——否决。新建并行执行通道违反"执行通道唯一"原则；Run 生命周期/事件落库/CLI handle 全要重做。
  - (b) **类型1 走 CLI Agent 子进程**——否决。`CliAgentClient` 是将外部 CLI 进程包装为 LlmProvider，不是创建子 RoleAgent；且不是所有 RoleAgent 都走 CLI adapter。
  - (c) **放宽 validate_plan 至最小 1 成员**——否决（ADR-0029 决策1 先例：动核心校验代价不成比例）。
  - (d) **类型2 走对话内弹窗确认**（不走审批收件箱）——否决。用户已选审批收件箱；且对话内弹窗需要前端 ChatView 新建确认组件，改动面更大。
  - (e) **LoopEngine Suspend 走事件总线 + 前端轮询**（不改 LoopEngine 核心循环）——否决。用户已选"在 LoopEngine 中引入 Suspend"；且事件总线方案需要前端轮询或 WebSocket，复杂度更高。
  - (f) **工具内同步阻塞等待确认**（通过 channel/oneshot）——否决。阻塞 async runtime，且 LoopEngine 无法在等待期间响应 cancel token。
  - (g) **不为类型2 引入 Suspend**（工具直接返回"已提交审批"结果，不暂停循环）——可行但体验差：模型只能在对话中说"已提交审批，请等待"，无法在审批结果回来后继续对话。Suspend 方案让模型在审批结果回来后自然恢复，体验更好。

## 风险与缓解

| 风险 | 缓解 |
|---|---|
| LoopEngine Suspend 改动影响既有工具调用流程 | `suspend_on_call` 默认 false，既有工具行为不变；Suspend 分流逻辑在工具执行后、tool_result 回喂前，不影响 hook/authorization 流程 |
| 单成员 Team 简化创建路径绕过校验，可能产生无效 Team | 简化路径只接受"当前 Role 克隆"作为成员（不接受外部参数指定 Role），成员来源可信；Team 拓扑固定 pipeline（无协作语义问题） |
| subagent 执行失败或超时 | Run 状态机既有 failed/timeout 终态覆盖；结果回传时 event payload 含 status，父对话模型可见失败原因 |
| 父对话 session 已关闭时 subagent 仍在运行 | subagent Run 独立于父对话 session（走 team_runner 链路），父 session 关闭不影响 subagent 执行；结果通过事件总线回传，父 session 重新打开后可见 |
| 类型2 审批等待期任务被手动跑掉/删除 | resolve_proposal 既有跳过逻辑零改动兼容（"skipping is the honest outcome"，ADR-0029 先例） |
| RoleAgent 滥用 create_self_task（无限派分身） | 每轮刷卡上限 `agent.tools.max_tasks_per_loop`（默认 3，低于管家 5），app_settings 键、默认值兜底、不进 Settings UI（ADR-0029 决策9 先例） |
| 契约变更漏 regen | 契约先行硬规则：LoopRunResult 扩字段 + TaskDto source 新取值提交内 `pnpm contracts:gen` + `pnpm contracts:check` |

## 影响范围

### 后端 core（crates/nuomi-core）

- `plugins/tools.rs` — `Tool` trait 增加 `suspend_on_call()` 方法（默认 false）；`ToolOutcome` 枚举 + `SuspendRequest` 结构
- `plugins/loop_engine.rs` — 核心循环工具执行分流 Result/Suspend；`LoopRunResult` 新增 `suspended` / `suspend_handle` 字段
- `services/single_role_materializer.rs` — 物化 RoleAgent 时注册 `create_self_task` / `propose_team_dispatch` 工具
- `services/` — 新增单成员 Team 简化创建函数（`create_shadow_team`）
- `domain/entities.rs` — Task source 新增 `'agent'` 取值（字符串字段，无 schema 变更）
- 新增工具实现文件（`plugins/agent_task_tools.rs` 或类似）

### src-tauri

- `commands.rs` — `LoopRunResultDto` 扩 `suspended` / `suspendHandle` 字段【契约】
- `TaskDto` — source 新增 `'agent'` 取值（无字段变更，只是取值扩展）
- 事件路由 — `tool.suspended` 事件透传至前端

### 前端

- `src/lib/ipc/bindings.gen.ts` — regen 产物（LoopRunResultDto 新字段）
- Board 卡片 source 徽标新增 `agent` 类型 + i18n
- ChatView — 监听 `tool.suspended` 事件，显示"已提交审批，等待确认"提示（不弹窗，审批走收件箱）
- ApprovalsView — 零改动（proposals 合并展示，source='agent' 自然出现）

### 不涉及

- validate_plan 修改 / 单成员团进 team_former / ChatView 新建确认弹窗 / 新提案类型 / 自动放行——明确排除

## 实施计划

1. **LoopEngine Suspend 机制** — `Tool` trait 扩展 + `ToolOutcome` 枚举 + LoopEngine 核心循环分流 + `LoopRunResult` 新字段 + 单测
2. **单成员 Team 简化创建** — `create_shadow_team` services 函数 + 单测
3. **2 个新工具** — `create_self_task` / `propose_team_dispatch` 实现 + 注册进 materializer + 单测
4. **契约 regen** — LoopRunResultDto 扩字段 + TaskDto source 取值扩展【契约】
5. **事件路由** — `tool.suspended` 事件透传 + 结果回传 steering queue 接线
6. **前端** — Board source 徽标 + ChatView suspended 提示 + i18n
7. **门禁** — cargo test + clippy + contracts:check + typecheck + lint + vitest

## 验证门禁

- `cargo test -p nuomi-core`：LoopEngine Suspend 分流测试、单成员 Team 创建测试、工具参数校验、刷卡上限、propose_team_dispatch 挂起路径
- `cargo clippy --all-targets -- -D warnings` 零警告
- `pnpm contracts:gen` 无未提交 diff + `pnpm contracts:check`
- `pnpm typecheck && pnpm lint && pnpm test -- --no-file-parallelism`
- 人工验证：`pnpm tauri dev` 下与 RoleAgent 对话——
  1. "帮我创建一个后台任务来检查代码风格" → 看板出现 source=agent 的卡 + 自动启动单成员 Team Run
  2. "组队执行这个任务" → 收件箱出现提案 → 批准后建团、Run 起飞
  3. subagent 完成后 → 父对话自动收到结果摘要
