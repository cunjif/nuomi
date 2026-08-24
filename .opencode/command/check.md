---
description: 一键质量门检查 / Run all quality gates and report a compact evidence table.
---

Run the full quality gate suite for nuomi and report results. Do not fix anything — just measure.

## Gates / 门禁（全部执行，逐条记录）
1. From `src-tauri/`: `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`
2. From repo root: `pnpm typecheck`, `pnpm lint`, `pnpm test`

## Report format / 汇报格式
A table: gate | command | result (PASS/FAIL) | failure digest (first meaningful error lines only).

End with one line: `GATES: n/m green`. If any fail, list the owning team member per AGENTS.md §8 ownership map — do not attempt fixes.
