import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { injectIpcCommands } from "../../lib/ipc/client";
import { tdState, testDoubleCommands } from "../../lib/ipc/test-double";
import { renderWithProviders } from "../../test/helpers";
import { SchedulerView } from "./SchedulerView";

describe("SchedulerView — view scope toggle (task 7.3)", () => {
  it("renders the scope toggle and calls setViewScope on click", async () => {
    const base = testDoubleCommands();
    const setViewScope = vi.fn(base.setViewScope);
    injectIpcCommands({ ...base, setViewScope });

    renderWithProviders(<SchedulerView workspaceId={null} />);

    const focusedBtn = await screen.findByText("仅看聚焦");
    fireEvent.click(focusedBtn);
    await waitFor(() => expect(setViewScope).toHaveBeenCalledWith("scheduler", "focused"));
  });
});

describe("SchedulerView — schedule list (task 7.3)", () => {
  it("renders existing schedules from the test double", async () => {
    tdState.schedules.push({
      id: "s1",
      name: "nightly-build",
      cronExpr: "@every 3600",
      taskTitle: "build",
      taskDescription: "",
      enabled: true,
      lastTriggeredAt: null,
      nextTriggerAt: null,
      targetKind: "task",
      agent: null,
      teamId: null,
      sessionMode: "per_trigger",
      sessionId: null,
      autoDispatch: true,
      workspaceId: "__migrated__",
      workspaceRootPath: null,
    });
    injectIpcCommands(testDoubleCommands());

    renderWithProviders(<SchedulerView workspaceId={null} />);

    expect(await screen.findByText("nightly-build")).toBeInTheDocument();
  });
});
