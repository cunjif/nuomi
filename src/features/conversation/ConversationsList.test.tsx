import { fireEvent, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { seedConversation, seedStewardSession } from "../../lib/ipc/test-double";
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
