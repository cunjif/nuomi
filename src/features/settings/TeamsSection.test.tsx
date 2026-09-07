import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { RoleDto, TeamDto } from "../../lib/ipc/bindings.gen";
import { injectIpcCommands } from "../../lib/ipc/client";
import { useToastStore } from "../../lib/store/toastStore";
import { renderWithProviders } from "../../test/helpers";
import { i18n } from "../../i18n";
import { TeamsSection } from "./TeamsSection";

const ok = <T,>(data: T) => ({ status: "ok" as const, data });

const toastText = (): string =>
  useToastStore
    .getState()
    .toasts.map((toast) => toast.message)
    .join("\n");

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

function team(overrides: Partial<TeamDto>): TeamDto {
  return {
    id: "team-x",
    name: "dream",
    topology: "pipeline",
    memberRoleIds: [],
    config: {},
    createdAt: 1,
    updatedAt: 1,
    ...overrides,
  };
}

describe("TeamsSection (SPEC team-shell-m1 T5)", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("renders teams with topology badge and member count", async () => {
    injectIpcCommands({
      listTeams: vi
        .fn()
        .mockResolvedValue(ok([team({ memberRoleIds: ["ra", "rb"], topology: "group_chat" })])),
      listRoles: vi.fn().mockResolvedValue(ok([])),
      upsertTeam: vi.fn(),
      deleteTeam: vi.fn(),
    } as never);
    renderWithProviders(<TeamsSection />);

    const row = (await screen.findByText("dream")).closest("li");
    expect(row?.textContent).toContain("Group chat");
    expect(row?.textContent).toContain("2 member(s)");
  });

  it("submits memberRoleIds in the reordered selection order", async () => {
    const created = team({ id: "t9", name: "dream", memberRoleIds: ["ra", "rb"] });
    const upsertTeam = vi.fn().mockResolvedValue(ok(created));
    injectIpcCommands({
      listRoles: vi.fn().mockResolvedValue(
        ok([role({ id: "ra", name: "alpha" }), role({ id: "rb", name: "beta" })]),
      ),
      listTeams: vi.fn().mockResolvedValueOnce(ok([])).mockResolvedValueOnce(ok([created])),
      upsertTeam,
      deleteTeam: vi.fn(),
    } as never);
    renderWithProviders(<TeamsSection />);

    fireEvent.change(await screen.findByLabelText(/^name$/i), { target: { value: "dream" } });
    // Check beta first, then alpha → initial order [beta, alpha].
    fireEvent.click(screen.getByLabelText("beta"));
    fireEvent.click(screen.getByLabelText("alpha"));
    // Move alpha up → expected order [alpha, beta].
    fireEvent.click(screen.getByRole("button", { name: /move up alpha/i }));

    fireEvent.click(screen.getByRole("button", { name: /^save$/i }));

    await waitFor(() =>
      expect(upsertTeam).toHaveBeenCalledWith({
        name: "dream",
        topology: "pipeline",
        memberRoleIds: ["ra", "rb"],
        config: {},
      })
    );
    expect(await screen.findByText("dream")).toBeInTheDocument();
  });

  it("writes group_chat config.max_rounds and keeps input on failure with missing members", async () => {
    const upsertTeam = vi.fn().mockResolvedValue({
      status: "error" as const,
      error: {
        generic: {
          code: "team.member_missing",
          message: "unknown member roles: ghost",
          details: { missing: ["ghost"] },
        },
      },
    });
    injectIpcCommands({
      listRoles: vi.fn().mockResolvedValue(ok([role({ id: "ra", name: "alpha" })])),
      listTeams: vi.fn().mockResolvedValue(ok([])),
      upsertTeam,
      deleteTeam: vi.fn(),
    } as never);
    renderWithProviders(<TeamsSection />);

    fireEvent.change(await screen.findByLabelText(/^name$/i), { target: { value: "circle" } });
    fireEvent.change(screen.getByLabelText(/topology/i), { target: { value: "group_chat" } });
    fireEvent.change(screen.getByLabelText(/max rounds/i), { target: { value: "9" } });
    fireEvent.click(screen.getByLabelText("alpha"));
    fireEvent.click(screen.getByRole("button", { name: /^save$/i }));

    await waitFor(() =>
      expect(upsertTeam).toHaveBeenCalledWith({
        name: "circle",
        topology: "group_chat",
        memberRoleIds: ["ra"],
        config: { max_rounds: 9 },
      })
    );
    // Failure toast names the missing member; form input is preserved.
    await waitFor(() => expect(toastText()).toMatch(/missing members: ghost/i));
    expect(screen.getByLabelText(/^name$/i)).toHaveValue("circle");
  });
});
