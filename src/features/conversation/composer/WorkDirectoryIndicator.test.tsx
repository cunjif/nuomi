import { screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { injectIpcCommands } from "../../../lib/ipc/client";
import { testDoubleCommands } from "../../../lib/ipc/test-double";
import { renderWithProviders } from "../../../test/helpers";
import type { IpcError, Result, WorkspaceEntryDto } from "../../../lib/ipc/bindings.gen";
import { WorkDirectoryIndicator } from "./WorkDirectoryIndicator";

const okResult = <T,>(data: T): Result<T, IpcError> => ({ status: "ok", data });

const workspace = (dirs: WorkspaceEntryDto["directories"]): WorkspaceEntryDto => ({
  id: "ws-1",
  rootPath: "/home/u/ws-1",
  colorTag: "blue",
  createdAt: 0,
  isActive: true,
  directoryPresent: true,
  isPinned: true,
  isOpen: true,
  isFocused: true,
  openedAt: 0,
  lastFocusedAt: 0,
  label: "dir-a",
  labelFull: "dir-a",
  directories: dirs,
  anomaly: false,
});

function setup(open: WorkspaceEntryDto | null): void {
  const base = testDoubleCommands();
  injectIpcCommands({
    ...base,
    workspaceGetOpen: vi.fn(async () => okResult(open)),
  });
}

describe("WorkDirectoryIndicator", () => {
  it("renders the bound directory display name with absPath tooltip", async () => {
    setup(
      workspace([
        {
          id: "dir-a",
          absPath: "/home/u/projects/dir-a",
          displayName: "dir-a",
          mounted: true,
          addedAt: 0,
          available: true,
          isActive: true,
        },
      ]),
    );
    renderWithProviders(<WorkDirectoryIndicator workDirectoryId="dir-a" />);

    const label = await screen.findByLabelText("工作目录");
    expect(label.textContent).toContain("dir-a");
    expect(label.getAttribute("title")).toBe("/home/u/projects/dir-a");
  });

  it("renders nothing when workDirectoryId is null", () => {
    setup(workspace([]));
    renderWithProviders(<WorkDirectoryIndicator workDirectoryId={null} />);
    expect(screen.queryByLabelText("工作目录")).toBeNull();
  });

  it("renders nothing when the directory cannot be resolved (cross-workspace)", async () => {
    setup(workspace([]));
    renderWithProviders(<WorkDirectoryIndicator workDirectoryId="dir-elsewhere" />);
    expect(screen.queryByLabelText("工作目录")).toBeNull();
  });
});
