import { screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";
import type { AgentDetailDto } from "../../../lib/ipc/bindings.gen";
import { injectIpcCommands } from "../../../lib/ipc/client";
import { renderWithProviders } from "../../../test/helpers";
import { useToastStore } from "../../../lib/store/toastStore";
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

describe("AgentBottomSheet — conversation detail panel (chat_rich_bind 5.5)", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
    useToastStore.setState({ toasts: [] });
  });

  function conversationDetail(overrides: Partial<object> = {}): object {
    return {
      sessionId: "s1",
      title: "chat",
      kind: "chat",
      workspace: { id: "ws-1", label: "alpha" },
      workDirectory: { id: "d1", displayName: "root", absPath: "C:/proj/a", available: true },
      participants: [
        { kind: "role", id: "r1", name: "coder", authPolicy: "unset" },
      ],
      ...overrides,
    };
  }

  it("renames the panel title and shows the session info section", async () => {
    injectIpcCommands({
      getAgentDetail: vi.fn().mockResolvedValue(ok(cliDetail({}))),
      getConversationDetail: vi.fn().mockResolvedValue(ok(conversationDetail())),
    } as never);
    renderWithProviders(
      <AgentBottomSheet agentKind="role" agentId="r1" onClose={vi.fn()} sessionId="s1" />,
    );
    expect(await screen.findByText("Conversation detail")).toBeInTheDocument();
    expect(await screen.findByTestId("conversation-info")).toBeInTheDocument();
    expect(screen.getByText("alpha")).toBeInTheDocument();
    expect(screen.getByText("root")).toBeInTheDocument();
    // Existing agent info stays.
    expect(await screen.findByText("codex-cli")).toBeInTheDocument();
  });

  it("marks an unavailable bound directory (spec 5.5.3-1)", async () => {
    injectIpcCommands({
      getAgentDetail: vi.fn().mockResolvedValue(ok(cliDetail({}))),
      getConversationDetail: vi.fn().mockResolvedValue(
        ok(conversationDetail({ workDirectory: { id: "d1", displayName: "gone", absPath: "C:/x", available: false } })),
      ),
    } as never);
    renderWithProviders(
      <AgentBottomSheet agentKind="role" agentId="r1" onClose={vi.fn()} sessionId="s1" />,
    );
    expect(await screen.findByTestId("directory-unavailable")).toBeInTheDocument();
  });

  it("three-state policy applies immediately via setAgentAuthPolicy", async () => {
    const setPolicy = vi.fn().mockResolvedValue(ok(null));
    injectIpcCommands({
      getAgentDetail: vi.fn().mockResolvedValue(ok(cliDetail({}))),
      getConversationDetail: vi.fn().mockResolvedValue(ok(conversationDetail())),
      setAgentAuthPolicy: setPolicy,
    } as never);
    renderWithProviders(
      <AgentBottomSheet agentKind="role" agentId="r1" onClose={vi.fn()} sessionId="s1" />,
    );
    await screen.findByTestId("policy-r1");
    fireEvent.click(screen.getByRole("button", { name: "Deny" }));
    await waitFor(() => expect(setPolicy).toHaveBeenCalledWith("r1", "deny"));
  });

  it("policy save failure toasts and reverts (spec 5.5.3-2)", async () => {
    const setPolicy = vi.fn().mockRejectedValue(new Error("boom"));
    injectIpcCommands({
      getAgentDetail: vi.fn().mockResolvedValue(ok(cliDetail({}))),
      getConversationDetail: vi.fn().mockResolvedValue(ok(conversationDetail())),
      setAgentAuthPolicy: setPolicy,
    } as never);
    renderWithProviders(
      <AgentBottomSheet agentKind="role" agentId="r1" onClose={vi.fn()} sessionId="s1" />,
    );
    await screen.findByTestId("policy-r1");
    fireEvent.click(screen.getByRole("button", { name: "Authorize" }));
    await waitFor(() => expect(useToastStore.getState().toasts.length).toBe(1));
    await waitFor(() => expect(screen.queryByTestId("policy-r1")).not.toBeNull());
  });

  it("hides the session-info section without a session (legacy usage)", async () => {
    injectIpcCommands({
      getAgentDetail: vi.fn().mockResolvedValue(ok(cliDetail({}))),
    } as never);
    renderWithProviders(
      <AgentBottomSheet agentKind="role" agentId="r1" onClose={vi.fn()} sessionId={null} />,
    );
    expect(await screen.findByText("codex-cli")).toBeInTheDocument();
    expect(screen.queryByTestId("conversation-info")).not.toBeInTheDocument();
  });
});
