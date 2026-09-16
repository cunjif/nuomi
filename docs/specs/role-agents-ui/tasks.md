# TASKS: Nuomi Role Agents 配置体验与对话手选（role-agents-ui）

> 对应规格：`docs/specs/role-agents-ui/spec.md`
> 对应设计：`docs/specs/role-agents-ui/design.md`
> 任务规划原则：垂直切割（按业务功能分组）· 可验收 · 原子性 · 有序性（被依赖者在前）。
> 关键约束（来自已评审确认的设计决策，实施时必须遵守）：
> 1. **数据模型不变**：复用 `roles` 表 + `provider_id` / `params.agent_profile_id`，零新 migration、零 Rust 实体变更（spec D5）。
> 2. **Part 1 纯前端独立可交付**：AC1-AC5 不依赖 settings-integration spec；Part 2-UI（AC6）可并行，Part 2-E2E（AC7）依赖 settings-integration spec 阶段一+阶段二。
> 3. **就绪判定**：`isRoleReady(role) = role.providerId !== null || readAgentProfileId(role.params) !== null`，前端 helper，供 RolesSection 与 AgentPickerPopover 共用（spec D7）。
> 4. **AgentOptionDto 扩展走 IPC 契约**：若采用 design.md 模块 C 方案 1，`AgentOptionDto` 加 `ready: bool` 后端预判 + `pnpm contracts:gen`；若方案 2 前端交叉查询则零 Rust 改动。任务 7 按方案 1 编写，可在实施时降级为方案 2。

---

## 1. Part 1 前置：共享 helper 抽取

> 目标：将 `readAgentProfileId` 与新增 `isRoleReady` 抽到共享位置，供 RolesSection、RoleBindingPanel、AgentPickerPopover 共用。

### 1.1 抽取 isRoleReady + readAgentProfileId 到共享 util
- [ ] 在 `src/lib/conversation/roleReady.ts`（或 `src/features/settings/roleReady.ts`）新建共享 util，迁移 `readAgentProfileId`（现 `RoleForm.tsx:23-29`）并新增 `isRoleReady(role: { providerId: string | null; params: JsonValue }): boolean`（`role.providerId !== null || readAgentProfileId(role.params) !== null`）。更新 `RoleForm.tsx` 改为从共享 util import（保持既有导出向后兼容，避免破坏其他消费方）。用 `grep` 全量排查 `readAgentProfileId` 引用点同步适配。
- **验收**：`pnpm typecheck` 通过；既有 `readAgentProfileId` 调用点行为不变；`isRoleReady` 导出可用。
- **依赖**：无。

---

## 2. Part 1：RoleBindingPanel 组件（AC2）

> 目标：新建全字段绑定面板，暴露 systemPromptOverride / temperature / maxTokens / toolAllowlist / requiredCapabilities，支持新建与编辑预填。

### 2.1 新建 RoleBindingPanel 组件
- [ ] 新建 `src/features/settings/RoleBindingPanel.tsx`，实现 `RoleBindingPanelProps { initialRole?: RoleDto; presetBinding?: { mode: BindingMode; providerId?: string; agentProfileId?: string }; onClose: () => void }`。字段状态：`name`（编辑模式 `disabled` 显示 `initialRole.name`）、`bindingMode` select（none/provider/cli）、`providerId`/`agentProfileId` select（条件渲染，query `["providers"]`/`["agentProfiles"]`）、`systemPromptOverride` textarea、`temperature` number input（空 → null）、`maxTokens` number input（空 → null）、`toolAllowlist` input（逗号分隔，split/trim，空 → []）、`requiredCapabilities` checkbox 组（复用 `RoleForm` 的 `CAPABILITY_KEYS` 或抽共享）。提交调 `ipc.upsertRole`，映射逻辑对齐 `RoleForm.tsx:71-89`（provider 模式写 `providerId`+`providerIds=[providerId]`+`params={}`；cli 模式写 `providerId=null`+`providerIds=[]`+`params={agent_profile_id}`；none 模式全 null），成功后 `qc.invalidateQueries(["roles"])` + toast + `onClose()`。渲染为受控 dialog（对齐 `RoleDirectorDialog` 的 `RolesSection.tsx:146-186` 样式：`fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4` + `bg-surface-raised` 卡片）。
- **验收**：组件渲染全部字段；编辑模式预填 `initialRole` 值；反向引导预填 `presetBinding` 值；提交写入 `RoleInput` 全字段（不再写死空/null）；`pnpm typecheck && pnpm lint` 通过。
- **依赖**：1.1。

### 2.2 RoleBindingPanel 组件测试
- [ ] 为 `RoleBindingPanel` 编写 vitest 组件测试（若有 @testing-library）：(a) 新建模式渲染空字段；(b) 编辑模式预填 `initialRole` 全字段；(c) `presetBinding={mode:"provider",providerId:"x"}` 预填 bindingMode+providerId；(d) 提交调 `ipc.upsertRole` 入参含 `temperature`/`maxTokens`/`toolAllowlist` 实际值；(e) `bindingMode=cli` 提交 `params={agent_profile_id}` 且 `providerId=null`。mock `ipc` + TanStack Query。
- **验收**：测试全绿；覆盖新建/编辑/预填/提交映射四场景。
- **依赖**：2.1。

---

## 3. Part 1：RolesSection 分组重构 + RoleRow 绑定按钮（AC1, AC3）

> 目标：RolesSection 按绑定状态分"已就绪"/"未绑定"两组；每行加"绑定"/"编辑绑定"按钮打开 RoleBindingPanel。

### 3.1 RolesSection 按绑定状态分组
- [ ] 改 `src/features/settings/RolesSection.tsx:227-237` 的 `groups` 数组：用 `isRoleReady`（任务 1.1）分两组——`{ label: t("settings.roles.groupReady"), roles: roles.filter(isRoleReady) }` 与 `{ label: t("settings.roles.groupUnbound"), roles: roles.filter((r) => !isRoleReady(r)) }`。builtin/generated 徽章保留在 `RoleRow` 内（既有 `:88-97` 逻辑不变）。空组不渲染（既有 `group.roles.length > 0` 守护保留）。
- **验收**：已绑定 Role 渲染在"已就绪"组；未绑定 Role 渲染在"未绑定"组；builtin/generated 徽章仍显示；空组不渲染。
- **依赖**：1.1。

### 3.2 RoleRow 增加"绑定"/"编辑绑定"按钮
- [ ] 改 `RolesSection.tsx` 的 `RoleRow`（`:64-128`）props 增加 `onOpenBinding: (role: RoleDto) => void` 回调。在操作区（`:104-125` 的 `<div className="mt-1.5 flex gap-2">`）增加按钮：`isRoleReady(role)` ? "编辑绑定" : "去绑定"，点击调 `onOpenBinding(role)`。builtin Role 允许编辑绑定（删除保护保留）。`RolesSection` 顶层管理 `bindingPanelRole: RoleDto | null` 状态，传 `onOpenBinding={(role) => setBindingPanelRole(role)}`，在 section 末尾渲染 `{bindingPanelRole && <RoleBindingPanel initialRole={bindingPanelRole} onClose={() => setBindingPanelRole(null)} />}`。
- **验收**：每行有"绑定"/"编辑绑定"按钮；点击打开 `RoleBindingPanel` 预填该 Role；builtin Role 可编辑绑定不可删除；保存后列表刷新到正确分组。
- **依赖**：2.1, 3.1。

---

## 4. Part 1：Provider / CLI Agent 反向引导按钮（AC4）

> 目标：Provider 详情区与 CLI Agent 行各加"绑定到 Role"按钮，点击打开 RoleBindingPanel 预填。

### 4.1 ProvidersSection 反向引导按钮
- [ ] 改 `src/features/settings/ProvidersSection.tsx`：顶层增加 `bindingPreset` 状态。在右侧详情区（`:151-163`）`ProviderForm` 下方（`:159` 后），`selected && !creating` 时渲染"绑定到 Role"按钮（`t("settings.roles.bindToRole")`），点击 `setBindingPreset({ mode: "provider", providerId: selected.id })`。section 末尾渲染 `{bindingPreset && <RoleBindingPanel presetBinding={bindingPreset} onClose={() => setBindingPreset(null)} />}`。
- **验收**：选中 Provider 后详情区显示"绑定到 Role"按钮；点击打开 `RoleBindingPanel` 预填 `bindingMode=provider` + 该 Provider id；保存后 Roles tab 列表含新 Role。
- **依赖**：2.1。

### 4.2 CliAgentsSection 反向引导按钮
- [ ] 改 `src/features/settings/CliAgentsSection.tsx`：顶层增加 `bindingPreset` 状态。在每行操作区（`:85-115` 的 `<div className="mt-1.5 flex gap-2">`）与 check/delete 并列加"绑定到 Role"按钮，点击 `setBindingPreset({ mode: "cli", agentProfileId: profile.id })`。section 末尾渲染 `{bindingPreset && <RoleBindingPanel presetBinding={bindingPreset} onClose={() => setBindingPreset(null)} />}`。
- **验收**：每行有"绑定到 Role"按钮；点击打开 `RoleBindingPanel` 预填 `bindingMode=cli` + 该 profile id；保存后 Roles tab 列表含新 Role。
- **依赖**：2.1。

---

## 5. Part 1：i18n + 质量门（AC5, AC10, AC11）

> 目标：新增 i18n key 中英双语；视觉对齐手绘纸质风格；Part 1 全量质量门。

### 5.1 新增 i18n key
- [ ] 在 `src/i18n/` 既有 locale 文件（zh/en）新增 key：`settings.roles.groupReady`、`settings.roles.groupUnbound`、`settings.roles.bindToRole`、`settings.roles.editBinding`、`settings.roles.goBind`、`settings.roles.temperature`、`settings.roles.maxTokens`、`settings.roles.toolAllowlist`、`settings.roles.toolAllowlistHint`（留空 = 不限制）、`composer.roleAgentGroup`、`composer.roleUnbound`。中英双语同步。用 `grep` 确认无硬编码字符串遗漏。
- **验收**：全部新 key 中英双语存在；`pnpm typecheck` 通过（i18n key 类型检查若有）；无硬编码字符串。
- **依赖**：2.1, 3.1, 3.2, 4.1, 4.2。

### 5.2 Part 1 全量质量门
- [ ] 运行 `pnpm typecheck && pnpm lint && pnpm test`（用 `cmd.exe /c` 包装执行，对齐既有 esbuild 规避实践；单线程模式运行测试套件）。修复所有 type/lint 错误。确认既有 Role 相关测试不回归（builtin 保护、upsert 幂等、Team 消费 Role）。
- **验收**：`pnpm typecheck && pnpm lint && pnpm test` 全绿（AC5）；既有测试不回归（AC9）。
- **依赖**：5.1。

---

## 6. Part 2-UI：AgentPickerPopover 就绪过滤（AC6）

> 目标：对话 agent 选择器将 Role 组拆为"已就绪 Role Agent"（可点选）与"未绑定 Role"（灰显 + "去绑定"引导）。
> 注意：本组可与 Part 1 并行（仅依赖任务 1.1 的 `isRoleReady` 导出）。

### 6.1 扩展 AgentOptionDto 携带 ready 字段（方案 1）
- [ ] 在 `src-tauri/src/tauri_cmds.rs` 的 `AgentOptionDto` struct 增加 `#[serde(rename = "ready")] pub ready: bool` 字段。在 `list_agent_options` 实现处（`src-tauri/src/commands.rs`）对 Role 选项判定 `ready = role.provider_id.is_some() || role.params.get("agent_profile_id").and_then(Value::as_str).is_some()`；对 CLI 选项 `ready = profile.enabled`。运行 `pnpm contracts:gen` 重生成 bindings。**若实施时评估改 Rust 成本过高，降级为方案 2（任务 6.1-alt）**。
- **验收**：`AgentOptionDto` 含 `ready` 字段；bindings 重生成；Rust `cargo build` 通过。
- **依赖**：1.1。

### 6.1-alt [回退] 前端交叉查询判定就绪（方案 2）
- [ ] 若不改 Rust：在 `AgentPickerPopover` 额外 `useQuery(["roles"], ipc.listRoles)`，用 `role.id` 匹配 `options` 中的 Role 选项 + `isRoleReady` 判定。构建 `Map<roleId, ready>` 供过滤用。
- **验收**：就绪判定正确；多一次 IPC 但零 Rust 改动。
- **依赖**：1.1。

### 6.2 AgentPickerPopover Role 组拆分
- [ ] 改 `src/features/conversation/composer/AgentPickerPopover.tsx:77-91`：将 `role` 组拆为 `readyRoles = role.filter((o) => o.ready)` 与 `unboundRoles = role.filter((o) => !o.ready)`。就绪组用既有可点选渲染（`:80-89`），组标题改为 `t("composer.roleAgentGroup")`。未绑定组灰显（`text-ink-muted/50 cursor-not-allowed`）+ 每项带"去绑定"按钮（调 `onGoToSettings?.()` 回调）。`AgentPickerPopoverProps` 扩展 `onGoToSettings?: () => void`。`AgentChip`（`AgentChip.tsx`）透传该回调。
- **验收**：就绪 Role 可点选；未绑定 Role 灰显 + "去绑定"按钮；`onGoToSettings` 回调可触发跳转。
- **依赖**：6.1 或 6.1-alt。

### 6.3 Settings tab 路由支持"去绑定"跳转
- [ ] 扩展 `SettingsView`（`src/features/settings/SettingsView.tsx`）支持外部指定初始 tab：接受 `initialTab?: string` prop（或读 URL hash `#roles`），初始化 `activeTab` 时优先用该值。conversation 页面层提供"去绑定"回调：调 `setActiveTab("roles")` + 打开 Settings（若 Settings 是 dialog/tab，需路由或状态提升）。`AgentChip` 的 `onGoToSettings` 从 conversation 页面层注入。
- **验收**：从 chat "去绑定"点击跳转到 Settings → Roles tab；`RoleBindingPanel` 可直接在该 tab 打开绑定。
- **依赖**：6.2。

---

## 7. Part 2-E2E：对话手选 Role Agent 端到端验收（AC7, AC8）

> 目标：选中就绪 Role Agent 后，对话由该 Role 的 Provider/CLI Agent + overlay 接管。
> **前置门禁**：依赖 settings-integration spec 阶段一（AC1 物化）+ 阶段二（AC3 Role 覆盖 + AC5 resolve_agent 消费）落地。若该 spec 未落地，本组验收挂起。

### 7.1 等待 settings-integration spec 落地
- [ ] 确认 `docs/specs/settings-integration/spec.md` 阶段一+阶段二已实施且 AC1/AC3/AC5 验收通过。若未落地，本任务及 7.2 标记为"依赖挂起"，不阻塞 Part 1 + Part 2-UI 交付。
- **验收**：settings-integration spec AC1/AC3/AC5 已验收通过。
- **依赖**：外部（settings-integration spec）。

### 7.2 端到端测试：选就绪 Role Agent → 对话消费绑定
- [ ] 编写端到端测试（vitest + FakeLlm 或既有测试模式）：(a) Settings 配一个 Provider + 创建 Role 绑定该 Provider + 设 `temperature: 0.3` + `systemPromptOverride: "你是翻译员"`；(b) chat 界面 `AgentPickerPopover` 选该 Role Agent；(c) 发一条消息；(d) FakeLlm 断言收到的 `temperature === 0.3` 且 systemPrompt 含"你是翻译员"；(e) 同样测试 CLI Agent 绑定的 Role Agent。零外网。
- **验收**：FakeLlm 断言 Role overlay 生效（AC7）；`pnpm test` 全绿（AC8）；既有测试不回归（AC9）。
- **依赖**：7.1, 6.2。

---

## 8. 收尾：全量质量门 + AC 核验

### 8.1 全量质量门复核
- [ ] 运行 `pnpm typecheck && pnpm lint && pnpm test`（cmd.exe /c 包装 + 单线程模式）。若任务 6.1 改了 Rust，额外运行 `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test`（注意 rustfmt 可能缺失，格式化交由用户环境）。修复所有问题。
- **验收**：双端质量门全绿（AC5, AC8, AC11）。
- **依赖**：5.2, 7.2（若已落地）。

### 8.2 AC 清单逐项核验报告
- [ ] 逐项核验 spec §4 验收标准 AC1-AC11，输出核验报告（每项 ✅/⏳依赖挂起 + 证据指针）。AC7 若 settings-integration spec 未落地则标记"⏳ 依赖 settings-integration spec 阶段一+阶段二"。
- **验收**：AC 清单全项核验；依赖项明确标注。
- **依赖**：8.1。
