import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { injectIpcCommands } from "../../lib/ipc/client";
import { tdSeedFiles, tdState, testDoubleCommands } from "../../lib/ipc/test-double";
import { renderWithProviders } from "../../test/helpers";
import { Toaster } from "../../components/ui/Toaster";
import { MonacoTab } from "./MonacoTab";

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
  it("writes the edited content on Ctrl+S", async () => {
    tdSeedFiles({ "README.md": "hello" });
    const base = testDoubleCommands();
    const writeFile = vi.fn(base.writeFile);
    injectIpcCommands({ ...base, writeFile });

    renderTab("README.md");

    const editor = await screen.findByLabelText("editor");
    expect(editor).toHaveValue("hello");

    fireEvent.change(editor, { target: { value: "hello nuomi" } });
    fireEvent.keyDown(editor.closest("section") as HTMLElement, { key: "s", ctrlKey: true });

    await waitFor(() => expect(writeFile).toHaveBeenCalledWith("README.md", "hello nuomi"));
    await screen.findByText("文件已保存");
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
    const editor = await screen.findByLabelText("editor");
    fireEvent.change(editor, { target: { value: "y" } });
    fireEvent.click(screen.getByRole("button", { name: "保存 (Ctrl+S)" }));

    await screen.findByText(/路径越界/);
  });
});
