# prime-agent — 组件级研究与可吸收点

## 1. 定位与架构骨架
TS 单体 AgentSession（1.17 万行）+ Python IPython 内核（ZeroMQ/Jupyter 5.3）作为持久执行大脑；`refinement/` 持续自改进、`rlm-runtime.ts` 递归子代理、`autonomous.ts` 无人值守。关键目录：`packages/coding-agent/src/core/{refinement,tools,kernel,compaction}`、`agent-messages.ts`、`prime-agent-runtime/src/rlm/`。

## 2. Continual Harness 深挖
**数据结构**：`HarnessEntry{id,kind,title,content,scope,reference,arguments,metadata,version,created_at/updated_at}`（refinement/refinement.ts:34-48）；`HarnessState.entries` 按 prompt/memory/skill/subagent 四类分桶 + `refinements[]` 事件日志（:59-63）。**base system prompt 不可改**，prompt 类仅为补充笔记（:680-682）。

**触发**：手动 `/refine`；自动双触发——turn_interval（默认 25 轮）与 compact（压缩时），带 20 分钟冷却（settings-manager.ts:25-27）。触发后先过**审查门**（`AUTO_REFINE_REVIEW_SYSTEM_PROMPT`，refinement.ts:175-185）：LLM 判断轨迹是否含值得写入的证据，拒绝一次性噪音，返回 `shouldRefine` JSON。

**写回循环**：`planRefinement`（:880-948）取轨迹切片（末 80k 字符）+ harness 概览 + 近 20 条 refinement 历史 → 输出 create/update/delete JSON 提案（含 rationale 证据与 expectedOutcome）；`applyRefinementProposal`（:716-811）逐条校验后应用，version+1，事件落 `refinements[]`。

**稳定性保障（核心）**：
- **回滚**：每次应用记录 before/after 快照，`rollbackProposal`（:813-845）逆序还原；全局 refinement 追加至 `refinements.jsonl`（:374-379）支持跨会话回滚。
- **乐观并发**：apply 时对比 `baselineState`（规划期捕获），条目已被内核/他 会话改动则拒改："entry changed during refinement planning"（:735-749）。
- **去重/冲突**：create 冲突报 "entry already exists"（:760-762）；local refine 中 global 条目只读，需覆盖则建 local 条目（scope 隔离消解冲突，:144）。
- **契约校验**：skill 必须带 python reference+arguments 签名（:692-712），防止写入不可调用的坏技能。
- **原子落盘 + 容错**：temp+rename 写（:345-359）；损坏状态降级为空而非崩溃（:289-301）。
- **branchVersion 守卫**：agent-session.ts:2421/2430 应用前校验会话分支版本，过期提案直接丢弃。

```ts
// refinement.ts:716-749（节选）apply 前的校验与基线冲突检测
const before = cloneEntry(records[id]);
const baseline = cloneEntry(options.baselineState?.entries[edit.kind][id]);
if (options.baselineState && !proposalModifiedKeys.has(entryKey)
    && JSON.stringify(before) !== JSON.stringify(baseline)) {
  appliedEdits.push({ ...edit, id, before, applied: false,
    error: "entry changed during refinement planning" });
  continue;
}
if (edit.action === "create" && before) {
  appliedEdits.push({ ...edit, id, before, applied: false, error: "entry already exists" });
  continue;
}
```

## 3. RLM 递归子代理与 agent_message
- **深度控制**：spawn 前 `_rlmDepth >= _rlmMaxDepth` 即拒（agent-session.ts:10212-10214）；默认深度 2，`RLM_MAX_DEPTH` env 可调（settings-manager.ts:136，agent-session.ts:1585-1587）。
- **注册表**：`RlmSubagentRegistryEntry{status: running|completed|error}`（rlm-runtime.ts:22-38）；`rlm.list_subagents()/delete_subagent` 管理子代。
- **通信**：admission 立即返回 handle（rlm_child_id/session_dir/model），**永不返回答案**——结果只能经 `agent_message.send(receiver_role="parent")` 或文件回传（refinement.ts:138）；家族 reach 限 parent/sibling/child，禁止全局广播（agent-messages.ts:25,322-329）；单条 ≤16384 字符、每会话 pending ≤20、令牌桶限速 3/s（:13-16）。

## 4. 其它差异化机制
- **IPython 持久内核**：kernel/state-snapshot.ts:5-15——逐变量 pickle 快照，超限/不可序列化变量跳过并报告而非整体失败；ZeroMQ + fork server 免冷启动。解决"长任务变量跨轮保持"。代价：Python 生态绑定。
- **孤儿进程日志**：orphan-process-journal.ts:20-90——启动时记录 pid，重启后按身份核验清理泄漏内核。
- **文件变更队列**：tools/file-mutation-queue.ts:19——按路径串行化写操作防并发冲突。
- **自主模式四重预算**：autonomous.ts:51-54——maxContinuations=3/maxTurns=12/maxTokens=80k/timeout=30min + 质量门（:273-284），决策枚举含 `missing_terminal_evidence`。

## 5. 组件评分表
| 维度 | 分 | 理由 |
|---|---|---|
| 编排 | 4 | 递归子代理动态生成，但无 pipeline/router 拓扑 |
| 沙箱 | 3 | 内核进程隔离 + 孤儿防护，无权限审批体系 |
| 持久化 | 4 | 快照+JSONL 历史+原子写，但非 append-only 事件溯源 |
| 扩展性 | 5 | 生命周期事件 + 自定义工具/命令/UI 全覆盖 |
| 上下文 | 4 | 压缩+快照+harness 外置，单体会话是隐患 |
| 路由 | 3 | 模型匹配打分简单，无 Master-Slave |
| 可观测 | 4 | telemetry/timings/stats 全链路 |

## 6. 可吸收清单
| 机制 | 收益 | 难度 | 实现要点 | 冲突点 |
|---|---|---|---|---|
| 基线冲突检测+回滚快照 | 演进不被并发写坏 | 低 | refinement 事件记录 before/after，apply 前比对 | 无，契合 append-only 事件 |
| 审查门+冷却+turn_interval | 防自我修改风暴 | 低 | 演进调度器加 review LLM 门控 | nuomi scheduler 需加冷却状态 |
| scope 隔离（local/global 只读） | 控制爆炸半径 | 低 | 记忆条目加 scope 字段 | 与 MemoryEntry 现结构兼容 |
| base prompt 不可变约定 | 系统稳定底线 | 低 | PromptVersion 候选永不覆盖 base | 无 |
| agent_message 家族通道 | 群聊/子代理安全通信 | 中 | reach 限制+配额+限速，复用白板 | 与 group_chat Selector 并行，需整合 |
| 深度/预算门 | 递归不失控 | 低 | depth 计数在 spawn 入口拒绝 | 契合 orchestrator |
| 孤儿进程日志 | 崩溃恢复清理 | 中 | Run 心跳超时已有，补 pid journal | 与 interrupted 状态机衔接 |

## 7. 明确不建议吸收
- **单体 AgentSession**：1.17 万行巨石与 nuomi 插件内核相悖，仅取机制不取结构。
- **IPython 内核**：Rust 无对应生态，且 ZMQ/pickle 依赖重；nuomi 长任务上下文应由 WhiteBoard + 快照事件承担。
- **JSONL 文件态 harness 存储**：nuomi 已有 SQLite WAL + 事件溯源，勿引入第二存储。
- **自动改 prompt 直接生效**：prime-agent 仅靠审查门+回滚，无评估闭环；nuomi 应走 PromptVersion 候选→评估→激活状态机，更稳。
