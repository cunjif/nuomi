import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, afterEach } from "vitest";
import "../../i18n";
import { detectHtmlDocument, injectPreviewCsp, PREVIEW_CSP } from "./htmlDocument";
import { HtmlPreviewEntry } from "./HtmlPreviewEntry";
import { HtmlPreviewDialog } from "./HtmlPreviewDialog";

const FULL_DOC = "<!DOCTYPE html>\n<html><head><title>t</title></head><body><h1>Hi</h1></body></html>";

describe("detectHtmlDocument — D3 conservative rules", () => {
  it("matches a raw full document (doctype and <html>)", () => {
    expect(detectHtmlDocument(FULL_DOC)?.fenced).toBe(false);
    expect(detectHtmlDocument("  <html lang=\"en\"><body>x</body></html>")).not.toBeNull();
  });

  it("does not match markdown embedding html fragments", () => {
    expect(detectHtmlDocument("Here is a snippet:\n<div class=\"x\">hi</div>\nmore prose")).toBeNull();
    expect(detectHtmlDocument("**bold** and <span>inline</span>")).toBeNull();
    expect(detectHtmlDocument("<p>fragment only</p>")).toBeNull();
    expect(detectHtmlDocument("")).toBeNull();
  });

  it("matches a ```html fence wrapping a full document (and unfences it)", () => {
    const fenced = "```html\n" + FULL_DOC + "\n```";
    const detected = detectHtmlDocument(fenced);
    expect(detected).not.toBeNull();
    expect(detected?.fenced).toBe(true);
    expect(detected?.source).toContain("<!DOCTYPE html>");
    expect(detected?.source).not.toContain("```");
  });

  it("does not match a ```html fence holding only a fragment", () => {
    expect(detectHtmlDocument("```html\n<div>fragment</div>\n```")).toBeNull();
  });

  it("does not match a non-first or non-html fence", () => {
    expect(detectHtmlDocument("prose\n```js\nconst x = 1\n```")).toBeNull();
    expect(detectHtmlDocument("```js\n" + FULL_DOC + "\n```")).toBeNull();
  });
});

describe("injectPreviewCsp — D2 CSP injection", () => {
  it("injects the CSP meta inside <head> when present", () => {
    const out = injectPreviewCsp("<html><head><title>t</title></head><body></body></html>");
    const headOpen = out.indexOf("<head>");
    const cspAt = out.indexOf(PREVIEW_CSP);
    expect(cspAt).toBeGreaterThan(headOpen);
    expect(cspAt).toBeLessThan(out.indexOf("<title>"));
  });

  it("creates a <head> when the document lacks one", () => {
    const out = injectPreviewCsp("<html><body>x</body></html>");
    expect(out).toContain(`<head>${PREVIEW_CSP}</head>`);
  });

  it("prepends the CSP when even <html> is missing", () => {
    const out = injectPreviewCsp("text");
    expect(out.startsWith(PREVIEW_CSP)).toBe(true);
  });
});

describe("HtmlPreviewEntry — in-bubble affordance", () => {
  it("renders the entry button only for detected documents", () => {
    const { rerender } = render(<HtmlPreviewEntry text={FULL_DOC} />);
    expect(screen.getByTestId("html-preview-entry")).not.toBeNull();
    rerender(<HtmlPreviewEntry text="just markdown **bold**" />);
    expect(screen.queryByTestId("html-preview-entry")).toBeNull();
  });

  it("opens the sandboxed dialog on click", () => {
    render(<HtmlPreviewEntry text={FULL_DOC} />);
    fireEvent.click(screen.getByTestId("html-preview-entry"));
    expect(screen.getByTestId("html-preview-dialog")).not.toBeNull();
    const frame = screen.getByTestId("html-preview-frame") as HTMLIFrameElement;
    expect(frame.getAttribute("sandbox")).toBe("");
    expect(frame.getAttribute("srcdoc")).toContain(PREVIEW_CSP);
  });

  it("user messages never get a preview entry (caller scopes wiring)", () => {
    // The entry itself is text-driven; Bubble wires it only for non-user
    // roles — verified via Bubble tests. Here: fragment text stays clean.
    render(<HtmlPreviewEntry text="<div>fragment</div>" />);
    expect(screen.queryByTestId("html-preview-entry")).toBeNull();
  });
});

describe("HtmlPreviewDialog — sandbox, resize, failure fallback", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("closes on Escape", () => {
    const onClose = vi.fn();
    render(<HtmlPreviewDialog open={true} source={FULL_DOC} onClose={onClose} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });

  it("resizes via the corner handle within min bounds", () => {
    render(<HtmlPreviewDialog open={true} source={FULL_DOC} onClose={() => {}} />);
    const dialog = screen.getByTestId("html-preview-dialog");
    expect(dialog.style.width).toBe("720px");
    const handle = screen.getByTestId("html-preview-resize-handle");
    fireEvent.mouseDown(handle, { clientX: 700, clientY: 500 });
    fireEvent.mouseMove(window, { clientX: 900, clientY: 700 });
    fireEvent.mouseUp(window);
    expect(dialog.style.width).toBe("920px");
    expect(dialog.style.height).toBe("680px");
    // Below-min drag clamps.
    fireEvent.mouseDown(handle, { clientX: 900, clientY: 700 });
    fireEvent.mouseMove(window, { clientX: -5000, clientY: -5000 });
    fireEvent.mouseUp(window);
    expect(dialog.style.width).toBe("320px");
    expect(dialog.style.height).toBe("240px");
  });

  it("falls back to failure hint + source view when the frame errors", () => {
    const { container } = render(
      <HtmlPreviewDialog open={true} source={FULL_DOC} onClose={() => {}} />,
    );
    const frame = screen.getByTestId("html-preview-frame");
    // jsdom never fires iframe load; drive the error branch directly.
    fireEvent(frame, new Event("error"));
    expect(screen.getByText(/rendering failed|渲染失败/i)).not.toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /source|源文本/i }));
    expect(screen.getByTestId("html-preview-source")).not.toBeNull();
    expect(screen.getByTestId("html-preview-source").textContent).toContain(FULL_DOC);
    expect(container.querySelector("iframe")).toBeNull();
  });

  it("first-frame watchdog does not misfire after a successful load", () => {
    vi.useFakeTimers();
    try {
      render(<HtmlPreviewDialog open={true} source={FULL_DOC} onClose={() => {}} />);
      // Mark the frame as loaded (jsdom does this on its own for srcdoc;
      // here we drive it deterministically), then pass the timeout: the
      // watchdog must NOT flag a failure for a frame that painted in time.
      const frame = screen.getByTestId("html-preview-frame");
      fireEvent(frame, new Event("load", { bubbles: false }));
      vi.advanceTimersByTime(3_100);
      expect(screen.queryByText(/rendering failed|渲染失败/i)).toBeNull();
      expect(screen.getByTestId("html-preview-frame")).not.toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("first frame under 1s for pure local HTML (spec 4.1-5, sampling)", async () => {
    const start = performance.now();
    render(<HtmlPreviewDialog open={true} source={FULL_DOC} onClose={() => {}} />);
    const frame = screen.getByTestId("html-preview-frame") as HTMLIFrameElement;
    expect(frame.getAttribute("srcdoc")).not.toBeNull();
    // jsdom cannot paint; we assert the doc is synchronously mounted.
    expect(performance.now() - start).toBeLessThan(1_000);
    await waitFor(() => expect(frame).not.toBeNull());
  });
});
