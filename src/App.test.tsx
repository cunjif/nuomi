import { screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { tdState } from "./lib/ipc/test-double";
import { renderWithProviders } from "./test/helpers";
import App from "./App";

describe("App shell (U8)", () => {
  it("renders the three-pane shell with nav and connection status", async () => {
    tdState.sessions.push({ id: "s1", title: "demo session", createdAt: 1, updatedAt: 1 });
    renderWithProviders(<App />);

    // Nav entries for every surface (after the workspace gate resolves).
    for (const label of ["对话", "看板", "Trace", "审批", "定时任务", "设置"]) {
      expect(await screen.findByRole("button", { name: label })).toBeInTheDocument();
    }
    // Connection status resolves via the sessions probe.
    await screen.findByText("已连接");
    // Custom title bar: window buttons only render under the Tauri runtime
    // (decorations:false window + data-tauri-drag-region header) — absent
    // in the plain-browser test environment.
    expect(screen.queryByRole("button", { name: "关闭" })).toBeNull();
    expect(screen.queryByRole("button", { name: "最小化" })).toBeNull();
    // The title row is the window drag region: "deep" so clicks on the
    // covering (absolutely positioned) tab strip still move the window.
    const dragRegion = document.querySelector('[data-tauri-drag-region="deep"]');
    expect(dragRegion).not.toBeNull();
    // Interactive children must NOT redeclare the attribute — a bare/self
    // value in the path would cancel the drag.
    expect(dragRegion?.querySelectorAll("[data-tauri-drag-region]")).toHaveLength(0);
    // Session list shows the seeded session.
    await screen.findByText("demo session");
    // Chat placeholder prompts to pick a session.
    expect(screen.getByText("选择或新建一个会话开始对话")).toBeInTheDocument();
  });
});
