import { act, fireEvent, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { useUiStore } from "../../lib/store/uiStore";
import { useToastStore } from "../../lib/store/toastStore";
import { AreaNav } from "./AreaNav";

// AreaNav imports the window API at module load; jsdom has no Tauri runtime
// (WindowControls stays unmounted) but the import must resolve.
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    listen: () => Promise.resolve(() => {}),
    onResized: () => Promise.resolve(() => {}),
    onMoved: () => Promise.resolve(() => {}),
  }),
}));

afterEach(() => {
  act(() => {
    useUiStore.setState({
      activeWorkspaceId: null,
      leftPanel: { width: 224, collapsed: false, hidden: false },
      rightPanel: { width: 224, collapsed: false, hidden: false },
    });
    useToastStore.setState({ toasts: [] });
  });
});

describe("AreaNav — side panel toggles", () => {
  it("cycles the left panel between expanded and the icon dock", () => {
    renderWithProviders(<AreaNav />);
    const toggle = screen.getByTestId("left-panel-toggle");
    expect(toggle).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(toggle);
    expect(useUiStore.getState().leftPanel.collapsed).toBe(true);
    expect(useUiStore.getState().leftPanel.hidden).toBe(false);
    fireEvent.click(toggle);
    expect(useUiStore.getState().leftPanel.collapsed).toBe(false);
    expect(useUiStore.getState().leftPanel.hidden).toBe(false);
  });

  it("toggles the right panel independently", () => {
    useUiStore.setState({ activeWorkspaceId: "ws-a" });
    renderWithProviders(<AreaNav />);
    fireEvent.click(screen.getByTestId("right-panel-toggle"));
    expect(useUiStore.getState().rightPanel.collapsed).toBe(true);
    expect(useUiStore.getState().leftPanel.collapsed).toBe(false);
    fireEvent.click(screen.getByTestId("right-panel-toggle"));
    expect(useUiStore.getState().rightPanel.collapsed).toBe(false);
  });

  it("warns instead of toggling when no workspace is open", () => {
    renderWithProviders(<AreaNav />);
    fireEvent.click(screen.getByTestId("right-panel-toggle"));
    const toasts = useToastStore.getState().toasts;
    expect(toasts).toHaveLength(1);
    expect(toasts[0]).toMatchObject({ kind: "warn" });
    expect(useUiStore.getState().rightPanel.collapsed).toBe(false);
    expect(useUiStore.getState().rightPanel.hidden).toBe(false);
  });

  it("still toggles the left panel without an open workspace", () => {
    renderWithProviders(<AreaNav />);
    fireEvent.click(screen.getByTestId("left-panel-toggle"));
    expect(useUiStore.getState().leftPanel.collapsed).toBe(true);
    expect(useToastStore.getState().toasts).toHaveLength(0);
  });

  it("restores a fully hidden panel directly as the dock", () => {
    useUiStore.setState({ leftPanel: { width: 224, collapsed: false, hidden: true } });
    renderWithProviders(<AreaNav />);
    const toggle = screen.getByTestId("left-panel-toggle");
    expect(toggle).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(toggle);
    expect(useUiStore.getState().leftPanel.hidden).toBe(false);
    expect(useUiStore.getState().leftPanel.collapsed).toBe(true);
  });
});
