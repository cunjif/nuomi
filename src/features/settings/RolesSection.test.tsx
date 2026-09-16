import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { AgentProfileDto, ProviderDto, RoleDto } from "../../lib/ipc/bindings.gen";
import { injectIpcCommands } from "../../lib/ipc/client";
import { renderWithProviders } from "../../test/helpers";
import { i18n } from "../../i18n";
import { RolesSection } from "./RolesSection";

const ok = <T,>(data: T) => ({ status: "ok" as const, data });

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

describe("RolesSection (SPEC team-shell-m1 T5)", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("renders binding badges for provider-bound and CLI-agent-bound roles", async () => {
    injectIpcCommands({
      listRoles: vi.fn().mockResolvedValue(
        ok([
          role({ id: "r1", name: "coder", providerId: "prov-1" }),
          role({ id: "r2", name: "reviewer", providerId: null, params: { agent_profile_id: "agent-1" } }),
          role({ id: "r3", name: "freerole", providerId: null }),
        ]),
      ),
      listProviders: vi.fn().mockResolvedValue(ok([provider()])),
      listAgentProfiles: vi.fn().mockResolvedValue(ok([profile()])),
      upsertRole: vi.fn(),
      deleteRole: vi.fn(),
    } as never);
    renderWithProviders(<RolesSection />);

    expect(await screen.findByText("coder")).toBeInTheDocument();
    expect(screen.getByText("reviewer")).toBeInTheDocument();
    expect(screen.getByText("Provider: gpt-main")).toBeInTheDocument();
    expect(screen.getByText("CLI Agent: alpha")).toBeInTheDocument();
    expect(screen.getByText("Default")).toBeInTheDocument();
  });

  it("submits a CLI-agent binding as params.agent_profile_id with providerId null", async () => {
    const created = role({
      id: "r9",
      name: "scribe",
      providerId: null,
      params: { agent_profile_id: "agent-9" },
    });
    const upsertRole = vi.fn().mockResolvedValue(ok(created));
    injectIpcCommands({
      listRoles: vi
        .fn()
        .mockResolvedValueOnce(ok([]))
        .mockResolvedValueOnce(ok([created])),
      listProviders: vi.fn().mockResolvedValue(ok([provider()])),
      listAgentProfiles: vi.fn().mockResolvedValue(ok([profile({ id: "agent-9", name: "beta" })])),
      upsertRole,
      deleteRole: vi.fn(),
    } as never);
    renderWithProviders(<RolesSection />);

    fireEvent.change(await screen.findByLabelText(/^name$/i), { target: { value: "scribe" } });
    fireEvent.change(screen.getByLabelText(/^binding$/i), { target: { value: "cli" } });
    fireEvent.change(screen.getByLabelText(/select cli agent/i), {
      target: { value: "agent-9" },
    });
    fireEvent.click(screen.getByRole("button", { name: /^save$/i }));

    await waitFor(() =>
      expect(upsertRole).toHaveBeenCalledWith({
        name: "scribe",
        providerId: null,
        providerIds: [],
        systemPromptOverride: null,
        toolAllowlist: [],
        requiredCapabilities: [],
        temperature: null,
        maxTokens: null,
        params: { agent_profile_id: "agent-9" },
      })
    );
    // Invalidate triggers the second list fetch; the new role shows up.
    expect(await screen.findByText("scribe")).toBeInTheDocument();
  });

  it("switching the binding back to Provider clears the agent_profile_id param key", async () => {
    const created = role({ id: "r8", name: "planner", providerId: "prov-1" });
    const upsertRole = vi.fn().mockResolvedValue(ok(created));
    injectIpcCommands({
      listRoles: vi.fn().mockResolvedValue(ok([])),
      listProviders: vi.fn().mockResolvedValue(ok([provider()])),
      listAgentProfiles: vi.fn().mockResolvedValue(ok([profile({ id: "agent-9", name: "beta" })])),
      upsertRole,
      deleteRole: vi.fn(),
    } as never);
    renderWithProviders(<RolesSection />);

    fireEvent.change(await screen.findByLabelText(/^name$/i), { target: { value: "planner" } });
    fireEvent.change(screen.getByLabelText(/^binding$/i), { target: { value: "cli" } });
    fireEvent.change(screen.getByLabelText(/select cli agent/i), { target: { value: "agent-9" } });
    fireEvent.change(screen.getByLabelText(/^binding$/i), { target: { value: "provider" } });
    fireEvent.change(screen.getByLabelText(/select provider/i), { target: { value: "prov-1" } });
    fireEvent.click(screen.getByRole("button", { name: /^save$/i }));

    await waitFor(() =>
      expect(upsertRole).toHaveBeenCalledWith(
        expect.objectContaining({ providerId: "prov-1", params: {} })
      )
    );
  });

  it("groups roles into ready / unbound sections with binding buttons", async () => {
    injectIpcCommands({
      listRoles: vi.fn().mockResolvedValue(
        ok([
          role({ id: "r1", name: "Coder", builtin: true, providerId: "prov-1", requiredCapabilities: ["reasoning"] }),
          role({ id: "r2", name: "weekly-bot", generated: true, providerId: "prov-1" }),
          role({ id: "r3", name: "mine" }),
        ]),
      ),
      listProviders: vi.fn().mockResolvedValue(ok([provider()])),
      listAgentProfiles: vi.fn().mockResolvedValue(ok([])),
      upsertRole: vi.fn(),
      deleteRole: vi.fn(),
    } as never);
    renderWithProviders(<RolesSection />);

    expect(await screen.findByText("Ready (2)")).toBeInTheDocument();
    expect(screen.getByText("Unbound (1)")).toBeInTheDocument();
    // Built-in roles expose no delete button but do expose a binding button.
    expect(screen.queryByRole("button", { name: /^delete coder$/i })).toBeNull();
    expect(screen.getByRole("button", { name: /^delete mine$/i })).toBeInTheDocument();
  });

  it("restores presets via the seed command and reports the counts", async () => {
    const seedBuiltinRoles = vi
      .fn()
      .mockResolvedValue(ok({ inserted: 11, updated: 0, skipped: 0 }));
    // After invalidation the refetch shows the 11 seeded built-ins.
    const presets = Array.from({ length: 11 }, (_, i) =>
      role({ id: `preset-${i}`, name: `Preset ${i}`, builtin: true }),
    );
    injectIpcCommands({
      listRoles: vi
        .fn()
        .mockResolvedValueOnce(ok([]))
        .mockResolvedValueOnce(ok(presets)),
      listProviders: vi.fn().mockResolvedValue(ok([])),
      listAgentProfiles: vi.fn().mockResolvedValue(ok([])),
      seedBuiltinRoles,
    } as never);
    renderWithProviders(<RolesSection />);

    fireEvent.click(await screen.findByRole("button", { name: /restore presets/i }));
    await waitFor(() => expect(seedBuiltinRoles).toHaveBeenCalled());
    // The seeded double surfaces 11 built-in presets in the grouped list.
    expect(await screen.findByText(/Unbound \(11\)/)).toBeInTheDocument();
  });

  it("generates a role through the Role Director dialog", async () => {
    const generateRole = vi.fn().mockResolvedValue(ok(role({ id: "r9", name: "周报助手", generated: true })));
    injectIpcCommands({
      listRoles: vi
        .fn()
        .mockResolvedValueOnce(ok([]))
        .mockResolvedValueOnce(ok([role({ id: "r9", name: "周报助手", generated: true })])),
      listProviders: vi.fn().mockResolvedValue(ok([])),
      listAgentProfiles: vi.fn().mockResolvedValue(ok([])),
      generateRole,
    } as never);
    renderWithProviders(<RolesSection />);

    fireEvent.click(await screen.findByRole("button", { name: /role director/i }));
    fireEvent.change(await screen.findByRole("textbox", { name: /role director/i }), {
      target: { value: "我要一个帮我写周报的角色" },
    });
    fireEvent.click(screen.getByRole("button", { name: /^generate$/i }));
    await waitFor(() => expect(generateRole).toHaveBeenCalledWith("我要一个帮我写周报的角色"));
    expect(await screen.findByText("周报助手")).toBeInTheDocument();
  });
});
