#!/usr/bin/env node
// nuomi "upper" example plugin (Node variant, zero dependencies).
// Speaks NPP v1: JSON-RPC 2.0, one message per stdin line (ndjson), replies on
// stdout. Protocol reference: docs/plugins/plugin-protocol.md.
"use strict";

const readline = require("readline");

const API_VERSION = 1;

const TOOL = {
  name: "upper",
  description: "Uppercase the given text",
  inputSchema: {
    type: "object",
    properties: { text: { type: "string" } },
    required: ["text"],
  },
};

function send(message) {
  // Every outbound message is exactly one ndjson line on stdout.
  process.stdout.write(JSON.stringify(message) + "\n");
}

function reply(id, result) {
  send({ jsonrpc: "2.0", id, result });
}

function methodNotFound(id) {
  // Unrecognized methods MUST error — the basis for add-only protocol evolution.
  send({ jsonrpc: "2.0", id, error: { code: -32601, message: "Method not found" } });
}

function handle(method, params, id) {
  // Returns false when the plugin wants to exit.
  if (method === "initialize") {
    reply(id, {
      api_version: API_VERSION,
      capabilities: { tools: true, hooks: true, events: true },
    });
  } else if (method === "tools/list") {
    reply(id, { tools: [TOOL] });
  } else if (method === "tools/call") {
    const text = String((params.arguments || {}).text || "");
    reply(id, { content: [{ type: "text", text: text.toUpperCase() }] });
  } else if (method === "hook/handle") {
    const point = params.point || "";
    const tool = (params.payload || {}).tool || "?";
    send({
      jsonrpc: "2.0",
      method: "nuomi/log",
      params: { level: "info", message: `hook ${point} fired for ${tool}` },
    });
    reply(id, { decision: "allow" });
  } else if (method === "event") {
    // Fire-and-forget; the topic filter lives in plugin.toml ([[events]]).
  } else if (method === "shutdown") {
    reply(id, {});
    return false;
  } else if (id !== undefined && id !== null) {
    methodNotFound(id);
  }
  return true;
}

function main() {
  send({
    jsonrpc: "2.0",
    method: "nuomi/log",
    params: { level: "info", message: "upper plugin starting (node)" },
  });
  const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
  rl.on("line", (line) => {
    const trimmed = line.trim();
    if (trimmed.length === 0) return;
    let message;
    try {
      message = JSON.parse(trimmed);
    } catch {
      return;
    }
    if (!handle(message.method || "", message.params || {}, message.id)) {
      rl.close();
    }
  });
}

main();
