/**
 * Slash-command type definitions (P1.1 registry upgrade).
 * Separated from registry.ts to keep the registry focused on lookup logic.
 */

/** Command grouping for `/help` display and completion ordering. */
export type CommandCategory = "session" | "agent" | "context" | "task" | "view" | "plugin";

/** Argument shape — drives the completion panel's parameter mode. */
export type ArgKind = "none" | "text" | "path" | "agent" | "team" | "file" | "schedule";

/** A single dynamic suggestion for command parameter completion. */
export interface CommandSuggestion {
  /** Display label. */
  label: string;
  /** Value inserted into the input on selection. */
  value: string;
  /** Optional description shown in the completion panel. */
  description?: string;
}
