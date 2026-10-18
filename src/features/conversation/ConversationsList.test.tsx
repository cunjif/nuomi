import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { seedConversation, seedStewardSession, tdState } from "../../lib/ipc/test-double";
import { useUiStore } from "../../lib/store/uiStore";
import { stubLocalStorage } from "../../test/stubStorage";
import { renderWithProviders } from "../../test/helpers";
import { ConversationsList } from "./ConversationsList";

beforeEach(() => {
  stubLocalStorage();
  useUiStore.setState({ selectedSessionId: null, conversationWorkspaceFilter: "all" });
});

describe("ConversationsList — steward session isolation (spec 5.2)", () => {
  it("the 'all' view never lists the steward session but lists chat and genuine background sessions", async () => {
    seedConversation({ id: "im-1", title: "普通单聊" });
    seedConversation({ id: "bg-1", title: "真后台任务", kind: "background" });
    seedStewardSession({ id: "steward-1", title: "新管家会话" });
    renderWithProviders(<ConversationsList />);

    expect(await screen.findByText("普通单聊")).toBeTruthy();
    expect(screen.getByText("真后台任务")).toBeTruthy();
    // Steward session is physically excluded at the data source.
    expect(screen.queryByText("新管家会话")).toBeNull();
  });

  it("the 'background' filter view keeps genuine background sessions and never shows the steward session", async () => {
    seedConversation({ id: "im-1", title: "普通单聊" });
    seedConversation({ id: "bg-1", title: "真后台任务", kind: "background" });
    seedStewardSession({ id: "steward-1", title: "新管家会话" });
    renderWithProviders(<ConversationsList />);

    // Wait for initial load, then switch to the background filter tab.
    await screen.findByText("普通单聊");
    fireEvent.click(screen.getByRole("tab", { name: "后台" }));

    expect(await screen.findByText("真后台任务")).toBeTruthy();
    expect(screen.queryByText("普通单聊")).toBeNull();
    expect(screen.queryByText("新管家会话")).toBeNull();
  });

  it("batch mode selection pool never contains the steward session", async () => {
    seedConversation({ id: "im-1", title: "普通单聊" });
    seedStewardSession({ id: "steward-1", title: "新管家会话" });
    renderWithProviders(<ConversationsList />);

    await screen.findByText("普通单聊");
    fireEvent.click(screen.getByRole("button", { name: "批量" }));

    // Batch checkboxes only exist for listed (non-steward) conversations.
    const checkboxes = screen.getAllByRole("checkbox") as HTMLInputElement[];
    const stewardCheckbox = checkboxes.find((c) => c.closest("li")?.textContent?.includes("新管家会话"));
    expect(stewardCheckbox).toBeUndefined();
    expect(screen.getByLabelText(/全选/)).toBeTruthy();
  });
});

describe("ConversationsList — workspace filter (spec 5.5.1-7a, ADR-0021 task 6.9)", () => {
  function seedWorkspaces(): void {
    tdState.workspaces.push(
      {
        id: "ws1",
        rootPath: "/tmp/alpha",
        colorTag: "paper-yellow",
        createdAt: 1,
        isActive: true,
        directoryPresent: true,
        label: "alpha-beta",
        labelFull: "alpha-beta",
      },
      {
        id: "ws2",
        rootPath: "/tmp/gamma",
        colorTag: "paper-blue",
        createdAt: 1,
        isActive: false,
        directoryPresent: true,
        label: "gamma",
        labelFull: "gamma",
      },
    );
  }

  it("filtering by a multi-directory workspace shows sessions from both directories", async () => {
    seedWorkspaces();
    seedConversation({ id: "im-a", title: "alpha期会话", workspaceId: "ws1" });
    seedConversation({ id: "im-b", title: "beta期会话", workspaceId: "ws1" });
    seedConversation({ id: "im-c", title: "gamma会话", workspaceId: "ws2" });
    renderWithProviders(<ConversationsList />);

    // "all" aggregates across workspaces.
    expect(await screen.findByText("alpha期会话")).toBeTruthy();
    expect(screen.getByText("beta期会话")).toBeTruthy();
    expect(screen.getByText("gamma会话")).toBeTruthy();

    // Filtering by ws1 (two work directories) surfaces BOTH directories'
    // sessions — the ownership key is the workspace id, not the directory.
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "ws1" } });
    expect(await screen.findByText("alpha期会话")).toBeTruthy();
    expect(screen.getByText("beta期会话")).toBeTruthy();
    expect(screen.queryByText("gamma会话")).toBeNull();
  });

  it("switching the filter to the other workspace shows only its sessions", async () => {
    seedWorkspaces();
    seedConversation({ id: "im-a", title: "alpha期会话", workspaceId: "ws1" });
    seedConversation({ id: "im-c", title: "gamma会话", workspaceId: "ws2" });
    renderWithProviders(<ConversationsList />);

    await screen.findByText("alpha期会话");
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "ws2" } });

    expect(await screen.findByText("gamma会话")).toBeTruthy();
    expect(screen.queryByText("alpha期会话")).toBeNull();
  });

  it("labels filter options with the derived workspace label instead of the root path", async () => {
    seedWorkspaces();
    renderWithProviders(<ConversationsList />);

    const combo = await screen.findByRole("combobox");
    await waitFor(() => {
      expect(combo.querySelector('option[value="ws1"]')).not.toBeNull();
    });
    expect(combo.querySelector('option[value="ws1"]')?.textContent).toBe("alpha-beta");
    expect(combo.querySelector('option[value="ws2"]')?.textContent).toBe("gamma");
  });
});
