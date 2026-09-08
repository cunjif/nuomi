/**
 * WYSIWYG markdown editor tests: pure helpers plus the
 * source-while-focused / rendered-while-blurred interaction model.
 */
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { renderWithProviders } from "../../../test/helpers";
import { lexBlocks, WysiwygMarkdownEditor } from "./wysiwyg-markdown";

const DOC = "# Hi\n\npara **bold**";

describe("wysiwyg-markdown — pure helpers", () => {
  it("lexes a document into top-level block raws that reassemble it", () => {
    const doc = "# Title\n\npara one\ncontinues\n\n- a\n- b\n";
    const blocks = lexBlocks(doc);
    expect(blocks.join("")).toBe(doc);
    // Blank lines are standalone "space" tokens; block raws carry no trailing
    // newlines of their own.
    expect(blocks).toEqual(["# Title", "\n\n", "para one\ncontinues", "\n\n", "- a\n- b\n"]);
  });
});

describe("wysiwyg-markdown — component", () => {
  it("renders blurred blocks as HTML and reveals raw source on click", async () => {
    const onChange = vi.fn();
    renderWithProviders(<WysiwygMarkdownEditor path="d.md" content={DOC} onChange={onChange} />);

    const container = await screen.findByTestId("wysiwyg-editor");
    // Blurred: rendered, no markdown syntax visible.
    const heading = container.querySelector('[data-block-index="0"]') as HTMLElement;
    expect(heading.innerHTML).toContain("<h1>Hi</h1>");
    expect(screen.queryByRole("textbox")).toBeNull();

    // Click the heading: Typora-style raw source appears in a textarea.
    fireEvent.mouseDown(heading);
    const ta = container.querySelector("textarea") as HTMLTextAreaElement;
    expect(ta).not.toBeNull();
    expect(ta.value).toBe("# Hi");
    expect(document.activeElement).toBe(ta);

    // Blur renders it back.
    fireEvent.blur(ta);
    await waitFor(() => expect(container.querySelector("textarea")).toBeNull());
    expect((container.querySelector('[data-block-index="0"]') as HTMLElement).innerHTML).toContain("<h1>Hi</h1>");
  });

  it("typing source renders on blur and can split into multiple blocks", async () => {
    const onChange = vi.fn();
    renderWithProviders(<WysiwygMarkdownEditor path="d.md" content={DOC} onChange={onChange} />);

    const container = await screen.findByTestId("wysiwyg-editor");
    fireEvent.mouseDown(container.querySelector('[data-block-index="2"]') as HTMLElement);
    const ta = container.querySelector("textarea") as HTMLTextAreaElement;
    expect(ta.value).toBe("para **bold**");

    // Type multi-block markdown: renders + splits on blur.
    fireEvent.change(ta, { target: { value: "new **stuff**\n\nsecond" } });
    fireEvent.blur(ta);

    await waitFor(() => expect(onChange).toHaveBeenCalledWith("# Hi\n\nnew **stuff**\n\nsecond"));
    const html = (container.querySelector('[data-block-index="2"]') as HTMLElement).innerHTML;
    expect(html).toContain("<strong>stuff</strong>");
    // 3 real blocks + 2 blank-line spacers (spacers carry a block index too
    // so a click on them lands on the nearest editable block).
    expect(container.querySelectorAll("[data-block-index]").length).toBe(5);
  });

  it("exposes a reveal API (onReady) that opens the block source for a 0-based line", async () => {
    const onReady = vi.fn();
    renderWithProviders(
      <WysiwygMarkdownEditor path="d.md" content={DOC} onChange={() => {}} onReady={onReady} />,
    );

    await screen.findByTestId("wysiwyg-editor");
    expect(onReady).toHaveBeenCalledTimes(1);
    const api = onReady.mock.calls[0]?.[0] as { revealLine: (line: number) => void };

    // Line 0 = heading; blank-line blocks are never reveal targets.
    api.revealLine(0);
    await waitFor(() => expect(document.activeElement?.tagName).toBe("TEXTAREA"));
    const ta = document.activeElement as HTMLTextAreaElement;
    expect(ta.value).toBe("# Hi");

    // Escape renders back before revealing the next target.
    fireEvent.keyDown(ta, { key: "Escape" });
    api.revealLine(2); // "para **bold**"
    await waitFor(() => expect((document.activeElement as HTMLTextAreaElement).value).toBe("para **bold**"));
  });
});
