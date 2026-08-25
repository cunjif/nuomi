# SPEC: Nuomi M-BOT1 — Bots & Telemetry（bots-telemetry-m1）

> 状态：已批准（决策 D1–D8 已定稿）
> 前置：`docs/specs/team-shell-m1.md`（**M-TEAM1 已交付**：壳侧事件桥 `src-tauri/src/events.rs` 的 ADR-0002 多通道 partition 与 `forward_event` 广播订户、tasks_runs Run 行生命周期、`store/repos` function-per-operation 惯例）；间接依赖 `docs/specs/harness-kernel-v1.md`（K1 插件内核的 EventBus `context().subscribe()` broadcast 总线）。
> 需求来源：`pr.md` §6「支持Telemetry、飞书Bot、QQBot」；需求权威仍为 `pr.md`。

## 0. 决策记录摘要

| # | 决策 |
|---|---|
| D1 | 新模块 `crates/nuomi-core/src/integrations/`：`#[async_trait] trait OutboundSink { fn kind(); async fn send(&self, title, body) -> Result<(), OutboundError> }`；错误枚举 `OutboundError(thiserror)`（http/协议/超时/配置缺失分变体） |
| D2 | migration `migrations/0004_integrations.sql` 建 `integrations(id, name UNIQUE, kind CHECK IN('feishu_bot','qq_webhook','telemetry'), config_json, events TEXT JSON数组(触发主题白名单；通知派发忽略 telemetry 行), enabled, created_at, updated_at)`；`domain::Integration{ kind: IntegrationKind, config: IntegrationConfig{webhook_url?, secret?}, events, ... }` 与 `repos::integrations` CRUD 照 `repos/roles.rs` 惯例（&Connection 函数式 + StoreError::NotFound） |
| D3 | FeishuSink：自定义机器人 webhook POST `{"msg_type":"text","content":{"text":"{title}\n{body}"}}`；config.secret 存在时加签——`sign = base64(hmac_sha256(key="{timestamp}\n{secret}", body=""))` 附顶层 `timestamp`/`sign` 字段；HTTP 非 200 或 `resp.code != 0` → `OutboundError` |
| D4 | GenericWebhookSink：POST `{"source":"nuomi","title","body","meta"}` 到配置 URL（QQ 消息网关与自建遥测端点的最小公约数）；TelemetryExporter：订阅内核总线、按该集成 events 白名单过滤域事件（空数组=全量域事件）、批量 NDJSON POST（每行 `{topic, taskId?, runId?, payload, ts}`；batch_size 默认 10 / flush_interval 默认 5s）、受 CancellationToken 监督、关闭即 flush 余量 |
| D5 | 安全：webhook URL 含访问令牌属敏感——DB 原样存但 DTO/UI 一律掩码显示（保留协议+域名+尾4位，如 `https://open.feishu.cn/***abcd`）；tracing INFO 以上不出现完整 URL 与正文；send 超时 10s |
| D6 | 壳侧接线：`src-tauri/lib.rs` setup 在事件桥（forward_event）之外再 spawn 一个通知派发任务——同一 broadcast 总线的第二个订户，按启用集成的 events 白名单匹配并格式化发送（run 终态 / approval.requested / team.formed 等，经对应 Sink 出站）；单条失败仅 tracing 告警，不重试、不阻塞、不崩溃；Lagged 处理同 forward_event |
| D7 | IPC 四步链：`list_integrations() -> Vec<IntegrationDto>` / `upsert_integration(input) -> IntegrationDto`（id 缺省新建，uuid v7）/ `delete_integration(id)` / `test_integration(id) -> TestIntegrationDto{ok,error}`（发测试消息；发送失败不抛 IpcError，建模为业务结果 `{ok:false, error:<掩码后原因>}`——测试命令的职责就是报告可达性结果）；错误 code 两枚：`integration.not_found` / `integration.invalid`（URL 非法、name 重复等，details.reason） |
| D8 | UI：Settings「集成」段（列表：kind 徽标 / 掩码 URL / enabled 开关；表单：name、kind 三选、webhookUrl、secret 可选、events 多选固定清单 `[run.state_changed, task.status_changed, team.formed, approval.requested]`；「发送测试」按钮 toast 结果）；i18n zh-CN/en 双语；组件测试走 test-double |

## 1. 目标 / Goals

1. **出站通知闭环**：内核总线上的域事件（Run 终态、审批请求、组队完成等）按每集成白名单路由到飞书自定义机器人或 QQ webhook 消息网关——用户无需盯屏即可感知关键节点。
2. **遥测外送**：域事件可批量以 NDJSON 推送到自建遥测端点（batch_size 10 / flush_interval 5s），供外部观测与分析；CancellationToken 监督、关闭不丢批。
3. **安全与可预期**：敏感 URL 跨一切边界掩码、send 限时 10s、派发失败只告警不重试不阻塞主流程；IPC 四步契约链四处一致、i18n 双语、组件测试守护关键流。

## 2. 用户故事 / User Stories

- **US1** 开发者启用了绑定 `run.state_changed` 白名单的飞书集成后离开工位；一张任务的 Run succeeded 时，群里收到「Run succeeded（taskId/runId）」文本消息，签名校验通过。
- **US2** 团队运维在 Settings 配置 kind=telemetry 的集成指向内网遥测网关；跑完一轮自发组队后，网关收到一批 NDJSON 行（task./run./team. 事件逐行一条），进程退出前余量批次被强制 flush，零丢失。
- **US3** 开发者新建 QQ webhook 集成后点「发送测试」，网关收到 `{"source":"nuomi",...}` 测试消息，toast 显示成功；随后误将该集成 disabled——之后任何事件都不再产生网络请求。

## 3. 验收标准 / Acceptance Criteria

**内核 integrations**
- [ ] AC1 FeishuSink **payload 形状 + 签名数学单测**：已知 timestamp/secret 向量直接以 hmac_sha256 直算期望 sign 对照（base64 编码断言）；带 secret 时顶层含 `timestamp`/`sign`、不带时两字段缺席；body 文本为 `title\nbody` 拼接；wiremock 回环脚本化 HTTP 非 200 与 `resp.code != 0` 两分支均报 `OutboundError`。
- [ ] AC2 GenericWebhookSink payload **表驱动**：四字段定值 `{"source":"nuomi","title","body","meta"}` 形状与值逐例断言（含 meta 为对象透传），与 D4 一致。
- [ ] AC3 TelemetryExporter 批量聚合：灌入两个域事件 → 恰好**一次** NDJSON POST 两行；触发 CancellationToken 关闭 → 余量未满批次立即 flush（零丢失断言）。
- [ ] AC4 disabled 集成零网络请求：`enabled=0` 的行无论派发还是导出路径均不产生任何 HTTP 流量（wiremock 请求计数为零断言）。

**壳侧派发**
- [ ] AC5 派发任务按白名单路由且失败不崩：fake/mocked OutboundSink 断言——仅 events 白名单命中的启用集成收到格式化消息（run.state_changed / approval.requested / team.formed 各 ≥1 例）；sink 返回 Err 仅 tracing 告警、循环继续、后续事件照常派发；broadcast Lagged 分支有 warn 且不断流。

**IPC 契约链**
- [ ] AC6 四处一致：四命令完成 Rust handler（薄层）+ specta builder 注册 + `bindings.gen.ts` 再生 + 前端消费端与 test-double 同提交更新；`IntegrationDto` 仅携带掩码 URL（原始 URL 与 secret 不出 IPC 边界）；稳定错误 code 两枚（`integration.not_found`/`integration.invalid`）齐备。

**前端**
- [ ] AC7 Settings「集成」段组件测试（test-double）：列表渲染 kind 徽标、掩码 URL、enabled 开关切换生效；表单 kind 三选、events 多选固定清单、「发送测试」成功/失败两路 toast（失败保留输入）；键盘可达；全部新增文案 zh-CN/en 双语。

**质量门**
- [ ] AC8 双端质量门全绿：`cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test` 与 `pnpm typecheck && pnpm lint && pnpm test`。

## 4. 非目标 / Non-goals

QQ 官方开放平台 websocket 网关全量实现（webhook 网关模式已覆盖最小可用）；入站命令解析（Bot 只出站通知，不做反向控制）；OTLP 协议兼容（NDJSON 即终态格式）；失败重试/退避与补投递队列（D6 定死失败即弃）；webhook URL/secret 迁移 OS keyring（后续里程碑再评估）。

## 5. 技术约束 / Technical Constraints

- 锁定栈不变：Rust(edition 2021) + tokio + SQLite(rusqlite+WAL) + reqwest(rustls)。新增 workspace 依赖仅限 `hmac`/`sha2`/`base64`（D3 签名数学所需）。遵守 `.opencode/rules/rust-core.md`（SQLite 操作 spawn_blocking 包裹、每模块 thiserror 枚举、库路径零 unwrap/expect、参数绑定 SQL）与 `.opencode/rules/ipc-contract.md`（camelCase DTO、内部标记枚举、稳定 code、四步契约链）。
- 迁移只增不改：新增 `0004_integrations.sql` 一个文件，永不编辑既有 0001–0003；`PRAGMA user_version` 语义照旧。
- 枚举序列化：`IntegrationKind` serde 序列化字面量与 DB CHECK 值严格一致（`feishu_bot`/`qq_webhook`/`telemetry`，snake_case rename），TS 侧联合由生成的 bindings 镜像，禁止手抄；时间戳 i64 Unix 毫秒（`*At`），id uuid v7 String（复用 `domain::new_id`）。
- 敏感数据铁律：webhook URL 与 secret 按 D5 原样入库（本里程碑显式决策），但**只写出不回读**——list/get 的 DTO 一律掩码、upsert 后响应同样掩码；`tracing` INFO 以上不得出现完整 URL、secret 或消息正文；send 统一 10s 超时。
- 派发语义：通知派发任务是 forward_event 之外的**第二个** broadcast 订阅者，二者互不干扰；每条事件先落 EventRecord 再上总线（既有铁律），派发只消费总线故天然晚于落库；单次失败即弃（warn 级），绝不重试、绝不持有锁跨越 `.await`、绝不 panic 外泄。
- 白名单语义收口：`events` 列是触发主题白名单——通知派发按其精确匹配 topic，空数组=匹配全部域主题（task./run./approval./schedule./team.）；session 高频通道事件永不外发；kind=telemetry 行由 TelemetryExporter 处理（通知派发忽略之），exporter 忽略 events 字段、始终全量导出。
- 测试纪律：单测零外网——sink payload/签名用已知向量直算 + wiremock 回环脚本化；派发任务用 fake OutboundSink 注入成功/失败；repo 测试用临时目录 SQLite；修 bug 先写失败测试。

## 6. 任务拆分 / Task Breakdown

| # | 任务 | 内容 | 前置 |
|---|---|---|---|
| B1 | 本 SPEC | 权威 SPEC 定稿（本文档，D1–D8 固化） | M-TEAM1 ✅ 已交付 |
| B2 | core integrations | `0004_integrations.sql` + `domain::{Integration, IntegrationKind, IntegrationConfig}` + `repos::integrations` CRUD + `OutboundSink`/`OutboundError` + FeishuSink（含加签）/GenericWebhookSink/TelemetryExporter（批量+CancellationToken）+ workspace 依赖 hmac/sha2/base64；AC1–AC4 表驱动/wiremock 单测 | B1 |
| B3 | 壳侧派发 | `src-tauri/lib.rs` setup 第二订户通知派发任务（白名单匹配 + 格式化 + Sink 出站 + 失败仅 warn）；fake sink 单测覆盖路由/失败/Lagged（AC5） | B2 |
| B4 | IPC 契约链 | `list_integrations`/`upsert_integration`/`delete_integration`/`test_integration` 四命令 impl + `IntegrationDto`（掩码）+ builder 注册 + contracts:gen 再生 + test-double 同步更新（AC6） | B2 |
| B5 | UI + 收尾验收 | Settings「集成」段（列表徽标/开关 + 表单 + 发送测试 toast）+ i18n zh-CN/en + 组件测试（test-double，AC7）；双端全量质量门复核 + AC 清单逐项核验报告（AC8） | B3, B4 |
