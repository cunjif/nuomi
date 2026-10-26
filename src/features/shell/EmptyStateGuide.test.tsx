/**
 * EmptyStateGuide tests: empty-state copy, the recent-workspace list with
 * derived labels (spec 5.5.1-2a), anomaly handling (spec 5.5.3-2c) and the
 * switch action.
 */
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { tdState } from "../../lib/ipc/test-double";
import type { RecentWorkspaceDto, WorkspaceEntryDto } from "../../lib/ipc/bindings.gen";
import { EmptyStateGuide } from "./EmptyStateGuide";

const LAST_USED_AT = 1_700_000_000_000;

function makeWs(overrides: Partial<WorkspaceEntryDto> & { id: string }): WorkspaceEntryDto {
  return {
    rootPath: `C:\\projects\\${overrides.id}`,
    colorTag: "ink-blue",
    createdAt: 1,
    isActive: false,
    directoryPresent: true,
    ...overrides,
  };
}

function makeRecent(
  overrides: Partial<RecentWorkspaceDto> & { workspaceId: string },
): RecentWorkspaceDto {
  return {
    lastUsedAt: LAST_USED_AT,
    isPinned: false,
    label: null,
    labelFull: null,
    anomaly: false,
    ...overrides,
  };
}

beforeEach(() => {
  tdState.workspaces.length = 0;
  tdState.recentWorkspaces = null;
});

describe("EmptyStateGuide — empty state", () => {
  it("shows guidance and no recent list when nothing was used", () => {
    renderWithProviders(<EmptyStateGuide />);
    expect(screen.getByText("没有已开启的工作区")).toBeTruthy();
    expect(screen.getByText("打开一个目录开始工作，或从最近使用列表中恢复")).toBeTruthy();
    expect(screen.getByText("新增工作区")).toBeTruthy();
    expect(screen.queryByText("最近使用")).toBeNull();
  });
});

describe("EmptyStateGuide — recent list", () => {
  it("renders entries with derived labels, pin stars, dates and tooltips", async () => {
    tdState.recentWorkspaces = [
      makeRecent({
        workspaceId: "ws-1",
        label: "alpha",
        labelFull: "alpha-beta",
        isPinned: true,
      }),
    ];
    renderWithProviders(<EmptyStateGuide />);

    expect(await screen.findByText("最近使用")).toBeTruthy();
    expect(screen.getByText("alpha")).toBeTruthy();
    expect(screen.getByText("★")).toBeTruthy();
    expect(screen.getByText(new Date(LAST_USED_AT).toLocaleDateString())).toBeTruthy();
    expect(screen.getByTitle("alpha-beta")).toBeTruthy();
  });

  it("falls back to the workspace id when no label was derived", async () => {
    tdState.recentWorkspaces = [makeRecent({ workspaceId: "ws-raw" })];
    renderWithProviders(<EmptyStateGuide />);
    expect(await screen.findByText("ws-raw")).toBeTruthy();
  });

  it("switches to the workspace on click", async () => {
    tdState.workspaces.push(makeWs({ id: "ws-1" }));
    tdState.recentWorkspaces = [makeRecent({ workspaceId: "ws-1", label: "alpha" })];
    renderWithProviders(<EmptyStateGuide />);

    fireEvent.click(await screen.findByText("alpha"));
    await waitFor(() => {
      expect(tdState.workspaces.find((w) => w.id === "ws-1")?.isFocused).toBe(true);
    });
  });
});

describe("EmptyStateGuide — anomaly entries", () => {
  it("marks anomalous entries disabled with a warning icon and tooltip", async () => {
    tdState.recentWorkspaces = [
      makeRecent({ workspaceId: "ws-bad", label: "bad", anomaly: true }),
    ];
    renderWithProviders(<EmptyStateGuide />);

    const label = await screen.findByText("bad");
    const row = label.closest("button");
    expect(row?.getAttribute("aria-disabled")).toBe("true");
    expect(screen.getByText("⚠")).toBeTruthy();
    expect(screen.getByTitle("工作区迁移异常，需人工处理")).toBeTruthy();
  });

  it("does not switch when an anomalous entry is clicked", async () => {
    tdState.workspaces.push(makeWs({ id: "ws-bad" }));
    tdState.recentWorkspaces = [
      makeRecent({ workspaceId: "ws-bad", label: "bad", anomaly: true }),
    ];
    renderWithProviders(<EmptyStateGuide />);

    fireEvent.click(await screen.findByText("bad"));
    expect(tdState.workspaces.find((w) => w.id === "ws-bad")?.isFocused).toBeFalsy();
  });
});
