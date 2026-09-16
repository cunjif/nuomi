# SPEC: Nuomi Role Agents 配置体验与对话手选（role-agents-ui）

> 状态：待批准（Part 2 依赖 settings-integration spec，需该 spec 阶段一+阶段二落地后方可端到端验收）
> 前置：`docs/specs/cli-agents-m1.md`（M-CLI1 ✅）、`docs/specs/team-shell-m1.md`（M-TEAM1 ✅）、`docs/specs/settings-integration/spec.md`（待批准，Part 2 依赖）、`docs/specs/harness-kernel-v1.md`（K6 ✅）。
> 需求来源：用户请求「设置页"模型提供方"/"CLI Agents"配置完成后没有提供 Role 绑定的显眼入口；考虑让 Role 绑定 Provider 或 CLI Agent 成为真正具有功能的 Role Agent，对话时可手选」；需求权威仍为 `pr.md`。
> 事实基础：`RoleForm.tsx:99-113` 已有 `bindingMode` 下拉（none/provider/cli）但入口隐蔽于新建表单内；`RoleForm.tsx:82-85` 将 `toolAllowlist`/`temperature`/`maxTokens` 写死空/null，无编辑入口；`AgentPickerPopover.tsx:25-31` 调 `ipc.listAgentOptions()` 列出所有 Role 不区分绑定状态；`AgentChip.tsx:36` 调 `ipc.setConversationAgent` 绑定 agent 到 session；`RolesSection.tsx:227-237` 按 builtin/generated/custom 分组而非按绑定状态分组。

## 0. 问题诊断 / Problem Diagnosis

### 0.1 绑定入口隐蔽，配置流断裂

用户在 Providers tab 或 CLI Agents tab 完成配置后，**无任何引导**跳转到 Roles tab 进行绑定。绑定能力虽已存在于 `RoleForm` 的 `bindingMode` 下拉（`RoleForm.tsx:99-113`），但：
- 藏在"新建 Role"表单内部，用户不知道"配完 Provider 后下一步去哪"；
- 仅在**新建**时可选，已存在的 Role **无"重新绑定"或"去绑定"入口**（`RolesSection.tsx:104-125` 的 RoleRow 只有删除按钮）；
- Providers tab（`ProvidersSection.tsx:151-163`）与 CLI Agents tab（`CliAgentsSection.tsx:85-115`）的详情区**无"绑定到 Role"反向引导按钮**。

### 0.2 RoleForm overlay 字段不全

`RoleForm.tsx:82-85` 提交时将 `toolAllowlist: []`、`temperature: null`、`maxTokens: null` 写死，用户无法通过 UI 配置这三项 Role 覆盖参数。运行时 `RoleOverlay`（`single_role_materializer.rs:19-23`）与编排器（`pipeline.rs:42` / `group_chat.rs:227`）均消费这三项，但 UI 无编辑入口——配置能力被截断。

### 0.3 对话选择器不区分就绪状态

`AgentPickerPopover.tsx:77-91` 将所有 Role 列为可选项（`groupAgentOptions` 按 kind 分组，`agentResolve.ts:33-40`），未区分"已绑定 Provider/CLI Agent 的就绪 Role Agent"与"未绑定的空 Role"。用户选一个未绑定 Role 后，运行时走 env 兜底（若 DB 无配置则无响应），体验断裂——选了"Role"但实际没有功能。

### 0.4 配置即执行脱节（Part 2 前置）

普通对话 chat 当前走 facade env 单 Provider 路径（`facade.rs:333`），**不读 DB Role 绑定**（settings-integration spec §0.1 诊断）。即使 Part 1 让用户在 UI 上手选了 Role Agent，若 settings-integration spec 未落地，chat 路径仍不消费该绑定——Part 2 的端到端验收依赖 settings-integration spec 阶段一（AC1 物化）+ 阶段二（AC3 Role 覆盖 + AC5 resolve_agent 消费）。

## 1. 决策记录摘要 / Decisions

| # | 决策 |
|---|---|
| D1 | **Roles tab 内部重构**（不新增 tab）：将 `RolesSection` 分组从 builtin/generated/custom 改为**按绑定状态**分两组——"已就绪（可对话使用）"与"未绑定（空 Role，尚不可用）"；builtin/generated 标签作为次级徽章保留。 |
| D2 | **绑定面板暴露全部 overlay 字段**：新建 `RoleBindingPanel` 组件替代当前 `RoleForm` 的 bindingMode 内联选择，暴露 `systemPromptOverride` / `temperature` / `toolAllowlist` / `maxTokens` / `requiredCapabilities` 全部字段；既有 `RoleForm` 保留为"快速新建"入口，绑定面板同时用于新建与编辑已有 Role 的绑定。 |
| D3 | **Provider / CLI Agent 详情页反向引导**：在 `ProviderForm` 详情区（`ProvidersSection.tsx:151-163` 右侧）与 `CliAgentsSection` 每行操作区（`CliAgentsSection.tsx:85-115`）各加"绑定到 Role"按钮，点击弹出 `RoleBindingPanel` 预填该 Provider/CLI Agent，配完即可绑定。 |
| D4 | **对话选择器区分就绪**：`AgentPickerPopover` 将 Role 组拆为"已就绪 Role Agent"（可点选）与"未绑定 Role"（灰显 + "去绑定"引导跳转 Settings）；就绪判定 `role.providerId != null OR readAgentProfileId(role.params) != null`。 |
| D5 | **数据模型不变**：复用现有 `roles` 表 + `provider_id` / `params.agent_profile_id` 约定键，无新表、无新 migration（既有 0010 已就绪）。 |
| D6 | **分阶段交付**：Part 1（UI 重构，§AC1-AC5）纯前端改动，独立可交付；Part 2（对话手选端到端生效，§AC6-AC8）依赖 settings-integration spec 阶段一+阶段二落地，可并行开发但端到端验收需该 spec 先行。 |
| D7 | **就绪判定在前端**：`RoleDto` 已携带 `providerId` 与 `params` 字段，前端用 `readAgentProfileId`（`RoleForm.tsx:23-29`）+ `providerId` 判定，无需扩展 `AgentOptionDto` 或新增 IPC；`listAgentOptions` 返回的 Role 选项需携带绑定信息（见 design.md 模块 C 决策）。 |
| D8 | **i18n + 设计系统对齐**：新增 i18n key 遵循 `settings.roles.*` / `composer.*` 既有命名空间；视觉对齐手绘纸质风格（global.css 手绘基因），分组用既有 `sketch-card` / `pixel-fill-accent` token，不引入新设计语言。 |

## 2. 目标 / Goals

1. **绑定入口显眼化**：用户配完 Provider/CLI Agent 后，能在 Roles tab 一眼看出哪些 Role 已就绪、哪些未绑定，并能一键打开绑定面板完成绑定。
2. **overlay 字段全暴露**：用户可在 UI 配置 `systemPromptOverride` / `temperature` / `toolAllowlist` / `maxTokens` / `requiredCapabilities` 全部 Role 覆盖参数。
3. **反向引导闭环**：Provider/CLI Agent 详情页提供"绑定到 Role"入口，形成"配 Provider → 绑定 Role → 使用"完整引导链。
4. **对话手选就绪 Role Agent**：chat 界面 agent 选择器仅列已就绪 Role Agent 为可选项，未绑定 Role 灰显并引导去绑定；选中后该 Role Agent 接管对话（依赖 settings-integration spec）。
5. **不回归**：既有 Role 数据、builtin Role 保护、Team 编排消费 Role 的行为零变更。

## 3. 用户故事 / User Stories

- **US1** 用户在 Providers tab 配好一个 OpenAI 兼容 Provider，点该 Provider 详情页的"绑定到 Role"按钮，弹出绑定面板预填了该 Provider，填 Role 名"翻译员" + systemPrompt 后保存——Roles tab 立即显示"翻译员 → Provider: xxx"在"已就绪"组。
- **US2** 用户在 Roles tab 看到一个未绑定的"总结员" Role 在"未绑定"组，点"去绑定"按钮，弹出绑定面板，选一个已配 CLI Agent，保存——该 Role 移到"已就绪"组。
- **US3** 用户在 Roles tab 打开一个已就绪 Role 的绑定面板，修改 `temperature: 0.3` 与 `toolAllowlist: ["read_file","write_file"]`，保存——运行时 FakeLlm 断言收到 temperature=0.3 且仅这两个工具可用。
- **US4** 用户在 chat 界面点 agent 选择器，"Role Agent"组列出所有已就绪 Role（带绑定目标徽章），未绑定 Role 灰显且带"去绑定"提示；选一个就绪 Role Agent 后，该对话由该 Role 的 Provider/CLI Agent + overlay 接管。
- **US5** 用户在 CLI Agents tab 配好一个 claude_code profile，点该行"绑定到 Role"按钮，弹出绑定面板预填了该 CLI Agent，填 Role 名"审查员"保存—— Roles tab 显示"审查员 → CLI: claude_code"在"已就绪"组。

## 4. 验收标准 / Acceptance Criteria

**阶段一（Part 1：Roles tab UI 重构，独立可交付）**

- [ ] AC1 `RolesSection` 按**绑定状态**分两组渲染："已就绪（可对话使用）"（`providerId != null` 或 `params.agent_profile_id != null`）与"未绑定（空 Role，尚不可用）"；builtin/generated 标签作为次级徽章保留在行内；空组不渲染。
- [ ] AC2 新建 `RoleBindingPanel` 组件替代 `RoleForm` 的 bindingMode 内联选择，暴露 `systemPromptOverride` / `temperature` / `maxTokens` / `toolAllowlist`（逗号分隔或 tag 输入） / `requiredCapabilities` 全部字段；提交时写入 `RoleInput` 对应字段（不再写死空/null）；既有 `RoleForm` 保留为快速新建入口或被绑定面板取代（design.md 定夺）。
- [ ] AC3 每个 Role 行（含 builtin）提供"绑定"/"编辑绑定"按钮打开 `RoleBindingPanel` 预填该 Role 当前值；builtin Role 允许编辑绑定但不允许删除（既有保护保留）。
- [ ] AC4 `ProvidersSection` 的 Provider 详情区（右侧 `ProviderForm` 旁或下方）提供"绑定到 Role"按钮，点击弹出 `RoleBindingPanel` 预填 `bindingMode=provider` + 该 Provider id；`CliAgentsSection` 每行操作区提供"绑定到 Role"按钮，点击弹出 `RoleBindingPanel` 预填 `bindingMode=cli` + 该 profile id。
- [ ] AC5 Part 1 双端质量门全绿：`pnpm typecheck && pnpm lint && pnpm test`（纯前端改动，Rust 端零变更）。

**阶段二（Part 2：对话手选 Role Agent，依赖 settings-integration spec）**

- [ ] AC6 `AgentPickerPopover` 的 Role 组拆为"已就绪 Role Agent"（可点选）与"未绑定 Role"（灰显 + "去绑定"引导跳转 Settings → Roles tab）；就绪判定用 D7 前端逻辑；`listAgentOptions` 返回的 Role 选项需携带 `providerId` / `params` 或后端预判 `ready: boolean`（design.md 模块 C 定夺）。
- [ ] AC7 选中就绪 Role Agent 后，`setConversationAgent` 绑定到 session，后续对话由该 Role 的 Provider/CLI Agent + overlay 接管——FakeLlm 断言收到的 systemPrompt/temperature/tools 与 Role 配置一致（**依赖 settings-integration spec AC1+AC3+AC5 落地**）。
- [ ] AC8 Part 2 端到端质量门全绿：`pnpm typecheck && pnpm lint && pnpm test` + 既有 Rust 测试不回归（Part 2 无 Rust 改动，除非 design.md 模块 C 选择扩展 AgentOptionDto）。

**横切**

- [ ] AC9 既有 Role 数据零迁移：builtin Role 保护不变、Team 编排消费 Role 行为不变、`upsertRole` 幂等性（name 为 key）不变。
- [ ] AC10 i18n 全 key 覆盖（中英双语对齐既有 `settings.roles.*` / `composer.*` 命名空间），无硬编码字符串。
- [ ] AC11 视觉对齐手绘纸质风格：分组卡片用 `sketch-card`、按钮用 `pixel-fill-accent` / 既有 border token、绑定面板用 `bg-surface-raised`，不引入新设计语言。

## 5. 非目标 / Non-goals

引入"1 Role : N 绑定"新数据模型（同一 Role 派生多个 Role Agent 实例）——本次保持 1 Role : 1 绑定（Provider 或 CLI Agent 二选一）的现有模型；新增 Multi-Agent 编排机制；Telemetry / 飞书 / QQBot 增强；UI 主题打磨 / Monaco 体验 / 自发组队 dry-run 预览；改 `team_runner::materialize` 物化逻辑；改 Rust `Role` 实体或 `roles` 表 schema。

## 6. 技术约束 / Technical Constraints

- 锁定栈不变：React18/TS strict + Vite + Tailwind + Zustand + TanStack Query。遵守 `.opencode/rules/frontend.md`（若有）与既有 `src/features/settings/` 组件风格。
- **零新 migration、零 Rust 实体变更**：Part 1 纯前端；Part 2 若需扩展 `AgentOptionDto` 携带绑定信息，走 IPC 契约扩展（`src-tauri/src/tauri_cmds.rs` + `commands.rs` + `pnpm contracts:gen`），但不改 `roles` 表 schema。
- IPC 契约纪律：若扩展 `AgentOptionDto`，遵守 `.opencode/rules/ipc-contract.md`（camelCase DTO、稳定 code、四步契约链），重新生成 bindings。
- Part 2 依赖声明：端到端验收（AC7）需 settings-integration spec 阶段一（AC1 物化）+ 阶段二（AC3 Role 覆盖 + AC5 resolve_agent 消费）先行落地；Part 2 可在 settings-integration 落地前开发 UI（AC6），但 AC7 验收挂起。
- 测试纪律：前端组件测试用 vitest + @testing-library（既有模式）；零外网；Storybook 若无则不强求。
- i18n 纪律：所有新增文案走 `src/i18n/` 既有 key 体系，中英双语同步。

## 7. 实施计划 / Implementation Plan

| 阶段 | 优先级 | 内容 | 前置 | 验收 |
|---|---|---|---|---|
| Part 1 | 高 | `RoleBindingPanel` 组件 + `RolesSection` 按绑定状态分组重构 + Provider/CLI Agent 反向引导按钮 + i18n | 无 | AC1-AC5, AC9-AC11 |
| Part 2-UI | 中 | `AgentPickerPopover` 就绪过滤 + 未绑定灰显引导 | Part 1 | AC6, AC10-AC11 |
| Part 2-E2E | 中 | 端到端验收：选就绪 Role Agent → 对话消费该绑定 | settings-integration spec 阶段一+阶段二 | AC7, AC8 |
| 收尾 | — | 全量质量门复核 + AC 清单逐项核验报告 | Part 1 + Part 2 | AC5, AC8, AC11 |

## 8. 风险与回滚 / Risks & Rollback

- **风险**：`RolesSection` 分组从 builtin/generated/custom 改为按绑定状态，可能影响用户既有心智模型。**缓解**：builtin/generated 标签作为次级徽章保留在行内，用户仍可识别 Role 来源；分组标题明确"已就绪"/"未绑定"语义。
- **风险**：`RoleBindingPanel` 暴露 `toolAllowlist` 编辑后，用户可能配出空 allowlist 误锁所有工具。**缓解**：UI 提示"留空 = 不限制（全部工具可用）"；提交时空数组语义为"不限制"而非"禁用全部"（与既有 `RoleOverlay` 语义对齐，`single_role_materializer.rs:19-23` 空 = 不过滤）。
- **风险**：Part 2 端到端验收依赖 settings-integration spec，若该 spec 延期则 AC7 挂起。**缓解**：Part 1 独立交付价值（绑定入口显眼化 + 字段全暴露），Part 2-UI 可先行，AC7 验收标记为"依赖 settings-integration"。
- **回滚边界**：Part 1 纯前端，出问题回滚至既有 `RoleForm` + `RolesSection` 即可，无数据影响。Part 2 若扩展 `AgentOptionDto`，回滚至既有 DTO + 前端用 `listRoles` 交叉判定就绪状态。

## 9. 待核实项 / To Be Verified

- `AgentOptionDto` 当前字段清单（`list_agent_options` 返回结构）——决定 Part 2 就绪判定是前端交叉查询还是扩展 DTO（design.md 模块 C 决策点）。
- 既有 `RoleForm` 是否有消费方依赖其导出的 `BindingMode` / `readAgentProfileId`——决定是替换还是保留为快速新建入口。
- `toolAllowlist` 在 UI 上是逗号分隔输入还是 tag 输入组件——design.md 定夺（取决于既有设计系统是否有 TagInput 原子）。
