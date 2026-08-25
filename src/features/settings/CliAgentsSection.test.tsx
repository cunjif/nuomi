import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { AgentProfileDto } from "../../lib/ipc/bindings.gen";
import { injectIpcCommands } from "../../lib/ipc/client";
import { renderWithProviders } from "../../test/helpers";
import { i18n } from "../../i18n";
import { CliAgentsSection } from "./CliAgentsSection";

const ok = <T,>(data: T) => ({ status: "ok" as const, data });

function profile(overrides: Partial<AgentProfileDto>): AgentProfileDto {
  return {
    id: "agent-x",
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

const twoProfiles = (): AgentProfileDto[] => [
  profile({ id: "agent-1", name: "alpha", flavor: "claude_code", enabled: true }),
  profile({ id: "agent-2", name: "beta", flavor: "codex", enabled: false }),
];

describe("CliAgentsSection (SPEC cli-agents-m1 C5)", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("renders profile names and flavors", async () => {
    injectIpcCommands({
      listAgentProfiles: vi.fn().mockResolvedValue(ok(twoProfiles())),
      upsertAgentProfile: vi.fn(),
      deleteAgentProfile: vi.fn(),
      checkCliAgent: vi.fn(),
    } as never);
    renderWithProviders(<CliAgentsSection />);

    expect(await screen.findByText("alpha")).toBeInTheDocument();
    expect(screen.getByText("beta")).toBeInTheDocument();
    // Flavor badges live inside their profile rows (the <select> options share
    // the same labels, so assert against each row's content).
    expect(screen.getByText("alpha").closest("li")?.textContent).toContain("Claude Code");
    expect(screen.getByText("alpha").closest("li")?.textContent).toContain("Enabled");
    expect(screen.getByText("beta").closest("li")?.textContent).toContain("Codex");
    expect(screen.getByText("beta").closest("li")?.textContent).toContain("Disabled");
  });

  it("shows the reported version line after a successful check", async () => {
    const overrides = {
      listAgentProfiles: vi.fn().mockResolvedValue(ok(twoProfiles())),
      upsertAgentProfile: vi.fn(),
      deleteAgentProfile: vi.fn(),
      checkCliAgent: vi
        .fn()
        .mockResolvedValue(ok({ ok: true, versionLine: "claude-code 1.2.3", error: null })),
    };
    injectIpcCommands(overrides as never);
    renderWithProviders(<CliAgentsSection />);

    fireEvent.click(await screen.findByRole("button", { name: /check alpha/i }));
    await waitFor(() => expect(overrides.checkCliAgent).toHaveBeenCalledWith("agent-1"));
    expect(await screen.findByText(/claude-code 1\.2\.3/)).toBeInTheDocument();
  });

  it("submits parsed form input to upsertAgentProfile and refreshes the list", async () => {
    const created = profile({
      id: "agent-3",
      name: "gamma",
      flavor: "codex",
      args: ["--full-auto", "--model o4"],
      env: { API_KEY: "x", OTHER: "y=2" },
      enabled: true,
    });
    const overrides = {
      listAgentProfiles: vi
        .fn()
        .mockResolvedValueOnce(ok(twoProfiles()))
        .mockResolvedValueOnce(ok([...twoProfiles(), created])),
      upsertAgentProfile: vi.fn().mockResolvedValue(ok(created)),
      deleteAgentProfile: vi.fn(),
      checkCliAgent: vi.fn(),
    };
    injectIpcCommands(overrides as never);
    renderWithProviders(<CliAgentsSection />);

    fireEvent.change(screen.getByLabelText(/^name$/i), { target: { value: "gamma" } });
    fireEvent.change(screen.getByLabelText(/^flavor$/i), { target: { value: "codex" } });
    fireEvent.change(screen.getByLabelText(/^command$/i), { target: { value: "codex" } });
    // Blank line skipped in args; BROKEN line (no '=') ignored in env.
    fireEvent.change(screen.getByLabelText(/arguments/i), {
      target: { value: "--full-auto\n\n--model o4" },
    });
    fireEvent.change(screen.getByLabelText(/environment/i), {
      target: { value: "API_KEY=x\nBROKEN\nOTHER=y=2" },
    });
    fireEvent.click(screen.getByRole("button", { name: /^save$/i }));

    await waitFor(() =>
      expect(overrides.upsertAgentProfile).toHaveBeenCalledWith({
        name: "gamma",
        flavor: "codex",
        command: "codex",
        args: ["--full-auto", "--model o4"],
        env: { API_KEY: "x", OTHER: "y=2" },
        workingDir: null,
        enabled: true,
      })
    );
    // Invalidate triggers the second list fetch; the new profile shows up.
    expect(await screen.findByText("gamma")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent(/1 environment line/i);
  });
});
