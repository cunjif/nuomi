# Rules — Git 工作流 / Git Workflow

## 提交信息 / Commit messages（Conventional Commits）
- 格式：`type(scope): subject`，scope 取 `ui | core | ipc | board | runs | agents | approvals | scheduler | deps | docs`。
- 示例 Examples:
  - `feat(board): drag-to-transition for task cards`
  - `feat(ipc)!: replace run.status string enum with tagged union`
  - `fix(core): reap orphaned child process on run cancellation`

## 分支 / Branches
- `main` 为主线；功能分支 `feat/<slug>`、修复 `fix/<slug>`，小而短命。Short-lived small branches off main.
- 破坏性契约变更单独成提交并用 `!` 标记，且在同提交内完成 bindings 再生（ipc-contract.md）。Breaking contract changes isolated + regenerated in same commit.

## 提交纪律 / Discipline
- **仅在用户明确要求时提交。** Commit only when the user explicitly asks.
- 小步提交：一个提交一个意图；禁止把格式化、重构、功能混在一个提交。One intent per commit.
- 禁止提交 / Never commit: `node_modules/`、`src-tauri/target/`、`dist/`、`*.db`、`.env*`、任何密钥。脚手架阶段先落 `.gitignore`。

## 交接前检查 / Pre-handoff checklist
1. 全部质量门绿灯（testing.md 命令清单）。All quality gates green.
2. `git diff --stat` 与改动文件清单写入汇报。Include diff summary in the report.
3. 未完成事项明确列出 TODO 清单，不留隐式半成品。Explicit leftover-TODO list; no implicit half-done work.
