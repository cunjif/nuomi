import { fireEvent, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { WorkspacePathBar } from "./WorkspacePathBar";
import type { ConversationDto } from "../../lib/ipc/client";

function makeConversation(overrides: Partial<ConversationDto> = {}): ConversationDto {
  return {
    id: "c1",
    title: "Chat",
    kind: "chat",
    teamId: null,
    taskId: null,
    scheduleId: null,
    createdAt: 0,
    updatedAt: 0,
    goal: null,
    mainAgentId: null,
    routeMode: null,
    whiteboardRouteMode: null,
    participantAgents: [{ kind: "role", id: "role-1", name: "Alice" }],
    todoList: [],
    workspaceId: "__migrated__",
    ...overrides,
  };
}

describe("WorkspacePathBar", () => {
  it("renders the full workspace root path verbatim (incl. \\\\?\\ prefix)", () => {
    const rootPath = "\\\\?\\C:\\Users\\james\\Codehub\\nuomi";
    renderWithProviders(
      <WorkspacePathBar
        rootPath={rootPath}
        conversation={null}
        onOpenWorkspaceList={() => {}}
        onAgentSidebarOpen={() => {}}
      />,
    );
    expect(screen.getByText(rootPath)).toBeTruthy();
  });

  it("clicking the path opens the workspace list", () => {
    const onOpen = vi.fn();
    renderWithProviders(
      <WorkspacePathBar
        rootPath="/tmp/ws"
        conversation={null}
        onOpenWorkspaceList={onOpen}
        onAgentSidebarOpen={() => {}}
      />,
    );
    fireEvent.click(screen.getByText("/tmp/ws"));
    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  it("shows avatar + more button for a conversation and opens the Agent sidebar", () => {
    const onOpenSidebar = vi.fn();
    renderWithProviders(
      <WorkspacePathBar
        rootPath="/tmp/ws"
        conversation={makeConversation()}
        onOpenWorkspaceList={() => {}}
        onAgentSidebarOpen={onOpenSidebar}
      />,
    );
    expect(screen.getByLabelText("Alice")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "打开 Agent 侧边栏" }));
    expect(onOpenSidebar).toHaveBeenCalledTimes(1);
  });

  it("hides avatar + more button when conversation is null", () => {
    renderWithProviders(
      <WorkspacePathBar
        rootPath="/tmp/ws"
        conversation={null}
        onOpenWorkspaceList={() => {}}
        onAgentSidebarOpen={() => {}}
      />,
    );
    expect(screen.queryByRole("button", { name: "打开 Agent 侧边栏" })).toBeNull();
  });
});
