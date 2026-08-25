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
    });
    const base = testDoubleCommands();
    const resolveApproval = vi.fn(base.resolveApproval);
    injectIpcCommands({ ...base, resolveApproval });

    renderWithProviders(<ApprovalsView />);

    expect(await screen.findByText("write_file")).toBeInTheDocument();
    expect(screen.getByText('{"path":"src/main.rs"}')).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "批准" }));

    await waitFor(() => expect(resolveApproval).toHaveBeenCalledWith("a1", true));
    await waitFor(() => expect(screen.queryByText("write_file")).not.toBeInTheDocument());
  });

  it("deny also resolves the item", async () => {
    tdState.approvals.push({ id: "a2", runId: "r2", toolName: "git_push", argumentsJson: "{}" });
    const base = testDoubleCommands();
    const resolveApproval = vi.fn(base.resolveApproval);
    injectIpcCommands({ ...base, resolveApproval });

    renderWithProviders(<ApprovalsView />);
    fireEvent.click(await screen.findByRole("button", { name: "拒绝" }));

    await waitFor(() => expect(resolveApproval).toHaveBeenCalledWith("a2", false));
  });
});
