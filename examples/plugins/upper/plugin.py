#!/usr/bin/env python3
"""nuomi "upper" example plugin (Python 3 variant, zero dependencies).

Speaks NPP v1: JSON-RPC 2.0, one message per stdin line (ndjson), replies on
stdout. Protocol reference: docs/plugins/plugin-protocol.md.
"""
import json
import sys

API_VERSION = 1

TOOL = {
    "name": "upper",
    "description": "Uppercase the given text",
    "inputSchema": {
        "type": "object",
        "properties": {"text": {"type": "string"}},
        "required": ["text"],
    },
}


def send(message: dict) -> None:
    """Every outbound message is exactly one ndjson line on stdout."""
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()


def reply(request_id, result: dict) -> None:
    send({"jsonrpc": "2.0", "id": request_id, "result": result})


def method_not_found(request_id) -> None:
    # Unrecognized methods MUST error — the basis for add-only protocol evolution.
    send({
        "jsonrpc": "2.0",
        "id": request_id,
        "error": {"code": -32601, "message": "Method not found"},
    })


def handle(method: str, params: dict, request_id) -> bool:
    """Returns False when the plugin wants to exit."""
    if method == "initialize":
        reply(request_id, {
            "api_version": API_VERSION,
            "capabilities": {"tools": True, "hooks": True, "events": True},
        })
    elif method == "tools/list":
        reply(request_id, {"tools": [TOOL]})
    elif method == "tools/call":
        text = str(params.get("arguments", {}).get("text", ""))
        reply(request_id, {"content": [{"type": "text", "text": text.upper()}]})
    elif method == "hook/handle":
        point = params.get("point", "")
        tool = params.get("payload", {}).get("tool", "?")
        send({"jsonrpc": "2.0", "method": "nuomi/log",
              "params": {"level": "info", "message": f"hook {point} fired for {tool}"}})
        reply(request_id, {"decision": "allow"})
    elif method == "event":
        # Fire-and-forget; the topic filter lives in plugin.toml ([[events]]).
        pass
    elif method == "shutdown":
        reply(request_id, {})
        return False
    elif request_id is not None:
        method_not_found(request_id)
    return True


def main() -> None:
    send({"jsonrpc": "2.0", "method": "nuomi/log",
          "params": {"level": "info", "message": "upper plugin starting (python)"}})
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not handle(message.get("method", ""), message.get("params") or {},
                      message.get("id")):
            return


if __name__ == "__main__":
    main()
