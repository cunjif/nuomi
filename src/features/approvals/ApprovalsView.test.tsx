import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { injectIpcCommands } from "../../lib/ipc/client";
import { tdState, testDoubleCommands } from "../../lib/ipc/test-double";
import { renderWithProviders } from "../../test/helpers";
import { ApprovalsView } from "./ApprovalsView";

describe("ApprovalsView — approve flow (AC10)", () => {
  it("calls resolveApproval(true) and clears the inbox item", async () => {
    tdState.approvals.push({
      id: "a1",
      runId: "r1",
      toolName: "write_file",
      argumentsJson: '{"path":"src/main.rs"}',
      workspaceId: "__migrated__",
      workspaceRootPath: null,
    });
    const base = testDoubleCommands();
    const resolveApproval = vi.fn(base.resolveApproval);
    injectIpcCommands({ ...base, resolveApproval });

    renderWithProviders(<ApprovalsView workspaceId={null} />);

    expect(await screen.findByText("write_file")).toBeInTheDocument();
    expect(screen.getByText('{"path":"src/main.rs"}')).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "批准" }));

    await waitFor(() => expect(resolveApproval).toHaveBeenCalledWith("a1", true));
    await waitFor(() => expect(screen.queryByText("write_file")).not.toBeInTheDocument());
  });

  it("deny also resolves the item", async () => {
    tdState.approvals.push({ id: "a2", runId: "r2", toolName: "git_push", argumentsJson: "{}", workspaceId: "__migrated__", workspaceRootPath: null });
    const base = testDoubleCommands();
    const resolveApproval = vi.fn(base.resolveApproval);
    injectIpcCommands({ ...base, resolveApproval });

    renderWithProviders(<ApprovalsView workspaceId={null} />);
    fireEvent.click(await screen.findByRole("button", { name: "拒绝" }));

    await waitFor(() => expect(resolveApproval).toHaveBeenCalledWith("a2", false));
  });
});

describe("ApprovalsView — view scope toggle (task 7.3)", () => {
  it("renders the scope toggle and calls setViewScope on click", async () => {
    const base = testDoubleCommands();
    const setViewScope = vi.fn(base.setViewScope);
    injectIpcCommands({ ...base, setViewScope });

    renderWithProviders(<ApprovalsView workspaceId={null} />);

    const focusedBtn = await screen.findByText("仅看聚焦");
    fireEvent.click(focusedBtn);
    await waitFor(() => expect(setViewScope).toHaveBeenCalledWith("approvals", "focused"));
  });
});
