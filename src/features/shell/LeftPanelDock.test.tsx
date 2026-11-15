import { fireEvent, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { seedConversation } from "../../lib/ipc/test-double-state";
import { useUiStore } from "../../lib/store/uiStore";
import { LeftPanelDock } from "./LeftPanelDock";

afterEach(() => {
  useUiStore.setState({
    leftPanel: { width: 224, collapsed: false, hidden: false },
    view: "chat",
    selectedSessionId: null,
    openSessionIds: [],
  });
});

describe("LeftPanelDock — sessions floating panel", () => {
  it("opens the floating sessions panel with the full ConversationsList", () => {
    renderWithProviders(<LeftPanelDock />);
    expect(screen.queryByTestId("dock-sessions-panel")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "会话" }));
    expect(screen.getByTestId("dock-sessions-panel")).toBeInTheDocument();
    // Full reuse: the list heading, filter tabs and the workspace filter
    // (sunk to the bottom) are all present.
    expect(screen.getAllByText("会话").length).toBeGreaterThan(0);
    expect(screen.getByRole("tab", { name: "全部" })).toBeInTheDocument();
    expect(screen.getByRole("combobox")).toBeInTheDocument();
  });

  it("toggles the panel off on a second click on the trigger", () => {
    renderWithProviders(<LeftPanelDock />);
    const trigger = screen.getByRole("button", { name: "会话" });
    fireEvent.click(trigger);
    expect(screen.getByTestId("dock-sessions-panel")).toBeInTheDocument();
    // The trigger is excluded from click-outside, so a plain second click
    // closes the panel.
    fireEvent.click(trigger);
    expect(screen.queryByTestId("dock-sessions-panel")).toBeNull();
  });

  it("closes on outside mousedown", () => {
    renderWithProviders(<LeftPanelDock />);
    fireEvent.click(screen.getByRole("button", { name: "会话" }));
    expect(screen.getByTestId("dock-sessions-panel")).toBeInTheDocument();
    fireEvent.mouseDown(document.body);
    expect(screen.queryByTestId("dock-sessions-panel")).toBeNull();
  });

  it("closes when a session is picked from the floating list", async () => {
    seedConversation({ id: "conv-1", title: "悬浮面板里的会话" });
    renderWithProviders(<LeftPanelDock />);
    fireEvent.click(screen.getByRole("button", { name: "会话" }));
    // The row's accessible name includes badge/time extras — match the title.
    fireEvent.click(await screen.findByRole("button", { name: /悬浮面板里的会话/ }));
    expect(useUiStore.getState().selectedSessionId).toBe("conv-1");
    expect(screen.queryByTestId("dock-sessions-panel")).toBeNull();
  });
});
