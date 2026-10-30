import { describe, expect, it } from "vitest";
import { useUiStore } from "./uiStore";

function reset(): void {
  useUiStore.setState({
    tempFileTab: null,
    tempFileTabs: [],
    activeTempFile: null,
    activeWorkspaceId: null,
    editorByWorkspace: {},
  });
}

describe("uiStore tempFileTabs multi-tab", () => {
  it("opens multiple files as independent tabs and de-dups by absPath", () => {
    reset();
    const s = useUiStore.getState();
    s.openTempFileTab("C:\\a.txt", "a.txt", "content-a");
    s.openTempFileTab("C:\\b.txt", "b.txt", "content-b");
    // Re-open a.txt → focus, no new tab
    s.openTempFileTab("C:\\a.txt", "a.txt", "content-a");

    const state = useUiStore.getState();
    expect(state.tempFileTabs).toHaveLength(2);
    expect(state.activeTempFile).toBe("C:\\a.txt");
    expect(state.tempFileTabs[0]?.absPath).toBe("C:\\a.txt");
    expect(state.tempFileTabs[1]?.absPath).toBe("C:\\b.txt");
  });

  it("updates draft and sets dirty; clears dirty after save", () => {
    reset();
    const s = useUiStore.getState();
    s.openTempFileTab("C:\\a.txt", "a.txt", "original");
    s.updateTempFileDraft("C:\\a.txt", "edited");
    const mid = useUiStore.getState();
    expect(mid.tempFileTabs[0]?.draft).toBe("edited");
    expect(mid.tempFileTabs[0]?.dirty).toBe(true);
    // Save → content becomes draft, dirty cleared
    s.clearTempFileDirty("C:\\a.txt");
    const saved = useUiStore.getState();
    expect(saved.tempFileTabs[0]?.content).toBe("edited");
    expect(saved.tempFileTabs[0]?.dirty).toBe(false);
  });

  it("closing the active tab activates the neighbor", () => {
    reset();
    useUiStore.getState().openTempFileTab("C:\\a.txt", "a.txt", "a");
    useUiStore.getState().openTempFileTab("C:\\b.txt", "b.txt", "b");
    expect(useUiStore.getState().activeTempFile).toBe("C:\\b.txt");
    useUiStore.getState().closeTempFileTab("C:\\b.txt");
    const state = useUiStore.getState();
    expect(state.tempFileTabs).toHaveLength(1);
    expect(state.activeTempFile).toBe("C:\\a.txt");
  });

  it("workspace switch destroys all temp tabs", () => {
    reset();
    useUiStore.getState().openTempFileTab("C:\\a.txt", "a.txt", "a");
    useUiStore.getState().openTempFileTab("C:\\b.txt", "b.txt", "b");
    useUiStore.getState().setActiveWorkspaceId("ws-new");
    const state = useUiStore.getState();
    expect(state.tempFileTabs).toHaveLength(0);
    expect(state.activeTempFile).toBeNull();
    expect(state.tempFileTab).toBeNull();
  });
});
