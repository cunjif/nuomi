# nuomi 插件格式参考（plugin.toml v1）

> 适配 API 版本：`api_version = 1`（协议规范见 [plugin-protocol.md](plugin-protocol.md)）
> 决策依据：[ADR 0009](../adr/0009-plugin-sideload-spi.md)

一个 nuomi 插件 = **一个目录**，目录根部必须有一份 `plugin.toml` 清单。插件以独立进程运行，通过 NPP（JSON-RPC over stdio，ndjson 帧）与内核通信，因此可以用任何语言编写（Python / Node / Rust / Go / shell 包装……）。

## 清单结构

```toml
# ── 元信息（id/name/version/api_version/entry 必填）─────────────────────
id = "upper"                        # 必填；kebab-case，^[a-z][a-z0-9-]*$；全局唯一
name = "Uppercase Tools"            # 必填；展示名
version = "0.1.0"                   # 必填；语义化版本（信息性，内核不做依赖解析）
api_version = 1                     # 必填；插件面向的 NPP API 版本，须 <= 内核 NPP_API_VERSION
description = "文本大写工具与调用日志钩子"   # 可选
authors = ["you@example.com"]       # 可选
license = "MIT"                     # 可选

# ── 入口（必填）────────────────────────────────────────────────────────
# argv 数组，相对路径基于插件目录解析。禁止 shell 字符串 —— 内核不做 shell 拼接。
entry = ["python3", "plugin.py"]    # Node 变体: ["node", "plugin.js"]

# ── 权限声明（可选；v1 仅在启动报告与 nuomi plugin list 中展示）──────────
[permissions]
fs.read  = ["./data/**"]            # glob 列表
fs.write = []                       # glob 列表
network  = []                       # 允许访问的主机，如 ["api.example.com"]
shell    = false                    # 插件是否会再派生子进程

# ── 工具贡献（可选，可多个）─────────────────────────────────────────────
# 注册进内核 ToolRegistry，最终工具名为 "<plugin_id>.<name>"（自动加前缀）。
[[tools]]
name = "upper"                      # 插件内唯一；kebab-case
description = "把 text 转为大写"
input = { type = "object", properties = { text = { type = "string" } }, required = ["text"] }
timeout_ms = 10000                  # 可选；单次 tools/call 超时，默认 10000

# ── 钩子贡献（可选，可多个）─────────────────────────────────────────────
# 内核在钩子点以 hook/handle 请求调用插件；超时（5s）视为 allow。
[[hooks]]
point = "post_tool_call"            # pre_tool_call | post_tool_call | session_start | session_end
order = 100                         # 同点多个钩子的执行序（小者先）

# ── 事件订阅（可选，可多个）─────────────────────────────────────────────
# 内核事件总线上匹配 topic 的 Event 以 event 通知转发（fire-and-forget）。
[[events]]
topic = "session.*"                 # 点段通配，匹配内核总线语义（bus.rs topic_matches）
```

## 校验规则

| 规则 | 违反后果 |
|---|---|
| `id` 不匹配 `^[a-z][a-z0-9-]*$` | 该插件进入启动报告 `failed`，boot 继续 |
| `api_version` > 内核 `NPP_API_VERSION` | 握手阶段拒绝，`failed` |
| `entry` 为空或非数组 | `failed` |
| 工具名重复（同插件内）或注册后与既有工具重名（含前缀后） | `failed` |
| **未知键** | **警告不报错**（前向兼容：旧内核跑新清单只丢新能力） |
| 同 `id` 插件出现在多个扫描目录 | 先到先得（`NUOMI_PLUGIN_PATH` > 用户配置目录 > 工作区 `.nuomi/plugins`），后者 `skipped(duplicate)` |

## 侧载目录

按顺序扫描，先发现者胜出：

1. `NUOMI_PLUGIN_PATH` — 环境变量，PATH 风格分隔（Windows `;` / Unix `:`），每个元素是一个**插件目录**
2. `<用户配置目录>/nuomi/plugins` — Windows `%APPDATA%\nuomi\plugins`，macOS `~/Library/Application Support/nuomi/plugins`，Linux `~/.config/nuomi/plugins`
3. `<工作区>/.nuomi/plugins` — 项目自带插件

## 权限模型（v1）

v1 权限是**声明 + 展示**：boot 报告与 `nuomi plugin list` 会列出每个插件的权限面，执行治理仍由既有机制承担（PreToolCall 钩子可 deny、ApprovalGate 敏感工具名单、Exchange gate）。进程边界本身即隔离层。强制执行列入 v1.1 roadmap（ADR 0009 §5）。

## 后置项（v1.1+ roadmap）

- `[mcp_servers]` — 声明式接入既有 McpPlugin
- `[commands]` / `[skills]` — 维持 SPEC Non-goal，待 Command/Skills 插件化落地
- 权限强制执行、插件签名校验、WASM 载体（ADR 0009 §1）
