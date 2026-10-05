import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { tdSeedFiles } from "../../lib/ipc/test-double";
import { tdState } from "../../lib/ipc/test-double-state";
import { renderWithProviders } from "../../test/helpers";
import { Toaster } from "../../components/ui/Toaster";
import { useUiStore } from "../../lib/store/uiStore";
import { EditorArea } from "./EditorArea";

// Monaco never loads in jsdom — stand in with a plain textarea
// (same mock isolation as MonacoTab.test).
vi.mock("@monaco-editor/react", () => ({
  default: (props: { value: string; onChange: (v: string) => void }) => (
    <textarea aria-label="editor" value={props.value} onChange={(e) => props.onChange(e.target.value)} />
  ),
}));
// Skip the self-hosted monaco wiring (imports the real editor kernel).
vi.mock("./monacoSetup", () => ({}));

const folderPick = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: (...a: unknown[]) => folderPick(...a) }));

// uiStore.openWorkspace/focusWorkspace call tauri invoke directly; route
// open_workspace/focus_workspace into tdState so isFocused updates land.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (cmd: string, args?: Record<string, unknown>) => {
    if (cmd === "open_workspace" || cmd === "focus_workspace") {
      const id = args?.id as string;
      for (const w of tdState.workspaces ?? []) w.isFocused = w.id === id;
      return { workspaceId: id };
    }
    if (cmd === "close_workspace") return { closedId: args?.id, newFocusedId: null };
    if (cmd === "get_open_set") {
      return { openWorkspaces: [], focusedWorkspaceId: null, pinnedWorkspaceIds: [], unreadIndicators: [] };
    }
    if (cmd === "set_layout_snapshot") return null;
    return undefined;
  },
}));

function openInStore(path: string): void {
  useUiStore.setState({
    editorByWorkspace: {
      "ws-test": { openFiles: [path], activeFile: path, selectedPaths: [], lastSelectedPath: null, clipboardPaths: [], clipboardMode: null, dirtyPaths: {}, crossRefs: {} },
    },
  });
}

function renderPanel(): void {
  renderWithProviders(
    <>
      <EditorArea workspaceId="ws-test" />
      <Toaster />
    </>,
  );
}

describe("EditorArea — editor dirty flow", () => {
  it("marks the tab dirty on input and clears it after save; status bar shows language + hint", async () => {
    tdSeedFiles({ "README.md": "hello" });
    openInStore("README.md");
    renderPanel();

    const tab = await screen.findByRole("tab", { name: "README.md" });
    expect(tab).toHaveAttribute("title", "README.md");
    // .md files open in Typora-style WYSIWYG by default (需求: markdown 所见即所得).
    await screen.findByTestId("wysiwyg-editor");
    // Switch to source mode for the classic Monaco dirty/save flow.
    fireEvent.click(screen.getByRole("button", { name: "源码" }));
    // Status bar: recognized language id + save hint (zh-CN default resources).
    expect(screen.getByText("markdown")).toBeInTheDocument();
    expect(screen.getByText("Ctrl+S 保存")).toBeInTheDocument();

    fireEvent.change(await screen.findByLabelText("editor"), { target: { value: "hello nuomi" } });
    await waitFor(() => expect(tab).toHaveAttribute("title", "README.md · 未保存的更改"));

    fireEvent.keyDown(screen.getByLabelText("editor").closest("section") as HTMLElement, { key: "s", ctrlKey: true });
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

describe("EditorArea — workspace sidebar toolbar", () => {
  /** The workspace toolbar lives at the top of the explorer sidebar (用户截图). */
  function renderWithToolbar(): void {
    renderWithProviders(
      <>
        <EditorArea workspaceId="ws-test" />
        <Toaster />
      </>,
    );
  }

  it("shows the current workspace root in the sidebar toolbar", async () => {
    tdState.workspaceRoot = "D:\\projects\\demo";
    renderWithToolbar();

    const label = await screen.findByText("D:\\projects\\demo");
    expect(label).toHaveAttribute("title", "D:\\projects\\demo");
  });

  it("opens the switch dialog and activates a workspace on row click", async () => {
    tdState.workspaces.push(
      { id: "ws-current", rootPath: "C:\\workspace", colorTag: "ink-blue", createdAt: 1, isActive: false, isFocused: true, directoryPresent: true },
      { id: "ws-other", rootPath: "D:\\other", colorTag: "paper-yellow", createdAt: 2, isActive: false, isFocused: false, directoryPresent: true },
    );
    renderWithToolbar();

    fireEvent.click(await screen.findByRole("button", { name: "切换工作区" }));
    const dialog = await screen.findByRole("dialog", { name: "切换工作区" });
    // Clicking a non-focused workspace row activates it (openWorkspace) and closes.
    fireEvent.click(within(dialog).getByText("D:\\other"));

    await waitFor(() => {
      expect(tdState.workspaces.find((w) => w.id === "ws-other")?.isFocused).toBe(true);
    });
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "切换工作区" })).toBeNull());
  });

  it("取消 closes the dialog without persisting", async () => {
    tdState.workspaces.push(
      { id: "ws-current", rootPath: "C:\\workspace", colorTag: "ink-blue", createdAt: 1, isActive: false, isFocused: true, directoryPresent: true },
    );
    renderWithToolbar();

    fireEvent.click(await screen.findByRole("button", { name: "切换工作区" }));
    fireEvent.click(screen.getByText("取消"));

    await waitFor(() => expect(screen.queryByRole("dialog", { name: "切换工作区" })).toBeNull());
    expect(tdState.workspaces.find((w) => w.id === "ws-current")?.isFocused).toBe(true);
  });

  it("新增工作区 button calls the OS folder picker", async () => {
    folderPick.mockResolvedValue("D:\\picked\\dir");
    tdState.workspaces.push(
      { id: "ws-current", rootPath: "C:\\workspace", colorTag: "ink-blue", createdAt: 1, isActive: false, isFocused: true, directoryPresent: true },
    );
    renderWithToolbar();

    fireEvent.click(await screen.findByRole("button", { name: "切换工作区" }));
    fireEvent.click(await screen.findByText("新增工作区"));

    await waitFor(() => expect(folderPick).toHaveBeenCalled());
  });
});
