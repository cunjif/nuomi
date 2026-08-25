/**
 * Defensive accessors for event payloads (`JsonValue` from generated
 * bindings) — never trust wire shapes across the IPC boundary.
 */
import type { JsonValue } from "../ipc/bindings.gen";

export function asRecord(value: JsonValue | undefined): Partial<Record<string, JsonValue>> | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  return value;
}

export function asString(value: JsonValue | undefined): string {
  return typeof value === "string" ? value : "";
}
