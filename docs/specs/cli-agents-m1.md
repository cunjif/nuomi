# SPEC: Nuomi M-CLI1 — CLI Agent 接入（cli-agents-m1）

> 状态：已批准
> 前置：`docs/specs/harness-kernel-v1.md`（已交付，其 D5 曾推迟本项）；UI 部分依赖 `docs/specs/ui-m1.md` 的 Settings 页。
> 需求来源：`pr.md` §2「支持不同 Cli Agent 接入」（Codex CLI、Claude Code、OpenCode CLI 等）；需求权威仍为 `pr.md`。

## 0. 决策记录摘要

| # | 决策 |
|---|---|
| D1 | 需求来源 `pr.md` §2「支持不同 Cli Agent 接入」（Codex CLI、Claude Code、OpenCode CLI 等）；`harness-kernel-v1` D5 曾推迟本项，本期落地 |
| D2 | CLI Agent 以实现现有 **`LlmProvider` trait** 的方式接入（`crates/nuomi-core/src/providers/client.rs`）：`id()` / `complete()` / `stream()` → Pipeline / Router / GroupChat 执行器与 ProviderResolver **零改动**，CLI Agent 即成为 Team 成员 |
| D3 | 新模块 `crates/nuomi-core/src/adapters/`（`cli.rs`），不改动 providers 既有协议客户端（OpenAICompatible / AnthropicCompatible 不受影响） |
| D4 | 协议方言 v1 三种 **flavor**：`claude_code`（`claude -p "{prompt}" --output-format stream-json --verbose` 的 JSONL）、`codex`（`codex exec --json "{prompt}"` 的 JSONL）、`plain`（stdout 按行纯文本）。解析必须容错：未知行跳过不报错 |
| D5 | prompt 传递约定：args 模板支持 `{prompt}` 占位符（替换为单条参数，禁止 shell 字符串拼接）；模板无占位符时 prompt 写入 stdin 后关闭 stdin |
| D6 | 安全铁律：arg 数组传递（`tokio::process::Command` 无 shell）；可执行文件白名单校验（构造 `CliAgentClient` 时显式传入 allowlist，空 allowlist = 全部拒绝并报明确错误）；`.kill_on_drop(true)` 保证取消/丢弃即回收子进程；stderr 只截断进错误消息，密钥与完整 prompt 不打 INFO 以上日志；工作目录默认 workspace root |
| D7 | 数据模型：migration `migrations/0003_agent_profiles.sql`（append-only）建表 `agent_profiles(id TEXT PK, name TEXT UNIQUE NOT NULL, adapter TEXT NOT NULL DEFAULT 'cli' CHECK IN ('cli'), flavor TEXT NOT NULL CHECK IN ('claude_code','codex','plain'), command TEXT NOT NULL, args TEXT NOT NULL DEFAULT '[]' /*JSON数组*/, env TEXT NOT NULL DEFAULT '{}' /*JSON对象*/, working_dir TEXT NULL, enabled INTEGER NOT NULL DEFAULT 1, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL)`；domain 实体 `AgentProfile { id, name, adapter, flavor: CliFlavor, command, args: serde_json::Value, env: serde_json::Value, working_dir: Option<String>, enabled: bool, created_at, updated_at }`，serde 风格与现有 domain 实体一致（snake_case）；repo 收敛在 `store/repos/agent_profiles.rs`，SQL 参数绑定 |
| D8 | 测试策略：解析器为纯函数单测（合成 JSONL）；子进程机制用 fixture `crates/nuomi-core/tests/fixtures/fake_cli.js`（Node 脚本，dev 机必有 node，跨平台免编译；argv 选 flavor、读 stdin、输出确定性 JSONL/文本）；集成测试证明 CLI Agent 经 ProviderResolver 加入 PipelineExecutor 团队跑通；单测零网络 |
| D9 | IPC 四步契约链（ADR-0002 / ipc-contract 规则）：命令 `list_agent_profiles` / `upsert_agent_profile` / `delete_agent_profile` / `check_cli_agent`（探测可用性：以 `--version` 覆盖 args 运行+超时，返回 ok/version_line/error）；DTO camelCase + 内部标记枚举；IpcError code 如 `"agent_profile.not_found"`；同提交再生 bindings 并更新前端消费端与 test-double |
| D10 | UI：Settings 页新增「CLI Agents」管理段（列表 + 表单 name/flavor/command/args/env/enabled + 「检测」按钮显示 version 或 toast 错误），i18n zh-CN/en 双语，组件测试走 test-double |

## 1. 目标 / Goals

1. 任意外部 CLI Agent（Claude Code / Codex / 自定义脚本）注册为**一等 Team 成员**，参与 Pipeline / Router / GroupChat 编排——编排器与 Provider 解析层零改动。
2. CLI Agent 配置持久化（SQLite `agent_profiles`）并可经桌面 Settings 管理、探测可用性（版本检测）。
3. 子进程安全：白名单校验、arg 数组传递、取消即回收、无密钥泄漏。

## 2. 用户故事 / User Stories

- **US1** 开发者在 Settings 注册 `codex exec --json "{prompt}"` 为「架构师」CLI Agent，点击检测看到版本号；把该 profile 绑定到某 Role 的 provider_id 后发起 Pipeline 团队任务，CLI Agent 作为其中一站执行并把输出交给下一站。
- **US2** 开发者注册一个 plain 方言的自定义脚本 Agent；任务取消时子进程被立即回收。
- **US3** 白名单外的可执行文件被拒绝启动并给出明确错误。

## 3. 验收标准 / Acceptance Criteria

**内核 adapters 模块**
- [ ] AC1 `CliAgentClient` 实现 `LlmProvider`；经 `ProviderResolver::with(profile.id, client)` 注册后 `PipelineExecutor` 端到端集成测试通过（fixture fake_cli.js 提供确定性输出）。
- [ ] AC2 三种 flavor 解析器表驱动单测：合法行→正确事件映射（TextDelta / Completed / usage），非法/未知行→静默跳过；空输出→Completed(空)。
- [ ] AC3 白名单：allowlist 外命令返回明确错误（错误文本含命令名）；空 allowlist 拒绝一切。
- [ ] AC4 取消回收：stream 中途 drop 流后子进程终止（kill_on_drop 生效，测试轮询确认进程退出或用 fixture 落盘标记验证）。

**存储层**
- [ ] AC5 migration 0003 append-only、`user_version` 推进；repo 测试覆盖 happy path + 重复 name 冲突错误路径（tempfile SQLite）。

**IPC 契约链**
- [ ] AC6 IPC 四处一致：Rust 命令 + specta builder 注册 + bindings.gen.ts 再生 + 前端消费端/test-double 同提交更新；`check_cli_agent` 有超时保护（如 10s）。

**前端**
- [ ] AC7 Settings「CLI Agents」段组件测试通过（mock test-double）；全部新增文案 i18n 双语。

**质量门**
- [ ] AC8 双端质量门全绿：`cargo fmt / clippy -D warnings / test` 与 `pnpm typecheck / lint / test`。

## 4. 非目标 / Non-goals

OpenCode / Kiro / CodeBuddy 等其余 CLI 的专用方言（plain 兜底已可用）；CLI Agent 的流式双向会话（`--input-format stream-json` 多轮交互流）；HTTP / MCP adapter 类型（`AgentProfile.adapter` 仅 `'cli'`，枚举留扩展）；Team 编排接入桌面壳的端到端 UI（后续 M-TEAM1）；Telemetry / 飞书 / QQBot。

## 5. 技术约束 / Technical Constraints

- 锁定栈不变：Rust(edition 2021) + tokio + SQLite(rusqlite+WAL) + Tauri 2 + React18/TS strict。遵守 `.opencode/rules/rust-core.md`（SQLite 操作 `spawn_blocking` 包裹、每模块 thiserror 枚举、库路径无 `unwrap()/expect()`）与 `.opencode/rules/ipc-contract.md`（DTO camelCase、内部标记枚举、稳定 IpcError code、四处一致契约链）。
- 子进程：`tokio::process` + futures `BoxStream`；`stream()` 返回的流必须 self-contained（drop 即杀子进程）。
- 安全铁律沿用 D6：arg 数组、白名单、kill_on_drop、stderr 截断进错误消息、INFO 以上日志无密钥与完整 prompt 正文。
- 测试纪律：解析器纯函数单测 + fixture 子进程集成测试；单测零网络；migration 只增不改。

## 6. 任务拆分 / Task Breakdown

| # | 任务 | 前置 |
|---|---|---|
| C1 | SPEC + AGENTS.md 修订（AgentProfile 描述从「仅留占位」改为已实现 adapters/cli） | – |
| C2 | migration 0003 + AgentProfile/CliFlavor 实体 + repos::agent_profiles + 单测 | – |
| C3 | `adapters::{mod,cli}.rs`：CliAgentClient + 三 flavor 解析器 + fixtures/fake_cli.js + 单测 + 集成测试 | C2 |
| C4 | src-tauri：4 个命令 impl + DTO + builder 注册 + contracts:gen + 集成测试 | C2 |
| C5 | Settings「CLI Agents」段 + i18n + test-double + 组件测试 | C4 |
| C6 | 全量质量门复核 + AC 核验报告 | C1–C5 |
