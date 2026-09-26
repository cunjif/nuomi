import { fireEvent, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { ChatTab } from "./ChatTab";

describe("ChatTab", () => {
  it("renders kind icon + title (≤20 chars) with truncate class", () => {
    renderWithProviders(
      <ChatTab sessionId="s1" kind="chat" title="Hello" active={false} onActivate={() => {}} onClose={() => {}} />,
    );
    const tab = screen.getByRole("tab");
    expect(tab.textContent).toContain("💬");
    expect(tab.textContent).toContain("Hello");
    expect(tab.querySelector(".truncate")).not.toBeNull();
  });

  it("title >20 chars: truncated with ellipsis, full title in title attribute", () => {
    const longTitle = "This is a very long conversation title that exceeds twenty chars";
    renderWithProviders(
      <ChatTab sessionId="s1" kind="chat" title={longTitle} active={false} onActivate={() => {}} onClose={() => {}} />,
    );
    const tab = screen.getByRole("tab");
    expect(tab.getAttribute("title")).toBe(longTitle);
    expect(tab.textContent).toContain("…");
    expect(tab.textContent).not.toContain(longTitle);
  });

  it("active tab uses bg-surface (merges with header below), inactive uses muted", () => {
    const { rerender } = renderWithProviders(
      <ChatTab sessionId="s1" kind="chat" title="A" active={false} onActivate={() => {}} onClose={() => {}} />,
    );
    const inactive = screen.getByRole("tab");
    expect(inactive.className.split(" ")).not.toContain("bg-surface");
    expect(inactive.className.split(" ")).toContain("text-ink-muted");

    rerender(
      <ChatTab sessionId="s1" kind="chat" title="A" active={true} onActivate={() => {}} onClose={() => {}} />,
    );
    const active = screen.getByRole("tab");
    expect(active.className.split(" ")).toContain("bg-surface");
    expect(active.className.split(" ")).toContain("text-ink-accent");
  });

  it("close button has opacity-0 by default (hover-revealed)", () => {
    renderWithProviders(
      <ChatTab sessionId="s1" kind="chat" title="A" active={false} onActivate={() => {}} onClose={() => {}} />,
    );
    const closeBtn = screen.getByRole("button", { name: "close" });
    expect(closeBtn.className).toContain("opacity-0");
  });

  it("click tab triggers onActivate with sessionId", () => {
    const onActivate = vi.fn();
    renderWithProviders(
      <ChatTab sessionId="s1" kind="chat" title="A" active={false} onActivate={onActivate} onClose={() => {}} />,
    );
    fireEvent.click(screen.getByRole("tab"));
    expect(onActivate).toHaveBeenCalledWith("s1");
  });

  it("click close triggers onClose with sessionId and stops propagation", () => {
    const onActivate = vi.fn();
    const onClose = vi.fn();
    renderWithProviders(
      <ChatTab sessionId="s1" kind="chat" title="A" active={false} onActivate={onActivate} onClose={onClose} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "close" }));
    expect(onClose).toHaveBeenCalledWith("s1");
    expect(onActivate).not.toHaveBeenCalled();
  });
});
