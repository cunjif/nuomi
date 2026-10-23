import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { SpeakerBubble } from "./SpeakerBubble";
import type { PaletteColor } from "../avatarPalette";

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
