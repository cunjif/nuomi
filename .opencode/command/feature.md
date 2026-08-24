---
description: 规划并派发一个纵切特性 / Plan & dispatch a vertical feature slice across the nuomi team.
agent: conductor
---

You are planning a new feature slice for nuomi. Feature request: **$ARGUMENTS**

## Steps / 步骤
1. Orient: read root `AGENTS.md` (§4 domain model, §9 current milestone) and survey existing code under `src/` and `src-tauri/src/` relevant to this feature.
2. Decide scope: which milestone does this belong to? If it exceeds or reorders milestones, STOP and escalate to the user first.
3. Produce numbered work orders — each with: owner (ui-engineer | core-engineer | bridge-engineer), goal, exact file paths, rules files to follow, done-criteria, dependencies. Contract touchpoints (new IPC commands/events) must be their own work order assigned to bridge-engineer BEFORE dependent UI/core orders.
4. Mark parallel-safe orders explicitly, then dispatch them in parallel; run sequential ones in dependency order.
5. After all orders complete: verify integration yourself per AGENTS.md §7 (contract lockstep + full gates) and report evidence.

## Output / 输出
Work-order table (id, owner, summary, deps, done-criteria) → dispatch → acceptance report with gate evidence and leftover TODOs. 中英双语汇报。
