/**
 * Conversation kind constants and helpers.
 * Avoids magic strings scattered across the frontend.
 */

export const CONVERSATION_KINDS = ["chat", "group", "background", "scheduled"] as const;
export type ConversationKind = (typeof CONVERSATION_KINDS)[number];

export const AGENT_REF_KINDS = ["cli", "role"] as const;
export type AgentRefKind = (typeof AGENT_REF_KINDS)[number];

/** app_settings key for the default agent binding. */
export const DEFAULT_AGENT_SETTING_KEY = "conversation.default_agent";

export function isConversationKind(s: string): s is ConversationKind {
  return (CONVERSATION_KINDS as readonly string[]).includes(s);
}
