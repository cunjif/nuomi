# SPEC: Nuomi M-FORM1 — 自发组队（auto-team-m1）

> 状态：已批准（决策 D1–D8 已定稿）
> 前置：`docs/specs/team-shell-m1.md`（**M-TEAM1 已交付**：`services/team_runner.rs` 的 materialize 物化注册表与 `run_team` 三拓扑执行、`repos::{roles,teams}` 仓储、tasks_runs 的 Run 行生命周期、看板卡片「用团队运行」入口）；间接依赖 `docs/specs/harness-kernel-v1.md`（K6 编排执行器）与 `docs/specs/cli-agents-m1.md`（M-CLI1 的 `adapters::cli` 与 `tests/fixtures/fake_cli.js`）。
> 需求来源：`pr.md` §2「支持自发 Agent Team」与 §6「支持自动创建 Agent 群里」；需求权威仍为 `pr.md`。

## 0. 决策记录摘要

| # | 决策 |
|---|---|
| D1 | 新服务 `crates/nuomi-core/src/services/team_former.rs`：`form_team(db_path, bus: Option<EventBus>, secrets, cwd, session_id: Option<&str>, task: &str, max_members: usize) -> FormedTeam { team: Team, created_role_ids: Vec<String>, rationale: String }` |
| D2 | 规划器 = 物化注册表（复用 team_runner `materialize`）的 default provider（is_master 优先）**单次** LLM 调用，输出结构化 JSON 计划；解析失败重试一次，仍失败报 `OrchestratorError::InvalidTeam("plan invalid")`；无可用 provider → 明确错误，**不静默降级** |
| D3 | 计划契约：`{"topology":"pipeline"\|"router"\|"group_chat","members":[{"kind":"role"\|"provider"\|"cli_profile","id":"...","roleName":"...","systemPrompt":"..."}],"config":{"maxRounds":6,"required":["cap"]},"rationale":"..."}`；kind=role 复用现有 Role（必须存在）；kind=provider 以 provider_id=id 新建 Role；kind=cli_profile 以 params.agent_profile_id=id 新建 Role（M-TEAM1 运行时约定键）；systemPrompt 写 system_prompt_override |
| D4 | 安全校验**先于任何落库**：成员数 ≥2 且 ≤ max_members（默认 5）；所有引用 id 预校验存在（不存在 → 错误**一次性列出全部缺失**，零部分写入）；roleName 与现有 Role 重名时自动加后缀 `name-2`/`name-3`；topology 非法值 → 计划无效 |
| D5 | 持久化：新建 Role 行 + Team 行（name=`auto-{task前12字符slug}-{短uuid}`，config 合并 maxRounds/required）；发布总线事件 topic=`team.formed` payload `{sessionId?, teamId, memberCount, rationale}`——src-tauri `events.rs` 的 `is_domain_topic` 增加 `team.` 前缀路由到 `event://domain`（加法变更 + 表驱动测试更新） |
| D6 | IPC 四步链：单命令 `form_team(task, sessionId?) -> TeamDto`（内部走 run_team 同款物化）；错误 code 三枚：`"team.no_planner"` / `"team.plan_invalid"`（含计划引用未知成员 id，details.reason 一次性列出全部缺失 id）/ `"team.form_failed"` 兜底；`"team.member_missing"` 属 upsert_team 路径、form_team 不产生；**不做 dry-run**（后续里程碑） |
| D7 | UI：看板卡片 ⋯ 菜单新增「自发组队运行」→ form_team 成功后直接以返回 teamId 调 runTeamOnTask（两步合一的菜单项，toast 展示组建结果摘要 + run id）；i18n 双语；组件测试走 test-double |
| D8 | 测试策略：规划器 FakeLlm/wiremock 回环脚本化合法与非法 JSON；端到端 form→run_team_on_task 用 CLI profile 成员（node fixture）零外网 |

## 1. 目标 / Goals

1. **一句话任务 → 可运行团队**：以物化注册表默认 provider（master 优先）单次 LLM 调用产出结构化组队计划，经安全校验后持久化为 Role/Team 行，返回的 teamId 可直接投入 run_team 执行——用户无需手工建模。
2. **安全与可预期**：校验先于任何落库（成员数边界、引用存在性、topology 合法性），零部分写入；重名自动加后缀不动既有行；一切失败给出稳定错误 code，绝不静默降级。
3. **产品表面与可观测闭环**：看板一键「自发组队运行」两步合一直达 Run；`team.formed` 域事件进 `event://domain` 可订阅；IPC 契约四处一致、i18n 双语、组件测试守护关键流。

## 2. 用户故事 / User Stories

- **US1** 开发者在看板给一张任务卡点开 ⋯ 菜单选「自发组队运行」（任务文本即输入）；系统自动规划出三成员 pipeline 团队（其中一员绑定其已启用的 CLI Agent profile），toast 展示组建摘要与新 Run id，Run 详情抽屉实时刷新直至 succeeded。
- **US2** 规划器输出了引用不存在 CLI profile 的计划——开发者立即得到列出全部缺失 id 的明确错误，数据库没有任何半成品 Role/Team；补齐 profile 后重试，新 Role 因与现有 `reviewer` 重名自动落为 `reviewer-2`，既有配置分毫未动。
- **US3** 开发者在会话中触发组队（携带 sessionId）：`event://domain` 收到 `team.formed`（teamId/memberCount/rationale），随后的群聊 turn 与 WhiteBoard 事件照常进入 Trace 时间线。

## 3. 验收标准 / Acceptance Criteria

**内核 services/team_former**
- [ ] AC1 计划解析**表驱动**单测：合法计划（pipeline/router/group_chat 三种 topology 各 ≥1 例）；坏 JSON 两分支——首次解析失败重试一次后成功、重试仍失败报 `OrchestratorError::InvalidTeam("plan invalid")`；非法 topology 值判计划无效；缺字段（members 为空/缺 roleName/config 缺失等）逐例断言。
- [ ] AC2 引用预校验**零部分写入**：kind=role/provider/cli_profile 引用不存在 id 时，错误详情一次性列出**全部**缺失 id（缺失清单断言），tempfile SQLite 断言 roles/teams 行数不变；成员数 <2 或 > max_members（默认 5）同样拒绝且零写入。
- [ ] AC3 重名自动后缀：roleName 与现有 Role 冲突时自动命名 `name-2`/`name-3`（含连续冲突场景），既有 Role 行不被修改。

**端到端**
- [ ] AC4 form→run 端到端（CLI profile 成员 + node fixture，零外网）：form_team 产出的 Team 经 run_team_on_task 跑通 queued→running→succeeded；Run 行生命周期符合铁律（state_changed 先落库再副作用）。

**事件路由**
- [ ] AC5 `team.formed` 事件经壳桥 `events.rs` 路由到 `event://domain`：`is_domain_topic` 增加 `team.` 前缀为纯加法变更，表驱动测试更新并覆盖既有前缀（task./run./approval./schedule.）回归。

**IPC 契约链**
- [ ] AC6 四处一致：`form_team(task, sessionId?) -> TeamDto` 完成 Rust handler + specta builder 注册 + `bindings.gen.ts` 再生 + 前端消费端与 test-double 同提交更新；DTO camelCase、稳定错误 code 三枚（`team.no_planner`/`team.plan_invalid`（含未知成员引用）/`team.form_failed`，`team.member_missing` 属 upsert_team 路径）齐备。

**前端**
- [ ] AC7 看板卡片 ⋯ 菜单「自发组队运行」组件测试（test-double）：成功路径两步合一——form_team 后直接以返回 teamId 调 runTeamOnTask，toast 含组建结果摘要与 run id，Run 抽屉可见；失败路径 toast 错误且保留输入；键盘可达；全部新增文案 i18n zh-CN/en 双语。

**质量门**
- [ ] AC8 双端质量门全绿：`cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test` 与 `pnpm typecheck && pnpm lint && pnpm test`。

## 4. 非目标 / Non-goals

联网检索外部优秀实践参与组队（pr §3 定期联网调研属 Evolution 白名单范畴，后续里程碑）；dry-run 组队预览（显式不做，见 D6）；抢占式模型等新 Multi-Agent 编排机制（现有三拓扑之外不加新机制）；Telemetry 上报、飞书 Bot、QQ Bot。

## 5. 技术约束 / Technical Constraints

- 锁定栈不变：Rust(edition 2021) + tokio + SQLite(rusqlite+WAL)。遵守 `.opencode/rules/rust-core.md`（SQLite 操作 spawn_blocking 包裹、每模块 thiserror 枚举、库路径零 unwrap/expect、参数绑定 SQL）与 `.opencode/rules/ipc-contract.md`（camelCase DTO、内部标记枚举、稳定 code、四步契约链）。
- 迁移只增不改：本里程碑**零新 migration**（roles/teams/provider_configs/agent_profiles/tasks_runs 均就绪）；cli_profile 绑定沿用 `roles.params_json` 约定键 `agent_profile_id`（M-TEAM1 D2b）。
- 校验时序铁律：D4 全部安全校验先于任何 INSERT；新建 Role 行与 Team 行的写入阶段任一步失败即整场失败，不得留下半成品（实现上收敛为同一 SQLite 事务）。
- 错误映射隔离：team_former 自有映射表——`InvalidTeam("plan invalid")` → `"team.plan_invalid"`（含计划引用未知成员 id，details.reason 一次性列出全部缺失 id）、无可用 provider → `"team.no_planner"`、其余失败兜底 → `"team.form_failed"`；`"team.member_missing"` 仅由 upsert_team 路径产生、form_team 不产生；**不得改动** `commands.rs::map_orchestrator_error` 对 run_team 既有 `"team.invalid_config"` 的映射。
- 规划器取钥只走 SecretStore/keyring；tracing INFO 以上无密钥与完整 prompt 正文；规划 prompt 仅携带任务文本与候选清单摘要（provider 能力标签 / role 名 / profile 名），不倾倒整库。
- 事件通道遵循 ADR-0002：`team.formed` 属全局域通道低频结构化事件，payload 中 sessionId 可选；每条事件先落 EventRecord 再 emit。
- 测试纪律：单测零外网——FakeLlm 注入为主，wiremock 回环仅限规划器脚本化 JSON 场景（D8）；CLI 成员走 `tests/fixtures/fake_cli.js`；repo 测试用临时目录 SQLite；修 bug 先写失败测试。
- 团队命名与上限：name=`auto-{task前12字符slug}-{短uuid}`；max_members 参数化、默认 5，UI 入口使用默认值。

## 6. 任务拆分 / Task Breakdown

| # | 任务 | 内容 | 前置 |
|---|---|---|---|
| F1 | 本 SPEC | 权威 SPEC 定稿（本文档，D1–D8 固化） | M-TEAM1 ✅ 已交付 |
| F2 | core former | `services/team_former.rs`：规划器单次调用（materialize default provider）+ JSON 解析重试一次 + D4 预校验（缺失清单/重名后缀/成员数边界）+ 同事务持久化 + `team.formed` 总线事件；AC1–AC3 表驱动单测 + FakeLlm/wiremock 脚本化测试 | F1 |
| F3 | IPC + 事件路由 | src-tauri `form_team` 命令 impl + DTO + builder 注册 + contracts:gen（AC6）；`events.rs::is_domain_topic` 加 `team.` 前缀 + 表驱动测试更新（AC5）；form→run_team_on_task 端到端集成测试（node fixture 零外网，AC4） | F2 |
| F4 | UI | 看板卡片 ⋯「自发组队运行」两步合一菜单项 + toast 摘要 + i18n 双语 + test-double 更新 + 组件测试（AC7） | F3 |
| F5 | 收尾验收 | 全量质量门复核（双端）+ AC 清单逐项核验报告（AC8） | F1–F4 |
