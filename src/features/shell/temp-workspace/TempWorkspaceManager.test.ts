import { describe, expect, it, vi } from "vitest";
import { injectIpcCommands } from "../../../lib/ipc/client";
import { testDoubleCommands } from "../../../lib/ipc/test-double";
import { tdReset, tdSeedFiles } from "../../../lib/ipc/test-double";
import { useUiStore } from "../../../lib/store/uiStore";
import { handleDropEvent } from "./TempWorkspaceManager";

function resetStore(): void {
  useUiStore.setState({
    tempFileTabs: [],
    activeTempFile: null,
    tempFileTab: null,
    activeWorkspaceId: "ws-test",
    editorByWorkspace: {},
  });
}

describe("TempWorkspaceManager handleDropEvent", () => {
  it("routes directories to openDirectory", async () => {
    injectIpcCommands(testDoubleCommands());
    tdReset();
    resetStore();
    const openDir = vi.spyOn(useUiStore.getState(), "openDirectory").mockResolvedValue(undefined);

    await handleDropEvent(["C:\\some-dir"]);

    expect(openDir).toHaveBeenCalledWith("C:\\some-dir");
    openDir.mockRestore();
  });

  it("opens an external file as a temp tab", async () => {
    injectIpcCommands(testDoubleCommands());
    tdReset();
    tdSeedFiles({ "external.txt": "hello" });
    resetStore();

    // Force classifyPath to return "file" by making listDir fail for this path.
    const base = testDoubleCommands();
    injectIpcCommands({
      ...base,
      listDir: async () => ({ status: "error", error: { generic: { code: "not_dir", message: "" } } } as never),
      readExternalFile: async () => ({ status: "ok", data: "hello" } as never),
      getWorkspace: async () => ({ status: "ok", data: { root: "C:\\workspace", configured: true } } as never),
    });

    await handleDropEvent(["C:\\external\\external.txt"]);

    const tabs = useUiStore.getState().tempFileTabs;
    expect(tabs).toHaveLength(1);
    expect(tabs[0]?.absPath).toBe("C:\\external\\external.txt");
    expect(tabs[0]?.content).toBe("hello");
  });

  it("focuses an already-open temp tab instead of duplicating", async () => {
    injectIpcCommands(testDoubleCommands());
    tdReset();
    tdSeedFiles({ "external.txt": "hello" });
    resetStore();
    useUiStore.getState().openTempFileTab("C:\\external\\external.txt", "external.txt", "hello");

    const base = testDoubleCommands();
    injectIpcCommands({
      ...base,
      listDir: async () => ({ status: "error", error: { generic: { code: "not_dir", message: "" } } } as never),
      readExternalFile: async () => ({ status: "ok", data: "hello" } as never),
      getWorkspace: async () => ({ status: "ok", data: { root: "C:\\workspace", configured: true } } as never),
    });

    await handleDropEvent(["C:\\external\\external.txt"]);

    expect(useUiStore.getState().tempFileTabs).toHaveLength(1);
    expect(useUiStore.getState().activeTempFile).toBe("C:\\external\\external.txt");
  });

  it("opens multiple files as independent tabs", async () => {
    injectIpcCommands(testDoubleCommands());
    tdReset();
    tdSeedFiles({ "a.txt": "aaa", "b.txt": "bbb" });
    resetStore();

    const base = testDoubleCommands();
    injectIpcCommands({
      ...base,
      listDir: async () => ({ status: "error", error: { generic: { code: "not_dir", message: "" } } } as never),
      readExternalFile: async (p: string) => ({
        status: "ok",
        data: p.endsWith("a.txt") ? "aaa" : "bbb",
      } as never),
      getWorkspace: async () => ({ status: "ok", data: { root: "C:\\workspace", configured: true } } as never),
    });

    await handleDropEvent(["C:\\external\\a.txt", "C:\\external\\b.txt"]);

    expect(useUiStore.getState().tempFileTabs).toHaveLength(2);
  });
});
