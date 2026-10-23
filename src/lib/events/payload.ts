/**
 * Defensive accessors for event payloads (`JsonValue` from generated
 * bindings) — never trust wire shapes across the IPC boundary.
 */
import type { JsonValue } from "../ipc/bindings.gen";
import type { SourceEntryDto } from "./types";

export function asRecord(value: JsonValue | undefined): Partial<Record<string, JsonValue>> | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  return value;
}

export function asString(value: JsonValue | undefined): string {
  return typeof value === "string" ? value : "";
}

const SOURCE_KINDS: ReadonlySet<string> = new Set([
  "workspace_file",
  "external_file",
  "web_url",
  "cli_agent",
]);

/**
 * Guards a `message` payload's `sources` array: length- and field-checked,
 * malformed entries dropped silently (render-degradation principle — a bad
 * entry never throws, it just does not become a chip). Returns `undefined`
 * for absent/empty/fully-malformed shapes so "no sources" stays
 * indistinguishable from legacy events.
 */
export function asSources(value: JsonValue | undefined): SourceEntryDto[] | undefined {
  if (!Array.isArray(value)) return undefined;
  const entries: SourceEntryDto[] = [];
  for (const item of value) {
    if (typeof item !== "object" || item === null) continue;
    const record = item as Partial<Record<string, JsonValue>>;
    const kind = typeof record.kind === "string" ? record.kind : "";
    const target = typeof record.target === "string" ? record.target : "";
    if (!SOURCE_KINDS.has(kind) || target === "") {
      console.debug("[sources] dropping malformed source entry");
      continue;
    }
    entries.push({
      kind: kind as SourceEntryDto["kind"],
      target,
      label: typeof record.label === "string" ? record.label : target,
      agentId: typeof record.agentId === "string" ? record.agentId : "",
      agentName: typeof record.agentName === "string" ? record.agentName : "",
    });
  }
  return entries.length > 0 ? entries : undefined;
}
