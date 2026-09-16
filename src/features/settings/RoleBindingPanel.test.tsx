import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { AgentProfileDto, ProviderDto, RoleDto } from "../../lib/ipc/bindings.gen";
import { injectIpcCommands } from "../../lib/ipc/client";
import { renderWithProviders } from "../../test/helpers";
import { i18n } from "../../i18n";
import { RoleBindingPanel } from "./RoleBindingPanel";

const ok = <T,>(data: T) => ({ status: "ok" as const, data });

function provider(overrides: Partial<ProviderDto> = {}): ProviderDto {
  return {
    id: "prov-1",
    name: "gpt-main",
    protocol: "open_ai_compatible",
    baseUrl: "https://api.example.com/v1",
    hasKey: true,
    capabilities: ["chat"],
    isMaster: false,
    settings: { models: [{ id: "gpt-4o-mini", capabilities: ["reasoning"] }], enabled: true },
    ...overrides,
  };
}

function profile(overrides: Partial<AgentProfileDto> = {}): AgentProfileDto {
  return {
    id: "agent-1",
    name: "alpha",
    adapter: "cli",
    flavor: "claude_code",
    command: "claude",
    args: [],
    env: {},
    workingDir: null,
    enabled: true,
    createdAt: 1,
    updatedAt: 1,
    ...overrides,
  };
}

function role(overrides: Partial<RoleDto>): RoleDto {
  return {
    id: "role-x",
    name: "coder",
    providerId: null,
    providerIds: [],
    systemPromptOverride: null,
    toolAllowlist: [],
    requiredCapabilities: [],
    temperature: null,
    maxTokens: null,
    params: {},
    builtin: false,
    generated: false,
    ephemeral: false,
    source: null,
    createdAt: 1,
    updatedAt: 1,
    ...overrides,
  };
}

describe("RoleBindingPanel", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("prefills from presetBinding in provider mode and submits with all overlay fields", async () => {
    const created = role({ id: "r1", name: "translator", providerId: "prov-1" });
    const upsertRole = vi.fn().mockResolvedValue(ok(created));
    injectIpcCommands({
      listProviders: vi.fn().mockResolvedValue(ok([provider()])),
      listAgentProfiles: vi.fn().mockResolvedValue(ok([profile()])),
      upsertRole,
    } as never);
    const onClose = vi.fn();
    renderWithProviders(
      <RoleBindingPanel
        presetBinding={{ mode: "provider", providerId: "prov-1" }}
        onClose={onClose}
      />,
    );

    fireEvent.change(await screen.findByLabelText(/^name$/i), { target: { value: "translator" } });
    fireEvent.change(screen.getByLabelText(/temperature/i), { target: { value: "0.3" } });
    fireEvent.change(screen.getByLabelText(/max tokens/i), { target: { value: "4096" } });
    fireEvent.change(screen.getByLabelText(/tool allowlist/i), {
      target: { value: "read_file, write_file" },
    });
    fireEvent.click(screen.getByRole("button", { name: /^save$/i }));

    await waitFor(() =>
      expect(upsertRole).toHaveBeenCalledWith({
        name: "translator",
        providerId: "prov-1",
        providerIds: ["prov-1"],
        systemPromptOverride: null,
        toolAllowlist: ["read_file", "write_file"],
        requiredCapabilities: [],
        temperature: 0.3,
        maxTokens: 4096,
        params: {},
      }),
    );
    expect(onClose).toHaveBeenCalled();
  });

  it("editing mode prefills from initialRole and locks the name input", async () => {
    const existing = role({
      id: "r2",
      name: "reviewer",
      providerId: null,
      params: { agent_profile_id: "agent-1" },
      temperature: 0.7,
      maxTokens: 2048,
      toolAllowlist: ["grep", "glob"],
      systemPromptOverride: "You are a reviewer.",
    });
    injectIpcCommands({
      listProviders: vi.fn().mockResolvedValue(ok([provider()])),
      listAgentProfiles: vi.fn().mockResolvedValue(ok([profile()])),
      upsertRole: vi.fn().mockResolvedValue(ok(existing)),
    } as never);
    renderWithProviders(<RoleBindingPanel initialRole={existing} onClose={vi.fn()} />);

    const nameInput = await screen.findByLabelText(/^name$/i);
    expect(nameInput).toBeDisabled();
    expect((nameInput as HTMLInputElement).value).toBe("reviewer");
    expect((screen.getByLabelText(/temperature/i) as HTMLInputElement).value).toBe("0.7");
    expect((screen.getByLabelText(/max tokens/i) as HTMLInputElement).value).toBe("2048");
    expect((screen.getByLabelText(/tool allowlist/i) as HTMLInputElement).value).toBe("grep, glob");
  });

  it("submits a CLI-agent binding as params.agent_profile_id with providerId null", async () => {
    const created = role({ id: "r3", name: "scribe", params: { agent_profile_id: "agent-1" } });
    const upsertRole = vi.fn().mockResolvedValue(ok(created));
    injectIpcCommands({
      listProviders: vi.fn().mockResolvedValue(ok([provider()])),
      listAgentProfiles: vi.fn().mockResolvedValue(ok([profile()])),
      upsertRole,
    } as never);
    renderWithProviders(
      <RoleBindingPanel presetBinding={{ mode: "cli", agentProfileId: "agent-1" }} onClose={vi.fn()} />,
    );

    fireEvent.change(await screen.findByLabelText(/^name$/i), { target: { value: "scribe" } });
    fireEvent.click(screen.getByRole("button", { name: /^save$/i }));

    await waitFor(() =>
      expect(upsertRole).toHaveBeenCalledWith(
        expect.objectContaining({
          name: "scribe",
          providerId: null,
          providerIds: [],
          params: { agent_profile_id: "agent-1" },
        }),
      ),
    );
  });
});
