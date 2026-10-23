import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { renderWithProviders } from "../../test/helpers";
import { SpeakerAside } from "./SpeakerAside";

describe("SpeakerAside — speaker column (avatar over name)", () => {
  it("renders the agent name under a palette-colored avatar", () => {
    renderWithProviders(<SpeakerAside name="码农小助手" seed="role-42" />);
    expect(screen.getByText("码农小助手")).toBeInTheDocument();
    expect(screen.getByLabelText("码农小助手")).toBeInTheDocument();
  });

  it("renders the fixed user identity with the initial 你", () => {
    renderWithProviders(<SpeakerAside name="你" isUser />);
    expect(screen.getAllByText("你").length).toBeGreaterThanOrEqual(2);
    expect(screen.getByLabelText("你")).toBeInTheDocument();
  });

  it("falls back to the themed palette when no seed is provided", () => {
    const { container } = render(<SpeakerAside name="assistant" />);
    const avatar = screen.getByLabelText("assistant");
    expect(avatar.style.backgroundColor).toBe("var(--nuomi-accent)");
    expect(container.textContent).toContain("assistant");
  });

  it("truncates a long name to the column width", () => {
    renderWithProviders(<SpeakerAside name="一个特别特别特别长的角色名称" seed="x" />);
    const name = screen.getByText("一个特别特别特别长的角色名称");
    expect(name.className).toContain("truncate");
  });
});
