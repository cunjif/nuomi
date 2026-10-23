import { screen } from "@testing-library/react";
import { describe, expect, it, beforeEach } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { tdState, seedConversation, seedRole } from "../../lib/ipc/test-double";
import { ConversationView } from "./ConversationView";
import { useUiStore } from "../../lib/store/uiStore";

/** A ready role (providerId bound) so ConversationView renders the panel. */
function seedReadyRole(id: string): void {
  seedRole({
    id,
    name: id,
    providerId: "p1",
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
  });
}

describe("ConversationView — floating more button (Agent 详情入口)", () => {
  beforeEach(() => {
    seedReadyRole("role-cv");
  });

  it("renders the embedded ··· row above the transcript, in-flow (not floating)", async () => {
    seedConversation({ id: "cv1", title: "t", createdAt: 1, updatedAt: 1 });
    tdState.events.set("cv1", [
      { seq: 1, kind: "message", payload: { role: "user", content: "你好" }, createdAt: 1 },
    ]);
    useUiStore.getState().selectSession("cv1");

    const { container } = renderWithProviders(<ConversationView />);
    expect(await screen.findByText("你好")).toBeInTheDocument();

    // The ··· affordance is an EMBEDDED row: in-flow (no absolute/z-index),
    // right-aligned, above the transcript — not an overlay floating over
    // the message list.
    const more = screen.getByRole("button", { name: "打开 Agent 侧边栏" });
    expect(more.className).not.toContain("absolute");
    expect(more.className).not.toContain("z-20");
    expect(more.parentElement?.className).toContain("justify-end");

    // Compact footprint: 24px button (size-6) + 4px row top padding and no
    // bottom padding — the row must stay at ~28px so the transcript keeps
    // the reclaimed space (regression guard against re-bulking the row).
    expect(more.className).toContain("size-6");
    expect(more.parentElement?.className).toContain("pt-1");
    expect(more.parentElement?.className).toContain("pb-0");

    // No path band under the nav tabs: the panel root's only child is the
    // flex-col content wrapper — the embedded row and the kind view are
    // its in-flow children (regression guard against reintroducing a
    // middle path strip).
    const root = container.firstElementChild;
    expect(root?.className).toContain("flex");
    expect(root?.className).toContain("h-full");
    expect(root?.children.length).toBe(1);
    const content = root?.firstElementChild;
    expect(content?.className).toContain("flex-col");
    expect(content?.firstElementChild?.className).toContain("shrink-0");

    // No overlay headroom: the message scroll keeps its small top padding.
    const scroll = screen.getByTestId("message-scroll");
    expect(scroll.className).toContain("pt-1");
    expect(scroll.className).not.toContain("pt-9");
  });

  it("hides the floating ··· when no conversation data is loaded", () => {
    useUiStore.setState({ selectedSessionId: null });
    renderWithProviders(<ConversationView />);
    expect(screen.queryByRole("button", { name: "打开 Agent 侧边栏" })).toBeNull();
  });
});
