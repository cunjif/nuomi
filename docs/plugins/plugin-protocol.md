# Nuomi Plugin Protocol（NPP）v1 规范

> 传输：JSON-RPC 2.0 over stdio，**ndjson 帧**（每行一条消息）；stderr 为自由文本，由内核捕获进启动报告。
> 适配内核版本：`NPP_API_VERSION = 1` · 决策依据：[ADR 0009](../adr/0009-plugin-sideload-spi.md)

## 生命周期

```
内核                                插件进程
 │ spawn (entry argv, piped stdio)   │
 │ ── initialize (request) ─────────▸│  握手：api_version 校验
 │ ◂──────── (result) ───────────────│  返回 {api_version, capabilities}
 │ ── initialized (notification) ───▸│  start 阶段
 │ ── tools/list (request) ─────────▸│  init 阶段（仅当清单声明了 [[tools]]）
 │ ◂── nuomi/log (notification) ─────│  任意时刻，插件可打日志
 │ ◂── event (notification) ─────────│  内核→插件：转发的总线事件（注意方向）
 │ ── hook/handle (request) ────────▸│  钩子点触发（带 5s 超时，超时=allow）
 │ ── tools/call (request) ─────────▸│  Agent 调用工具（带工具级 timeout_ms）
 │ ── shutdown (request) ───────────▸│  dispose 阶段；2s 宽限后 kill
```

## 消息帧

每行一个 JSON-RPC 消息（`\n` 结尾）：

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"api_version":1,"host":{"name":"nuomi","version":"0.1.0"},"capabilities":{}}}
{"jsonrpc":"2.0","id":1,"result":{"api_version":1,"capabilities":{"tools":true,"hooks":false,"events":false}}}
{"jsonrpc":"2.0","method":"nuomi/log","params":{"level":"info","message":"upper ready"}}
```

- 请求必须带 `id`；通知不得带 `id`。
- 插件对不认识的方法**必须**回 JSON-RPC error（code `-32601 Method not found`）——这是 add-only 演进的基础。
- stderr 随意写（调试输出会被捕获展示），**不要往 stdout 写任何非 ndjson 内容**。

## 方法参考

### host→plugin: `initialize`（request）

```json
params: { "api_version": 1, "host": { "name": "nuomi", "version": "…" }, "capabilities": {} }
result: { "api_version": 1, "capabilities": { "tools": true, "hooks": true, "events": true } }
```

- 内核 `api_version` ≥ 插件清单 `api_version` 时继续；否则内核立即终止该插件并记 `failed`。
- 插件返回的 `api_version` 应等于清单值；`capabilities` 声明实际支持的能力（内核只对清单声明且插件确认的贡献注册回调）。

### host→plugin: `initialized`（notification）

`params: {}` — 握手完成，插件可开始工作（如预热）。

### host→plugin: `tools/list`（request）

```json
params: {}
result: { "tools": [ { "name": "upper", "description": "…", "inputSchema": { … } } ] }
```

- `name` 是插件内名字；内核注册时自动加 `<plugin_id>.` 前缀。
- 内核以清单 `[[tools]]` 为准做交集：清单未声明的工具不会被注册（清单是权限边界）。

### host→plugin: `tools/call`（request）

```json
params: { "name": "upper", "arguments": { "text": "hello" } }
result: { "content": [ { "type": "text", "text": "HELLO" } ] }
```

- 结果形状与 MCP 对齐；内核取 `content` 中所有 `type=="text"` 的 `text` 拼接为工具输出字符串。
- 超时（工具级 `timeout_ms`，默认 10000）→ 内核回 `HarnessError::PluginFailed`，Agent 收到错误；插件进程不因此被杀（连续失败由未来版本治理）。

### host→plugin: `hook/handle`（request）

```json
params: { "point": "pre_tool_call", "payload": { "tool": "upper.upper", "arguments": {…} } }
result: { "decision": "allow" }
// 或
result: { "decision": "deny", "reason": "sensitive path" }
```

- `point` ∈ `pre_tool_call | post_tool_call | session_start | session_end`；`payload` 为对应钩子事件负载。
- 超时（5s）/错误 → 视为 `allow`，并写警告日志——钩子故障不能阻断主流程。

### host→plugin: `event`（notification）

```json
params: { "topic": "session.message", "payload": { … } }
```

- 内核把总线上匹配清单 `[[events]].topic` 的 Event 转发过来；fire-and-forget，插件无回执。
- 事件负载为 `serde_json::Value`（内核 Event.payload 原样），语义遵循 ADR-0002（只加不改删）。

### host→plugin: `shutdown`（request）

`params: {}` → 插件应在 2s 内回结果并自行退出；超时内核 SIGKILL。`dispose` 错误只记录、不中断内核关停扫尾。

### plugin→host: `nuomi/log`（notification）

```json
params: { "level": "info|warn|error", "message": "…" }
```

→ 内核 `tracing`。v1 仅此一个 plugin→host 方法；未来扩展（如 `nuomi/config/get`）在此命名空间下 add-only。

## 版本化与兼容

- 同一 `api_version`（major）内：只能**新增**方法/字段；改名、改语义、删除 = major+1。
- 旧内核遇到新字段必须忽略；新内核遇到旧清单必须照常工作。
- 协议破坏性变更走 ADR（AGENTS.md §7.7 / ADR-0001 缓解条款）。
