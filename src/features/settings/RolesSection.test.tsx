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
    modelId: null,
    createdAt: 1,
    updatedAt: 1,
    ...overrides,
  };
}

describe("RolesSection (SPEC team-shell-m1 T5)", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("renders binding badges for ready roles (provider-bound and CLI-agent-bound)", async () => {
    injectIpcCommands({
      listRoles: vi.fn().mockResolvedValue(
        ok([
          role({ id: "r1", name: "coder", providerId: "prov-1" }),
          role({ id: "r2", name: "reviewer", providerId: null, params: { agent_profile_id: "agent-1" } }),
        ]),
      ),
      listProviders: vi.fn().mockResolvedValue(ok([provider()])),
      listAgentProfiles: vi.fn().mockResolvedValue(ok([profile()])),
      upsertRole: vi.fn(),
      deleteRole: vi.fn(),
    } as never);
    renderWithProviders(<RolesSection />);

    // Role names appear in both the quick-binding <select> options and the
    // role list <span>s — assert at least one match for each.
    expect((await screen.findAllByText("coder")).length).toBeGreaterThan(0);
    expect((await screen.findAllByText("reviewer")).length).toBeGreaterThan(0);
    // Badge text appears once provider/profile data has loaded. The
    // provider has a model, so the badge includes it.
    expect(await screen.findByText("Provider: gpt-main/gpt-4o-mini")).toBeInTheDocument();
    expect(await screen.findByText("CLI Agent: alpha")).toBeInTheDocument();
  });

  it("creates an unbound role template via the form (binding is done separately)", async () => {
    const created = role({ id: "r9", name: "scribe", providerId: null });
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

    // The "Name" label appears in both RoleForm and RoleQuickBinding;
    // the form's input is the first one.
    const nameInputs = await screen.findAllByLabelText(/^name$/i);
    fireEvent.change(nameInputs[0]!, { target: { value: "scribe" } });
    // The "Save" button also appears in both forms; use the first one.
    const saveButtons = screen.getAllByRole("button", { name: /^save$/i });
    fireEvent.click(saveButtons[0]!);

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
        params: {},
      })
    );
  });

  it("does not render unbound (not-ready) roles in the role list", async () => {
    injectIpcCommands({
      listRoles: vi.fn().mockResolvedValue(
        ok([
          role({ id: "r1", name: "Coder", builtin: true, providerId: "prov-1", requiredCapabilities: ["reasoning"] }),
          role({ id: "r3", name: "mine" }),
        ]),
      ),
      listProviders: vi.fn().mockResolvedValue(ok([provider()])),
      listAgentProfiles: vi.fn().mockResolvedValue(ok([])),
      upsertRole: vi.fn(),
      deleteRole: vi.fn(),
    } as never);
    renderWithProviders(<RolesSection />);

    // "Coder" appears in both the quick-binding <select> option and the
    // role list — assert at least one match.
    expect((await screen.findAllByText("Coder")).length).toBeGreaterThan(0);
    // "mine" is unbound (no provider, no agent_profile_id) so it is not
    // shown in the ready-only list (only in the <select> option).
    // The role list <span> should not contain "mine".
    const mineMatches = screen.queryAllByText("mine");
    // All matches should be <option> elements, not <span> role names.
    for (const el of mineMatches) {
      expect(el.tagName).toBe("OPTION");
    }
  });

  it("generates a role through the Role Director dialog", async () => {
    const binding = { bindingMode: "provider" as const, providerId: "prov-1", agentProfileId: null };
    const generateRole = vi.fn().mockResolvedValue(ok(role({ id: "r9", name: "周报助手", generated: true })));
    injectIpcCommands({
      listRoles: vi
        .fn()
        .mockResolvedValueOnce(ok([]))
        .mockResolvedValueOnce(ok([role({ id: "r9", name: "周报助手", generated: true })])),
      listProviders: vi.fn().mockResolvedValue(ok([{ id: "prov-1", name: "P1", protocol: "openai_compatible", baseUrl: "http://x", hasKey: true, capabilities: [], isMaster: false, settings: { models: [], defaultModel: null, temperature: null, topP: null, maxTokens: null, timeoutSecs: null, retry: null, maxConcurrency: null, priority: null, roles: [], enabled: true } }])),
      listAgentProfiles: vi.fn().mockResolvedValue(ok([])),
      getRoleDirectorBinding: vi.fn().mockResolvedValue(ok(binding)),
      setRoleDirectorBinding: vi.fn().mockResolvedValue(ok(null)),
      generateRole,
    } as never);
    renderWithProviders(<RolesSection />);

    fireEvent.click(await screen.findByRole("button", { name: /role director/i }));
    fireEvent.change(await screen.findByRole("textbox", { name: /role director/i }), {
      target: { value: "我要一个帮我写周报的角色" },
    });
    fireEvent.click(screen.getByRole("button", { name: /^generate$/i }));
    await waitFor(() => expect(generateRole).toHaveBeenCalledWith("我要一个帮我写周报的角色", binding));
    expect(await screen.findByText("周报助手")).toBeInTheDocument();
  });
});
