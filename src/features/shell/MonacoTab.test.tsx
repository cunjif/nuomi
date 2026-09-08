import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { injectIpcCommands } from "../../lib/ipc/client";
import { tdSeedFiles, tdState, testDoubleCommands } from "../../lib/ipc/test-double";
import { renderWithProviders } from "../../test/helpers";
import { Toaster } from "../../components/ui/Toaster";
import { activateBuiltinEditorExtensions } from "../../lib/editor-ext";
import { requestOpenSymbol } from "../../lib/editor-ext/indexer/workspace-index";
import { MonacoTab } from "./MonacoTab";

// MonacoTab is normally hosted by EditorArea, which activates the builtin
// editor extensions at module load; standalone tests do it explicitly so the
// WYSIWYG/preview registrations are queryable.
activateBuiltinEditorExtensions();

// Monaco never loads in jsdom — stand in with a plain textarea.
vi.mock("@monaco-editor/react", () => ({
  default: (props: { value: string; onChange: (v: string) => void }) => (
    <textarea aria-label="editor" value={props.value} onChange={(e) => props.onChange(e.target.value)} />
  ),
}));
// Skip the self-hosted monaco wiring (imports the real editor kernel).
vi.mock("./monacoSetup", () => ({}));

function renderTab(path: string): void {
  renderWithProviders(
    <>
      <MonacoTab path={path} />
      <Toaster />
    </>,
  );
}

describe("MonacoTab — edit + Ctrl+S save flow (AC10)", () => {
  it("writes the edited content on Ctrl+S (source mode)", async () => {
    tdSeedFiles({ "README.md": "hello" });
    const base = testDoubleCommands();
    const writeFile = vi.fn(base.writeFile);
    injectIpcCommands({ ...base, writeFile });

    renderTab("README.md");

    // .md files open in Typora-style WYSIWYG by default; switch to source.
    await screen.findByTestId("wysiwyg-editor");
    fireEvent.click(screen.getByRole("button", { name: "源码" }));

    const editor = await screen.findByLabelText("editor");
    expect(editor).toHaveValue("hello");

    fireEvent.change(editor, { target: { value: "hello nuomi" } });
    fireEvent.keyDown(editor.closest("section") as HTMLElement, { key: "s", ctrlKey: true });

    await waitFor(() => expect(writeFile).toHaveBeenCalledWith("README.md", "hello nuomi"));
    await screen.findByText("文件已保存");
  });

  it("saves WYSIWYG edits round-tripped back to markdown on Ctrl+S", async () => {
    tdSeedFiles({ "notes.md": "hello" });
    const base = testDoubleCommands();
    const writeFile = vi.fn(base.writeFile);
    injectIpcCommands({ ...base, writeFile });

    renderTab("notes.md");

    // Click the rendered block: it swaps to a raw-source textarea (Typora).
    const container = await screen.findByTestId("wysiwyg-editor");
    fireEvent.mouseDown(container.querySelector("[data-block-index]") as HTMLElement);
    const ta = container.querySelector("textarea") as HTMLTextAreaElement;
    fireEvent.change(ta, { target: { value: "hello nuomi" } });
    fireEvent.keyDown(ta, { key: "s", ctrlKey: true });

    await waitFor(() => expect(writeFile).toHaveBeenCalledWith("notes.md", "hello nuomi"));
    await screen.findByText("文件已保存");
  });

  it("shows the outline beside the WYSIWYG pane and drops the rendered preview in source mode", async () => {
    tdSeedFiles({ "doc.md": "# Title\n\nbody" });
    const base = testDoubleCommands();
    injectIpcCommands({ ...base });

    renderTab("doc.md");

    await screen.findByTestId("wysiwyg-editor");
    // Outline toggle available next to the WYSIWYG pane (headings → symbols).
    expect(screen.getByRole("button", { name: "大纲" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "源码" }));
    await screen.findByLabelText("editor");
    // Source mode is plain Monaco — no rendered markdown pane (需求: 源码不显示渲染).
    expect(screen.queryByTestId("markdown-preview")).toBeNull();
    expect(screen.getByRole("button", { name: "大纲" })).toBeInTheDocument();
  });

  it("lands a pending cross-file jump in WYSIWYG mode (markdown never mounts Monaco)", async () => {
    tdSeedFiles({ "doc.md": "# Title\n\nbody" });
    const base = testDoubleCommands();
    injectIpcCommands({ ...base });

    // F12 from another file while doc.md is closed: the jump is queued.
    requestOpenSymbol("doc.md", 2); // 0-based line of "body"
    renderTab("doc.md");

    const container = await screen.findByTestId("wysiwyg-editor");
    await waitFor(() =>
      expect((container.querySelector("textarea") as HTMLTextAreaElement | null)?.value).toBe("body"),
    );
  });

  it("shows a toast when the path escapes the workspace", async () => {
    tdSeedFiles({ "README.md": "x" });
    const base = testDoubleCommands();
    injectIpcCommands({
      ...base,
      writeFile: async () => ({
        status: "error",
        error: { generic: { code: "workspace.escape_denied", message: "escape" } },
      }),
    });
    (tdState.files.get("README.md") as { content: string }).content = "x";

    renderTab("README.md");
    await screen.findByTestId("wysiwyg-editor");
    fireEvent.click(screen.getByRole("button", { name: "源码" }));
    const editor = await screen.findByLabelText("editor");
    fireEvent.change(editor, { target: { value: "y" } });
    fireEvent.click(screen.getByRole("button", { name: "保存 (Ctrl+S)" }));

    await screen.findByText(/路径越界/);
  });
});
