---
description: 评审当前改动 / Structured review of the working diff by qa-guardian.
agent: qa-guardian
---

Review the current uncommitted changes (working diff) in this repository. Focus area hint: **$ARGUMENTS** (may be empty).

## Steps / 步骤
1. `git status` + `git diff` to enumerate every changed file. Nothing outside the diff is in scope unless a change interacts with it.
2. Walk the full qa-guardian checklist from your instructions: rules compliance, contract integrity, state-machine coverage, async triad, security.
3. Run both gate suites and capture output summaries.
4. Deliver the verdict in the mandated format: VERDICT / Blockers (file:line + rule) / Warnings / Tests added / Gate evidence.

If the diff touches IPC types without regenerated bindings or test-double updates, that is an automatic BLOCK per `.opencode/rules/ipc-contract.md`.
