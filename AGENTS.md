# nuomi · 糯米 — Agent Harness 内核

> 插件化 Agent Harness：以 Rust 自研插件内核为心脏，打通 Provider 编排、多 Agent 协作、长期记忆与 Self-Evolution——先内核、后 UI。
> A plugin-based Agent Harness built on a self-developed Rust plugin kernel (Cordis-inspired): provider orchestration, multi-agent collaboration, long-term memory, and self-evolution — kernel first, UI later.

本文件是所有 AI 开发者（人类与 agent）的最高上下文。规则细则在 `.opencode/rules/`，团队定义在 `.opencode/agent/`。
**需求权威是 `pr.md`；本文件与其冲突时，以 pr.md 为准并修订本文件。**
This file is the top-level context for every developer (human or agent). Detailed rules live in `.opencode/rules/`; the team roster lives in `.opencode/agent/`. `pr.md` is the authoritative requirements doc.

---

## 1. 产品定位 / Product Vision

- **一句话 / One-liner**: 可插件化的 Agent Harness 内核 + 终端验收入口：Provider Master-Slave 编排、Role/Team 多 Agent 协作（Selector+Handoff 群聊）、Long-term Memory 与 Self-Evolution。
- **核心价值 / Core values**: 一切组件皆插件 · 可观测 (append-only 事件溯源) · 可编排 (Pipeline/Router/群聊) · 可演进 (GEPA 式反思进化 + 白名单联网学习)
- **参照系 / References**: Cordis（理念借鉴）、Pi、Hermes Agent、Orca、Deepseek Harness、OpenAI Swarm（Handoff 模式）
- **权威 SPEC / Canonical spec**: `docs/specs/harness-kernel-v1.md`

## 2. 技术栈 / Tech Stack（锁定，不得擅自更换）

| Layer 层 | Choice 选型 |
|---|---|
| Desktop 壳 | **Tauri 2**（Rust 核心 + 系统 WebView）|
| Frontend 前端 | **React 18 + TypeScript(strict) + Vite** |
| UI 状态 | **Zustand**（本地 UI 态）+ **TanStack Query**（IPC 数据态）|
| Styling 样式 | **Tailwind CSS**（暗色优先，design token 化）|
| Backend 核心 | **Rust (edition 2021) + tokio** 异步运行时 |
| Persistence 存储 | **SQLite**（rusqlite + WAL，编号 SQL 迁移，只增不改）|
| IPC 契约 | **tauri-specta** 生成 TS bindings —— 契约单一来源 |
| 工具链 | pnpm (Node ≥ 20) · cargo · vitest · cargo test/clippy |

> 更换以上任何选型必须先写 ADR（`docs/adr/NNNN-title.md`）并获得用户确认。
> Swapping any of these requires an ADR first and explicit user approval.

## 3. 架构地图 / Architecture Map

```
nuomi/
├─ AGENTS.md                  # 本文件 this file
├─ opencode.json              # opencode 项目配置（权限、规则注册）
├─ pr.md                      # ★ 需求权威 authoritative requirements
├─ docs/
│  ├─ adr/                    # 架构决策记录 Architecture Decision Records
│  └─ specs/                  # 权威 SPEC（当前: harness-kernel-v1.md）
├─ crates/                    # ★ Rust 核心 workspace（内核优先阶段的主战场）→ owner: core-engineer
│  ├─ nuomi-core/             #   业务逻辑 lib（唯一实现，供 cli 与未来 tauri 壳共享）
│  │  ├─ src/domain/          #   领域实体 + 运行状态机 entities + run state machine（单一事实源）
│  │  ├─ src/harness/         #   插件内核 kernel · plugin registry · context · event bus · sideload (第三方插件 NPP, ADR 0009)
│  │  ├─ src/providers/       #   Provider 客户端与 Master-Slave 编排 (OpenAICompatible/AnthropicCompatible)
│  │  ├─ src/orchestrator/    #   Role/Team 执行器：Pipeline · Router · 群聊(Selector+Handoff) · WhiteBoard
│  │  ├─ src/store/           #   SQLite repositories + migrations runner
│  │  └─ src/evolution/       #   Self-Evolution：轨迹聚合 · prompt 版本化 · 白名单联网调研
│  └─ nuomi-cli/              #   headless 验收入口 bin：run / resume / REPL / plugin list → owner: bridge-engineer
├─ migrations/                # NNN__name.sql 只增不改 append-only（随 nuomi-core 打包）
├─ examples/plugins/          # 第三方插件示例（upper: 工具+钩子+事件, Python/Node, ADR 0009）
├─ docs/plugins/              # 插件开发文档（manifest 格式 / NPP 协议 / 入门教程）
├─ src-tauri/                 # （UI 里程碑回归时启用）Tauri 薄壳 bin，依赖 nuomi-core → owner: bridge-engineer
├─ src/                       # （UI 里程碑回归时启用）React 前端 frontend → owner: ui-engineer
│  ├─ features/<domain>/      #   board · runs · agents · approvals · scheduler · settings
│  ├─ components/ui/          #   设计系统原子 design-system primitives
│  ├─ lib/ipc/                #   生成的 IPC 绑定 generated bindings（禁止手改 never hand-edit）
│  └─ lib/store/              #   zustand stores
└─ .opencode/                 # AI 开发团队配置 dev-team config
```

**边界规则 / Boundary rules**
1. `commands/` 只做参数校验与转发；业务逻辑一律在 `domain/` 与 `orchestrator/`。Commands stay thin; logic lives in domain/orchestrator.
2. 前端不直接触碰文件系统/进程/SQL，一切经由 IPC 契约。Frontend never touches fs/process/sqlite directly — IPC only.
3. orchestrator 只依赖 trait `AgentAdapter`，不感知具体 vendor。Adapters hide vendors behind the `AgentAdapter` trait.
4. 跨层修改（契约变更）由 bridge-engineer 或 conductor 统一协调。Cross-layer contract changes are coordinated, never ad-hoc.

## 4. 领域模型 / Domain Model（权威定义 canonical）

| Entity 实体 | 含义 / Meaning |
|---|---|
| **ProviderConfig** Provider 配置 | 一个模型服务端点：协议（OpenAICompatible/AnthropicCompatible）、base URL、keyring 密钥引用、能力标签、Master/Slave 角色。A model endpoint config with capability tags and master/slave role. |
| **Role** 角色 | 同一 Provider 之上的行为覆盖层：SystemPrompt 覆盖、工具集白名单、温度等参数；params.agent_profile_id 可绑定 CLI Agent。Behavior overlay on a provider; params.agent_profile_id binds a CLI Agent. |
| **Team** 团队 | 多个 Role 的组合与协作拓扑（pipeline/router/group_chat）+ 群聊参数（最大轮数、Selector 配置）；经 services/team_runner 物化执行，Run 行生命周期接入 tasks_runs。Composition of roles + collaboration topology; materialized and executed via services/team_runner, run lifecycle wired into tasks_runs. |
| **AgentProfile** 智能体档案 | 可执行的 agent 定义：名称、adapter 类型（v1 支持 cli，claude_code/codex/plain 三方言）、启动命令或 provider+role 绑定；经 providers/adapters 的 LlmProvider 接入编排，配置存 SQLite agent_profiles（SPEC 见 docs/specs/cli-agents-m1.md）。An executable agent definition. |
| **Session** 会话 | 一轮对话/转录，可续传可回放（`nuomi resume`）。Resumable conversation. |
| **EventRecord** 事件 | 只追加的事件日志：`thought \| tool_call \| tool_result \| message \| state_changed \| usage`。Append-only event log. |
| **WhiteBoardNote** 黑板笔记 | 群聊共享黑板上的 append-only 结构化条目，各 Agent 可读写并同步进上下文。Shared group-chat blackboard entry. |
| **MemoryEntry** 长期记忆 | 跨会话记忆：内容、来源会话、标签；SQLite 存储 + 关键词检索（向量后置）。Cross-session memory. |
| **PromptVersion** Prompt 版本 | SystemPrompt 的版本化记录：候选→激活状态机，带 diff；由 Evolution 引擎产出。Versioned system prompt candidate. |
| **Artifact** 产物 | Run 产生的文件/输出，带保留期清理策略。Outputs produced by runs, with retention policy. |

### Run 状态机 / Run Lifecycle State Machine

```
queued ─▶ running ⇄ awaiting_approval ─▶ succeeded
             │   │
             │   └──▶ cancelled        (user 用户主动)
             └──────▶ failed | timed_out
崩溃恢复 crash recovery: running --(orphan 心跳超时)--> interrupted ─▶ requeue 可重新入队
```

- **铁律 / Iron rule**: 每次状态迁移必须先把 `state_changed` EventRecord 落库，再产生外部副作用。
  Persist the `state_changed` event BEFORE any external side effect.
- Rust 端 `domain/run_state.rs` 的枚举是唯一权威；TS 端 union 通过生成的 bindings 镜像，两端禁止各写一份手抄版。
  The Rust enum in `domain/run_state.rs` is the single source of truth; TS mirrors it via generated bindings only.

## 5. 产品表面 / Product Surfaces

内核优先阶段（当前）：**headless CLI**（`nuomi run` / `nuomi resume` / REPL）为唯一产品表面。
UI 里程碑回归时启用：Board 看板 · Run 详情（实时事件流）· Agents 管理 · Approvals 收件箱 · Scheduler 定时任务 · Settings（provider 密钥，OS keyring 加密存储）。
桌面壳与 CLI 通过同一 SQLite 库共享会话——设 `NUOMI_DB_PATH` 指向同一文件（或 CLI `--db`），任一端创建的会话可在另一端 list/resume 续传。

## 6. 开发命令 / Development Commands

```bash
pnpm install                 # 安装依赖 install deps
pnpm tauri dev               # 桌面开发模式 desktop dev
pnpm tauri build             # 打包 build bundles
pnpm typecheck && pnpm lint  # 前端静态检查 frontend static checks
pnpm test                    # vitest 单测 frontend tests
pnpm contracts:gen           # 重新生成 IPC 绑定 regenerate TS bindings（改 Rust 类型后必跑，UI 阶段）
cargo fmt --all              # 格式化 format (workspace root)
cargo clippy --all-targets -- -D warnings   # lint 必须零警告 must be clean
cargo test                   # Rust 测试 rust tests
pnpm coverage:rust           # Rust 行覆盖率 summary（cargo-llvm-cov；下钻 HTML: pnpm coverage:rust:html）rust line coverage
```

**跨平台构建 / Cross-platform builds**（CI 未实测）
- CI 入口：`.github/workflows/release.yml`——push tag `v*` 或手动触发；windows/macos/ubuntu 三平台矩阵，桌面 bundle 与 CLI（`nuomi-cli-{win64|macos-arm64|macos-x64|linux-x64}`）均以 workflow artifacts 上传。
- 本机前置：Linux 需系统库 `libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev patchelf`（Debian/Ubuntu 系）；macOS 需 Xcode CLT（`xcode-select --install`）；Windows 无额外依赖。
- 本机构建命令不变：`pnpm tauri build` 出各平台原生 bundle；`cargo build --release -p nuomi-cli` 出 CLI。

## 7. 工作流硬规则 / Workflow Hard Rules

1. **契约先行 / Contract-first**: 改 Rust 类型 ⇒ 同一提交内重新生成 bindings + 更新消费端。One commit touches type + regenerated bindings + consumers.
2. **完成需证据 / Evidence-before-done**: `lint + typecheck + tests` 全绿才算完成，汇报时附命令输出摘要。Done means gates green; attach evidence.
3. **迁移只增不改 / Migrations append-only**: 已发布的 migration 文件永不编辑。Never edit shipped migration files.
4. **密钥安全 / Secrets**: API key 只经 OS keyring / 加密存储；不进仓库、不打进 INFO 以上日志。Keys via OS keyring; never in repo or logs.
5. **禁止抑制 / No suppression**: 无 `unwrap()`（测试外）、无 `as any`、无 `@ts-ignore`。No unwrap outside tests, no any/ts-ignore.
6. **提交纪律 / Commit discipline**: 仅在用户明确要求时 git 提交。Commit only when explicitly asked.
7. **重大决策 / Major decisions**: 写 ADR 到 `docs/adr/`，等用户确认再动工。Write an ADR and wait for user confirmation.
8. **实现原理沉淀 / Principle docs**: 每次完成核心功能编码后，把实现原理（设计思想、关键权衡、数据流/调用链）追加或更新到根目录 `principle.md`（该文件不入库）。After core feature coding, record implementation principles into the repo-root `principle.md` (gitignored).

## 8. 开发团队 / AI Dev Team（`.opencode/agent/`）

| Member 成员 | Role 职责 | Owns 负责区域 |
|---|---|---|
| **conductor** 指挥 | 规划拆解、派单、跨层集成、最终验收 | 全局 cross-cutting |
| **ui-engineer** 前端工程 | React 功能实现与视觉质量 | `src/**`（不含 lib/ipc 手写部分；UI 里程碑回归时启用）|
| **core-engineer** 核心工程 | 领域模型、插件内核、编排引擎、存储、Evolution | `crates/nuomi-core/src/{domain,harness,providers,orchestrator,store,evolution}` |
| **bridge-engineer** 桥接工程 | headless CLI、agent 适配器、（未来）IPC 契约与 bindings | `crates/nuomi-cli` + `src-tauri/{commands,adapters}` + `src/lib/ipc` |
| **qa-guardian** 质量守护 | 评审、回归测试、质量门禁 | 仅测试与评审报告 tests & review reports |

在 opencode 中用 @mention 调度；常用入口见 `.opencode/command/`（`/feature`、`/review`、`/check`）。

## 9. 当前阶段 / Current Phase — Harness 内核优先（Kernel-First）

里程碑路线 / Milestones: **K0** workspace 脚手架 + 迁移框架 → **K1** 插件内核 (Plugin/Context/EventBus) → **K2** 存储层 repositories → **K3** Provider 层 (OpenAICompatible/AnthropicCompatible) + Master-Slave 编排 → **K4** Loop Engine (ReAct) + SystemPrompt 插件 → **K5** Memory/Hook/MCP 插件 → **K6** Role/Team 编排（Pipeline · Router · 群聊 Selector+Handoff）+ WhiteBoard → **K7** headless CLI (run/resume/REPL) → **K8** Self-Evolution（反思进化 + 白名单联网学习）。
已交付 / Delivered: **M-CLI1** CLI Agent 接入（adapters/cli + AgentProfile 存储 + Settings 管理）✅；**M-TEAM1** Team 编排接入桌面壳（team_runner 物化注册表 + Run 生命周期接线 + Roles/Teams 管理与看板运行入口）✅；**M-BOT1** Telemetry/Bot 集成（出站 sink 抽象 + 飞书签名 webhook + 批量遥测导出 + Settings 管理）✅。下一目标 / Next: pr.md 主需求已全量落地，转入打磨与增强阶段（候选：亮色主题打磨、Monaco 体验、自发组队 dry-run 预览等）。
UI 里程碑回归时恢复: Board → Approvals → Scheduler → DAG。

绿地纪律 / Greenfield discipline: 目录结构与命名现在定死；宁可先建空模块 + TODO 占位，也不要出现第二套并行约定。Structure beats consistency-recovered later.
