import { describe, expect, it, beforeEach, vi, afterEach } from "vitest";
import { useUiStore } from "./uiStore";
import { navigateSource } from "../../features/chat/sourceNavigation";
import { tdState } from "../ipc/test-double-state";
import { setOpenUrlImpl } from "../openUrl";
import { useToastStore } from "./toastStore";
import type { SourceEntryDto } from "../events/types";
import "../../test/helpers";
import "../../i18n";

function source(kind: SourceEntryDto["kind"], target: string): SourceEntryDto {
  return { kind, target, label: target, agentId: "r1", agentName: "R" };
}

describe("uiStore — per-workspace tab memory (spec 5.3.1)", () => {
  beforeEach(() => {
    useUiStore.setState({
      openSessionIds: ["s1", "s2"],
      selectedSessionId: null,
      activeWorkspaceId: "w1",
      lastActiveSessionByWorkspace: {},
      tempFileTab: null,
    });
  });

  it("activation writes the memory bucketed by the active workspace", () => {
    useUiStore.getState().activateChatTab("s1");
    expect(useUiStore.getState().lastActiveSessionByWorkspace["w1"]).toBe("s1");
    // Switching workspaces keeps other buckets untouched.
    useUiStore.getState().switchWorkspace("w2");
    useUiStore.getState().activateChatTab("s2");
    expect(useUiStore.getState().lastActiveSessionByWorkspace["w1"]).toBe("s1");
    expect(useUiStore.getState().lastActiveSessionByWorkspace["w2"]).toBe("s2");
  });

  it("activation of an unknown session writes nothing", () => {
    useUiStore.getState().activateChatTab("ghost");
    expect(useUiStore.getState().lastActiveSessionByWorkspace).toEqual({});
    expect(useUiStore.getState().selectedSessionId).toBeNull();
  });
});

describe("uiStore — temp file tab (spec 5.2.1-6, D10)", () => {
  beforeEach(() => {
    useUiStore.setState({ tempFileTab: null, activeArea: "chat" });
  });

  it("opens read-only temp tab and reveals the workbench", () => {
    useUiStore.getState().openTempFile("C:\\external\\notes.txt", "notes.txt");
    const s = useUiStore.getState();
    expect(s.tempFileTab).toEqual({ absPath: "C:\\external\\notes.txt", name: "notes.txt" });
    expect(s.activeArea).toBe("workbench");
  });

  it("closes the temp tab", () => {
    useUiStore.getState().openTempFile("C:\\external\\notes.txt", "notes.txt");
    useUiStore.getState().closeTempFile();
    expect(useUiStore.getState().tempFileTab).toBeNull();
  });

  it("workspace switch destroys the temp tab (no persistence)", () => {
    useUiStore.getState().openTempFile("C:\\external\\notes.txt", "notes.txt");
    useUiStore.getState().switchWorkspace("w2");
    expect(useUiStore.getState().tempFileTab).toBeNull();
  });
});

describe("navigateSource — three-branch orchestration (spec 5.2.3)", () => {
  beforeEach(() => {
    useToastStore.setState({ toasts: [] });
    setOpenUrlImpl(async () => true);
    useUiStore.setState({
      activeWorkspaceId: "w1",
      editorByWorkspace: {},
      tempFileTab: null,
      activeArea: "chat",
    });
    tdState.sourceProbes.clear();
    tdState.files.clear();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("workspace_file branch: focuses directory, opens file, reveals workbench", async () => {
    tdState.sourceProbes.set("C:\\proj\\a\\src\\main.rs", {
      kind: "workspace_file",
      workspaceId: "w1",
      directoryId: "d1",
      relativePath: "src/main.rs",
      normalizedTarget: null,
      available: true,
    });
    tdState.workspaces.length = 0;
    tdState.workspaces.push({
      id: "w1",
      rootPath: "C:\\proj\\a",
      colorTag: "paper-yellow",
      createdAt: 1,
      isActive: true,
      directoryPresent: true,
      isPinned: false,
      isOpen: true,
      isFocused: true,
      openedAt: null,
      lastFocusedAt: null,
      label: null,
      labelFull: null,
      directories: [
        {
          id: "d1",
          absPath: "C:\\proj\\a",
          displayName: "a",
          mounted: true,
          available: true,
          addedAt: 1,
          isActive: false,
        },
      ],
    } as unknown as (typeof tdState.workspaces)[number]);
    await navigateSource(source("workspace_file", "C:\\proj\\a\\src\\main.rs"));
    const s = useUiStore.getState();
    expect(s.activeArea).toBe("workbench");
    expect(s.editorByWorkspace["w1"]?.openFiles).toContain("src/main.rs");
    expect(s.editorByWorkspace["w1"]?.activeFile).toBe("src/main.rs");
    expect(tdState.activeDirectoryId).toBe("d1");
  });

  it("external_file branch: opens the temp tab instead of a workspace file", async () => {
    tdState.sourceProbes.set("C:\\outside\\ref.txt", {
      kind: "external_file",
      workspaceId: null,
      directoryId: null,
      relativePath: null,
      normalizedTarget: "C:\\outside\\ref.txt",
      available: true,
    });
    await navigateSource(source("external_file", "C:\\outside\\ref.txt"));
    const s = useUiStore.getState();
    expect(s.tempFileTab).toEqual({ absPath: "C:\\outside\\ref.txt", name: "ref.txt" });
    expect(s.activeArea).toBe("workbench");
    expect(Object.keys(s.editorByWorkspace)).toHaveLength(0);
  });

  it("web_url branch: routes to the external opener", async () => {
    const opener = vi.fn(async () => true);
    setOpenUrlImpl(opener);
    await navigateSource(source("web_url", "https://example.com/x"));
    expect(opener).toHaveBeenCalledWith("https://example.com/x");
    expect(useUiStore.getState().tempFileTab).toBeNull();
  });

  it("unavailable branch: toast and no state change", async () => {
    await navigateSource(source("workspace_file", "C:\\gone\\missing.rs"));
    expect(useToastStore.getState().toasts.length).toBe(1);
    expect(useUiStore.getState().activeArea).toBe("chat");
  });

  it("cli_agent entries are inert", async () => {
    await navigateSource(source("cli_agent", "ap-1"));
    expect(useToastStore.getState().toasts.length).toBe(0);
    expect(useUiStore.getState().tempFileTab).toBeNull();
  });
});
