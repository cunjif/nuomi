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
  /**
   * DUAL MEANING, discriminated by `type`:
   *  - on `session.delta`: NON-PERSISTENT live-stream ordinal injected by the
   *    kernel (per-session counter starting at 1) — dedupe/reorder only,
   *    never persisted, never used for history gap recovery;
   *  - on every other event (`session.message`, whiteboard notes, …): the
   *    AUTHORITATIVE per-aggregate seq of the durable row (events table),
   *    recoverable via listEvents(afterSeq).
   */
  seq?: number;
  payload: JsonValue;
};

/** Global low-frequency structured channel (task/run/approval/schedule). */
export const DOMAIN_CHANNEL = "event://domain";

/** Per-session high-frequency channel. */
export const sessionChannel = (sessionId: string): string => `event://session/${sessionId}`;

/**
 * Source-evidence entry riding the assistant `message` event payload
 * (`sources` array, chat_rich_bind spec 5.2 / design D1). Mirrors the Rust
 * `services::source_collector::SourceEntry` serde shape (camelCase keys).
 */
export type SourceEntryDto = {
  kind: "workspace_file" | "external_file" | "web_url" | "cli_agent";
  /** Absolute path (files) or URL (web) — the jump target. */
  target: string;
  /** Display label (file name / host / CLI agent name). */
  label: string;
  /** Owning RoleAgent (distinguishes collectors in group chats). */
  agentId: string;
  agentName: string;
};
