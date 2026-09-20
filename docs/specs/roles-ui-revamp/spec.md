# SPEC: Nuomi Roles 设置页 UI 改造（roles-ui-revamp）

> 状态：待批准
> 前置：`docs/specs/cli-agents-m1.md`（M-CLI1 ✅）、`docs/specs/role-agents-ui/spec.md`（Part 1 ✅）、`docs/specs/harness-kernel-v1.md`（K6 ✅）。
> 需求来源：用户重新设计 Roles 设置页 UI（三区块解耦：新增Role → 绑定Role → 可用Role Agent），并要求 CLI Agent 增加 ModelId、Provider 支持 per-ModelId 超参、Settings tab 精简为「自进化」；自进化设计参考 PrimeAgent Continual Harness、Hermes Agent 自动技能创建、RSI 递归自改进理念。
> 需求权威：`pr.md` §2（Agent 方面：支持同一 Provider/CLI 配置不同 Role）、§3（架构：Self-Evolution 基于 GEPA + Long-term Memory）、§5（UI）。
> 事实基础：`RolesSection.tsx`（308行，标题栏按钮组+RoleForm+分组列表+RoleDirectorDialog+RoleBindingPanel）；`RoleForm.tsx`（197行，含 bindingMode 下拉+Provider/CLI 选择）；`RoleBindingPanel.tsx`（277行，全字段模态框）；`CliAgentForm.tsx`（177行，无 ModelId 字段）；`ProviderForm.tsx`（699行，温度/maxTokens 在 Provider 级"高级"区）；`SettingsView.tsx`（86行，access tab = SensitiveToolsEditor + OnlineAuthToggle）；`AgentProfileDto`（无 modelId）；`ModelEntryDto = { id, capabilities }`（无 per-model 超参）。

## 0. 问题诊断 / Problem Diagnosis

### 0.1 Roles 页职责耦合，绑定入口隐蔽

现有 `RolesSection` 将"新增 Role"（`RoleForm`，含绑定方式下拉+Provider/CLI 选择）与"角色列表"（按就绪/未绑定分组）混在一个滚动区。用户配完 Provider/CLI Agent 后，无显眼引导完成绑定；`RoleForm` 的 bindingMode 下拉藏在新建表单内部，已存在 Role 的重新绑定需点击列表行按钮打开模态框——配置流断裂。

### 0.2 CLI Agent 缺少 ModelId，无法精确绑定

`AgentProfileDto`（`bindings.gen.ts:815`）无 modelId 字段。CLI Agent（如 claude_code）支持通过 `--model` 参数指定模型，但当前配置无法在 UI 上显式声明该 Agent 使用哪个 ModelId，导致区块②绑定下拉无法以 `CLIAgent/ModelId` 粒度展示可选项，用户无法区分同一 CLI Agent 的不同模型实例。

### 0.3 Provider 超参为 Provider 级，无法 per-ModelId 配置

`ProviderSettingsDto`（`bindings.gen.ts:968-977`）的 `temperature` / `topP` / `maxTokens` 在 Provider 顶层，同一 Provider 下所有 ModelId 共享一套超参。实际场景中同一 Provider 的不同模型（如 deepseek-chat vs deepseek-reasoner）需要不同温度和 maxTokens，当前 UI（`ProviderForm.tsx:558-633` 高级折叠区）无法满足。

### 0.4 Settings「工具与授权」tab 价值低，自进化能力未充分暴露

`SettingsView.tsx:41-47` 的 access tab 渲染 `SensitiveToolsEditor`（敏感工具审批白名单）+ `OnlineAuthToggle`（自进化在线学习授权）。敏感工具白名单配置低频且概念晦涩，与自进化授权混在一个 tab 内信息架构不清。同时，`OnlineAuthToggle` 仅暴露在线学习授权开关，nuomi 的 Self-Evolution 引擎（GEPA 式反思进化 + 白名单联网学习 + 长期记忆，`crates/nuomi-core/src/evolution/`）的丰富配置能力未在 UI 暴露——用户无法配置精炼触发条件、自动技能创建策略、持久记忆策略等关键参数。

## 1. 决策记录摘要 / Decisions

| # | 决策 |
|---|---|
| D1 | **Roles 页三区块解耦**：区块①「新增Role」只定义角色模板（名称+能力+SystemPrompt），移除绑定方式下拉与 Provider/CLI 选择；区块②「绑定Role」为新增的内联快速绑定表单（Role Agent 名称+Role 下拉+Provider/CLI 混合下拉+能力勾选+工具白名单 tag 输入）；区块③「可用Role Agent」为平铺卡片列表（取消就绪/未绑定分组）。 |
| D2 | **区块①按钮组**：从 `RolesSection` 标题栏迁移到 `RoleForm` 内右上角；改为「预定义角色」「角色导演」2 个按钮；移除「恢复预置」按钮。「预定义角色」按钮弹出列表展示预定义角色+自定义角色供选用。 |
| D3 | **区块②混合下拉**：Provider/CLI Agent 下拉为单一混合下拉，用 optgroup 分两组——「Provider/ModelId」列出每个已配置 Provider 的每个 ModelId（`provider.name/model.id`），「CLI Agent/ModelId」列出每个已配置 CLI Agent 的 ModelId（`agent.name/agent.modelId`）。选中后写入 Role 的 `providerId` 或 `params.agent_profile_id`。 |
| D4 | **CLI Agent 增加 ModelId**：`AgentProfileDto` / `AgentProfileInput` 增加 `modelId: string \| null` 字段（填空，可选）；`CliAgentForm.tsx` 增加 ModelId 输入框；后端 `AgentProfile` 实体 + 新 migration（append-only 编号）。 |
| D5 | **Provider per-ModelId 超参**：`ModelEntryDto` 扩展为 `{ id, capabilities, temperature?, maxTokens?, topP? }`；`ProviderForm.tsx` 的温度/maxTokens/topP 从 Provider 级"高级"区移到每个 ModelId 行内，可独立配置；Provider 顶层保留 `timeoutSecs` / `retry` / `maxConcurrency` / `priority` 等连接级参数。后端 `ModelEntry` 实体变更 + settings JSON 兼容迁移（旧数据顶层 temperature/maxTokens 回填到各 model）。 |
| D6 | **区块③卡片改造**：取消「已就绪/未绑定」分组，改为平铺列表；每个卡片展示「role [名称]」标签 + 能力徽章 + 内置/生成徽章 + SystemPromptOverride 描述（截断 2 行）+ 绑定信息（`Provider/CLI Agent: name/modelId`）+ [修改] + [删除]（内置角色除外）。[修改] 打开既有 `RoleBindingPanel` 模态框做全字段编辑。 |
| D7 | **Settings tab 精简为「自进化」**：移除 access tab 的 `SensitiveToolsEditor`；access tab 改名「自进化」（labelKey: `settings.tab.evolution`），渲染全新设计的 `EvolutionSettingsPanel` 组件替代 `OnlineAuthToggle`。敏感工具白名单配置移除（审批门改用默认名单，后续可按需恢复）。 |
| D8 | **RoleBindingPanel 保留**：作为区块③[修改]按钮的全字段编辑入口（绑定目标+超参+工具白名单+能力+SystemPrompt），不再从区块①或区块②触发。 |
| D9 | **数据模型变更范围**：D4 需新 migration（agent_profiles 增加 model_id 列）；D5 需 settings JSON 兼容迁移逻辑（无新表，ProviderSettings JSON 结构变更，旧数据自动回填）；Roles 表 schema 不变（复用 provider_id / params.agent_profile_id 约定键）。 |
| D10 | **i18n + 设计系统对齐**：新增 i18n key 遵循 `settings.roles.*` / `settings.cliAgents.*` / `settings.evolution.*` 既有命名空间；视觉对齐既有 `sketch-card` / `pixel-fill-accent` / `bg-surface-raised` token，不引入新设计语言。 |
| D11 | **Role Agent 概念与区块②名称语义**：**Role 是纯模板**（SystemPrompt + 能力声明），在区块①创建；**Role Agent 是 Role 绑定了 Provider/model 或 CLI Agent/model 之后真正具有模型推理能力的实例化结果**，在区块②创建。区块②的「名称」是 Role Agent 实例的唯一键，**独立输入，不随 Role 下拉自动填充**。数据模型层面：Role Agent 仍存储在 `roles` 表中，`name` 为唯一键（`upsertRole` 幂等），包含 `providerId` / `params.agent_profile_id` 绑定信息；Role 下拉为可选继承源——选中则继承该 Role 的 SystemPrompt/能力作为初始值，用户可在区块②覆盖。 |
| D12 | **区块②工具白名单 tag 输入 + 补全**：工具白名单采用 tag 输入组件（非逗号分隔文本），支持已有工具列表的补全下拉（多选 tag 化展示），类似 IDE 的 tag input + autocomplete 模式。补全数据源为后端注册的全部工具名列表。 |
| D13 | **Provider per-ModelId 超参 UI 分层展示**：每个 model 行采用分层展示——**内联始终可见**：能力勾选（Reasoning/Image/Voice/Video）；**点击展开折叠区**：温度 / topP / maxTokens 等超参（高级配置，默认隐藏）；**自动推理参数推荐**：提供「自动推荐」按钮，根据任务类型/模型能力自动推荐合适的超参组合（参考 Hermes Agent 的自动推理参数能力），用户可一键应用或手动调整。 |
| D14 | **「自进化」tab 设计（基于 PrimeAgent/Hermes/RSI）**：新建 `EvolutionSettingsPanel` 组件，围绕 PrimeAgent Continual Harness H=(ρ,G,K,M) 模型设计四个配置维度：(1) **白名单联网学习源管理**——配置允许 Agent 联网学习的域名/来源白名单；(2) **GEPA 式反思进化参数**——暴露 `/refine` 类参数（精炼触发条件、最小编辑策略、证据支持阈值、回滚策略）；(3) **自动技能创建**——类似 Hermes 的自动技能创建开关，Agent 解决难题后自动写入可复用技能文档；(4) **持久记忆策略**——跨会话记忆的保留期、检索策略配置。既有 `OnlineAuthToggle` 的在线学习授权开关作为维度(1)的子项保留。 |

## 2. 目标 / Goals

1. **三区块解耦**：Roles 页拆为「新增Role（定义模板）→ 绑定Role（创建 Role Agent 实例）→ 可用Role Agent（实例列表）」三段，职责清晰，配置流连贯。
2. **CLI Agent 支持 ModelId**：用户可在 CLI Agent 配置中声明 ModelId，绑定下拉以 `CLIAgent/ModelId` 粒度展示。
3. **Provider per-ModelId 超参**：同一 Provider 的不同 ModelId 可独立配置温度/maxTokens/topP，支持自动推荐。
4. **自进化 tab 全面设计**：围绕 Continual Harness H=(ρ,G,K,M) 模型，暴露联网学习源、反思进化参数、自动技能创建、持久记忆策略四个维度的配置，超越简单开关。
5. **不回归**：既有 Role 数据、builtin Role 保护、Team 编排消费 Role 的行为零变更；RoleBindingPanel 全字段编辑能力保留。

## 3. 用户故事 / User Stories

- **US1** 用户在 Roles tab 区块①填写名称「翻译员」+ 勾选「推理」+ 填写 SystemPrompt，点保存——区块③立即出现「role 翻译员」卡片（未绑定状态）。
- **US2** 用户在区块②输入 Role Agent 名称「翻译员-DeepSeek」+ 从 Role 下拉选「翻译员」（继承 SystemPrompt/能力）+ 从混合下拉选「DeepSeek/deepseek-chat」+ 勾选能力 + 用 tag 输入添加工具白名单（`read_file` `write_file`），点保存——区块③出现「role 翻译员-DeepSeek」卡片，绑定信息为「Provider/CLI Agent: DeepSeek/deepseek-chat」。
- **US3** 用户在区块③点「翻译员-DeepSeek」卡片的 [修改] 按钮，弹出 RoleBindingPanel 模态框，预填当前绑定与全部 overlay 字段（SystemPrompt/温度/maxTokens/工具白名单/能力），修改后保存——卡片信息更新。
- **US4** 用户在 CLI Agents tab 新建一个 claude_code Agent，填写名称「CodeBuddy」+ 命令 + ModelId「sonnet」，保存——区块②混合下拉的「CLI Agent/ModelId」组出现「CodeBuddy/sonnet」选项。
- **US5** 用户在 Providers tab 编辑 DeepSeek Provider，为 deepseek-chat 设温度 0.7 / maxTokens 8192，为 deepseek-reasoner 设温度 0.0 / maxTokens 16384，保存——两个模型各自携带独立超参。
- **US6** 用户点区块①「预定义角色」按钮，弹出列表展示所有预定义角色和自定义角色，选用一个预定义角色后其配置填入区块①表单。
- **US7** 用户在 Settings 看到 tab 导航为「模型提供方 | Roles | Teams | CLI Agents | 集成 | 自进化」，点「自进化」看到四个配置维度：联网学习源白名单、反思进化参数、自动技能创建开关、持久记忆策略。
- **US8** 用户在「自进化」tab 的「联网学习源白名单」区添加 `cordis.moe` 和 `github.com`，开启在线学习授权——Agent 在自进化过程中可从这些域名获取实现案例。
- **US9** 用户在「自进化」tab 的「反思进化参数」区配置：精炼触发条件为「连续 3 次失败」、最小编辑策略为「prompt note」、证据支持阈值为 0.8、启用回滚——`/refine` 管道按此参数运行。
- **US10** 用户在「自进化」tab 开启「自动技能创建」开关——Agent 解决难题后自动写入可复用技能文档（SKILL.md 格式），后续遇到类似问题可直接复用。
- **US11** 用户在「自进化」tab 的「持久记忆策略」区配置：记忆保留期 90 天、检索策略为「关键词+语义混合」——跨会话记忆按此策略存储与检索。
- **US12** 用户在 Providers tab 的某个 model 行点「自动推荐」按钮——系统根据该模型的能力标签（如 reasoning）自动推荐温度 0.7 / topP 0.95 / maxTokens 8192，用户一键应用后可手动微调。

## 4. 验收标准 / Acceptance Criteria

**Part 1：Roles 页三区块 UI 重构（纯前端）**

- [ ] AC1 区块①`RoleForm` 移除 `bindingMode` 下拉与 Provider/CLI 选择框，只保留名称+所需能力+SystemPrompt+保存；按钮组（预定义角色、角色导演）位于表单内右上角；无「恢复预置」按钮。
- [ ] AC2 「预定义角色」按钮点击弹出列表（模态框或 Popover），展示所有 builtin Role（预定义）与非 builtin Role（自定义），选用后将其 name/systemPromptOverride/requiredCapabilities 填入区块①表单。
- [ ] AC3 新增区块②`RoleQuickBinding` 组件：**名称独立输入**（Role Agent 实例唯一键，不随 Role 下拉自动填充）+ Role 下拉（可选继承源，选中后继承 SystemPrompt/能力作为初始值）+ Provider/CLI 混合下拉 + 所需能力勾选 + **工具白名单 tag 输入组件**（支持补全下拉，多选 tag 化展示）+ 保存按钮；混合下拉用 optgroup 分「Provider/ModelId」与「CLI Agent/ModelId」两组，选中后写入 `providerId` 或 `params.agent_profile_id`。
- [ ] AC4 区块③`RolesSection` 取消「已就绪/未绑定」分组，改为平铺卡片列表；每个卡片展示「role [name]」标签 + 能力徽章（Re/I/Vo/Vi）+ 内置/生成徽章 + SystemPromptOverride 截断 2 行 + 绑定信息（`Provider/CLI Agent: name/modelId`）+ [修改] + [删除]（builtin 角色无删除）。
- [ ] AC5 区块③ [修改] 按钮点击打开 `RoleBindingPanel` 模态框，预填该 Role 全部字段（名称/绑定方式/Provider或CLI/能力/温度/maxTokens/工具白名单/SystemPrompt），保存后列表刷新。
- [ ] AC6 `RoleBindingPanel` 保留既有全字段编辑能力不变；温度/maxTokens 字段保留在模态框内作为 Role 级覆盖（运行时 Role overlay 优先于 Provider per-ModelId 超参）。
- [ ] AC7 工具白名单 tag 输入组件：支持从后端注册的全部工具名列表补全下拉；用户输入时模糊匹配展示候选；选中后以 tag 形式展示，可逐个删除；空列表语义为「不限制（全部工具可用）」。

**Part 2：CLI Agent ModelId（前后端）**

- [ ] AC8 `CliAgentForm` 增加 ModelId 输入框（填空，可选），保存时写入 `AgentProfileInput.modelId`；`AgentProfileDto` 携带 `modelId: string | null`。
- [ ] AC9 后端 `AgentProfile` 实体增加 `model_id` 字段 + append-only migration（新编号，增加列）；既有数据 model_id 默认 null；`upsertAgentProfile` 持久化 model_id。
- [ ] AC10 区块②混合下拉的「CLI Agent/ModelId」组：对每个已配置 CLI Agent，若 `modelId` 非空则展示 `agent.name/agent.modelId`，若为空则展示 `agent.name`（无 ModelId 后缀）。

**Part 3：Provider per-ModelId 超参（前后端）**

- [ ] AC11 `ModelEntryDto` 扩展为 `{ id: string; capabilities: CapabilityDto[]; temperature?: number | null; maxTokens?: number | null; topP?: number | null }`。
- [ ] AC12 `ProviderForm` 每个 model 行采用分层展示：**内联始终可见**——能力勾选（Re/I/Vo/Vi）；**折叠区（点击展开）**——温度 / topP / maxTokens 超参输入；**「自动推荐」按钮**——根据模型能力标签自动推荐超参组合，一键应用后可手动微调。
- [ ] AC13 后端 `ModelEntry` 实体变更；ProviderSettings JSON 兼容迁移：旧数据顶层 `temperature`/`maxTokens`/`topP` 自动回填到各 model 的同名字段（若 model 未独立设值）；新数据不再写 Provider 顶层超参（保留字段做兼容读取）。
- [ ] AC14 Provider 顶层保留 `timeoutSecs` / `retry` / `maxConcurrency` / `priority` / `proxy` / `enabled` 等连接级参数不变。

**Part 4：自进化 tab 设计（纯前端 + IPC 契约扩展）**

- [ ] AC15 `SettingsView` 的 TABS 移除 access tab 的 `SensitiveToolsEditor`；access tab 改名「自进化」（labelKey: `settings.tab.evolution`），渲染新建的 `EvolutionSettingsPanel` 组件。
- [ ] AC16 `EvolutionSettingsPanel` 包含四个配置维度卡片：(1) **联网学习源白名单**——tag 输入管理允许联网学习的域名/来源，包含既有 `OnlineAuthToggle` 的在线学习授权开关作为总开关；(2) **反思进化参数**——精炼触发条件（连续失败次数/手动触发）、最小编辑策略（prompt note / memory / skill / sub-agent spec）、证据支持阈值（0-1 滑块）、回滚开关；(3) **自动技能创建**——开关 + 技能文档格式选择（SKILL.md 兼容 agentskills.io 标准）；(4) **持久记忆策略**——保留期天数 + 检索策略选择（关键词/语义/混合）。
- [ ] AC17 自进化配置持久化：新增 IPC 命令 `getEvolutionSettings` / `setEvolutionSettings`，后端存储于 settings 表（JSON）；既有 `OnlineAuthToggle` 的 `getOnlineAuth` / `setOnlineAuth` 逻辑作为维度(1)子项兼容保留。
- [ ] AC18 自进化配置消费：`crates/nuomi-core/src/evolution/` 引擎读取上述配置——联网学习受白名单约束、`/refine` 管道按反思进化参数运行、自动技能创建按开关决定是否写入 SKILL.md、长期记忆按持久记忆策略管理。

**横切**

- [ ] AC19 既有 Role 数据零迁移：builtin Role 保护不变、Team 编排消费 Role 行为不变、`upsertRole` 幂等性（name 为 key）不变。
- [ ] AC20 IPC 契约纪律：D4/D5/AC17 涉及 DTO 变更，遵守契约先行——同一提交内 Rust 类型变更 + 重新生成 bindings + 更新消费端；`pnpm contracts:gen`。
- [ ] AC21 i18n 全 key 覆盖（中英双语对齐既有 `settings.roles.*` / `settings.evolution.*` 命名空间），无硬编码字符串。
- [ ] AC22 质量门全绿：`cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test` + `pnpm typecheck && pnpm lint && pnpm test`。

## 5. 非目标 / Non-goals

引入"1 Role : N 绑定"新数据模型（同一 Role 派生多个 Role Agent 实例）——保持 1 Role : 1 绑定（name 为唯一键）；新增 Multi-Agent 编排机制；Telemetry / 飞书 / QQBot 增强；改 `team_runner::materialize` 物化逻辑；改 `roles` 表 schema；Monaco 体验；自发组队 dry-run 预览；敏感工具白名单恢复（本期移除，后续按需评估）；实现 PrimeAgent 的完整 Continual Harness CRUD（本期仅暴露配置 UI，CRUD 引擎能力为后续里程碑）；实现 Hermes 的完整 MLOps/训练管线（本期仅借鉴自动技能创建理念）。

## 6. 技术约束 / Technical Constraints

- 锁定栈不变：Tauri 2 + React18 + TS strict + Vite + Zustand + TanStack Query + Tailwind + Vitest；IPC 类型只从 `bindings.gen.ts` 导入。
- 前端不直接触盘/进程/SQL——一切经 IPC 契约。
- **Migration 只增不改**：D4 新 migration 走新编号（`NNN__agent_profile_model_id.sql`）；D5 ProviderSettings 为 JSON 存储，兼容迁移在实体反序列化层处理（无新 migration 文件），但必须保证旧数据可读；AC17 自进化配置存储复用 settings 表 JSON（无新表）。
- **契约先行**：D4/D5/AC17 涉及 `AgentProfileDto` / `AgentProfileInput` / `ModelEntryDto` / `EvolutionSettingsDto` 变更，同一提交内 Rust 类型 + bindings + 消费端同步更新。
- **密钥安全**：API key 只经 OS keyring；modelId / 自进化配置非敏感数据可明文存储。
- 测试纪律：前端组件测试用 vitest + @testing-library（既有模式）；后端单测用 cargo test；零外网。
- **自动推荐超参**：AC12 的「自动推荐」基于模型能力标签的静态规则映射（如 reasoning → temperature 0.7），本期不引入 LLM 调用做推荐——保持零额外 token 消耗。

## 7. 实施计划 / Implementation Plan

| 阶段 | 优先级 | 内容 | 前置 | 验收 |
|---|---|---|---|---|
| Part 1 | 高 | Roles 页三区块 UI 重构：`RoleForm` 精简 + `RoleQuickBinding` 新建（含 tag 工具白名单 + 补全）+ `RolesSection` 平铺卡片 + 按钮组迁移 + 预定义角色列表 + i18n | 无 | AC1-AC7, AC19, AC21 |
| Part 2 | 高 | CLI Agent ModelId：后端实体+migration+IPC + `CliAgentForm` 增字段 + bindings 再生成 + 区块②下拉展示 | 无 | AC8-AC10, AC20, AC22 |
| Part 3 | 高 | Provider per-ModelId 超参：后端 ModelEntry 扩展+兼容迁移+IPC + `ProviderForm` 超参分层展示+自动推荐 + bindings 再生成 | 无 | AC11-AC14, AC20, AC22 |
| Part 4 | 中 | 自进化 tab：`EvolutionSettingsPanel` 四维度设计 + IPC `get/setEvolutionSettings` + 后端 evolution 引擎消费配置 + i18n | 无 | AC15-AC18, AC21 |
| 收尾 | — | 全量质量门复核 + AC 清单逐项核验报告 | Part 1-4 | AC22 |

## 8. 风险与回滚 / Risks & Rollback

- **风险**：D5 ProviderSettings JSON 结构变更，旧数据顶层 temperature/maxTokens 需回填到各 model。**缓解**：实体反序列化层做兼容——若 model 未独立设值且顶层有值，则回填；新数据双写过渡期后清理顶层。回滚：恢复旧 ProviderSettings 结构即可，JSON 原始数据未丢失。
- **风险**：D7 移除 SensitiveToolsEditor 后，审批门失去可配置名单，全部敏感工具走默认名单。**缓解**：后端默认名单保留（`ui-m1.md` D8 默认含文件写入/shell 写/git 写/联网 fetch）；本期非目标为恢复，后续可按需在「自进化」tab 或独立入口恢复配置。
- **风险**：Part 2/3 涉及后端实体变更 + migration，若 migration 编号冲突或回滚不干净可能影响存量数据。**缓解**：migration 只增不改、append-only；model_id 默认 null 不破坏既有数据；ProviderSettings 兼容迁移为纯反序列化逻辑无 schema 变更。
- **风险**：D14 自进化 tab 暴露丰富配置后，用户可能配出冲突参数（如精炼触发条件过激进 + 证据阈值过低导致频繁无效精炼）。**缓解**：UI 提供参数合理性提示；后端 `/refine` 管道对不合理组合做兜底校验（如证据阈值最低 0.5）。
- **风险**：AC12 自动推荐超参基于静态规则，可能不适用于所有模型。**缓解**：推荐仅为初始值建议，用户可完全手动覆盖；UI 明确标注「推荐值，请根据实际效果调整」。
- **回滚边界**：Part 1 纯前端，回滚至既有 `RoleForm` + `RolesSection` 即可。Part 2 回滚需 down migration（删 model_id 列）。Part 3 回滚为前端恢复 + 后端恢复旧 ProviderSettings 读取。Part 4 纯前端 + IPC 新增命令，回滚移除 `EvolutionSettingsPanel` 恢复 `OnlineAuthToggle` 即可，无数据影响。

## 9. 参考项目与设计借鉴 / References & Design Inspiration

### 9.1 PrimeAgent (PrimeIntellect) — Continual Harness + RLM

- **核心理念**："当模型能力逼近天花板，'模型怎么工作'比'模型有多强'更重要。"
- **Continual Harness H=(ρ,G,K,M)**：将 harness 自己的状态（prompts ρ、sub-agents G、skills K、memory M）抽象为 Agent 可 CRUD 的东西，从 Agent 自己的轨迹在线精炼而不重置。
- **`/refine` 自改进管道**：读取 Agent 轨迹 → 应用最小相关 CRUD 编辑 → 改进是证据支持的而非任意的 → 精炼两阶段（Planning 后台不阻塞 + Applying 在 turn 边界短暂阻塞）→ 支持精炼历史回滚。
- **对 nuomi 的启发**：D14「自进化」tab 的「反思进化参数」维度直接借鉴 `/refine` 的触发条件/最小编辑策略/证据支持阈值/回滚策略四参数；Continual Harness H=(ρ,G,K,M) 模型作为自进化配置的四维度信息架构骨架。

### 9.2 RSI (Recursive Self-Improvement) — 递归自改进

- **核心理念**：AI 系统能够递归地改进自身，每一次改进都基于前一次的成果，形成正向飞轮。
- **对 nuomi 的启发**：nuomi 的 Self-Evolution（GEPA 式反思进化 + 白名单联网学习）天然契合 RSI 理念——从轨迹中学习 → 最小编辑 → 证据支持 → 在线精炼 → 递归飞轮。D14「自进化」tab 是将这一理念从后端引擎暴露到用户可配置 UI 的第一步。

### 9.3 Hermes Agent (Nous Research) — 长期在线数字员工

- **核心理念**："让 AI 成为长期在线的数字员工，而非一次性聊天机器人。运行越久越聪明。"
- **自动技能创建**：解决难题后写下可复用技能文档（SKILL.md 格式，兼容 agentskills.io 开放标准），永不忘记解决方法。
- **持久记忆**：跨会话记住偏好、项目和环境，运行越久越了解你。
- **对 nuomi 的启发**：D14「自进化」tab 的「自动技能创建」维度直接借鉴 Hermes 的自动技能创建理念（SKILL.md 格式 + agentskills.io 标准）；「持久记忆策略」维度借鉴 Hermes 的跨会话持久记忆概念；AC12 的「自动推荐超参」借鉴 Hermes 的自动推理参数能力。

### 9.4 借鉴映射总结

| nuomi 自进化 tab 维度 | 借鉴来源 | 关键理念 |
|---|---|---|
| 联网学习源白名单 | nuomi 既有 + Hermes 多平台接入 | 白名单约束联网学习范围 |
| 反思进化参数 | PrimeAgent `/refine` + RSI | 触发条件/最小编辑/证据阈值/回滚 |
| 自动技能创建 | Hermes Agent | SKILL.md + agentskills.io 标准 |
| 持久记忆策略 | Hermes Agent + nuomi Long-term Memory | 保留期/检索策略 |
| 自动推荐超参 | Hermes Agent 自动推理参数 | 静态规则映射，零 token 消耗 |
