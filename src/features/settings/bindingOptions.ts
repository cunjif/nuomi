import type { AgentProfileDto, ProviderDto } from "../../lib/ipc/bindings.gen";

export interface BindingOption {
  value: string;
  label: string;
  group: "provider" | "cli";
}

export interface DecodedBinding {
  kind: "provider" | "cli";
  providerId?: string;
  modelId?: string;
  agentProfileId?: string;
}

/**
 * Build mixed binding options from providers × models and CLI agent profiles.
 * Provider options encode as `provider:<id>:<modelId>`;
 * CLI agent options encode as `cli:<id>` (modelId carried on the profile itself).
 */
export function buildBindingOptions(
  providers: ProviderDto[],
  agentProfiles: AgentProfileDto[],
): BindingOption[] {
  const providerOpts: BindingOption[] = [];
  for (const p of providers) {
    for (const m of p.settings.models ?? []) {
      providerOpts.push({
        value: `provider:${p.id}:${m.id}`,
        label: `${p.name}/${m.id}`,
        group: "provider",
      });
    }
  }
  const cliOpts: BindingOption[] = agentProfiles.map((a) => ({
    value: `cli:${a.id}`,
    label: a.modelId !== null ? `${a.name}/${a.modelId}` : a.name,
    group: "cli",
  }));
  return [...providerOpts, ...cliOpts];
}

/** Decode a binding value back into its kind and identifiers. */
export function decodeBindingValue(value: string): DecodedBinding | null {
  if (value.startsWith("provider:")) {
    const parts = value.slice("provider:".length).split(":");
    if (parts.length >= 2) {
      return { kind: "provider", providerId: parts[0], modelId: parts.slice(1).join(":") };
    }
    return null;
  }
  if (value.startsWith("cli:")) {
    return { kind: "cli", agentProfileId: value.slice("cli:".length) };
  }
  return null;
}
