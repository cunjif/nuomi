import { screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { seedConversation, seedStewardSession, tdState } from "../../lib/ipc/test-double";
import { useUiStore } from "../../lib/store/uiStore";
import { stubLocalStorage } from "../../test/stubStorage";
import { renderWithProviders } from "../../test/helpers";
import { ChatTabBar } from "./ChatTabBar";

beforeEach(() => {
  stubLocalStorage();
  useUiStore.setState({
    openSessionIds: [],
    selectedSessionId: null,
    activeWorkspaceId: null,
  });
});

describe("ChatTabBar — tab separators", () => {
  it("draws one hairline between neighbouring tabs and none before the first", () => {
    useUiStore.setState({ openSessionIds: ["c1", "c2", "c3"] });
    seedConversation({ id: "c1", title: "会话一" });
    seedConversation({ id: "c2", title: "会话二" });
    seedConversation({ id: "c3", title: "会话三" });
    renderWithProviders(<ChatTabBar />);

    expect(screen.getAllByTestId("chat-tab-divider")).toHaveLength(2);
  });

  it("a lone tab has no separator (and the strip is not left with a leading rule)", () => {
    useUiStore.setState({ openSessionIds: ["c1"] });
    seedConversation({ id: "c1", title: "会话一" });
    renderWithProviders(<ChatTabBar />);

    expect(screen.queryAllByTestId("chat-tab-divider")).toHaveLength(0);
  });

  it("the rule is decorative, 1px wide, vertically centred, and tinted by the theme token", () => {
    useUiStore.setState({ openSessionIds: ["c1", "c2"] });
    seedConversation({ id: "c1", title: "会话一" });
    seedConversation({ id: "c2", title: "会话二" });
    renderWithProviders(<ChatTabBar />);

    const rule = screen.getAllByTestId("chat-tab-divider")[0]!;
    const classes = rule.className.split(" ");
    // `bg-divider` (not `bg-ink-muted/30`): this project's colors are `var(...)`
    // strings, and Tailwind v3 emits nothing for an opacity modifier on those —
    // the tint has to live in the per-theme --nuomi-divider token.
    expect(classes).toContain("bg-divider");
    expect(classes).toContain("w-px");
    expect(classes).toContain("h-4");
    // The strip row is items-end, so the rule needs self-center to sit in the
    // middle of the 40px chip instead of hugging its baseline.
    expect(classes).toContain("self-center");
    expect(rule).toHaveAttribute("aria-hidden", "true");
  });

  it("never renders a chat tab for the steward session (spec 5.2 isolation)", async () => {
    useUiStore.setState({ openSessionIds: ["c1"] });
    seedConversation({ id: "c1", title: "会话一" });
    seedStewardSession({ id: "steward-1", title: "新管家会话" });
    renderWithProviders(<ChatTabBar />);

    // Tab titles resolve from the (steward-excluded) conversation listing.
    expect(await screen.findByText("会话一")).toBeTruthy();
    expect(screen.queryByText("新管家会话")).toBeNull();
  });
});

describe("ChatTabBar — workspace-scoped strip (ADR-0021 task 6.9)", () => {
  it("renders only tabs whose session belongs to the open workspace", async () => {
    useUiStore.setState({
      openSessionIds: ["w1a", "w1b", "w2a"],
      selectedSessionId: null,
      activeWorkspaceId: "ws1",
    });
    seedConversation({ id: "w1a", title: "alpha期会话", workspaceId: "ws1" });
    seedConversation({ id: "w1b", title: "beta期会话", workspaceId: "ws1" });
    seedConversation({ id: "w2a", title: "gamma会话", workspaceId: "ws2" });
    renderWithProviders(<ChatTabBar />);

    expect(await screen.findByText("alpha期会话")).toBeTruthy();
    expect(screen.getByText("beta期会话")).toBeTruthy();
    expect(screen.queryByText("gamma会话")).toBeNull();
  });

  it("re-homes the selection to the first surviving tab after a workspace switch", async () => {
    useUiStore.setState({
      openSessionIds: ["w1a", "w2a"],
      selectedSessionId: "w2a",
      activeWorkspaceId: "ws1",
      activeArea: "workbench",
    });
    seedConversation({ id: "w1a", title: "alpha期会话", workspaceId: "ws1" });
    seedConversation({ id: "w2a", title: "gamma会话", workspaceId: "ws2" });
    renderWithProviders(<ChatTabBar />);

    expect(await screen.findByText("alpha期会话")).toBeTruthy();
    // The stale ws2 selection activates the first ws1 tab instead — without
    // the activeArea side effect of selectSession (the user stays in the
    // editor area).
    await waitFor(() => {
      expect(useUiStore.getState().selectedSessionId).toBe("w1a");
    });
    expect(useUiStore.getState().activeArea).toBe("workbench");
  });

  it("tags each tab with its workspace color dot and a label-prefixed tooltip", async () => {
    tdState.workspaces.push({
      id: "ws1",
      rootPath: "/tmp/alpha",
      colorTag: "paper-yellow",
      createdAt: 1,
      isActive: true,
      directoryPresent: true,
      label: "alpha-beta",
      labelFull: "alpha-beta",
    });
    useUiStore.setState({ openSessionIds: ["w1a"], selectedSessionId: "w1a", activeWorkspaceId: "ws1" });
    seedConversation({ id: "w1a", title: "alpha期会话", workspaceId: "ws1" });
    renderWithProviders(<ChatTabBar />);

    const title = await screen.findByText("alpha期会话");
    const tab = title.closest('[role="tab"]');
    expect(tab).not.toBeNull();
    expect(tab!.getAttribute("title")).toBe("alpha-beta — alpha期会话");
    expect(tab!.querySelector('[data-testid="chat-tab-workspace-dot"]')).not.toBeNull();
  });

  it("hides foreign tabs when the open workspace has no sessions, keeping the stale selection", async () => {
    useUiStore.setState({
      openSessionIds: ["w1a"],
      selectedSessionId: "w1a",
      activeWorkspaceId: "ws-none",
    });
    seedConversation({ id: "w1a", title: "alpha期会话", workspaceId: "ws1" });
    renderWithProviders(<ChatTabBar />);

    // The workspace-scoped query resolves to empty → no tabs render (the "+"
    // button stays); the stale selection survives so switching back restores
    // the tab + selection.
    await waitFor(() => {
      expect(screen.queryByRole("tab")).toBeNull();
    });
    expect(screen.getByRole("button", { name: "新建会话" })).toBeTruthy();
    expect(useUiStore.getState().selectedSessionId).toBe("w1a");
  });

  it("empty state (no workspace open) keeps the cross-workspace fallback view", async () => {
    useUiStore.setState({ openSessionIds: ["c1"], selectedSessionId: null, activeWorkspaceId: null });
    seedConversation({ id: "c1", title: "存量会话" });
    renderWithProviders(<ChatTabBar />);

    expect(await screen.findByText("存量会话")).toBeTruthy();
  });
});
