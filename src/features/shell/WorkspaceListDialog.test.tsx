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
      makeWs({ id: "ws-1", rootPath: "C:\\alpha", label: "alpha", isFocused: true }),
      makeWs({ id: "ws-2", rootPath: "C:\\beta", label: "beta", isFocused: false }),
    );
    renderWithProviders(<WorkspaceListDialog open={true} onClose={onClose} />);
    await screen.findByText("beta");

    fireEvent.click(screen.getByText("beta"));
    await waitFor(() => {
      expect(tdState.workspaces.find((w) => w.id === "ws-2")?.isFocused).toBe(true);
    });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("clicking the active row does not trigger activation or close", async () => {
    const onClose = vi.fn();
    tdState.workspaces.push(
      makeWs({ id: "ws-1", rootPath: "C:\\alpha", label: "alpha", isFocused: true }),
    );
    renderWithProviders(<WorkspaceListDialog open={true} onClose={onClose} />);
    await screen.findByText("alpha");

    fireEvent.click(screen.getByText("alpha"));
    expect(onClose).not.toHaveBeenCalled();
  });
});

describe("WorkspaceListDialog — row identity (spec 5.5.1-2a)", () => {
  it("shows labels with full-label tooltips and last-used dates", async () => {
    const lastUsedAt = 1_700_000_000_000;
    tdState.workspaces.push(
      makeWs({ id: "ws-1", label: "alpha", labelFull: "alpha-beta", isFocused: true }),
    );
    tdState.recentWorkspaces = [
      {
        workspaceId: "ws-1",
        lastUsedAt,
        isPinned: false,
        label: "alpha",
        labelFull: "alpha-beta",
        anomaly: false,
      },
    ];
    renderWithProviders(<WorkspaceListDialog open={true} onClose={() => {}} />);

    expect(await screen.findByText("alpha")).toBeTruthy();
    expect(screen.getByTitle("alpha-beta")).toBeTruthy();
    expect(screen.getByText(new Date(lastUsedAt).toLocaleDateString())).toBeTruthy();
  });

  it("falls back to the workspace id when no label was derived", async () => {
    tdState.workspaces.push(makeWs({ id: "ws-raw", isFocused: true }));
    renderWithProviders(<WorkspaceListDialog open={true} onClose={() => {}} />);
    expect(await screen.findByText("ws-raw")).toBeTruthy();
  });

  it("shows a pin star for pinned workspaces", async () => {
    tdState.workspaces.push(
      makeWs({ id: "ws-1", label: "alpha", isFocused: true, isPinned: true }),
    );
    renderWithProviders(<WorkspaceListDialog open={true} onClose={() => {}} />);
    expect(await screen.findByText("★")).toBeTruthy();
  });

  it("marks anomalous workspaces disabled and refuses activation", async () => {
    tdState.workspaces.push(
      makeWs({ id: "ws-1", label: "alpha", isFocused: true }),
      makeWs({ id: "ws-bad", label: "bad", anomaly: true }),
    );
    renderWithProviders(<WorkspaceListDialog open={true} onClose={() => {}} />);

    const row = (await screen.findByText("bad")).closest("li");
    expect(row?.getAttribute("aria-disabled")).toBe("true");
    expect(screen.getByText("⚠")).toBeTruthy();
    expect(screen.getAllByTitle("工作区迁移异常，需人工处理").length).toBeGreaterThan(0);

    fireEvent.click(screen.getByText("bad"));
    expect(tdState.workspaces.find((w) => w.id === "ws-bad")?.isFocused).toBeFalsy();
    expect(tdState.workspaces.find((w) => w.id === "ws-1")?.isFocused).toBe(true);
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
      makeWs({ id: "ws-1", rootPath: "C:\\removable", label: "removable", isFocused: true }),
    );
    renderWithProviders(<WorkspaceListDialog open={true} onClose={() => {}} />);
    await screen.findByText("removable");

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
