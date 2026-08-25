# SPEC: Nuomi M-TEAM1 — Team 编排接入桌面壳（team-shell-m1）

> 状态：已批准
> 前置：`docs/specs/harness-kernel-v1.md`（K6 编排执行器已交付）与 `docs/specs/cli-agents-m1.md`（M-CLI1 已交付，复用其 `adapters::cli` 与 `tests/fixtures/fake_cli.js`）；UI 部分依赖 `ui-m1` 的 Settings/Board 页面骨架。
> 需求来源：`pr.md` §2「支持自发 Agent Team / 自定义 Agent Team」与 §6「支持将以创建的Agent组成群聊推进工作」；需求权威仍为 `pr.md`。

## 0. 决策记录摘要

| # | 决策 |
|---|---|
| D1 | 前置现状：roles/teams 表（migration 0001）与执行器 Pipeline/Router/GroupChat（含 Selector/WhiteBoard，K6 已交付并有测试）就绪；但桌面壳 `submit_task` 只走单 Provider Loop Engine，多 Provider 配置存库而运行时仅 boot 单例——本里程碑打通「配置 → 运行时物化 → 编排执行 → 事件可见」全链路 |
| D2 | 新服务 `crates/nuomi-core/src/services/team_runner.rs`：(a) Provider 物化注册表——从 DB `provider_configs` 按需构造 OpenAICompatibleClient/AnthropicCompatibleClient（密钥经 SecretStore/keyring 按 keyring_ref 取回；keyring_ref 缺失且无环境变量兜底时跳过该 provider 并 tracing 告警，不失败整场）；enabled 的 agent_profiles 物化为 CliAgentClient（白名单 = 该 profile 自身 command 的 basename——用户在 Settings 显式配置即授权自身，防配置损坏/注入后换命令执行）；is_master 配置为默认 provider；(b) Role↔AgentProfile 绑定经 roles.params_json 约定键 `agent_profile_id`（表结构不变，迁移只增不改），绑定优先于 provider_id |
| D3 | `TeamRunner::run(db_path, bus: Option<EventBus>, team_id, session_id, task)`：加载 team + 成员 roles（缺成员 → 明确错误），按 topology 分派执行器；GroupChat 用 LlmSelector（主 provider 判定，不可用时降级 RoundRobin）；Router 的 required 能力取自 team.config JSON 的 `required` 数组；WhiteBoardService 附总线镜像（`session.whiteboard`）；产出 TeamRunOutcome（transcript/steps/final_output/converged） |
| D4 | 运行生命周期接入 tasks_runs：桌面壳新增命令 `run_team_on_task(task_id, team_id)`——创建 Run 行（queued→running 先落 state_changed 事件铁律，复用 `commands.rs::transition_run` 模式）后由受监督后台任务执行 TeamRunner，终态 succeeded/failed 落库；取消经 CancellationToken（cancelled 终态同样先落库）；看板卡片 ⋯ 菜单提供「用团队运行」（等价键盘可达操作）；Run 详情抽屉复用现有实时刷新 |
| D5 | 会话级入口同时提供：`run_team(session_id, team_id, task)` 直接执行（Trace 群聊时间线 U12 已就绪消费 turn/whiteboard 事件），REPL/无任务场景可用；此路径不落 tasks_runs 行，结果经返回值与会话事件呈现 |
| D6 | IPC 四步契约链（ipc-contract 规则）：roles CRUD（list_roles/upsert_role/delete_role）、teams CRUD（list_teams/upsert_team/delete_team）、run_team_on_task、run_team、list_whiteboard_notes(sessionId)；DTO camelCase、内部标记枚举、稳定错误 code（`"role.not_found"` / `"team.not_found"` / `"team.member_missing"` / `"team.invalid_config"` / …）；同提交再生 bindings 并更新前端消费端与 test-double |
| D7 | UI：Settings 新增 Roles/Teams 管理段（Role 表单绑定 provider 或 CLI Agent profile 下拉；Team 表单选 topology + 按序勾选成员）；看板卡片菜单「团队运行」弹团队选择；全部新增文案 i18n zh-CN/en 双语；组件测试走 test-double |
| D8 | 收敛：`store/repos/providers.rs` 中遗留的 insert_role/get_role/insert_team/get_team 半成品函数并入新仓储 repos::roles / repos::teams（补全 list/upsert/delete）或删除，调用方同步迁移——避免双套约定（绿地纪律） |

## 1. 目标 / Goals

1. **配置即运行时**：Settings 配置的 Provider/Role/Team 与 CLI Agent profile 在一次团队运行中按需物化为真实客户端（协议客户端 + CliAgentClient），经 ProviderResolver 注入既有三拓扑执行器——编排器与解析层零改动。
2. **团队运行接入产品表面**：看板「用团队运行」携带完整 Run 生命周期（state_changed 铁律、终态落库、可取消）；会话级直接执行让群聊 turn/WhiteBoard 事件实时进入 U12 Trace 时间线。
3. **Roles/Teams 可管理**：CRUD + 绑定下拉（provider 或 CLI Agent profile）经 Settings 完成，IPC 契约四处一致，i18n 双语。

## 2. 用户故事 / User Stories

- **US1** 开发者在 Settings 建 3 个 Role（分别绑不同 provider 或 CLI Agent profile），组成 group_chat Team；在会话发起群聊任务，Trace 时间线实时看到各成员发言、handoff 链与 WhiteBoard 笔记流，直到收敛产出结论。
- **US2** 开发者在看板给某任务卡点 ⋯ 菜单选「用团队运行」并挑一个 pipeline Team；Run 行按 queued→running→succeeded 流转且每次迁移先落 state_changed 事件，Run 抽屉实时刷新；中途取消则 cancelled 终态落库。
- **US3** 开发者配置 router Team 并在 team.config 声明 `required` 能力数组；无成员匹配时得到明确错误（NoMatchingAgent → failed 终态 + 错误提示），而非静默失败或挂起。

## 3. 验收标准 / Acceptance Criteria

**内核 services/team_runner**
- [ ] AC1 注册表物化单测（MemorySecretStore + fake CLI fixture，零网络）：三类客户端按 DB 配置正确物化并注册进 ProviderResolver；keyring_ref 缺失且无环境变量的 provider 被跳过并告警、不失败整场；CliAgentClient 白名单 = profile command basename（篡改/换命令执行被拒）；is_master 成为默认 provider；params_json.agent_profile_id 绑定优先生效。
- [ ] AC2 TeamRunner 三拓扑集成测试各一（FakeLlm + fixtures/fake_cli.js）：pipeline 成员输出接力进下一站；router 无匹配报 NoMatchingAgent 明确错误；group_chat 收敛结束且 TeamRunOutcome（transcript/steps/final_output/converged）正确、WhiteBoard 落库并镜像 `session.whiteboard` 总线事件。
- [ ] AC3 缺失成员角色错误路径：team.member_role_ids 引用不存在 role 时返回 OrchestratorError::MemberNotFound（映射 `"team.member_missing"`），调用方拿到明确错误而非 panic/挂起。

**运行生命周期**
- [ ] AC4 run_team_on_task 的 Run 行生命周期符合铁律：queued→running 先落 state_changed 事件再产生外部副作用；终态 succeeded/failed 落库；存在取消路径（CancellationToken，cancelled 终态同样先落库）；后台任务受监督（执行 panic/join 失败时 Run 不悬挂于 running，兜底转 failed 并告警）。

**IPC 契约链**
- [ ] AC5 IPC 四处一致：list_roles/upsert_role/delete_role、list_teams/upsert_team/delete_team、run_team_on_task、run_team、list_whiteboard_notes 共 9 个命令全部完成 Rust handler + specta builder 注册 + bindings.gen.ts 再生 + 前端消费端与 test-double 同提交更新；DTO camelCase、内部标记枚举、稳定错误 code。

**前端**
- [ ] AC6 Settings「Roles」「Teams」管理段 + 看板卡片「用团队运行」菜单（含等价键盘可达操作）组件测试通过（mock test-double）；全部新增文案 i18n 双语。

**质量门**
- [ ] AC7 双端质量门全绿：`cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test` 与 `pnpm typecheck && pnpm lint && pnpm test`。

## 4. 非目标 / Non-goals

自发组队 auto-forming team（pr §6「支持自动创建Agent群里」，后续里程碑）；Telemetry 上报、飞书 Bot、QQ Bot；抢占式模型等新 Multi-Agent 编排机制（pr §2 列举项，现有三拓扑之外不加新机制）；sqlite-vec 向量检索；Approvals/Scheduler 与团队运行的联动；跨会话 Memory 自动注入群聊上下文。

## 5. 技术约束 / Technical Constraints

- 锁定栈不变：Rust(edition 2021) + tokio + SQLite(rusqlite+WAL) + Tauri 2 + React18/TS strict。遵守 `.opencode/rules/rust-core.md`（SQLite 操作 spawn_blocking 包裹、每模块 thiserror 枚举、库路径零 unwrap/expect、参数绑定 SQL）与 `.opencode/rules/ipc-contract.md`（camelCase DTO、内部标记枚举、稳定 code、四步契约链）。
- 迁移只增不改：本里程碑**零新 migration**（roles/teams/provider_configs/agent_profiles/tasks_runs/whiteboard_notes 均已就绪）；Role↔AgentProfile 绑定走 roles.params_json 约定键 `agent_profile_id`。
- 安全铁律沿用 M-CLI1 D6：子进程 arg 数组传递、白名单校验、`.kill_on_drop(true)`、stderr 截断进错误消息；密钥只走 SecretStore/keyring，tracing INFO 以上无密钥与完整 prompt 正文；环境变量兜底命名约定实现期固化于代码注释。
- 事件通道遵循 ADR-0002 命名空间：群聊增量走会话通道（turn 事件）、WhiteBoard 镜像 `session.whiteboard`、Run 结构迁移走 `event://domain`；每条事件先落 EventRecord 再 emit。
- 测试纪律：单测零网络（FakeLlm/fake_cli.js/MemorySecretStore）；repo 测试用临时目录 SQLite；domain/orchestrator 行覆盖 ≥80% 不回退；修 bug 先写失败测试。
- `TeamRunInput.model` 自所选 provider 的 params_json `model` 键读取（CLI Agent 成员忽略该字段）。

## 6. 任务拆分 / Task Breakdown

| # | 任务 | 内容 | 前置 |
|---|---|---|---|
| T1 | 数据层收敛 | providers.rs 中 role/team 半成品函数迁出为新 `repos::{roles,teams}.rs` 并补全 list/upsert/delete（D8）；repo 单测（tempfile SQLite，happy + 冲突路径） | 0001_init.sql roles/teams 表 ✅ 已交付（K2/K6） |
| T2 | TeamRunner 服务 | `services/team_runner.rs`：Provider 物化注册表 + `TeamRunner::run` 三拓扑分派 + TeamRunOutcome；单测覆盖 keyring_ref 缺失跳过、basename 白名单、master 默认、LlmSelector→RoundRobin 降级 | T1 |
| T3 | 集成测试 | 三拓扑端到端（FakeLlm/fake_cli.js，零网络）+ MemberNotFound/NoMatchingAgent 错误路径 + whiteboard 落库与总线镜像断言 | T2 |
| T4 | src-tauri 接入 | 10 个命令 impl + DTO + builder 注册 + contracts:gen；run_team_on_task 的 Run 生命周期（transition_run 复用 + 受监督后台任务 + CancellationToken 取消）+ run_team + list_whiteboard_notes；集成测试 | T2 |
| T5 | 前端 | Settings Roles/Teams 管理段（provider/profile 绑定下拉、topology + 成员顺序）、看板「团队运行」菜单、test-double 更新、i18n 双语、组件测试 | T4 |
| T6 | 收尾验收 | 全量质量门复核（双端）+ AC 清单逐项核验报告 | T1–T5 |
