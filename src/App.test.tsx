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
    // Session list shows the seeded session.
    await screen.findByText("demo session");
    // Chat placeholder prompts to pick a session.
    expect(screen.getByText("选择或新建一个会话开始对话")).toBeInTheDocument();
  });
});
