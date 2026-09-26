import { beforeEach, describe, expect, it } from "vitest";
import { stubLocalStorage } from "../../test/stubStorage";
import { useUiStore } from "./uiStore";

beforeEach(() => {
  stubLocalStorage();
  useUiStore.setState({ openSessionIds: [], selectedSessionId: null });
});

describe("uiStore openChatTab", () => {
  it("opens a new id: appends to the tail + sets selectedSessionId", () => {
    useUiStore.getState().openChatTab("a");
    const s = useUiStore.getState();
    expect(s.openSessionIds).toEqual(["a"]);
    expect(s.selectedSessionId).toBe("a");
  });

  it("new ids append in open order so the leftmost tab is the oldest", () => {
    useUiStore.getState().openChatTab("a");
    useUiStore.getState().openChatTab("b");
    useUiStore.getState().openChatTab("c");
    const s = useUiStore.getState();
    expect(s.openSessionIds).toEqual(["a", "b", "c"]);
    expect(s.selectedSessionId).toBe("c");
  });

  it("existing id: activates in place (no reorder) and without duplicate", () => {
    useUiStore.setState({ openSessionIds: ["a", "b", "c"], selectedSessionId: "c" });
    useUiStore.getState().openChatTab("a");
    const s = useUiStore.getState();
    expect(s.openSessionIds).toEqual(["a", "b", "c"]);
    expect(s.selectedSessionId).toBe("a");
  });

  it("existing id not at front: does not hoist it to position 0", () => {
    useUiStore.setState({ openSessionIds: ["a", "b", "c"], selectedSessionId: "a" });
    useUiStore.getState().openChatTab("c");
    const s = useUiStore.getState();
    expect(s.openSessionIds).toEqual(["a", "b", "c"]);
    expect(s.selectedSessionId).toBe("c");
  });
});

describe("uiStore closeChatTab", () => {
  it("non-active tab: removes only, selectedSessionId unchanged", () => {
    useUiStore.setState({ openSessionIds: ["a", "b", "c"], selectedSessionId: "b" });
    useUiStore.getState().closeChatTab("a");
    const s = useUiStore.getState();
    expect(s.openSessionIds).toEqual(["b", "c"]);
    expect(s.selectedSessionId).toBe("b");
  });

  it("active tab: transfers to right neighbor", () => {
    useUiStore.setState({ openSessionIds: ["a", "b", "c"], selectedSessionId: "b" });
    useUiStore.getState().closeChatTab("b");
    const s = useUiStore.getState();
    expect(s.openSessionIds).toEqual(["a", "c"]);
    expect(s.selectedSessionId).toBe("c");
  });

  it("active tab at end: transfers to left neighbor", () => {
    useUiStore.setState({ openSessionIds: ["a", "b", "c"], selectedSessionId: "c" });
    useUiStore.getState().closeChatTab("c");
    const s = useUiStore.getState();
    expect(s.openSessionIds).toEqual(["a", "b"]);
    expect(s.selectedSessionId).toBe("b");
  });

  it("closing the only tab: enters empty state", () => {
    useUiStore.setState({ openSessionIds: ["a"], selectedSessionId: "a" });
    useUiStore.getState().closeChatTab("a");
    const s = useUiStore.getState();
    expect(s.openSessionIds).toEqual([]);
    expect(s.selectedSessionId).toBeNull();
  });
});

describe("uiStore activateChatTab", () => {
  it("sets selectedSessionId and keeps the tab in its original slot", () => {
    useUiStore.setState({ openSessionIds: ["a", "b", "c"], selectedSessionId: "a" });
    useUiStore.getState().activateChatTab("c");
    const s = useUiStore.getState();
    expect(s.selectedSessionId).toBe("c");
    expect(s.openSessionIds).toEqual(["a", "b", "c"]);
  });

  it("no-op when id not in openSessionIds", () => {
    useUiStore.setState({ openSessionIds: ["a", "b"], selectedSessionId: "a" });
    const before = useUiStore.getState();
    useUiStore.getState().activateChatTab("z");
    expect(useUiStore.getState()).toBe(before);
  });
});

describe("uiStore selectSession协同 openChatTab", () => {
  it("selecting a session opens it as a tab + activates", () => {
    useUiStore.setState({ openSessionIds: [], selectedSessionId: null, activeArea: "workbench" });
    useUiStore.getState().selectSession("s1");
    const s = useUiStore.getState();
    expect(s.selectedSessionId).toBe("s1");
    expect(s.openSessionIds).toEqual(["s1"]);
    expect(s.activeArea).toBe("chat");
  });

  it("selecting sessions in sequence appends them, oldest-opened leftmost", () => {
    useUiStore.setState({ openSessionIds: [], selectedSessionId: null, activeArea: "chat" });
    useUiStore.getState().selectSession("s1");
    useUiStore.getState().selectSession("s2");
    useUiStore.getState().selectSession("s3");
    expect(useUiStore.getState().openSessionIds).toEqual(["s1", "s2", "s3"]);
  });

  it("selecting an already-open session activates without duplicate", () => {
    useUiStore.setState({ openSessionIds: ["s1", "s2"], selectedSessionId: "s2", activeArea: "workbench" });
    useUiStore.getState().selectSession("s1");
    const s = useUiStore.getState();
    expect(s.openSessionIds).toEqual(["s1", "s2"]);
    expect(s.selectedSessionId).toBe("s1");
  });

  it("selecting an already-open trailing session does not hoist it to position 0", () => {
    useUiStore.setState({ openSessionIds: ["s1", "s2", "s3"], selectedSessionId: "s1", activeArea: "workbench" });
    useUiStore.getState().selectSession("s3");
    const s = useUiStore.getState();
    expect(s.openSessionIds).toEqual(["s1", "s2", "s3"]);
    expect(s.selectedSessionId).toBe("s3");
    expect(s.activeArea).toBe("chat");
  });

  it("selectSession(null) clears selection without touching tabs", () => {
    useUiStore.setState({ openSessionIds: ["s1", "s2"], selectedSessionId: "s1" });
    useUiStore.getState().selectSession(null);
    const s = useUiStore.getState();
    expect(s.selectedSessionId).toBeNull();
    expect(s.openSessionIds).toEqual(["s1", "s2"]);
  });
});
