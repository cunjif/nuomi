import { fireEvent, screen, waitFor, within } from "@testing-library/react";
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
      workspaceLabel: null,
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
    tdState.approvals.push({ id: "a2", runId: "r2", toolName: "git_push", argumentsJson: "{}", workspaceId: "__migrated__", workspaceLabel: null });
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

describe("ApprovalsView — team proposals (P2-F 组 2)", () => {
  const proposalFixture = {
    id: "p1",
    kind: "team_plan",
    source: "scheduled",
    status: "pending",
    workspaceId: "__migrated__",
    workspaceLabel: null,
    createdAt: 1,
    resolvedAt: null,
    plan: {
      task: "Fix the storage layer",
      taskIds: ["t1", "t2"],
      topology: "pipeline" as const,
      members: [
        { kind: "role", refId: "r-writer", roleName: "writer", systemPrompt: null },
        {
          kind: "provider",
          refId: "p-exec",
          roleName: "executor",
          systemPrompt: "execute the plan",
        },
      ],
      maxRounds: 4,
      required: ["code"],
      rationale: "write then execute",
    },
  };

  it("renders proposal rows alongside tool approvals and clears on approve", async () => {
    tdState.approvals.push({
      id: "a1",
      runId: "r1",
      toolName: "write_file",
      argumentsJson: '{"path":"src/main.rs"}',
      workspaceId: "__migrated__",
      workspaceLabel: null,
    });
    tdState.proposals.push(proposalFixture);
    const base = testDoubleCommands();
    const resolveProposal = vi.fn(base.resolveProposal);
    injectIpcCommands({ ...base, resolveProposal });

    renderWithProviders(<ApprovalsView workspaceId={null} />);

    // Both row families visible in one inbox.
    expect(await screen.findByText("write_file")).toBeInTheDocument();
    expect(screen.getByText("组队提案")).toBeInTheDocument();
    expect(screen.getByText("定时自发")).toBeInTheDocument();
    expect(screen.getByText("write then execute")).toBeInTheDocument();
    expect(screen.getByText("2 个关联任务 · 最大轮数 4")).toBeInTheDocument();
    expect(screen.getByText("writer")).toBeInTheDocument();

    // The proposal row's approve button (scoped to its list item) resolves it.
    const row = screen.getByText("组队提案").closest("li");
    expect(row).not.toBeNull();
    if (row) {
      fireEvent.click(within(row).getByRole("button", { name: "批准" }));
    }

    await waitFor(() => expect(resolveProposal).toHaveBeenCalledWith("p1", true));
    await waitFor(() => expect(screen.queryByText("组队提案")).not.toBeInTheDocument());
    expect(screen.getByText("write_file")).toBeInTheDocument();
  });

  it("deny resolves the proposal without touching tool approvals", async () => {
    tdState.proposals.push({ ...proposalFixture, id: "p2", source: "idle" });
    const base = testDoubleCommands();
    const resolveProposal = vi.fn(base.resolveProposal);
    injectIpcCommands({ ...base, resolveProposal });

    renderWithProviders(<ApprovalsView workspaceId={null} />);

    expect(await screen.findByText("空闲扫描")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "拒绝" }));

    await waitFor(() => expect(resolveProposal).toHaveBeenCalledWith("p2", false));
    await waitFor(() => expect(screen.queryByText("空闲扫描")).not.toBeInTheDocument());
  });
});
