import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { tdSeedFiles } from "../../lib/ipc/test-double";
import { tdState } from "../../lib/ipc/test-double-state";
import { renderWithProviders } from "../../test/helpers";
import { Toaster } from "../../components/ui/Toaster";
import { useUiStore } from "../../lib/store/uiStore";
import { FilePanel } from "./FilePanel";

// Monaco never loads in jsdom — stand in with a plain textarea
// (same mock isolation as MonacoTab.test).
vi.mock("@monaco-editor/react", () => ({
  default: (props: { value: string; onChange: (v: string) => void }) => (
    <textarea aria-label="editor" value={props.value} onChange={(e) => props.onChange(e.target.value)} />
  ),
}));
// Skip the self-hosted monaco wiring (imports the real editor kernel).
vi.mock("./monacoSetup", () => ({}));

function openInStore(path: string): void {
  useUiStore.setState({ openFiles: [path], activeFile: path, dirtyPaths: {} });
}

function renderPanel(): void {
  renderWithProviders(
    <>
      <FilePanel />
      <Toaster />
    </>,
  );
}

describe("FilePanel — editor dirty flow", () => {
  it("marks the tab dirty on input and clears it after save; status bar shows language + hint", async () => {
    tdSeedFiles({ "README.md": "hello" });
    openInStore("README.md");
    renderPanel();

    const tab = await screen.findByRole("tab", { name: "README.md" });
    expect(tab).toHaveAttribute("title", "README.md");
    // Status bar: recognized language id + save hint (zh-CN default resources).
    expect(screen.getByText("markdown")).toBeInTheDocument();
    expect(screen.getByText("Ctrl+S 保存")).toBeInTheDocument();

    fireEvent.change(await screen.findByLabelText("editor"), { target: { value: "hello nuomi" } });
    await waitFor(() => expect(tab).toHaveAttribute("title", "README.md · 未保存的更改"));

    fireEvent.click(screen.getByRole("button", { name: "保存 (Ctrl+S)" }));
    await screen.findByText("文件已保存");
    await waitFor(() => expect(tab).toHaveAttribute("title", "README.md"));
  });

  it("asks for confirmation before closing a dirty tab; Esc keeps the tab open", async () => {
    tdSeedFiles({ "notes.txt": "seed" });
    openInStore("notes.txt");
    renderPanel();

    fireEvent.change(await screen.findByLabelText("editor"), { target: { value: "dirtier" } });

    const close = await screen.findByRole("button", { name: "关闭 notes.txt" });
    fireEvent.click(close);
    // Two-step confirm appears inline; the tab stays open.
    const confirm = screen.getByRole("button", { name: "确认关闭 notes.txt" });
    expect(screen.getByRole("tab", { name: "notes.txt" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "取消 notes.txt" })).toBeInTheDocument();

    // Esc keeps the tab and dismisses the confirm.
    fireEvent.keyDown(confirm, { key: "Escape" });
    expect(screen.queryByRole("button", { name: "确认关闭 notes.txt" })).toBeNull();
    expect(screen.getByRole("tab", { name: "notes.txt" })).toBeInTheDocument();

    // Second round: confirming discards the draft and closes.
    fireEvent.click(screen.getByRole("button", { name: "关闭 notes.txt" }));
    fireEvent.click(screen.getByRole("button", { name: "确认关闭 notes.txt" }));
    await waitFor(() => expect(screen.queryByRole("tab", { name: "notes.txt" })).toBeNull());
  });

  it("closes clean tabs immediately without confirmation", async () => {
    tdSeedFiles({ "a.json": "{}" });
    openInStore("a.json");
    renderPanel();

    fireEvent.click(await screen.findByRole("button", { name: "关闭 a.json" }));
    await waitFor(() => expect(screen.queryByRole("tab", { name: "a.json" })).toBeNull());
  });
});

describe("FilePanel — workspace title bar", () => {
  it("shows the current workspace root in the title bar", async () => {
    tdState.workspaceRoot = "D:\\projects\\demo";
    renderPanel();

    const label = await screen.findByText("D:\\projects\\demo");
    expect(label).toHaveAttribute("title", "D:\\projects\\demo");
  });

  it("opens the switch dialog and persists the new root on confirm", async () => {
    renderPanel();

    fireEvent.click(await screen.findByRole("button", { name: "切换工作区" }));
    const input = await screen.findByLabelText("工作区目录");
    expect(input).toHaveValue("C:\\workspace");

    fireEvent.change(input, { target: { value: "E:\\next" } });
    fireEvent.click(screen.getByRole("button", { name: "确认" }));

    await waitFor(() => {
      expect(tdState.workspaceRoot).toBe("E:\\next");
      expect(tdState.workspaceConfigured).toBe(true);
    });
    // The dialog closes after a successful switch; the title bar shows the new root.
    await waitFor(() => expect(screen.queryByLabelText("工作区目录")).toBeNull());
    expect(await screen.findByText("E:\\next")).toBeInTheDocument();
  });
});
