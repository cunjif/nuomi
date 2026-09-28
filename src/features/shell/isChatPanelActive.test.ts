import { describe, expect, it } from "vitest";
import type { View, ActiveArea } from "../../lib/store/uiStore";
import { isChatPanelActive } from "./isChatPanelActive";

describe("isChatPanelActive", () => {
  it("returns true only for the chat panel (view=chat, activeArea=chat)", () => {
    expect(isChatPanelActive("chat", "chat")).toBe(true);
  });

  it("returns false for every non-chat view regardless of activeArea", () => {
    const nonChatViews: View[] = [
      "board",
      "trace",
      "git",
      "approvals",
      "scheduler",
      "settings",
      "plugins",
    ];
    const areas: ActiveArea[] = ["chat", "workbench"];
    for (const view of nonChatViews) {
      for (const area of areas) {
        expect(isChatPanelActive(view, area)).toBe(false);
      }
    }
  });

  it("returns false when activeArea is workbench even if view is chat", () => {
    expect(isChatPanelActive("chat", "workbench")).toBe(false);
  });
});
