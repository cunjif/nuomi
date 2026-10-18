/**
 * OverviewGrid tests: derived-label card rendering (spec 5.5.1-2a — never
 * the placeholder uuid root path), hint copy, and directory accessibility
 * markers.
 */
import { screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { tdState } from "../../lib/ipc/test-double";
import type { WorkspaceEntryDto } from "../../lib/ipc/bindings.gen";
import { useUiStore } from "../../lib/store/uiStore";
import { OverviewGrid } from "./OverviewGrid";

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

beforeEach(() => {
  tdState.workspaces.length = 0;
  useUiStore.setState({ activeWorkspaceId: null });
});

describe("OverviewGrid — derived labels", () => {
  it("renders the open workspace card with label and full label", async () => {
    useUiStore.setState({ activeWorkspaceId: "ws-1" });
    tdState.workspaces.push(
      makeWs({ id: "ws-1", label: "alpha", labelFull: "alpha-beta-gamma", isActive: true }),
    );
    renderWithProviders(<OverviewGrid />);

    expect(await screen.findByText("alpha")).toBeTruthy();
    expect(screen.getByText("alpha-beta-gamma")).toBeTruthy();
    expect(
      screen.getByText("概览模式 · 仅用于监控与跳转，点击卡片进入单个工作区以启动 Agent"),
    ).toBeTruthy();
    expect(screen.getByText("✓ 可访问")).toBeTruthy();
  });

  it("falls back to the id and root path when no label exists", async () => {
    useUiStore.setState({ activeWorkspaceId: "ws-1" });
    tdState.workspaces.push(makeWs({ id: "ws-1", rootPath: "C:\\proj\\x" }));
    renderWithProviders(<OverviewGrid />);

    expect(await screen.findByText("ws-1")).toBeTruthy();
    expect(screen.getByText("C:\\proj\\x")).toBeTruthy();
  });

  it("shows the missing-directory marker when the directory is gone", async () => {
    useUiStore.setState({ activeWorkspaceId: "ws-1" });
    tdState.workspaces.push(
      makeWs({ id: "ws-1", label: "alpha", directoryPresent: false }),
    );
    renderWithProviders(<OverviewGrid />);

    expect(await screen.findByText("✗ 目录缺失")).toBeTruthy();
  });
});

describe("OverviewGrid — gating", () => {
  it("renders nothing without an active workspace", () => {
    const { container } = renderWithProviders(<OverviewGrid />);
    expect(container.firstChild).toBeNull();
  });
});
