# hermes-agent — 组件级研究与可吸收点

> 基于 NousResearch/hermes-agent 源码验证（Python 核心 + Ink TUI，~1.7 万测试）。分析地图 `references-analysis.md` 经源码抽查基本准确，以下均给出实测 file:line。

## 1. 定位与架构骨架

CLI-first 的全栈 Agent：`cli.py`（~21K 行 Mixin）+ `agent/` 核心循环 + `hermes_state*.py`（SQLite WAL + FTS5，SessionDB ~14.6K 行）+ `gateway/`（20+ 平台，run.py ~31.7K 行）+ 多层插件（通用/记忆/模型/context_engine/web 各自独立发现）。核心哲学是"窄腰"：核心工具 schema 每次调用都发送，新能力走 Skill/插件/CLI 命令而非加核心工具。

## 2. 上下文压缩算法深挖（`agent/context_compressor.py`，8454 行）

**触发**（`_compute_threshold_tokens`:3061-3100）：`threshold = effective_window × pct`，其中 `effective_window = context_length − max_tokens`（预留下输出空间）；默认 pct=0.50，窗口 <512K 时抬到 75%（`_effective_threshold_percent`:3044），floor≥窗口时退到 85% 防止永不触发（issue #14690）。支持按模型名子串匹配覆盖 + 绝对 token 上限。

**compress() 五阶段**（:7259）：
1. **廉价预裁剪**（`_prune_old_tool_results`:3727-3904，无 LLM）：MD5 去重重复工具输出（同文件读 5 次只留最新全文，旧副本换成 back-reference）；大结果替换为**带语义的 1 行摘要**而非通用占位符：
```python
# :3732 摘要形态
"[terminal] ran `npm test` -> exit 0, 47 lines output"
"[read_file] read config.py from line 1 (3,400 chars)"
# :3838 去重
h = hashlib.md5(content.encode("utf-8")).hexdigest()[:12]
if h in content_hashes:
    result[i] = {**msg, "content": "[Duplicate tool output — ...]"}
```
另截断老 assistant 消息里的 tool_call 参数 JSON（:1645）、剥离历史图片。
2. **边界**：头保护 system + 首个完整 exchange（protect_first_n=3，`_protect_head_size`:5997）；尾切点按 **token 预算回溯**（`_find_tail_cut_by_tokens`:6341，预算 = `summary_target_ratio(0.20) × context_length`，1.5× 软上限防切断超大消息，min_tail 消息数下限）。切点永不落在 tool_call/result 组内（`_align_boundary_backward`:6022），并单调锚定最后一条 user（#10896）与 assistant（#29824）消息在尾部。
3. **序列化中间轮** + **LLM 摘要**（`_generate_summary`:4662）：默认走便宜辅助模型（auxiliary_client），**刻意不设 max_tokens 硬帽**（Anthropic 线会把摘要截断在 thinking 上，:4980 注释）；空 content 视为失败进入主模型 fallback + 30-60s 冷却（#11978）。
4. **迭代更新**（:4924-4947）：二次压缩不是重摘，而是 `PREVIOUS SUMMARY + NEW TURNS → 更新`，模板含 `## Goal / Constraints / Completed Actions / Active State / Resolved Questions`，指令"已完成项从 In Progress 移入 Completed、继续编号、只删明显过时信息"。
5. **摘要注入**：单条 user 角色消息 + metadata 标记（`COMPRESSED_SUMMARY_METADATA_KEY`），随后 `_sanitize_tool_pairs`:5812 清理孤儿 tool 配对。

**不丢关键工具结果的三重保障**：① 预裁剪产生的是语义摘要而非占位符；② ghost-skill 防御（#32106，:3846）——刚加载/尾部仍引用的 skill 全文豁免降级；③ 失败时静态 fallback 摘要（:4257）保留 verbatim user 段落 + recovery 指针，宁可保底不丢上下文。

**与 prompt 缓存的冲突与权衡**：压缩是唯一允许的 mid-conversation 变更——接受一次全量缓存重建（无法避免），但通过 **cache scope = compression-lineage root**（`agent/prompt_cache_scope.py`:67）保证压缩轮换新 session_id 后 `prompt_cache_key` 仍指向旧 lineage 根，后续轮次继续命中同一 bucket（修 #79017）。即：**压缩牺牲一次性缓存，但不牺牲缓存隔离结构**。

## 3. prompt 缓存保护的硬约束清单

1. **系统 prompt 会话内字节稳定**（prompt_builder.py:850, 1940）——Skill 索引、环境信息一次性定格，LRU 缓存。
2. **Skill 内容按需以 user 消息注入**，不进 system prompt（skill_commands.py:667, 762）；slash 命令 scaffold 显式声明 Anthropic cache breakpoint（:407），`/reload-skills` 不破坏前缀缓存（:584-588）。
3. **严格角色交替**，三道防线：写入时 `_merge_adjacent_user_turns`（compressor:7218）；发请求前 `repair_message_sequence`（agent_runtime_helpers.py:562）Pass0 合并连续 assistant / Pass1 丢孤儿 tool / Pass2 `\n\n` 合并连续 user；各 transport 线端兜底（bedrock_adapter.py:806、anthropic_adapter.py:2498）。
4. failover 换后端时原位改写 system prompt 的 Model:/Provider: 行，保持字节结构（chat_completion_helpers.py:2370-2395）。
5. 工具集会话内固定（per-session toolset），中途换工具集 = 缓存失效，被禁止（prompt_builder.py:587）。

## 4. 其它差异化机制

- **FTS5 闭环学习**：`hermes_state_search.py`（2510 行）——FTS5 + trigram + CJK 专用索引表，后台分块回填（`fts_cjk_rebuild_step`:364），特殊字符引号包裹防 MATCH 语法炸（:46）。搜索经 `session_search` 工具暴露给模型（tools/session_search_tool.py:1065），跨压缩会话链（parent_session_id）可搜；curator.py 定期 LLM 评审整理 Skill/记忆。闭环 = 历史全量可检索 + 自动沉淀。
- **Honcho 辩证法用户建模**（plugins/memory/honcho/__init__.py）：5 个工具（profile/search/reasoning/context/conclude），peer 分 user/ai 双侧建模；dialectic agent 支持自然语言提问画像（cadence/depth 可调，:299-310）；`conclude` 把结论写入长期 profile 前馈后续会话。
- **AST 工具自注册**（tools/registry.py:111）：`ast.parse` 检测顶层 `registry.register()`（:100-108）避免全量 import；判定结果按 `(mtime_ns, size)` 磁盘缓存，~145ms/100 文件热启动。
- **安全纵深**：tools/threat_patterns.py（注入/promptware 扫描，上下文文件注入前必过）；tools/path_security.py（路径遍历）；agent/redact.py（凭证脱敏，压缩文本也过 `_redact_compaction_text`:1255）。
- **Cron + delegate**：cron/scheduler.py 60s tick + 跨平台文件锁；tools/async_delegation.py 完成事件持久化、跨重启补投（8 次上限/48h 终态）；delegate_tool.py spawn/steer/stop。

## 5. 组件评分表

| 维度 | 分 | 理由 |
|---|---|---|
| 编排 | 4 | delegate/Kanban 实用但无 Router/群聊拓扑 |
| 沙箱 | 4 | 7 种终端后端统一抽象，缺 OS 级隔离 |
| 持久化 | 5 | WAL+FTS5+CJK+会话链+崩溃恢复，业界标杆 |
| 扩展性 | 5 | 四层插件+AST 发现+窄腰，约束清晰 |
| 上下文 | 5 | 8.4K 行压缩器全是真实 issue 驱动的边界处理 |
| 路由 | 4 | fallback 链+辅助模型分任务路由，无 Master-Slave |
| 可观测 | 4 | 压缩遥测/budget 跟踪，缺事件溯源式回放 |

## 6. 可吸收清单

| 机制 | 对 nuomi 的收益 | 难度 | 实现要点 | 冲突点 |
|---|---|---|---|---|
| 廉价预裁剪（语义 1 行摘要+MD5 去重） | Token 创新最直接的落地：零 LLM 成本先回收大头 | 低 | loop_engine 压缩前独立 pass；摘要需 tool name+args 上下文 | 与 append-only 事件日志不冲突——裁剪只作用于发送视图，EventRecord 不动 |
| token 预算尾切点 + tool 组对齐 + user/assistant 锚定 | 压缩器正确性骨架，防切断工具对/丢当前任务 | 中 | `_estimate_msg_budget_tokens` 估算 + 单调锚点 | nuomi 尚无压缩器，直接按此设计 |
| 结构化摘要模板 + PREVIOUS SUMMARY 迭代更新 | 反思进化与压缩共用同一模板，信号不衰减 | 低 | evolution/reflection.rs 复用模板常量 | 需与 PromptVersion 状态机对齐（模板本身要版本化） |
| 严格 alternation 修复 pass + tool 配对 sanitize | Master-Slave/群聊 handoff 必然产生乱序消息 | 中 | providers 层发送前统一 repair | 群聊 Selector 多连发 user 消息时需合并策略 |
| cache scope = 会话 lineage root | resume/会话分裂后缓存 bucket 稳定 | 低-中 | sessions 表加 lineage 字段，prompt_cache_key 派生 | nuomi Run 状态机无"压缩轮换"概念，需引入逻辑会话 id |
| FTS5+CJK 全文检索 | 替换现有关键词检索，记忆/轨迹可召回 | 中 | rusqlite 支持 FTS5；CJK 需 trigram 或自写分词 | 现有 `store/repos/memory.rs` 检索接口需扩展 |
| 上下文文件威胁扫描 + 凭证脱敏 | 打磨阶段安全增强 | 中 | hook 插件 + redact 工具函数 | 无冲突，天然适配 approval_gate/hooks 层 |

## 7. 明确不建议吸收

- **20+ 平台 Gateway**：31.7K 行单文件是维护黑洞，nuomi 是桌面产品，飞书 sink（M-BOT1）已够。
- **Mixin 巨型 cli.py / run.py**：与 nuomi 插件内核哲学背道而驰，是反面教材。
- **Honcho SaaS 依赖**：外部服务 + OAuth，与 nuomi 本地 SQLite 优先、密钥不过仓库的原则冲突；可借鉴"peer 双侧建模+结论前馈"的思想但不引依赖。
- **7 种终端后端 / Kanban 看板**：nuomi 已有 Team 编排（Pipeline/Router/群聊），重复建设。
