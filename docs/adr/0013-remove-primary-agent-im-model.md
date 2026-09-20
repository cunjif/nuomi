# ADR 0013: 祛除主 Agent 概念，统一为 IM 式单聊/群聊模型

- 状态：Proposed（2026-09-18，待用户确认）
- 关联：ADR 0012（对话以 Role Agent 为单位）、ADR 0011（Settings 运行时统一）
- 背景：当前对话系统有"主 Agent"（`Session.agent` / `ConversationDto.agent`）和"参与者"（`conversation_participants` 表 / `ConversationDto.participantAgents`）两套独立的数据源，互不同步。`RoleAvatar` 显示主 Agent，`AgentSidebar` 显示参与者，两端数据割裂。用户要求祛除主 Agent 概念，以微信/QQ 等 IM 系统理念重构——对话系统仅有单聊（1 个 Role Agent）和群聊（多个 Role Agent）的区别。

## 决策

1. **祛除 `Session.agent` 字段**。`sessions` 表的 `agent_kind` / `agent_ref_id` 列废弃（migration 0021 将既有数据迁移到 `conversation_participants` 表后，列保留为 NULL 兼容但不再读写）。`Session` 实体移除 `agent` 字段。`ConversationDto.agent` 字段移除，统一用 `participantAgents`。前端所有 `conversation.agent` 引用改为 `conversation.participantAgents[0]`（单聊）或遍历 `participantAgents`（群聊）。

2. **单聊/群聊由参与者数量决定**。`participantAgents.length === 1` → 单聊（`kind = "chat"`）；`participantAgents.length > 1` → 群聊（`kind = "group"`）。`ConversationKind` 枚举保留（仍区分 `chat`/`group`/`background`/`scheduled`），但 `chat`/`group` 的区分不再由用户显式选择，而是由参与者数量自动推导。新建对话时选择 1 个 Role Agent → 单聊，选择多个 → 群聊。

3. **`create_conversation` 签名改为接受 `participants: Vec<(AgentRefKind, &str)>`**。替代当前的单个 `agent: Option<(AgentRefKind, &str)>` 参数。创建时把所有参与者写入 `conversation_participants` 表。`NewConversationDialog` 改为支持选择 1 个或多个 Role Agent（多选 UI）。

4. **`resolve_agent` 改为 `resolve_participants`**。从 `conversation_participants` 表读取参与者列表，返回 `Vec<ResolvedAgent>`。`resolve_default_agent`（全局默认 → builtin Role）保留作为"无显式参与者时的兜底"——但兜底结果也写入 `conversation_participants` 表（首次解析时物化），后续读取直接走表，不再每次走兜底链。

5. **`set_conversation_agent` IPC 废弃，改为 `set_conversation_participants`**。支持整体替换参与者列表（单聊切 Role Agent = 替换为 1 人；群聊加/减成员 = 替换为 N 人）。`/agent` slash 命令改为切换单聊的 Role Agent（替换 `participantAgents[0]`）。`add_conversation_agent` / `remove_conversation_agent` 保留用于群聊增删成员。

6. **`Schedule.agent` 保留**。Scheduler 仍需指定默认 agent 创建定时会话。但 `scheduler_service::create_conversation` 调用时，把 `s.agent` 写入 `conversation_participants` 表（而非 `session.agent`）。`Schedule.agent` 语义不变，只是透传目标改为参与者表。

7. **`AgentRefKind` 保留**。`Schedule.agent` 和 `ConversationParticipant` 仍依赖它。`AgentRefKind::Cli` 仍标记 deprecated（ADR 0012），对话参与者只接受 Role kind。

8. **`RoleAvatar` 改为纯展示 `participantAgents[0]`**。单聊时显示唯一参与者的头像；群聊时 `ConversationHeader` 显示 👥 群聊图标替代单个 avatar（或显示参与者叠加头像，后续 UI 迭代）。`AgentSidebar` 已用 `participantAgents`，数据天然打通。

9. **`reference_pre_check.rs` 引用清理更新**。Role 删除时的引用检查从 `sessions WHERE agent_kind='role'` 改为 `conversation_participants WHERE agent_kind='role'`；级联清空从 `UPDATE sessions SET agent_kind=NULL` 改为 `DELETE FROM conversation_participants WHERE agent_kind='role' AND agent_ref_id=?1`。

10. **`participantAgents` 的 `name` 字段在后端填充**。当前 `impl_get_conversation` 中 `participant_agents` 的 `name` 恒为空字符串（`commands.rs:3743`），改为通过 `name_agent_ref` 填充真实名称（与 `agent` 字段一致的处理）。

## Agent 详情面板联动（IM 式交互）

11. **Agent 详情面板 = 聊天用户信息页**。`AgentSidebar`（右侧滑出）上部展示当前对话所有参与者（`participantAgents`）的头像 + 名称列表，下部保留 `ConversationInfoPanel`（会话信息）。数据天然来自 `conversation_participants` 表（决策 1），与对话侧 `RoleAvatar` 共享同一数据源，不再割裂。

12. **"+"按钮加人，单聊自动升级群聊**。`AgentAvatarList` 末尾增加"+"圆形按钮（与头像同行，横向排列的最后一个），点击弹出 `AgentPickerPopover`（复用现有组件，只列 `isRoleReady` 的 Role，排除已在会话中的）。选择后调用 `addConversationAgent` 写入 `conversation_participants`。当参与者数量从 1 变为 2 时，`kind` 自动从 `chat` 升级为 `group`（后端 `impl_add_conversation_agent` 已有此逻辑 `commands.rs:3852-3858`），前端 `ConversationView` 重新渲染切换到 `GroupConversationView`。转换无需确认弹窗，会话标题保持不变。

13. **群聊中继续加人/移除成员**。群聊状态下"+"按钮持续可用，可加更多 Role Agent。点击参与者头像弹出 `AgentBottomSheet`（详情），底部增加"移除成员"按钮（调用 `removeConversationAgent`，仅群聊可用；单聊的唯一参与者不可移除）。移除后若参与者数量降为 1，`kind` 自动从 `group` 降级为 `chat`，同样无需确认，标题不变。

## 回滚边界

1. 数据迁移（migration 0021）为单向——把 `sessions.agent_kind/agent_ref_id` 搬到 `conversation_participants`。回滚需手动反向迁移（从 `conversation_participants` 恢复到 `sessions.agent_*`），仅恢复单聊场景（单参与者 → 主 Agent），群聊多参与者无法表达。
2. `Session.agent` 字段移除后回滚需恢复实体字段 + 所有读写点 + DTO 字段 + 前端引用，范围大但机械。
3. `create_conversation` 签名变更回滚需恢复 `agent: Option<...>` 参数 + 所有调用点适配。
4. `Schedule.agent` 保留，回滚不影响 scheduler 实体。

## 灰度策略

- **既有单聊会话兼容**：migration 0021 把 `sessions.agent_kind/agent_ref_id` 非空的行迁移到 `conversation_participants`（单行），迁移后 `sessions.agent_*` 置 NULL。既有单聊会话的 `participantAgents` 从空变为 1 项，UI 无感切换。
- **既有群聊会话兼容**：`conversation_participants` 已有数据保留，`sessions.agent_*` 迁移后可能产生重复（既有主 Agent + 既有参与者）——迁移时去重（同一 `(session_id, agent_kind, agent_ref_id)` 只保留一行）。
- **无 Agent 的会话**：`sessions.agent_*` 为 NULL 且 `conversation_participants` 为空 → 走 `resolve_default_agent` 兜底链，首次解析时物化到参与者表。
- **IPC 签名变更**：`ConversationDto.agent` 移除、`ConversationInput.agent` 移除、`setConversationAgent` → `setConversationParticipants`。前端 bindings 重新生成。

## 权衡

- **祛除主 Agent vs 保留并同步**：选祛除——用户明确要求"祛除主 Agent 的概念，以 IM 系统理念构建"；保留并同步两套数据源会持续维护割裂状态，语义混淆。
- **单聊/群聊由参与者数量推导 vs 显式 kind**：选推导——IM 系统中单聊/群聊的本质区别就是参与者数量；显式 kind 保留用于 `background`/`scheduled` 等非 IM 语义。
- **兜底链物化到表 vs 每次走兜底**：选物化——首次解析后写入 `conversation_participants`，后续读取走表，避免每次 `getConversation` 都走 `resolve_default_agent` 的 DB 查询链；且物化后参与者列表稳定，不会因全局默认变更而漂移。
- **`Schedule.agent` 保留 vs 一并祛除**：选保留——scheduler 创建定时会话时仍需指定默认 agent，祛除后 scheduler 无法表达"用哪个 Role Agent 执行定时任务"；`Schedule.agent` 语义独立于"主 Agent"，只是创建会话时的初始参与者。
- **整体替换 vs 增删 API**：选整体替换（`set_conversation_participants`）——单聊切 Role Agent 是整体替换 1 人，群聊增删成员也可用整体替换表达；同时保留 `add/remove` 用于群聊场景的增量操作。

## 实施阶段

- **阶段一（后端实体 + 迁移）**：migration 0021 数据迁移；`Session.agent` 移除；`ConversationDto.agent` 移除；`participantAgents.name` 填充。
- **阶段二（后端服务 + 命令）**：`create_conversation` 签名改 `participants: Vec`；`resolve_participants` 替代 `resolve_agent`；`set_conversation_participants` IPC；`reference_pre_check` 更新；`scheduler_service` 适配。
- **阶段三（前端）**：bindings 重新生成；`RoleAvatar` 改读 `participantAgents[0]`；`NewConversationDialog` 多选；`ConversationsList` 适配；`/agent` 命令适配；`AgentChip`/`AgentPickerPopover` 适配或移除；`AgentAvatarList` 增加"+"加人按钮 + 复用 `AgentPickerPopover`；`AgentBottomSheet` 增加"移除成员"按钮（仅群聊）；单聊↔群聊自动转换（`kind` 随参与者数量变化）。
- **阶段四（测试 + 质量门）**：`test-double` 适配；`conversation_service` 单测适配；`settings_integration_e2e` 适配；全量质量门。
