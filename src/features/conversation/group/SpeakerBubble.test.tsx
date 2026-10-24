import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SpeakerBubble } from "./SpeakerBubble";
import type { PaletteColor } from "../avatarPalette";
import "../../../i18n";

const ROLE_COLOR: PaletteColor = {
  bg: "#7aa2f7",
  fg: "#1a1b26",
} as unknown as PaletteColor;

describe("SpeakerBubble — RoleAgent bubble wrapping (spec 5.1.1-1)", () => {
  it("renders message text inside a rounded bubble container", () => {
    render(
      <SpeakerBubble roleName="Researcher" roleColor={ROLE_COLOR} content="hello group" />,
    );
    const body = screen.getByTestId("speaker-bubble-body");
    expect(body).not.toBeNull();
    expect(body.className).toContain("rounded-lg");
    expect(body.className).toContain("bg-surface-raised");
    expect(body.textContent).toContain("hello group");
  });

  it("renders Markdown content through the shared pipeline", () => {
    render(
      <SpeakerBubble roleName="Researcher" roleColor={ROLE_COLOR} content={"**bold** _ital_"} />,
    );
    const body = screen.getByTestId("speaker-bubble-body");
    expect(body.querySelector("strong")).not.toBeNull();
    expect(body.querySelector("em")).not.toBeNull();
  });

  it("keeps role name, color bar, handoff marker and quote block", () => {
    render(
      <SpeakerBubble
        roleName="Coder"
        roleColor={ROLE_COLOR}
        content="work done"
        isHandoff={true}
        quotedExcerpt="previous turn excerpt"
      />,
    );
    expect(screen.getByText(/Coder/)).not.toBeNull();
    expect(screen.getByText(/Coder/).textContent).toContain("→");
    const colorBar = document.querySelector(".w-1.shrink-0.rounded-full");
    expect(colorBar).not.toBeNull();
    expect(screen.getByText(/previous turn excerpt/)).not.toBeNull();
  });

  it("renders source chip row when sources are provided", () => {
    render(
      <SpeakerBubble
        roleName="Researcher"
        roleColor={ROLE_COLOR}
        content="answer"
        sources={[
          { kind: "workspace_file", target: "C:\\proj\\a.rs", label: "a.rs", agentId: "r1", agentName: "R" },
        ]}
      />,
    );
    expect(screen.getByTestId("source-chip-row")).not.toBeNull();
    expect(screen.getByText("a.rs")).not.toBeNull();
  });

  it("renders no chip row when sources are absent or empty", () => {
    const { rerender } = render(<SpeakerBubble roleName="Researcher" roleColor={ROLE_COLOR} content="answer" />);
    expect(screen.queryByTestId("source-chip-row")).toBeNull();
    rerender(
      <SpeakerBubble roleName="Researcher" roleColor={ROLE_COLOR} content="answer" sources={[]} />,
    );
    expect(screen.queryByTestId("source-chip-row")).toBeNull();
  });
});

describe("SpeakerBubble — WeChat-style toolbar (气泡工具栏)", () => {
  it("draws a hairline border on the bubble body", () => {
    render(<SpeakerBubble roleName="Researcher" roleColor={ROLE_COLOR} content="hello group" />);
    const body = screen.getByTestId("speaker-bubble-body");
    expect(body.className).toContain("border-ink-muted/40");
  });

  it("pops the toolbar on click and forwards the quote action with the seq", () => {
    const onQuote = vi.fn();
    render(
      <SpeakerBubble
        roleName="Researcher"
        roleColor={ROLE_COLOR}
        content="hello group"
        seq={7}
        onQuote={onQuote}
        toolbarOpen={true}
        onToolbarClose={() => {}}
      />,
    );
    const toolbar = screen.getByTestId("bubble-toolbar");
    expect(within(toolbar).getByRole("button", { name: "引用" })).toBeTruthy();
    expect(within(toolbar).getByRole("button", { name: "复制" })).toBeTruthy();
    expect(within(toolbar).queryByRole("button", { name: /回退/ })).toBeNull();

    fireEvent.click(within(toolbar).getByRole("button", { name: "引用" }));
    expect(onQuote).toHaveBeenCalledWith(7);
  });

  it("keeps the body inert when quote/seq are not wired", () => {
    const onToolbarToggle = vi.fn();
    render(
      <SpeakerBubble
        roleName="Researcher"
        roleColor={ROLE_COLOR}
        content=""
        onToolbarToggle={onToolbarToggle}
      />,
    );
    // No text → no copy; no seq/onQuote → no quote: nothing to toggle.
    expect(screen.queryByTestId("bubble-toolbar")).toBeNull();
    fireEvent.click(screen.getByTestId("speaker-bubble-body"));
    expect(onToolbarToggle).not.toHaveBeenCalled();
  });
});
