import { screen, waitFor, fireEvent } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { TempFileEntry } from "../../../lib/store/uiStore";
import { useUiStore } from "../../../lib/store/uiStore";
import { renderWithProviders } from "../../../test/helpers";
import { Toaster } from "../../../components/ui/Toaster";
import { EditableTempFileTab } from "./EditableTempFileTab";

vi.mock("@monaco-editor/react", () => ({
  default: (props: { value: string; onChange: (v: string) => void }) => (
    <textarea
      aria-label="editor"
      value={props.value}
      onChange={(e) => props.onChange(e.target.value)}
    />
  ),
}));
vi.mock("../monacoSetup", () => ({}));

function makeEntry(overrides: Partial<TempFileEntry> = {}): TempFileEntry {
  return {
    absPath: "C:\\external\\notes.txt",
    name: "notes.txt",
    content: "original content",
    draft: "original content",
    dirty: false,
    ...overrides,
  };
}

describe("EditableTempFileTab", () => {
  it("loads file content into the editor", async () => {
    const entry = makeEntry({ content: "hello world", draft: "hello world" });
    renderWithProviders(
      <>
        <EditableTempFileTab entry={entry} />
        <Toaster />
      </>,
    );
    const editor = await screen.findByRole("textbox", { name: "editor" });
    expect(editor).toHaveValue("hello world");
  });

  it("updates draft and sets dirty on edit", async () => {
    const entry = makeEntry();
    useUiStore.setState({
      tempFileTabs: [entry],
      activeTempFile: entry.absPath,
    });
    renderWithProviders(
      <>
        <EditableTempFileTab entry={entry} />
        <Toaster />
      </>,
    );
    const editor = await screen.findByRole("textbox", { name: "editor" });
    fireEvent.change(editor, { target: { value: "edited content" } });
    expect(useUiStore.getState().tempFileTabs[0]?.draft).toBe("edited content");
    expect(useUiStore.getState().tempFileTabs[0]?.dirty).toBe(true);
  });

  it("opens save dialog on Ctrl+S", async () => {
    const entry = makeEntry();
    renderWithProviders(
      <>
        <EditableTempFileTab entry={entry} />
        <Toaster />
      </>,
    );
    await screen.findByRole("textbox", { name: "editor" });
    fireEvent.keyDown(window, { key: "s", ctrlKey: true });
    await waitFor(() => expect(screen.getByText("保存到目标路径")).toBeDefined());
  });

  it("shows confirm dialog when closing a dirty tab", async () => {
    const entry = makeEntry({ draft: "edited", dirty: true });
    renderWithProviders(
      <>
        <EditableTempFileTab entry={entry} />
        <Toaster />
      </>,
    );
    await screen.findByRole("textbox", { name: "editor" });
    const closeBtn = screen.getByRole("button", { name: "关闭临时文件" });
    fireEvent.click(closeBtn);
    await waitFor(() => expect(screen.getByText("此临时文件有未保存的修改，确认关闭？")).toBeDefined());
  });
});
