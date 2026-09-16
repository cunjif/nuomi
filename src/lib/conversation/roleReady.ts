import type { JsonValue } from "../ipc/bindings.gen";

/** Reads the `agent_profile_id` convention key out of a role's params JSON. */
export function readAgentProfileId(params: JsonValue): string | null {
  if (params !== null && typeof params === "object" && !Array.isArray(params)) {
    const value = params["agent_profile_id"];
    if (typeof value === "string") return value;
  }
  return null;
}

/** A role is "ready" (usable as a Role Agent) when it binds a provider or a CLI agent profile. */
export function isRoleReady(role: { providerId: string | null; params: JsonValue }): boolean {
  return role.providerId !== null || readAgentProfileId(role.params) !== null;
}
