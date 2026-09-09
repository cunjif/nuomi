# 插件开发入门（10 分钟）

> 本教程带你手写一个完整插件：一个 `upper` 工具 + 一个 `post_tool_call` 日志钩子。
> 前置阅读：[插件格式](plugin-format.md) · [协议规范](plugin-protocol.md)

## 1. 目录结构

把插件放进任一侧载目录（推荐工作区 `.nuomi/plugins/upper/`）：

```
.nuomi/plugins/upper/
├─ plugin.toml     # 清单（必需）
└─ plugin.py       # 入口（任何语言皆可，本教程给 Python 与 Node 两个变体）
```

可运行的完整示例就在仓库里：[`examples/plugins/upper/`](../../examples/plugins/upper/)。

## 2. 写清单 plugin.toml

```toml
id = "upper"
name = "Uppercase Tools"
version = "0.1.0"
api_version = 1
entry = ["python3", "plugin.py"]     # Node: ["node", "plugin.cjs"]

[[tools]]
name = "upper"
description = "把 text 转为大写"
input = { type = "object", properties = { text = { type = "string" } }, required = ["text"] }

[[hooks]]
point = "post_tool_call"
order = 100

[[events]]
topic = "session.*"
```

## 3. 写入口（Python 变体）

插件就是一个 **ndjson JSON-RPC 服务器**：从 stdin 读一行、回一行。骨架约 80 行：

```python
#!/usr/bin/env python3
import json, sys

API_VERSION = 1
next_id = 0

def send(msg):                       # 所有出站消息都是一行 JSON
    sys.stdout.write(json.dumps(msg) + "\n")
    sys.stdout.flush()

def reply(id, result):
    send({"jsonrpc": "2.0", "id": id, "result": result})

def main():
    global next_id
    for line in sys.stdin:           # 主循环：读一行，处理，回一行
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        method, params, id = msg.get("method"), msg.get("params", {}), msg.get("id")
        if method == "initialize":
            reply(id, {"api_version": API_VERSION,
                       "capabilities": {"tools": True, "hooks": True, "events": True}})
        elif method == "tools/list":
            reply(id, {"tools": [{
                "name": "upper",
                "description": "把 text 转为大写",
                "inputSchema": {"type": "object",
                                 "properties": {"text": {"type": "string"}},
                                 "required": ["text"]}}]})
        elif method == "tools/call":
            text = str(params.get("arguments", {}).get("text", ""))
            reply(id, {"content": [{"type": "text", "text": text.upper()}]})
        elif method == "hook/handle":
            # 钩子：不拦截，只记日志（allow 是缺省安全值）
            reply(id, {"decision": "allow"})
        elif method == "shutdown":
            reply(id, {}); return
        elif id is not None:         # 不认识的方法必须回 error（协议演进的基础）
            send({"jsonrpc": "2.0", "id": id,
                  "error": {"code": -32601, "message": "Method not found"}})

main()
```

Node 变体见 [`examples/plugins/upper/plugin.cjs`](../../examples/plugins/upper/plugin.cjs)（同样的循环，`readline` 逐行处理；用 `.cjs` 扩展名使其在任何 package.json 约定下都按 CommonJS 加载）。

## 4. 验证

```bash
# 查看内核如何识别你的插件（不启动主程序）
nuomi plugin list

# 启动后，Agent 的工具列表里会出现 upper.upper；
# 让 Agent 调用："用 upper 工具把 'hello' 转成大写"
```

启动报告（`tracing` 日志）会显示每个插件 loaded / skipped / failed 及原因；工具调用失败不会拖垮内核。

## 5. 调试技巧

- **stderr 随便写**：`print("...", file=sys.stderr)` 会被内核捕获进启动报告，是首选调试通道。
- **stdout 只准 ndjson**：混入其他内容会破坏帧解析。
- **钩子超时 = allow**：钩子处理 5 秒内必须回；做重活请转后台。
- **工具重名**：注册名是 `<plugin_id>.<name>`，不同插件天然不冲突。

## 6. 下一步

- 把 `[[hooks]]` 的 `decision` 改为 `"deny"` 体验拦截（例如 deny 所有写类工具）
- 订阅 `session.message` 做转写管道
- 阅读 [plugin-protocol.md](plugin-protocol.md) 了解全部方法与版本化规则
