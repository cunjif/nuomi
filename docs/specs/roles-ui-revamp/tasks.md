# TASKS: Nuomi Roles 设置页 UI 改造（roles-ui-revamp）

> 对应 SPEC：`docs/specs/roles-ui-revamp/spec.md`（已批准）
> 对应 DESIGN：`docs/specs/roles-ui-revamp/design.md`（已批准）
> 验收标准：AC1-AC22（详见 spec.md §4）
> 任务规划原则：垂直切割（按业务功能分组）· 契约先行（后端 DTO + bindings 先于前端消费）· 原子性 · 有序性

## 依赖关系与优先级总览

| 任务组 | 优先级 | 前置依赖 | 验收映射 | 工作量估算 |
|--------|--------|----------|----------|-----------|
| 1. CLI Agent ModelId（Part 2） | 高 | 无 | AC8-AC10, AC20, AC22 | 1.5 人日 |
| 2. Provider per-ModelId 超参（Part 3） | 高 | 无 | AC11-AC14, AC20, AC22 | 2.5 人日 |
| 3. Roles 三区块 UI 重构（Part 1） | 高 | 无 | AC1-AC7, AC19, AC21 | 3.5 人日 |
| 4. 自进化 tab 设计（Part 4） | 中 | 任务 3.1（ToolTagInput） | AC15-AC18, AC21 | 3.5 人日 |
| 5. 集成验证与质量门（收尾） | — | 任务 1-4 全部完成 | AC19-AC22 | 0.5 人日 |

**并行策略**：任务 1/2/3 可完全并行（无交叉依赖）；任务 4 需等待任务 3.1（ToolTagInput 组件）完成后启动以复用该组件；任务 5 为最终收尾门禁。

---

## 1. CLI Agent ModelId 支持（Part 2）

> 目标：CLI Agent 配置增加 ModelId 字段，区块②混合下拉以 `CLIAgent/ModelId` 粒度展示。
> 范围：前后端 + migration + IPC 契约。
> 风险：R2（migration 编号冲突）、R9（契约先行纪律）。

### 1.1 新建 migration 0019 为 agent_profiles 增加 model_id 列
- [ ] 新建 `migrations/0019_agent_profile_model_id.sql`，内容为 `ALTER TABLE agent_profiles ADD COLUMN model_id TEXT;`（append-only，默认 null，无 NOT NULL 约束，无 CHECK 约束）；确认编号 0019 续 0018 之后无冲突

### 1.2 扩展后端 AgentProfile 实体与 repos SQL
- [ ] 在 `crates/nuomi-core/src/domain/entities.rs` 的 `AgentProfile` 结构体增加 `model_id: Option<String>` 字段（标注 `#[serde(default)]`）
- [ ] 在 `crates/nuomi-core/src/store/repos/agent_profiles.rs` 的 `insert`/`update` SQL 语句增加 `model_id` 列写入；`row_to_profile` 解析增加 `model_id: row.get(N)?`；测试 fixture 补充字段

### 1.3 扩展 DTO 与 IPC 映射并重新生成 bindings
- [ ] 在 `src-tauri/src/commands.rs` 的 `AgentProfileDto` 与 `AgentProfileInput` 增加 `model_id: Option<String>` 字段；`TryFrom<AgentProfile>` 映射新字段；`AgentProfileInput::into_entity` 映射 `model_id`；`impl_upsert_agent_profile` 更新分支增加 `prev.model_id = profile.model_id;`
- [ ] 执行 `pnpm contracts:gen` 重新生成 `src/lib/ipc/bindings.gen.ts`，确认 `AgentProfileInput`/`AgentProfileDto` 含 `modelId: string | null` 字段

### 1.4 CliAgentForm 增加 ModelId 输入框
- [ ] 在 `src/features/settings/CliAgentForm.tsx` 新增 `modelId` state（默认 `""`）+ 输入框（可选，placeholder 如 `sonnet`），置于 `command` 输入框之后；`onSubmit` 写入 `modelId: modelId.trim().length > 0 ? modelId.trim() : null`；成功后 `setModelId("")` 重置

### 1.5 补充 i18n 与测试覆盖
- [ ] 在 `src/i18n/locales/zh-CN.json` 与 `en.json` 新增 `settings.cliAgents.modelId` key（中英双语）
- [ ] 更新 `src/features/settings/CliAgentsSection.test.tsx` 适配 modelId 字段；后端 `crates/nuomi-core/src/store/repos/agent_profiles.rs` 测试增加 `model_id` roundtrip；`src-tauri/tests/cli_agents.rs` 集成测试增加 modelId 字段断言

---

## 2. Provider per-ModelId 超参配置（Part 3）

> 目标：同一 Provider 的不同 ModelId 可独立配置温度/maxTokens/topP，支持自动推荐。
> 范围：前后端 + 实体兼容迁移 + IPC 契约。
> 风险：R1（旧数据回填）、R6（自动推荐适用性）、R9（契约先行）。

### 2.1 扩展后端 ModelEntry 实体与 ProviderSettings 兼容迁移
- [ ] 在 `crates/nuomi-core/src/domain/entities.rs` 的 `ModelEntry` 结构体增加 `temperature: Option<f64>` / `top_p: Option<f64>` / `max_tokens: Option<i64>` 三个字段（均标注 `#[serde(default)]`）
- [ ] 在 `ProviderSettings::from_params` 反序列化后增加 post-processing 兼容迁移：遍历 models，若 model 的 temperature 为 None 且顶层 temperature 为 Some 则回填（top_p/max_tokens 同理）；可新增 `apply_legacy_toplevel_fallback(&mut self)` 私有方法；顶层字段保留做兼容读取源

### 2.2 扩展 ModelEntryDto 与映射并重新生成 bindings
- [ ] 在 `src-tauri/src/commands.rs` 的 `ModelEntryDto` 增加 `temperature?: number | null` / `topP?: number | null` / `maxTokens?: number | null` 三个可选字段；`ProviderSettingsDto::from_entity` 映射时 model 携带独立超参；`into_entity` 映射时 model 写独立超参且顶层 temperature/top_p/max_tokens 设为 None
- [ ] 执行 `pnpm contracts:gen` 重新生成 bindings，确认 `ModelEntryDto` 含三个可选超参字段

### 2.3 实现 recommendHyperparams 静态推荐规则函数
- [ ] 新建 `src/features/settings/recommendHyperparams.ts`，实现静态规则映射：reasoning → `{temperature:0.7, topP:0.95, maxTokens:8192}`；image → `{0.8, 1.0, 4096}`；voice/video → `{0.5, 0.9, 2048}`；多能力取 reasoning 优先；零 token 消耗
- [ ] 新建 `src/features/settings/recommendHyperparams.test.ts` 覆盖各能力组合与优先级规则

### 2.4 新建 ModelHyperParamsRow 组件实现 per-model 超参分层展示
- [ ] 新建 `src/features/settings/ModelHyperParamsRow.tsx`，接收 `model: ModelEntryDto` 与 `onChange`；内联始终可见：model id + 能力徽章（Re/I/Vo/Vi 可点击切换）+ [展开/折叠]按钮 + [删除]按钮；折叠区展开后：温度（range 0-2 step 0.1）+ topP（range 0-1 step 0.05）+ maxTokens（number）+ [自动推荐]按钮（调用 `recommendHyperparams` 一键填充）
- [ ] 新建 `src/features/settings/ModelHyperParamsRow.test.tsx` 覆盖内联展示、折叠展开、自动推荐填充、能力切换

### 2.5 重构 ProviderForm 超参分层展示
- [ ] 重构 `src/features/settings/ProviderForm.tsx`：移除 Provider 顶层 `temperature`/`topP`/`maxTokens` state；Models 区每行改用 `ModelHyperParamsRow` 组件；Advanced 折叠区仅保留 `timeoutSecs`/`retry`/`maxConcurrency`（移除三超参）；Routing 区 `priority` 保留不变；`onSubmit` 的 `settings` 对象顶层 temperature/topP/maxTokens 置 null，各 model 携带独立超参；`addModel` 默认 `{ id, capabilities: ["reasoning"], temperature: null, topP: null, maxTokens: null }`

### 2.6 补充 i18n 与测试覆盖
- [ ] 在 i18n 文件新增 `provider.modelAdvanced` / `provider.recommendHyperparams` / `provider.recommendApplied` key（中英双语）
- [ ] 更新 `src/features/settings/ProvidersSection.test.tsx` 适配 per-model 超参；后端增加 `ProviderSettings` 兼容迁移单测（旧数据顶层有值 → 回填到 model 场景）；`src-tauri/tests/settings_integration_e2e.rs` 增加 per-ModelId 超参 roundtrip 测试

---

## 3. Roles 页三区块 UI 重构（Part 1）

> 目标：Roles 页拆为「新增Role（定义模板）→ 绑定Role（创建 Role Agent 实例）→ 可用Role Agent（实例列表）」三段，职责清晰。
> 范围：纯前端（含共享基础组件 ToolTagInput，Part 4 亦复用）。
> 风险：R3（下拉数据源稀疏）、R7（工具白名单全限定名匹配）、R10（继承源覆盖用户编辑）。

### 3.1 新建 ToolTagInput 共享基础组件
- [ ] 新建 `src/features/settings/ToolTagInput.tsx`，Props 为 `{ value: string[]; onChange: (next: string[]) => void; candidates: string[]; placeholder?; emptyHintLabel? }`；实现 tag 输入 + 模糊匹配补全下拉（包含子串 + 不区分大小写，过滤已选中项，展示前 20 条）+ 键盘 ↑↓ 导航 + Enter 选中 + Backspace 空输入删末尾 tag + 每个 tag × 按钮删除 + 空列表显示「不限制」提示；无障碍属性 `role="combobox"` / `aria-expanded`
- [ ] 新建 `src/features/settings/ToolTagInput.test.tsx` 覆盖输入补全、选中追加、键盘导航、单项删除、空列表提示

### 3.2 新建 PresetRolePicker 预定义角色选择器
- [ ] 新建 `src/features/settings/PresetRolePicker.tsx`，Props 为 `{ onSelect: (role) => void; onClose: () => void }`；内部 query `ipc.listRoles`；渲染 Popover/模态框，列表分两段——builtin Role（预定义）在前，非 builtin Role（自定义）在后；点击行触发 `onSelect` 回调 `{ name, systemPromptOverride, requiredCapabilities }` + `onClose`
- [ ] 新建 `src/features/settings/PresetRolePicker.test.tsx` 覆盖列表分段展示、选中回调

### 3.3 重构 RoleForm 为纯模板表单
- [ ] 重构 `src/features/settings/RoleForm.tsx`：移除 `bindingMode`/`providerId`/`agentProfileId` state 与对应 UI（bindingMode 下拉、Provider/CLI 选择框）；保留 `name`/`systemPromptOverride`/`requiredCapabilities` 三字段；表单内右上角新增按钮组「预定义角色」（开 `PresetRolePicker`）+「角色导演」（开 `RoleDirectorDialog`）；移除「恢复预置」按钮；`onSubmit` 精简为 `upsertRole({ name, providerId: null, providerIds: [], systemPromptOverride: trim||null, toolAllowlist: [], requiredCapabilities, temperature: null, maxTokens: null, params: {} })`；保留 `CAPABILITY_KEYS` 与 `BindingMode` 类型 export 供 `RoleBindingPanel` 复用

### 3.4 新建 RoleQuickBinding 快速绑定表单
- [ ] 新建 `src/features/settings/RoleQuickBinding.tsx`，实现区块②：`name`（Role Agent 实例名，独立输入不随 Role 下拉自动填充）+ `sourceRoleId`（Role 下拉，可选继承源，首次选中继承 SystemPrompt/能力作初始值，用 ref 标记 `hasInherited` 防止后续切换覆盖用户编辑）+ `bindingValue`（Provider/CLI 混合下拉，用 optgroup 分「Provider/ModelId」与「CLI Agent/ModelId」两组）+ `requiredCapabilities` 能力勾选 + `ToolTagInput` 工具白名单 + 保存按钮
- [ ] 新建 `src/features/settings/bindingOptions.ts` 工具函数：`buildBindingOptions(providers, agentProfiles)` 遍历 providers × models 生成 `provider.name/model.id`（value 编码 `provider:<id>:<modelId>`），遍历 agentProfiles 生成 `agent.name/agent.modelId` 或 `agent.name`（value 编码 `cli:<id>`）；`decodeBindingValue(value)` 解析为 `{ kind, providerId?, modelId?, agentProfileId? }`
- [ ] `onSubmit` 逻辑：`decodeBindingValue` 解析选中值；kind=provider → `upsertRole({ name, providerId, providerIds: [providerId], ..., params: {} })`；kind=cli → `upsertRole({ name, providerId: null, providerIds: [], ..., params: { agent_profile_id: agentProfileId } })`；成功后 invalidate `["roles"]` + toast + 重置表单
- [ ] 新建 `src/features/settings/RoleQuickBinding.test.tsx` 覆盖名称独立输入、继承源预填、混合下拉分组、保存写入

### 3.5 重构 RolesSection 为平铺卡片列表
- [ ] 重构 `src/features/settings/RolesSection.tsx`：标题栏仅保留 `<h3>`，移除按钮组与 `directorOpen` state（按钮组迁入 `RoleForm`）；移除 `groups` 分组计算（`isRoleReady` 分组逻辑），改为单层平铺 `roles.map(role => <RoleRow .../>)`；渲染顺序改为 `<RoleForm /> → <RoleQuickBinding /> → 平铺卡片 ul`；`BindingBadge` 扩展展示 `Provider/CLI Agent: name/modelId` 粒度（从 `provider.settings.models` 或 `agentProfile.modelId` 解析 modelId）；`RoleRow` [修改] 打开 `RoleBindingPanel`，[删除] 两步确认（builtin 除外）

### 3.6 微调 RoleBindingPanel 工具白名单为 tag 输入
- [ ] 微调 `src/features/settings/RoleBindingPanel.tsx`：工具白名单输入从逗号分隔 `<input type="text">` 替换为 `ToolTagInput` 组件；`toolAllowlist` state 类型从 `string` 改为 `string[]`；`onSubmit` 直接传 `toolAllowlist` 数组（不再 `split(",")`）；其余全字段编辑能力（名称/绑定方式/Provider或CLI/能力/温度/maxTokens/SystemPrompt）保留不变

### 3.7 补充 i18n 与测试覆盖
- [ ] 在 i18n 文件新增 `settings.roles.presetPicker*` / `settings.roles.quickBinding*` / `settings.roles.toolTagInput*` key（中英双语，无硬编码字符串）
- [ ] 更新 `src/features/settings/RolesSection.test.tsx` 适配平铺列表（移除分组断言）；更新 `src/features/settings/RoleBindingPanel.test.tsx` 适配 tag 输入

---

## 4. 自进化 tab 设计（Part 4）

> 目标：Settings access tab 改名「自进化」，围绕 Continual Harness H=(ρ,G,K,M) 模型暴露四维度配置。
> 范围：前端 + IPC 契约扩展 + 后端配置消费接线。
> 前置：任务 3.1（ToolTagInput 组件）已完成。
> 风险：R4（双写不一致）、R5（冲突参数）、R8（敏感工具白名单移除）。

### 4.1 新增后端 EvolutionSettings 实体与子结构
- [ ] 在 `crates/nuomi-core/src/domain/entities.rs` 新增 `EvolutionSettings` 结构体聚合四个值对象：`OnlineLearningConfig { authorized: bool, allowlist: Vec<String> }`、`RefineConfig { trigger_failures: u32, min_edit_strategy: RefineStrategy, evidence_threshold: f64, rollback_enabled: bool }`、`SkillCreationConfig { enabled: bool, format: SkillFormat }`、`MemoryPolicy { retention_days: u32, retrieval: RetrievalStrategy }`；新增 `RefineStrategy`（PROMPT_NOTE/MEMORY/SKILL/SUB_AGENT_SPEC）、`SkillFormat`（SKILL_MD）、`RetrievalStrategy`（KEYWORD/SEMANTIC/HYBRID）枚举；实现 `Default`（默认值：trigger_failures=3, evidence_threshold=0.8, retention_days=90, allowlist=ResearchAllowlist::DOMAINS）；派生 `serde::Serialize/Deserialize` + `specta::Type`

### 4.2 新增 IPC 命令并注册与重新生成 bindings
- [ ] 在 `src-tauri/src/commands.rs` 新增 `EvolutionSettingsDto` + `RefineStrategyDto` + `SkillFormatDto` + `RetrievalStrategyDto` 类型定义（对应 TS 类型见 design.md §2.2.2）；新增 `impl_get_evolution_settings`（读 `app_settings` key=`evolution_settings` JSON，优先 `app_settings` fallback 读 `memory_entries` `online_authorized` 标记 + 默认 allowlist）与 `impl_set_evolution_settings`（写 `app_settings` key=`evolution_settings` JSON + 同步写 `memory_entries` `online_authorized` 标记兼容既有读取路径；校验 `evidence_threshold >= 0.5` / `trigger_failures >= 1` / `retention_days >= 1`，越界返回 `evolution.invalid_config`）
- [ ] 在 `src-tauri/src/tauri_cmds.rs` 注册 `get_evolution_settings`/`set_evolution_settings` 命令；在 `src-tauri/src/lib.rs` 的 invoke_handler 注册新命令
- [ ] 执行 `pnpm contracts:gen` 重新生成 bindings，确认含 `EvolutionSettingsDto` 及相关类型与 `getEvolutionSettings`/`setEvolutionSettings` IPC 方法

### 4.3 后端 evolution 引擎消费 EvolutionSettings 接线
- [ ] 在 `crates/nuomi-core/src/evolution/research.rs` 将 `ResearchAllowlist::is_allowed` 改为接受 `allowlist: &[String]` 参数（保留 `DOMAINS` 做 fallback 默认值）；`ResearchScheduler::run_once` 接受 `EvolutionSettings` 参数（替换固定 `authorized: bool`）
- [ ] 在 `crates/nuomi-core/src/evolution/reflection.rs` 的 `Reflector` 预留 `refine: RefineConfig` 参数接口（本期仅存储配置，完整 `/refine` 管道实现为后续里程碑）；`MemoryService` 预留 `memory_policy: MemoryPolicy` 参数接口（保留期/检索策略为后续实现）

### 4.4 新建 EvolutionSettingsPanel 四维度配置面板
- [ ] 新建 `src/features/settings/EvolutionSettingsPanel.tsx`，mount 时 query `ipc.getEvolutionSettings` 填充 state；渲染四张 `sketch-card` 卡片：维度(1) 联网学习源白名单——授权开关（复用既有 `OnlineAuthToggle` 视觉风格）+ `ToolTagInput`（candidates 可为空，自由输入域名）；维度(2) 反思进化参数——`triggerFailures` 数字输入(1-10) + 触发方式单选 + `minEditStrategy` 单选(prompt_note/memory/skill/sub_agent_spec) + `evidenceThreshold` 滑块(0.5-1.0) + `rollbackEnabled` switch；维度(3) 自动技能创建——`enabled` switch + `format` 单选(skill_md)；维度(4) 持久记忆策略——`retentionDays` 数字输入(1-365) + `retrieval` 单选(keyword/semantic/hybrid)；保存按钮调 `ipc.setEvolutionSettings` + invalidate `["evolutionSettings"]` + toast
- [ ] 新建 `src/features/settings/EvolutionSettingsPanel.test.tsx` 覆盖四维度渲染、保存写入、授权开关与既有逻辑兼容

### 4.5 微调 SettingsView tab 改名与渲染
- [ ] 微调 `src/features/settings/SettingsView.tsx`：TABS 数组 access 项 `labelKey` 从 `settings.tab.access` 改为 `settings.tab.evolution`（key 保留 `"access"` 或新增 `"evolution"` 并保留 `"access"` 做 alias）；TabPanel access/evolution case 改为渲染 `<EvolutionSettingsPanel />`；移除 `SensitiveToolsEditor` 与 `OnlineAuthToggle` import（文件保留不删便于后续恢复）；`initialTab` 外部传入 `"access"` 时 fallback 到 `"evolution"` 兼容深层链接
- [ ] 更新 `src/features/settings/SettingsView.test.tsx` 适配 evolution tab 渲染

### 4.6 补充 i18n 与测试覆盖
- [ ] 在 i18n 文件新增 `settings.evolution.*` 四维度 key（含 `settings.evolution.online.*` 兼容既有 `settings.onlineHeading`）+ `settings.tab.evolution`（中英双语）
- [ ] 后端新增 `EvolutionSettings` 序列化/默认值/兼容读取单测；`set_evolution_settings` 时 `memory_entries` 双写单测；`evolution::research` allowlist 参数化单测

---

## 5. 集成验证与质量门（收尾）

> 目标：全量质量门全绿，AC 清单逐项核验，实现原理沉淀。
> 前置：任务 1-4 全部完成。

### 5.1 执行全量质量门
- [ ] 执行 `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test` 确认 Rust 端零警告零失败
- [ ] 执行 `pnpm typecheck && pnpm lint && pnpm test` 确认前端类型检查、lint、单测全绿

### 5.2 AC 清单逐项核验报告
- [ ] 逐项核验 AC1-AC22 共 22 条验收标准，输出核验报告（每条标注通过/未通过 + 证据摘要）；重点核验横切项：AC19（既有 Role 数据零迁移、builtin 保护、Team 编排消费不变）、AC20（契约先行：DTO 变更 + bindings 再生成 + 消费端更新同一提交）、AC21（i18n 全 key 覆盖无硬编码）、AC22（质量门全绿）

### 5.3 实现原理沉淀至 principle.md
- [ ] 在根目录 `principle.md`（gitignored）追加本次实现原理：设计思想（三区块解耦 + per-ModelId 超参 + 自进化四维度）、关键权衡（ProviderSettings 读时兼容 vs 写时迁移、EvolutionSettings 双写兼容、自动推荐静态规则零 token）、数据流/调用链（区块②混合下拉 → upsertRole → roles 表；EvolutionSettingsPanel → IPC → app_settings + memory_entries 双写 → evolution 引擎消费）
