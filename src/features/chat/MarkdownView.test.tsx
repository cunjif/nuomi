import { render, screen, fireEvent } from "@testing-library/react";
import { describe, expect, it, vi, afterEach } from "vitest";
import { marked } from "marked";
import { MarkdownView } from "./MarkdownView";
import { setOpenUrlImpl } from "../../lib/openUrl";

describe("MarkdownView — bubble rich rendering", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    setOpenUrlImpl(async () => false);
  });

  it("renders code blocks, tables, lists and blockquotes", () => {
    const text = [
      "# Title",
      "",
      "- item one",
      "- item two",
      "",
      "> quoted line",
      "",
      "| a | b |",
      "| --- | --- |",
      "| 1 | 2 |",
      "",
      "```rust",
      "fn main() {}",
      "```",
    ].join("\n");
    const { container } = render(<MarkdownView text={text} />);
    const view = screen.getByTestId("markdown-view");
    expect(view.querySelector("h1")).not.toBeNull();
    expect(view.querySelectorAll("li").length).toBe(2);
    expect(view.querySelector("blockquote")).not.toBeNull();
    expect(view.querySelector("table")).not.toBeNull();
    expect(view.querySelector("pre code")).not.toBeNull();
    expect(container.textContent).toContain("Title");
  });

  it("strips script content — no script node survives sanitization", () => {
    const text = 'hello <script>window.__pwned = true</script> world';
    render(<MarkdownView text={text} />);
    const view = screen.getByTestId("markdown-view");
    expect(view.querySelector("script")).toBeNull();
    expect(document.querySelector("script")).toBeNull();
    expect(view.textContent).toContain("hello");
    expect("__pwned" in window).toBe(false);
  });

  it("degrades to raw text when marked.parse throws", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.spyOn(marked, "parse").mockImplementationOnce(() => {
      throw new Error("boom");
    });
    render(<MarkdownView text="**broken** input" />);
    expect(screen.getByText("**broken** input")).not.toBeNull();
    expect(screen.queryByTestId("markdown-view")).toBeNull();
    expect(warn).toHaveBeenCalledTimes(1);
    // Degradation log never carries the raw text (spec 4.4-1).
    expect(warn.mock.calls[0]?.[0]).not.toContain("**broken** input");
  });

  it("keeps streaming cursor outside the sanitized container", () => {
    render(<MarkdownView text="partial" streaming={true} />);
    expect(screen.getByTestId("markdown-view")).not.toBeNull();
    const cursor = document.querySelector(".animate-pulse");
    expect(cursor).not.toBeNull();
    expect(screen.getByTestId("markdown-view").contains(cursor as Node)).toBe(false);
  });

  it("intercepts anchor clicks and routes to the external opener", () => {
    const opener = vi.fn(async () => true);
    setOpenUrlImpl(opener);
    render(<MarkdownView text="[link](https://example.com/docs)" />);
    const anchor = screen.getByTestId("markdown-view").querySelector("a");
    expect(anchor).not.toBeNull();
    fireEvent.click(anchor as HTMLElement);
    expect(opener).toHaveBeenCalledWith("https://example.com/docs");
  });

  it("skips re-parsing when props are unchanged (memo)", () => {
    const parse = vi.spyOn(marked, "parse");
    const { rerender } = render(<MarkdownView text="same text" />);
    expect(parse).toHaveBeenCalledTimes(1);
    rerender(<MarkdownView text="same text" />);
    expect(parse).toHaveBeenCalledTimes(1);
    rerender(<MarkdownView text="new text" />);
    expect(parse).toHaveBeenCalledTimes(2);
  });

  it("renders 1KB markdown well under the 50ms budget (spec 4.1-1)", () => {
    const paragraph = "the quick brown fox jumps over the lazy dog. ".repeat(20);
    const text = Array.from({ length: 25 }, (_, i) => `${paragraph}\n\nitem ${i}`).join("\n");
    expect(text.length).toBeGreaterThanOrEqual(1000);
    const parseSpy = vi.spyOn(marked, "parse");
    const start = performance.now();
    render(<MarkdownView text={text} />);
    const elapsed = performance.now() - start;
    expect(parseSpy).toHaveBeenCalledTimes(1);
    expect(elapsed).toBeLessThan(50);
  });
});
