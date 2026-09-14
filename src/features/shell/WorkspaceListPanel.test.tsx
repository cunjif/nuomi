/**
 * WorkspaceListPanel tests: list rendering, add/remove/switch flows, color
 * dots, empty state and directory-missing indicator (spec 5.1–5.3).
 */
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { tdState } from "../../lib/ipc/test-double";
import type { WorkspaceEntryDto } from "../../lib/ipc/bindings.gen";
import { WorkspaceListPanel } from "./WorkspaceListPanel";

const openDialog = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: (...a: unknown[]) => openDialog(...a) }));

function makeWs(overrides: Partial<WorkspaceEntryDto> & { id: string }): WorkspaceEntryDto {
  return {
    rootPath: `C:\\projects\\${overrides.id}`,
    colorTag: "paper-yellow",
    createdAt: 1,
    isActive: false,
    directoryPresent: true,
    ...overrides,
  };
}

beforeEach(() => {
  tdState.workspaces.length = 0;
  openDialog.mockReset();
});

describe("WorkspaceListPanel — list rendering", () => {
  it("shows empty state when no workspaces registered", async () => {
    renderWithProviders(<WorkspaceListPanel />);
    expect(await screen.findByText("尚未纳管任何工作区，请选择目录新增")).toBeTruthy();
  });

  it("renders all registered workspaces", async () => {
    tdState.workspaces.push(
      makeWs({ id: "ws-1", rootPath: "C:\\projects\\alpha", isActive: true }),
      makeWs({ id: "ws-2", rootPath: "C:\\projects\\beta", isActive: false }),
    );
    renderWithProviders(<WorkspaceListPanel />);
    expect(await screen.findByText("C:\\projects\\alpha")).toBeTruthy();
    expect(screen.getByText("C:\\projects\\beta")).toBeTruthy();
  });

  it("shows active indicator on the active workspace", async () => {
    tdState.workspaces.push(
      makeWs({ id: "ws-1", rootPath: "C:\\alpha", isActive: true }),
      makeWs({ id: "ws-2", rootPath: "C:\\beta", isActive: false }),
    );
    renderWithProviders(<WorkspaceListPanel />);
    await screen.findByText("C:\\alpha");
    const activeRow = screen.getByText("C:\\alpha").closest("li");
    expect(activeRow?.textContent).toContain("●");
    const inactiveRow = screen.getByText("C:\\beta").closest("li");
    expect(inactiveRow?.textContent).not.toContain("●");
  });

  it("shows directory-missing warning when directoryPresent is false", async () => {
    tdState.workspaces.push(
      makeWs({ id: "ws-1", rootPath: "C:\\ghost", isActive: true, directoryPresent: false }),
    );
    renderWithProviders(<WorkspaceListPanel />);
    await screen.findByText("C:\\ghost");
    const row = screen.getByText("C:\\ghost").closest("li");
    expect(row?.textContent).toContain("⚠");
  });
});

describe("WorkspaceListPanel — add workspace flow", () => {
  it("calls addWorkspace after folder picker resolves", async () => {
    openDialog.mockResolvedValue("C:\\projects\\new-ws");
    renderWithProviders(<WorkspaceListPanel />);
    fireEvent.click(screen.getByText("新增工作区"));
    await waitFor(() => {
      expect(tdState.workspaces.some((w) => w.rootPath === "C:\\projects\\new-ws")).toBe(true);
    });
  });

  it("does nothing when folder picker is cancelled", async () => {
    openDialog.mockResolvedValue(null);
    renderWithProviders(<WorkspaceListPanel />);
    fireEvent.click(screen.getByText("新增工作区"));
    await waitFor(() => {
      expect(openDialog).toHaveBeenCalledTimes(1);
    });
    expect(tdState.workspaces.length).toBe(0);
  });
});

describe("WorkspaceListPanel — remove workspace flow", () => {
  it("shows confirmation bar then removes on confirm", async () => {
    tdState.workspaces.push(
      makeWs({ id: "ws-1", rootPath: "C:\\removable", isActive: true }),
    );
    renderWithProviders(<WorkspaceListPanel />);
    await screen.findByText("C:\\removable");

    fireEvent.click(screen.getByLabelText("移除"));
    expect(screen.getByText("确认移除此工作区？关联会话将保留为孤儿会话。")).toBeTruthy();

    fireEvent.click(screen.getByText("移除", { selector: "button" }));
    await waitFor(() => {
      expect(tdState.workspaces.length).toBe(0);
    });
  });

  it("cancel button hides the confirmation bar", async () => {
    tdState.workspaces.push(
      makeWs({ id: "ws-1", rootPath: "C:\\keep", isActive: true }),
    );
    renderWithProviders(<WorkspaceListPanel />);
    await screen.findByText("C:\\keep");

    fireEvent.click(screen.getByLabelText("移除"));
    fireEvent.click(screen.getByText("取消"));
    expect(screen.queryByText("确认移除此工作区？关联会话将保留为孤儿会话。")).toBeNull();
    expect(tdState.workspaces.length).toBe(1);
  });
});

describe("WorkspaceListPanel — switch workspace flow", () => {
  it("clicking a row activates that workspace", async () => {
    tdState.workspaces.push(
      makeWs({ id: "ws-1", rootPath: "C:\\alpha", isActive: true }),
      makeWs({ id: "ws-2", rootPath: "C:\\beta", isActive: false }),
    );
    renderWithProviders(<WorkspaceListPanel />);
    await screen.findByText("C:\\beta");

    fireEvent.click(screen.getByText("C:\\beta"));
    await waitFor(() => {
      expect(tdState.workspaces.find((w) => w.id === "ws-2")?.isActive).toBe(true);
      expect(tdState.workspaces.find((w) => w.id === "ws-1")?.isActive).toBe(false);
    });
  });

  it("clicking the active workspace row does not trigger activation", async () => {
    tdState.workspaces.push(
      makeWs({ id: "ws-1", rootPath: "C:\\active", isActive: true }),
    );
    renderWithProviders(<WorkspaceListPanel />);
    await screen.findByText("C:\\active");
    fireEvent.click(screen.getByText("C:\\active"));
    // Still the only workspace, still active — no error, no state change.
    expect(tdState.workspaces.find((w) => w.id === "ws-1")?.isActive).toBe(true);
  });
});
