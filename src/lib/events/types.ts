/**
 * ADR-0002 event wire shape (mirrors `src-tauri/src/events.rs::DomainEvent`).
 * Hand-written because events are not part of the specta command bindings.
 */
import type { JsonValue } from "../ipc/bindings.gen";

export type DomainEvent = {
  type: string;
  taskId?: string;
  runId?: string;
  sessionId?: string;
  seq?: number;
  payload: JsonValue;
};

/** Global low-frequency structured channel (task/run/approval/schedule). */
export const DOMAIN_CHANNEL = "event://domain";

/** Per-session high-frequency channel. */
export const sessionChannel = (sessionId: string): string => `event://session/${sessionId}`;
