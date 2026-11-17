import { fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { tdState } from "../../lib/ipc/test-double-state";
import { Toaster } from "../../components/ui/Toaster";
import { useUiStore } from "../../lib/store/uiStore";
import { RightPanelDock } from "./RightPanelDock";

function seedWorkspaceWithDirs(activeFirst: boolean): void {
  tdState.workspaces.push({
    id: "ws-a",
    rootPath: "C:\\root-ws-a",
    colorTag: "ink-blue",
    createdAt: 1,
    isActive: true,
    isFocused: true,
    isOpen: true,
    directoryPresent: true,
    label: "alpha-beta",
    labelFull: "alpha-beta",
    directories: [
      { id: "dir-1", absPath: "C:\\alpha", displayName: "alpha", mounted: true, addedAt: 1, available: true, isActive: activeFirst },
      { id: "dir-2", absPath: "C:\\beta", displayName: "beta", mounted: true, addedAt: 2, available: true, isActive: !activeFirst },
      { id: "dir-3", absPath: "C:\\gamma", displayName: "gamma", mounted: false, addedAt: 3, available: true, isActive: false },
    ],
  });
  tdState.activeDirectoryId = activeFirst ? "dir-1" : "dir-2";
}

/** Seeds files under the sandbox root (relative paths, test-double listDir). */
function seedFiles(): void {
  tdState.files.set("README.md", { isDir: false, size: 10, content: "hi" });
  tdState.files.set("src/main.rs", { isDir: false, size: 4, content: "fn m() {}" });
}

function renderDock(): void {
  renderWithProviders(<RightPanelDock workspaceId="ws-a" />);
}

afterEach(() => {
  tdState.workspaces.length = 0;
  tdState.activeDirectoryId = null;
  tdState.files.clear();
  useUiStore.setState({
    rightPanel: { width: 224, collapsed: false, hidden: false },
    editorByWorkspace: {},
    activeWorkspaceId: null,
  });
});

describe("RightPanelDock — directory badges", () => {
  it("renders mounted directories as initial badges with the active one marked", async () => {
    seedWorkspaceWithDirs(true);
    renderDock();
    const alpha = await screen.findByRole("tab", { name: "alpha" });
    expect(alpha).toHaveTextContent("A");
    expect(alpha).toHaveAttribute("aria-selected", "true");
    // Hover reveals the full path (absPath), not the truncated display name.
    expect(alpha).toHaveAttribute("title", "C:\\alpha");
    expect(screen.getByRole("tab", { name: "beta" })).toHaveTextContent("B");
    // Unmounted directories stay out of the badge list.
    expect(screen.queryByRole("tab", { name: "gamma" })).toBeNull();
  });

  it("focuses a directory and opens its browse popover on click", async () => {
    seedWorkspaceWithDirs(true);
    seedFiles();
    renderDock();
    fireEvent.click(await screen.findByRole("tab", { name: "beta" }));
    // Focus already moved to the clicked directory.
    expect(tdState.activeDirectoryId).toBe("dir-2");
    // Popover opens and the FileTree lists the newly focused root once the
    // workspace query settles (loading state hides the stale-root flash).
    expect(await screen.findByTestId("dock-browse-popover")).toBeInTheDocument();
    expect(await screen.findByText("README.md")).toBeInTheDocument();
    expect(screen.getByText("src")).toBeInTheDocument();
  });

  it("opens the browse popover directly for the already-active directory", async () => {
    seedWorkspaceWithDirs(true);
    seedFiles();
    renderDock();
    fireEvent.click(await screen.findByRole("tab", { name: "alpha" }));
    expect(await screen.findByTestId("dock-browse-popover")).toBeInTheDocument();
    expect(await screen.findByText("README.md")).toBeInTheDocument();
  });

  it("toggles the popover off when the same badge is clicked again", async () => {
    seedWorkspaceWithDirs(true);
    renderDock();
    const alpha = await screen.findByRole("tab", { name: "alpha" });
    fireEvent.click(alpha);
    expect(await screen.findByTestId("dock-browse-popover")).toBeInTheDocument();
    fireEvent.click(alpha);
    await waitFor(() => expect(screen.queryByTestId("dock-browse-popover")).toBeNull());
  });

  it("skips the popover for a stale directory and keeps the toast hint", async () => {
    seedWorkspaceWithDirs(true);
    const beta = tdState.workspaces[0]?.directories?.find((d) => d.id === "dir-2");
    if (beta) beta.available = false;
    renderWithProviders(
      <>
        <RightPanelDock workspaceId="ws-a" />
        <Toaster />
      </>,
    );
    fireEvent.click(await screen.findByRole("tab", { name: "beta" }));
    await waitFor(() => expect(screen.getByRole("status")).toBeInTheDocument());
    expect(screen.queryByTestId("dock-browse-popover")).toBeNull();
  });

  it("removes a directory from its context menu", async () => {
    seedWorkspaceWithDirs(true);
    renderDock();
    fireEvent.contextMenu(await screen.findByRole("tab", { name: "beta" }));
    fireEvent.click(screen.getByRole("button", { name: "从工作区移除" }));
    const dirs = tdState.workspaces[0]?.directories ?? [];
    expect(dirs.some((d) => d.id === "dir-2")).toBe(false);
  });

  it("routes active-directory removal through the dirty-files confirm dialog", async () => {
    seedWorkspaceWithDirs(true);
    useUiStore.setState({
      activeWorkspaceId: "ws-a",
      editorByWorkspace: {
        "ws-a": {
          openFiles: ["README.md"], activeFile: "README.md", selectedPaths: [], lastSelectedPath: null,
          clipboardPaths: [], clipboardMode: null, dirtyPaths: { "README.md": true }, crossRefs: {},
        },
      },
    });
    renderDock();
    fireEvent.contextMenu(await screen.findByRole("tab", { name: "alpha" }));
    fireEvent.click(screen.getByRole("button", { name: "从工作区移除" }));
    // The confirm dialog is up and the directory still exists.
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(tdState.workspaces[0]?.directories?.some((d) => d.id === "dir-1")).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "移除目录" }));
    const dirs = tdState.workspaces[0]?.directories ?? [];
    expect(dirs.some((d) => d.id === "dir-1")).toBe(false);
  });
});

describe("RightPanelDock — add-directory popover", () => {
  it("lists unmounted directories plus the browse entry", async () => {
    seedWorkspaceWithDirs(true);
    renderDock();
    await screen.findByRole("tab", { name: "alpha" });
    fireEvent.click(screen.getByRole("button", { name: "添加目录" }));
    expect(screen.getByTestId("dock-add-dir-popover")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "gamma" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "浏览新目录…" })).toBeInTheDocument();
  });

  it("mounts an unmounted directory from the popover", async () => {
    seedWorkspaceWithDirs(true);
    renderDock();
    await screen.findByRole("tab", { name: "alpha" });
    fireEvent.click(screen.getByRole("button", { name: "添加目录" }));
    fireEvent.click(screen.getByRole("button", { name: "gamma" }));
    const gamma = tdState.workspaces[0]?.directories?.find((d) => d.id === "dir-3");
    expect(gamma?.mounted).toBe(true);
  });
});
