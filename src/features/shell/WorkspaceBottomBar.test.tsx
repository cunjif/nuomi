/**
 * WorkspaceBottomBar (v2, ADR-0021) tests: mounted-directory chips, focus
 * switching, stale-directory toast (spec 5.5.3-1c), right-click remove with
 * the dirty guard, and the "+" popover (mount unmounted / browse new).
 */
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { tdState } from "../../lib/ipc/test-double-state";
import type { WorkDirectoryDto, WorkspaceEntryDto } from "../../lib/ipc/bindings.gen";
import { Toaster } from "../../components/ui/Toaster";
import { useUiStore } from "../../lib/store/uiStore";
import { WorkspaceBottomBar } from "./WorkspaceBottomBar";

const folderPick = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: (...a: unknown[]) => folderPick(...a) }));

function makeDir(
  overrides: Partial<WorkDirectoryDto> & { id: string; displayName: string },
): WorkDirectoryDto {
  return {
    absPath: `C:\\dirs\\${overrides.displayName}`,
    mounted: true,
    addedAt: 1,
    available: true,
    isActive: false,
    ...overrides,
  };
}

function seedOpenWs(dirs: WorkDirectoryDto[]): WorkspaceEntryDto {
  const label = dirs.map((d) => d.displayName).join("-");
  const ws: WorkspaceEntryDto = {
    id: "ws-1",
    rootPath: "C:\\legacy-root",
    colorTag: "ink-blue",
    createdAt: 1,
    isActive: true,
    isFocused: true,
    isOpen: true,
    directoryPresent: dirs.some((d) => d.available),
    label,
    labelFull: label,
    directories: dirs,
  };
  tdState.workspaces.push(ws);
  return ws;
}

function renderBar(workspaceId = "ws-1"): ReturnType<typeof renderWithProviders> {
  return renderWithProviders(
    <>
      <WorkspaceBottomBar workspaceId={workspaceId} />
      <Toaster />
    </>,
  );
}

beforeEach(() => {
  folderPick.mockReset();
  useUiStore.setState({ activeWorkspaceId: "ws-1", editorByWorkspace: {} });
});

describe("WorkspaceBottomBar — directory chips", () => {
  it("renders only mounted directories; the active one is selected", async () => {
    seedOpenWs([
      makeDir({ id: "dir-alpha", displayName: "alpha" }),
      makeDir({ id: "dir-beta", displayName: "beta" }),
      makeDir({ id: "dir-gamma", displayName: "gamma", mounted: false }),
    ]);
    renderBar();

    const alpha = await screen.findByRole("tab", { name: "alpha" });
    expect(alpha).toHaveAttribute("aria-selected", "true");
    expect(alpha).toHaveAttribute("title", "C:\\dirs\\alpha");
    expect(screen.getByRole("tab", { name: "beta" })).toHaveAttribute("aria-selected", "false");
    expect(screen.queryByRole("tab", { name: "gamma" })).toBeNull();
  });

  it("renders nothing when the open workspace is a different one", () => {
    seedOpenWs([makeDir({ id: "dir-alpha", displayName: "alpha" })]);
    renderBar("ws-other");
    expect(screen.queryByRole("tablist")).toBeNull();
    expect(screen.queryByRole("button", { name: "添加目录" })).toBeNull();
  });
});

describe("WorkspaceBottomBar — focus switching", () => {
  it("clicking an available chip focuses it", async () => {
    seedOpenWs([
      makeDir({ id: "dir-alpha", displayName: "alpha" }),
      makeDir({ id: "dir-beta", displayName: "beta" }),
    ]);
    renderBar();

    fireEvent.click(await screen.findByRole("tab", { name: "beta" }));
    await waitFor(() => {
      expect(screen.getByRole("tab", { name: "beta" })).toHaveAttribute("aria-selected", "true");
      expect(screen.getByRole("tab", { name: "alpha" })).toHaveAttribute("aria-selected", "false");
    });
    expect(tdState.activeDirectoryId).toBe("dir-beta");
  });

  it("clicking a stale chip toasts the missing-directory hint instead of focusing", async () => {
    seedOpenWs([
      makeDir({ id: "dir-alpha", displayName: "alpha" }),
      makeDir({ id: "dir-beta", displayName: "beta", available: false }),
    ]);
    renderBar();

    fireEvent.click(await screen.findByRole("tab", { name: /beta/ }));
    expect(await screen.findByText("目录不存在，可移除或重新定位")).toBeTruthy();
    expect(tdState.activeDirectoryId).not.toBe("dir-beta");
    expect(screen.getByRole("tab", { name: "alpha" })).toHaveAttribute("aria-selected", "true");
  });
});

describe("WorkspaceBottomBar — right-click menu", () => {
  it("offers focus and remove; remove drops the directory", async () => {
    seedOpenWs([
      makeDir({ id: "dir-alpha", displayName: "alpha" }),
      makeDir({ id: "dir-beta", displayName: "beta" }),
    ]);
    renderBar();

    fireEvent.contextMenu(await screen.findByRole("tab", { name: "beta" }));
    fireEvent.click(await screen.findByText("聚焦此目录"));
    fireEvent.contextMenu(screen.getByRole("tab", { name: "beta" }));
    fireEvent.click(screen.getByText("从工作区移除"));

    await waitFor(() => {
      expect(tdState.workspaces[0]?.directories?.map((d) => d.id)).toEqual(["dir-alpha"]);
    });
    await waitFor(() => expect(screen.queryByRole("tab", { name: "beta" })).toBeNull());
  });

  it("removing the active directory with dirty files asks for confirmation", async () => {
    seedOpenWs([makeDir({ id: "dir-alpha", displayName: "alpha" })]);
    useUiStore.setState({
      editorByWorkspace: {
        "ws-1": {
          openFiles: ["notes.txt"],
          activeFile: "notes.txt",
          selectedPaths: [],
          lastSelectedPath: null,
          clipboardPaths: [],
          clipboardMode: null,
          dirtyPaths: { "notes.txt": true },
          crossRefs: {},
        },
      },
    });
    renderBar();

    fireEvent.contextMenu(await screen.findByRole("tab", { name: "alpha" }));
    fireEvent.click(screen.getByText("从工作区移除"));
    expect(screen.getByText("该目录下有未保存的文件，移除目录将丢弃修改：")).toBeTruthy();
    expect(screen.getByText("notes.txt")).toBeTruthy();
    expect(tdState.workspaces[0]?.directories?.length).toBe(1);

    fireEvent.click(screen.getByRole("button", { name: "移除目录" }));
    await waitFor(() => {
      expect(tdState.workspaces[0]?.directories?.length ?? 0).toBe(0);
    });
    await waitFor(() => expect(useUiStore.getState().activeWorkspaceId).toBeNull());
  });

  it("removing a non-active directory skips the dirty confirmation", async () => {
    seedOpenWs([
      makeDir({ id: "dir-alpha", displayName: "alpha" }),
      makeDir({ id: "dir-beta", displayName: "beta" }),
    ]);
    useUiStore.setState({
      editorByWorkspace: {
        "ws-1": {
          openFiles: ["notes.txt"],
          activeFile: "notes.txt",
          selectedPaths: [],
          lastSelectedPath: null,
          clipboardPaths: [],
          clipboardMode: null,
          dirtyPaths: { "notes.txt": true },
          crossRefs: {},
        },
      },
    });
    renderBar();

    fireEvent.contextMenu(await screen.findByRole("tab", { name: "beta" }));
    fireEvent.click(screen.getByText("从工作区移除"));

    expect(screen.queryByText("该目录下有未保存的文件，移除目录将丢弃修改：")).toBeNull();
    await waitFor(() => {
      expect(tdState.workspaces[0]?.directories?.map((d) => d.id)).toEqual(["dir-alpha"]);
    });
  });
});

describe("WorkspaceBottomBar — add-directory popover", () => {
  it("lists unmounted directories, disables stale ones, mounts on click", async () => {
    seedOpenWs([
      makeDir({ id: "dir-alpha", displayName: "alpha" }),
      makeDir({ id: "dir-beta", displayName: "beta", mounted: false }),
      makeDir({ id: "dir-gamma", displayName: "gamma", mounted: false, available: false }),
    ]);
    renderBar();

    fireEvent.click(await screen.findByRole("button", { name: "添加目录" }));
    const betaBtn = screen.getByRole("button", { name: /beta/ });
    const gammaBtn = screen.getByRole("button", { name: /gamma/ });
    expect((gammaBtn as HTMLButtonElement).disabled).toBe(true);

    fireEvent.click(betaBtn);
    await waitFor(() => {
      expect(screen.getByRole("tab", { name: "beta" })).toHaveAttribute("aria-selected", "true");
    });
    expect(
      tdState.workspaces[0]?.directories?.find((d) => d.id === "dir-beta")?.mounted,
    ).toBe(true);
  });

  it("browsing joins the picked directory and focuses it", async () => {
    folderPick.mockResolvedValue("C:\\new-project");
    seedOpenWs([makeDir({ id: "dir-alpha", displayName: "alpha" })]);
    renderBar();

    fireEvent.click(await screen.findByRole("button", { name: "添加目录" }));
    fireEvent.click(await screen.findByText("浏览新目录…"));

    await waitFor(() => {
      expect(
        tdState.workspaces[0]?.directories?.some((d) => d.displayName === "new-project"),
      ).toBe(true);
    });
    expect(folderPick).toHaveBeenCalledWith({ directory: true, multiple: false });
    await waitFor(() => {
      expect(screen.getByRole("tab", { name: "new-project" })).toHaveAttribute(
        "aria-selected",
        "true",
      );
    });
  });
});
