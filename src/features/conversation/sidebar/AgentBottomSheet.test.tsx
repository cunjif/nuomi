import { screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { AgentDetailDto } from "../../../lib/ipc/bindings.gen";
import { injectIpcCommands } from "../../../lib/ipc/client";
import { renderWithProviders } from "../../../test/helpers";
import { i18n } from "../../../i18n";
import { AgentBottomSheet } from "./AgentBottomSheet";

const ok = <T,>(data: T) => ({ status: "ok" as const, data });

function cliDetail(overrides: Partial<AgentDetailDto>): AgentDetailDto {
  return {
    kind: "role",
    id: "r1",
    name: "coder",
    avatarUrl: null,
    role: "role",
    responsibility: null,
    boundModel: "gpt-4o",
    provider: null,
    bindingKind: "cli",
    cliAgentName: "codex-cli",
    cliAgentFlavor: "codex",
    cliAgentModel: "gpt-4o",
    enabled: true,
    ...overrides,
  };
}

function providerDetail(overrides: Partial<AgentDetailDto>): AgentDetailDto {
  return {
    kind: "role",
    id: "r1",
    name: "translator",
    avatarUrl: null,
    role: "role",
    responsibility: null,
    boundModel: null,
    provider: "prov-1",
    bindingKind: "provider",
    cliAgentName: null,
    cliAgentFlavor: null,
    cliAgentModel: null,
    enabled: true,
    ...overrides,
  };
}

describe("AgentBottomSheet — binding kind display", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("shows CLI Agent name + flavor + model when bindingKind=cli", async () => {
    injectIpcCommands({
      getAgentDetail: vi.fn().mockResolvedValue(ok(cliDetail({}))),
    } as never);
    renderWithProviders(
      <AgentBottomSheet agentKind="role" agentId="r1" onClose={vi.fn()} />,
    );

    expect(await screen.findByText("codex-cli")).toBeInTheDocument();
    expect(screen.getByText(/Codex/)).toBeInTheDocument();
    expect(screen.getByText("gpt-4o")).toBeInTheDocument();
  });

  it("shows 'Managed by CLI Agent' when cli binding has no model", async () => {
    injectIpcCommands({
      getAgentDetail: vi.fn().mockResolvedValue(
        ok(cliDetail({ cliAgentModel: null, boundModel: null })),
      ),
    } as never);
    renderWithProviders(
      <AgentBottomSheet agentKind="role" agentId="r1" onClose={vi.fn()} />,
    );

    expect(await screen.findByText("codex-cli")).toBeInTheDocument();
    expect(screen.getByText("Managed by CLI Agent")).toBeInTheDocument();
  });

  it("shows Provider + bound model when bindingKind=provider", async () => {
    injectIpcCommands({
      getAgentDetail: vi.fn().mockResolvedValue(
        ok(providerDetail({ boundModel: "claude-sonnet" })),
      ),
    } as never);
    renderWithProviders(
      <AgentBottomSheet agentKind="role" agentId="r1" onClose={vi.fn()} />,
    );

    expect(await screen.findByText("prov-1")).toBeInTheDocument();
    expect(screen.getByText("claude-sonnet")).toBeInTheDocument();
    expect(screen.queryByText("codex-cli")).not.toBeInTheDocument();
  });
});
