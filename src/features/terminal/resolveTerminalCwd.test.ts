import { describe, expect, it } from "vitest";
import "../../test/helpers";
import { tdState } from "../../lib/ipc/test-double";
import { resolveTerminalCwd } from "./resolveTerminalCwd";

function seedOpenWorkspace(): void {
  tdState.workspaces = [
    {
      id: "ws-1",
      rootPath: "C:\\ws-root",
      colorTag: "paper-yellow",
      createdAt: 1,
      isActive: true,
      directoryPresent: true,
      directories: [
        {
          id: "d1",
          absPath: "C:\\ws\\a",
          displayName: "a",
          mounted: true,
          addedAt: 1,
          available: true,
          isActive: false,
        },
        {
          id: "d2",
          absPath: "C:\\ws\\b",
          displayName: "b",
          mounted: true,
          addedAt: 2,
          available: true,
          isActive: false,
        },
        {
          id: "d3",
          absPath: "C:\\ws\\gone",
          displayName: "gone",
          mounted: true,
          addedAt: 3,
          available: false,
          isActive: false,
        },
      ],
    },
  ];
}

describe("resolveTerminalCwd", () => {
  it("uses the workspace's active work directory", async () => {
    seedOpenWorkspace();
    tdState.activeDirectoryId = "d2";

    await expect(resolveTerminalCwd("ws-1")).resolves.toEqual({
      cwd: "C:\\ws\\b",
      source: "workspace",
    });
  });

  it("falls back to the first mounted available directory when none is focused", async () => {
    seedOpenWorkspace();
    tdState.activeDirectoryId = "d3";

    await expect(resolveTerminalCwd("ws-1")).resolves.toEqual({
      cwd: "C:\\ws\\a",
      source: "workspace",
    });
  });

  it("ignores the open workspace when the terminal belongs to another one", async () => {
    seedOpenWorkspace();
    tdState.activeDirectoryId = "d2";

    await expect(resolveTerminalCwd("ws-other")).resolves.toEqual({
      cwd: "C:\\workspace",
      source: "workspace",
    });
  });

  it("falls back to the home dir when no workspace is configured", async () => {
    tdState.workspaces = [];
    tdState.workspaceConfigured = false;

    await expect(resolveTerminalCwd("ws-1")).resolves.toEqual({
      cwd: "/home",
      source: "home",
      fallbackReason: "workspace not available",
    });
  });
});