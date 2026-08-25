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
    systemPromptOverride: null,
    toolAllowlist: [],
    temperature: null,
    maxTokens: null,
    params: {},
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
        systemPromptOverride: null,
        toolAllowlist: [],
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
});
