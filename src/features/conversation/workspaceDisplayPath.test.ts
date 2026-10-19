import { describe, expect, it } from "vitest";
import type { WorkspaceEntryDto } from "../../lib/ipc/bindings.gen";
import { workspaceDisplayPath } from "./workspaceDisplayPath";

function makeWs(
  overrides: Partial<WorkspaceEntryDto> = {},
): WorkspaceEntryDto {
  return {
    id: "ws-1",
    rootPath: "ws-1",
    colorTag: "ink-blue",
    createdAt: 1,
    isActive: false,
    directoryPresent: true,
    ...overrides,
  };
}

function dir(absPath: string, available: boolean): NonNullable<WorkspaceEntryDto["directories"]> {
  return [
    {
      id: `d-${absPath}`,
      absPath,
      displayName: absPath.split(/[\\/]/).pop() ?? absPath,
      mounted: true,
      addedAt: 1,
      available,
      isActive: false,
    },
  ];
}

describe("workspaceDisplayPath (ADR-0021 兼容读取)", () => {
  it("单可用目录的工作区显示该目录绝对路径", () => {
    const ws = makeWs({
      rootPath: "uuid-placeholder",
      label: "proj",
      labelFull: "proj",
      directories: dir("C:\\code\\proj", true),
    });
    expect(workspaceDisplayPath(ws)).toBe("C:\\code\\proj");
  });

  it("多可用目录的工作区显示 labelFull", () => {
    const ws = makeWs({
      label: "a-b",
      labelFull: "a-b",
      directories: [
        ...dir("C:\\code\\a", true),
        ...dir("D:\\docs\\b", true),
      ],
    });
    expect(workspaceDisplayPath(ws)).toBe("a-b");
  });

  it("目录全失效的工作区回退 labelFull 而非 uuid 占位", () => {
    const ws = makeWs({
      rootPath: "0197aaaa-bbbb-7ccc-dddd-eeeeffff0000",
      label: "stale",
      labelFull: "stale",
      directories: dir("C:\\gone", false),
    });
    expect(workspaceDisplayPath(ws)).toBe("stale");
  });

  it("无目录行的 v1 工作区回退 rootPath 真路径", () => {
    const ws = makeWs({ rootPath: "C:\\legacy", directories: null });
    expect(workspaceDisplayPath(ws)).toBe("C:\\legacy");
  });
});
