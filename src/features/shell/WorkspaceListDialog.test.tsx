/**
 * WorkspaceListDialog tests: open/close lifecycle, list switching, add-activates
 * and orphan reclaim flow (spec 5.2.1 规则2 + 规则3).
 */
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { tdState } from "../../lib/ipc/test-double";
import type { WorkspaceEntryDto } from "../../lib/ipc/bindings.gen";
import { WorkspaceListDialog } from "./WorkspaceListDialog";

const openDialog = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: (...a: unknown[]) => openDialog(...a) }));

// uiStore.openWorkspace calls tauri invoke("open_workspace") directly (not
// ipc.openWorkspace), so mock the core invoke to route open_workspace into
// the test-double which updates isFocused on tdState.workspaces.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (cmd: string, args?: Record<string, unknown>) => {
    if (cmd === "open_workspace") {
      const id = args?.id as string;
      for (const w of tdState.workspaces ?? []) w.isFocused = w.id === id;
      return { workspaceId: id };
    }
    return undefined;
  },
}));

function makeWs(overrides: Partial<WorkspaceEntryDto> & { id: string }): WorkspaceEntryDto {
  return {
    rootPath: `C:\\projects\\${overrides.id}`,
    colorTag: "ink-blue",
    createdAt: 1,
    isActive: false,
    isFocused: false,
    directoryPresent: true,
    ...overrides,
  };
}

beforeEach(() => {
  tdState.workspaces.length = 0;
  openDialog.mockReset();
});

describe("WorkspaceListDialog — open/close lifecycle", () => {
  it("renders nothing when open=false", () => {
    renderWithProviders(<WorkspaceListDialog open={false} onClose={() => {}} />);
    expect(screen.queryByTestId("workspace-list-dialog")).toBeNull();
  });

  it("renders the dialog when open=true", async () => {
    renderWithProviders(<WorkspaceListDialog open={true} onClose={() => {}} />);
    expect(screen.getByTestId("workspace-list-dialog")).toBeTruthy();
    expect(screen.getByText("工作区列表")).toBeTruthy();
  });

  it("calls onClose when cancel button clicked", () => {
    const onClose = vi.fn();
    renderWithProviders(<WorkspaceListDialog open={true} onClose={onClose} />);
    fireEvent.click(screen.getByText("取消"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("calls onClose when backdrop clicked", () => {
    const onClose = vi.fn();
    renderWithProviders(<WorkspaceListDialog open={true} onClose={onClose} />);
    const backdrop = screen.getByTestId("workspace-list-dialog").parentElement?.querySelector("[aria-hidden='true']");
    expect(backdrop).toBeTruthy();
    fireEvent.mouseDown(backdrop!);
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

describe("WorkspaceListDialog — list switching", () => {
  it("clicking a non-active row activates it and closes the dialog", async () => {
    const onClose = vi.fn();
    tdState.workspaces.push(
      makeWs({ id: "ws-1", rootPath: "C:\\alpha", isFocused: true }),
      makeWs({ id: "ws-2", rootPath: "C:\\beta", isFocused: false }),
    );
    renderWithProviders(<WorkspaceListDialog open={true} onClose={onClose} />);
    await screen.findByText("C:\\beta");

    fireEvent.click(screen.getByText("C:\\beta"));
    await waitFor(() => {
      expect(tdState.workspaces.find((w) => w.id === "ws-2")?.isFocused).toBe(true);
    });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("clicking the active row does not trigger activation or close", async () => {
    const onClose = vi.fn();
    tdState.workspaces.push(
      makeWs({ id: "ws-1", rootPath: "C:\\alpha", isFocused: true }),
    );
    renderWithProviders(<WorkspaceListDialog open={true} onClose={onClose} />);
    await screen.findByText("C:\\alpha");

    fireEvent.click(screen.getByText("C:\\alpha"));
    expect(onClose).not.toHaveBeenCalled();
  });
});

describe("WorkspaceListDialog — add workspace", () => {
  it("add button calls folder picker then addWorkspace", async () => {
    openDialog.mockResolvedValue("C:\\new-project");
    renderWithProviders(<WorkspaceListDialog open={true} onClose={() => {}} />);
    fireEvent.click(screen.getByText("新增工作区"));
    await waitFor(() => {
      expect(tdState.workspaces.some((w) => w.rootPath === "C:\\new-project")).toBe(true);
    });
  });
});

describe("WorkspaceListDialog — remove workspace", () => {
  it("remove button shows confirmation then removes on confirm", async () => {
    tdState.workspaces.push(
      makeWs({ id: "ws-1", rootPath: "C:\\removable", isFocused: true }),
    );
    renderWithProviders(<WorkspaceListDialog open={true} onClose={() => {}} />);
    await screen.findByText("C:\\removable");

    fireEvent.click(screen.getByLabelText("移除"));
    expect(screen.getByText("确认移除此工作区？关联会话将保留为孤儿会话。")).toBeTruthy();

    fireEvent.click(screen.getByText("移除", { selector: "button" }));
    await waitFor(() => {
      expect(tdState.workspaces.length).toBe(0);
    });
  });
});

describe("WorkspaceListDialog — empty state", () => {
  it("shows empty guidance when no workspaces", async () => {
    renderWithProviders(<WorkspaceListDialog open={true} onClose={() => {}} />);
    expect(await screen.findByText("尚未纳管任何工作区，请选择目录新增")).toBeTruthy();
  });
});
