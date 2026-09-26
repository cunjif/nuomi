import { beforeEach, describe, expect, it } from "vitest";
import { stubLocalStorage } from "../../test/stubStorage";
import { useUiStore } from "./uiStore";

beforeEach(() => {
  stubLocalStorage();
  useUiStore.setState({ openSessionIds: [], selectedSessionId: null });
});

describe("uiStore openChatTab", () => {
  it("opens a new id: adds to front + sets selectedSessionId", () => {
    useUiStore.getState().openChatTab("a");
    const s = useUiStore.getState();
    expect(s.openSessionIds).toEqual(["a"]);
    expect(s.selectedSessionId).toBe("a");
  });

  it("existing id: activates (moves to front) without duplicate", () => {
    useUiStore.setState({ openSessionIds: ["a", "b", "c"], selectedSessionId: "c" });
    useUiStore.getState().openChatTab("a");
    const s = useUiStore.getState();
    expect(s.openSessionIds).toEqual(["a", "b", "c"]);
    expect(s.selectedSessionId).toBe("a");
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
  it("sets selectedSessionId + moves id to front", () => {
    useUiStore.setState({ openSessionIds: ["a", "b", "c"], selectedSessionId: "a" });
    useUiStore.getState().activateChatTab("c");
    const s = useUiStore.getState();
    expect(s.selectedSessionId).toBe("c");
    expect(s.openSessionIds).toEqual(["c", "a", "b"]);
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

  it("selecting an already-open session activates without duplicate", () => {
    useUiStore.setState({ openSessionIds: ["s1", "s2"], selectedSessionId: "s2", activeArea: "workbench" });
    useUiStore.getState().selectSession("s1");
    const s = useUiStore.getState();
    expect(s.openSessionIds).toEqual(["s1", "s2"]);
    expect(s.selectedSessionId).toBe("s1");
  });

  it("selectSession(null) clears selection without touching tabs", () => {
    useUiStore.setState({ openSessionIds: ["s1", "s2"], selectedSessionId: "s1" });
    useUiStore.getState().selectSession(null);
    const s = useUiStore.getState();
    expect(s.selectedSessionId).toBeNull();
    expect(s.openSessionIds).toEqual(["s1", "s2"]);
  });
});
