import { fireEvent, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { useUiStore, type PanelLayoutState } from "../../lib/store/uiStore";
import { LeftRail } from "./LeftRail";

function patchLeftPanel(patch: Partial<PanelLayoutState>): void {
  const cur = useUiStore.getState().leftPanel;
  useUiStore.setState({ leftPanel: { ...cur, ...patch } });
}

afterEach(() => {
  useUiStore.setState({
    leftPanel: { width: 224, collapsed: false, hidden: false },
    rightPanel: { width: 224, collapsed: false, hidden: false },
    view: "chat",
  });
});

describe("LeftRail — expanded state", () => {
  it("renders the nav with controlled width and the edge resizer", () => {
    patchLeftPanel({ width: 300 });
    renderWithProviders(<LeftRail />);
    const nav = screen.getByRole("navigation", { name: "糯米 · nuomi" });
    expect(nav).toHaveStyle({ width: "300px" });
    expect(screen.getByTestId("panel-resizer-left")).toBeInTheDocument();
    expect(screen.queryByTestId("left-panel-dock")).toBeNull();
    expect(screen.getByRole("button", { name: "设置" })).toBeInTheDocument();
  });

  it("resizer drag updates the store width", () => {
    renderWithProviders(<LeftRail />);
    fireEvent.mouseDown(screen.getByTestId("panel-resizer-left"), { button: 0, clientX: 0 });
    fireEvent.mouseMove(window, { clientX: 60 });
    expect(useUiStore.getState().leftPanel.width).toBe(284);
    fireEvent.mouseUp(window);
  });

  it("the chevron collapses the panel to the dock", () => {
    renderWithProviders(<LeftRail />);
    fireEvent.click(screen.getByTestId("panel-collapse-left"));
    expect(useUiStore.getState().leftPanel.collapsed).toBe(true);
    expect(screen.getByTestId("left-panel-dock")).toBeInTheDocument();
    expect(screen.queryByTestId("panel-resizer-left")).toBeNull();
  });
});

describe("LeftRail — collapsed dock", () => {
  it("dock icons switch views and mark the active one; expand restores the panel", () => {
    patchLeftPanel({ collapsed: true });
    renderWithProviders(<LeftRail />);
    expect(screen.getByTestId("left-panel-dock")).toBeInTheDocument();
    const board = screen.getByRole("button", { name: "看板" });
    expect(board).not.toHaveAttribute("aria-current");
    fireEvent.click(board);
    expect(useUiStore.getState().view).toBe("board");
    expect(screen.getByRole("button", { name: "看板" })).toHaveAttribute("aria-current", "page");
    fireEvent.click(screen.getByRole("button", { name: "展开面板" }));
    expect(useUiStore.getState().leftPanel.collapsed).toBe(false);
    expect(screen.getByTestId("panel-resizer-left")).toBeInTheDocument();
  });

  it("the hide button fully hides the rail (dock included)", () => {
    patchLeftPanel({ collapsed: true });
    renderWithProviders(<LeftRail />);
    fireEvent.click(screen.getByRole("button", { name: "完全隐藏面板" }));
    expect(useUiStore.getState().leftPanel.hidden).toBe(true);
    expect(screen.queryByTestId("left-panel-dock")).toBeNull();
  });

  it("the new-conversation icon opens the wizard dialog", async () => {
    patchLeftPanel({ collapsed: true });
    renderWithProviders(<LeftRail />);
    fireEvent.click(screen.getByRole("button", { name: "新建会话" }));
    await screen.findByRole("dialog");
  });
});

describe("LeftRail — hidden state", () => {
  it("renders neither nav nor dock", () => {
    patchLeftPanel({ hidden: true });
    renderWithProviders(<LeftRail />);
    expect(screen.queryByTestId("left-panel-dock")).toBeNull();
    expect(screen.queryByTestId("panel-resizer-left")).toBeNull();
    expect(screen.queryByRole("navigation", { name: "糯米 · nuomi" })).toBeNull();
  });
});
