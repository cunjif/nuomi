/**
 * WorkbenchNav tests: second-level tab switching, default selection and
 * active-state highlighting (spec 5.3 — workbench sub-tab navigation).
 */
import { fireEvent, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { useUiStore } from "../../lib/store/uiStore";
import { WorkbenchNav } from "./WorkbenchNav";

beforeEach(() => {
  useUiStore.setState({ activeArea: "workbench", workbenchSubTab: "workspaceList" });
});

describe("WorkbenchNav — second-level navigation", () => {
  it("renders both sub-tabs with correct labels", () => {
    renderWithProviders(<WorkbenchNav />);
    expect(screen.getByText("工作区列表")).toBeTruthy();
    expect(screen.getByText("文件编辑")).toBeTruthy();
  });

  it("defaults to workspaceList sub-tab", () => {
    renderWithProviders(<WorkbenchNav />);
    const tabs = screen.getAllByRole("tab");
    const wsListTab = tabs.find((t) => t.getAttribute("aria-selected") === "true");
    expect(wsListTab?.textContent).toContain("工作区列表");
  });

  it("clicking editor tab switches workbenchSubTab", () => {
    renderWithProviders(<WorkbenchNav />);
    fireEvent.click(screen.getByText("文件编辑"));
    expect(useUiStore.getState().workbenchSubTab).toBe("editor");
  });

  it("clicking workspaceList tab switches back", () => {
    useUiStore.setState({ workbenchSubTab: "editor" });
    renderWithProviders(<WorkbenchNav />);
    fireEvent.click(screen.getByText("工作区列表"));
    expect(useUiStore.getState().workbenchSubTab).toBe("workspaceList");
  });

  it("highlights the active tab with aria-selected", () => {
    useUiStore.setState({ workbenchSubTab: "editor" });
    renderWithProviders(<WorkbenchNav />);
    const tabs = screen.getAllByRole("tab");
    const editorTab = tabs.find((t) => t.textContent?.includes("文件编辑"));
    const wsListTab = tabs.find((t) => t.textContent?.includes("工作区列表"));
    expect(editorTab?.getAttribute("aria-selected")).toBe("true");
    expect(wsListTab?.getAttribute("aria-selected")).toBe("false");
  });
});
