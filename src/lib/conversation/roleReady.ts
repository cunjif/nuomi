import type { JsonValue } from "../ipc/bindings.gen";

/** Reads the `agent_profile_id` convention key out of a role's params JSON. */
export function readAgentProfileId(params: JsonValue): string | null {
  if (params !== null && typeof params === "object" && !Array.isArray(params)) {
    const value = params["agent_profile_id"];
    if (typeof value === "string") return value;
  }
  return null;
}

/** Reads the `model_id` convention key (provider-binding model pin) out of a role's params JSON. */
export function readModelId(params: JsonValue): string | null {
  if (params !== null && typeof params === "object" && !Array.isArray(params)) {
    const value = params["model_id"];
    if (typeof value === "string" && value.length > 0) return value;
  }
  return null;
}

/** Reads the `memory` config from a role's params JSON. */
export function readMemoryConfig(params: JsonValue): {
  enabled: boolean;
  tags: string[];
  autoRemember: "off" | "user_input_only";
} | null {
  if (params !== null && typeof params === "object" && !Array.isArray(params)) {
    const m = params["memory"];
    if (m !== null && typeof m === "object" && !Array.isArray(m)) {
      const enabled = (m as Record<string, unknown>)["enabled"];
      const tags = (m as Record<string, unknown>)["tags"];
      const autoRemember = (m as Record<string, unknown>)["autoRemember"];
      return {
        enabled: typeof enabled === "boolean" ? enabled : false,
        tags: Array.isArray(tags) ? tags.filter((t): t is string => typeof t === "string") : [],
        autoRemember:
          autoRemember === "user_input_only" ? "user_input_only" : "off",
      };
    }
  }
  return null;
}

/** A role is "ready" (usable as a Role Agent) when it binds a provider or a CLI agent profile. */
export function isRoleReady(role: { providerId: string | null; params: JsonValue }): boolean {
  return role.providerId !== null || readAgentProfileId(role.params) !== null;
}

/**
 * The steward main role id (seeded by migration 0035, Rust const
 * `STEWARD_MAIN_ROLE_ID`). The steward main role is the configuration
 * carrier for the steward chat surface — never a user-selectable chat
 * participant, so it is filtered out of all "pick a Role Agent" lists
 * on the conversation page (new-conversation wizard, in-chat agent
 * picker).
 */
export const STEWARD_MAIN_ROLE_ID = "role_steward_main";

/** True for the steward main role — excluded from chat-participant picks. */
export function isStewardMainRole(role: { id: string }): boolean {
  return role.id === STEWARD_MAIN_ROLE_ID;
}
