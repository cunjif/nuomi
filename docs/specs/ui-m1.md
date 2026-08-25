# SPEC: Nuomi UI M1 — 全量桌面控制台（ui-m1）

> 状态：已批准
> 前置：`docs/specs/harness-kernel-v1.md`（已交付）；需求权威 `pr.md` §5/§6。

## 0. 决策记录摘要

| # | 决策 |
|---|---|
| D1 | **全量一次交付**：三栏主界面 + 看板 + SubAgent/群聊 Trace + Approvals + Scheduler + 全量 Settings |
| D2 | **补内核实体**：Task / Run 状态机 / 审批门 / Scheduler 后端本期在 nuomi-core 实现 |
| D3 | 实体语义：**Session → Task → Run**；一次会话可发起/系统可创建多个 Task，每个 Task 由一个或多个 Run 执行 |
| D4 | 编辑器 = **Monaco**，写入边界 = **workspace 沙箱**（仅配置根目录内、原子写入、拒绝越界路径） |
| D5 | 左栏 git 面板**含写操作**（stage/commit/push 等，子进程参数数组传递 + git 可执行文件白名单） |
| D6 | 看板拖拽改状态列 = **自动触发 Run 执行**；必须提供等价键盘/菜单操作（a11y 规则） |
| D7 | 事件推送 = **多通道**：每 Session 独立通道 + 全局看板通道——修订 ipc-contract 规则见 ADR-0002 |
| D8 | 审批触发 = **可配置敏感工具名单**（默认含文件写入/shell 写/git 写/联网 fetch），命中即 `awaiting_approval` 暂停循环 |
| D9 | **Scheduler 本期包含**：cron/间隔触发器后端 + 管理页 |

## 1. 目标 / Goals

1. 内核补齐执行域实体：Task、Run（完整状态机 `queued→running⇄awaiting_approval→succeeded/failed/cancelled/timed_out`，含 orphan→interrupted 恢复）、审批门（敏感名单 Hook 拦截 + 暂停/恢复）、Scheduler（cron/间隔生成 queued Task）。
2. 新增受沙箱约束的 WorkspaceService（列目录/读/原子写）与 GitService（status/log/worktree 列表/stage/commit/push，白名单 git 子进程），供 IPC 层暴露。
3. Tauri 2 薄壳脚手架：tauri-specta 契约链路（commands + 多通道事件 emit + test-double），全部命令返回 `Result<T, IpcError>`。
4. React 前端：三栏主界面、看板、SubAgent/群聊 Trace 视图、Approvals 收件箱、Scheduler 管理页、全量 Settings；zh-CN 默认 + en 平行 i18n；暗色优先。
5. ADR-0002：事件多通道架构修订 ipc-contract 规则。

## 2. 用户故事 / User Stories

- US1 打开应用看到三栏主界面；左侧选会话，中间与 agent 对话并实时看到流式 token 与折叠的 tool_call 卡片。
- US2 我在会话中创建 Task，卡片出现在看板 queued 列；拖到 running 列自动派发执行，Run 详情页实时滚动事件流（按 seq 断线补拉）。
- US3 Agent 调用文件写入命中敏感名单，Run 变为 awaiting_approval；我在收件箱看到 diff 预览，批准后循环继续，拒绝则该 tool_call 返回拒绝结果。
- US4 我在左栏浏览 workspace 文件树，点击文件打开 Monaco tab 编辑并保存（越界路径被拒并有 toast 提示）。
- US5 左栏 git 面板查看 status/log/worktree，勾选变更 stage 后填写 message commit、一键 push。
- US6 群聊会话的 Trace 视图显示各成员发言时间线与 Handoff 链（控制权转移、环检测告警），旁挂 WhiteBoard 笔记流。
- US7 Scheduler 页创建 cron 任务「每天 9 点生成重构 Task」；到点看板自动出现 queued 卡片。
- US8 Settings 中新增 Anthropic Provider、录入 keyring 密钥、编辑审批敏感名单、开启 Evolution 在线学习授权。

## 3. 验收标准 / Acceptance Criteria

**内核新增**
- [ ] AC1 RunState 表驱动测试覆盖每条合法迁移 + 每类非法迁移；状态迁移先落库 EventRecord 再副作用（铁律）。
- [ ] AC2 审批门集成测试：fake provider 触发敏感工具 → Run 进入 awaiting_approval 且循环暂停 → approve 恢复执行成功 / deny 返回拒绝结果继续；名单可配置且热生效。
- [ ] AC3 Scheduler 测试：cron/间隔表达式到点生成 queued Task；取消调度器后不再触发。
- [ ] AC4 WorkspaceService：根内读写列目录正常；越界路径（`..`、绝对路径外、符号链接逃逸）一律拒绝；写入为原子替换（临时文件+rename）。
- [ ] AC5 GitService：只接受白名单子命令；参数数组传递无 shell 拼接；非 git 目录报明确错误。
- [ ] AC6 心跳/orphan：running 的 Run 崩溃恢复为 interrupted 且可 requeue。

**IPC / 壳**
- [ ] AC7 tauri-specta bindings 全链路跑通；每个命令四处一致（Rust 定义/注册/bindings/test-double）；错误统一 `IpcError{code}` 且 code 稳定可映射 i18n。
- [ ] AC8 多通道事件符合 ADR-0002：session 通道携带单调 seq，前端断线重连按 seq 补拉（集成测试）。

**前端**
- [ ] AC9 三栏布局三种形态可达：默认 / 打开文件（Monaco tab）/ SubAgent Trace；路由级 ErrorBoundary；每个异步区域 loading/empty/error 三态齐全，失败 toast 且保留输入。
- [ ] AC10 关键流组件测试（Vitest+Testing Library，走 test-double）：创建 Task、审批操作、查看实时日志、编辑保存文件。
- [ ] AC11 看板拖拽有键盘/菜单等价操作；列表虚拟化；高频 token 用 rAF 批量 flush。
- [ ] AC12 所有文案经 i18n（zh-CN 默认/en 齐全）；颜色间距只用 theme token。
- [ ] AC13 质量门全绿：Rust 三件套 + `pnpm typecheck && pnpm lint && pnpm test`。

## 4. 非目标 / Non-goals

Telemetry 上报、飞书/QQ Bot；sqlite-vec 向量检索；移动端/多窗口；协同编辑；git rebase/merge 冲突解决器；E2E 测试（M2+）。

## 5. 技术约束 / Technical Constraints

- 锁定栈不变：Tauri 2 + React18 + TS strict + Zustand(易失态)/TanStack Query(IPC 数据) + Tailwind(theme token) + Vitest；IPC 类型只从 `bindings.gen.ts` 导入。
- 前端不直接触盘/进程——一切经 Workspace/Git 服务命令；Zustand 禁存 IPC 数据副本。
- cron 解析用 Rust crate（引入时在任务中说明理由）；Monaco 经 `@monaco-editor/react` 懒加载。
- 子进程一律参数数组 + 可执行白名单 + kill on cancel/drop。
- 迁移只增不改：Task/Run/approval/schedule 新表走新编号迁移。

## 6. 任务拆分 / Task Breakdown

| # | 任务 | 内容 | 前置 |
|---|---|---|---|
| U0 | ADR-0002 + SPEC 落盘 | 修订 ipc-contract 事件通道规则；本 SPEC 入库 | – |
| U1 | Tauri 脚手架 | pnpm + Vite + Tauri2 + specta 链路 + IpcError + test-double 骨架 + CI 门 | U0 |
| U2 | 内核 Task/Run | 实体/迁移/repository + 状态机（表驱动）+ 孤儿恢复 | U0 |
| U3 | 审批门 | 敏感名单配置存储 + PreToolCall 暂停/恢复机制 + 收件箱数据面 | U2 |
| U4 | Workspace/Git 服务 | 沙箱 FS + 白名单 git 子进程服务 + 单测 | U2 |
| U5 | Scheduler | cron 解析/触发生成 Task + 管理数据面 | U2 |
| U6 | 事件桥 | EventBus→多通道 emit + seq 补拉命令 + 集成测试 | U1,U2 |
| U7 | Commands 层 | 全部 IPC 命令四步走 | U1–U6 |
| U8 | 前端骨架 | 三栏 shell + 路由 + theme tokens + i18n + useDomainEvents(rAF) | U1 |
| U9 | 对话界面 | sessions 树 + bubble 流 + 输入框 + 折叠工具卡 | U8 |
| U10 | 文件面板 | 文件树 + Monaco tab + 越界提示 | U8 |
| U11 | 看板 | 列式看板 + 拖拽(+a11y) + 自动派发 + Run 详情抽屉 | U8 |
| U12 | Trace 视图 | 群聊时间线 + Handoff 链 + WhiteBoard 流 | U8 |
| U13 | Approvals/Scheduler/Settings | 收件箱 diff 预览 + 定时任务页 + 设置页 | U8 |
| U14 | 收尾验收 | AC 核验 + 双端质量门 + 关键流组件测试齐备 | 全部 |
