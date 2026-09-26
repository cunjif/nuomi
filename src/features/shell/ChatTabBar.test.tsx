import { screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { seedConversation } from "../../lib/ipc/test-double";
import { useUiStore } from "../../lib/store/uiStore";
import { stubLocalStorage } from "../../test/stubStorage";
import { renderWithProviders } from "../../test/helpers";
import { ChatTabBar } from "./ChatTabBar";

beforeEach(() => {
  stubLocalStorage();
  useUiStore.setState({ openSessionIds: [], selectedSessionId: null });
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
});
