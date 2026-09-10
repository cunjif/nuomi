import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type * as Monaco from "monaco-editor";
import { stubLocalStorage } from "../../test/stubStorage";
import { injectIpcCommands, resetIpcCommands } from "../ipc/client";
import {
  attachMonacoProviders,
  getRegisteredEditorExtensions,
  initEditorExtEnabledStore,
  isEditorExtensionEnabled,
  findPreviewForPath,
  getOutlineProviders,
  getToolbarActions,
  getOverlays,
  ensureEditorExtensionsActivated,
  registerEditorExtension,
  setEditorExtensionEnabled,
  unregisterEditorExtension,
  useEditorExtVersion,
  resetEditorExtensionsForTest,
} from "./registry";
import type { EditorExtension } from "./types";
import { render } from "@testing-library/react";

function makeExt(id: string, titleI18nKey = `editor.ext.${id}`): EditorExtension {
  return { id, titleI18nKey, contribute: () => {} };
}

beforeEach(() => {
  stubLocalStorage();
  resetEditorExtensionsForTest();
  resetIpcCommands();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

/** Injects app_settings doubles backed by a Map; returns the store. */
function stubSettingsStore(): Map<string, string> {
  const store = new Map<string, string>();
  injectIpcCommands({
    appSettingGet: async (key: string) => ({ status: "ok", data: store.get(key) ?? null }),
    appSettingSet: async (key: string, value: string) => {
      store.set(key, value);
      return { status: "ok", data: null };
    },
  });
  return store;
}

describe("editor extension registry — registration", () => {
  it("registers an extension and lists it", () => {
    registerEditorExtension(makeExt("builtin.a"));
    expect(getRegisteredEditorExtensions().map((e) => e.id)).toEqual(["builtin.a"]);
  });

  it("dedupes by id — a later registration with the same id wins", () => {
    registerEditorExtension(makeExt("builtin.a"));
    registerEditorExtension(makeExt("builtin.b"));
    registerEditorExtension(makeExt("builtin.a", "editor.ext.replacement"));
    const ids = getRegisteredEditorExtensions().map((e) => e.id);
    expect(ids).toEqual(["builtin.b", "builtin.a"]);
    expect(getRegisteredEditorExtensions().at(-1)?.titleI18nKey).toBe("editor.ext.replacement");
  });

  it("unregister removes the extension and all of its contributions", () => {
    registerEditorExtension({
      id: "plugin.upper.editor",
      title: "Upper",
      contribute: (ctx) => {
        ctx.registerPreview({ mode: "replace", matcher: () => true, component: () => null });
        ctx.registerOutline({ id: "s", languages: ["*"], provideSymbols: () => [] });
        ctx.registerToolbarAction({ id: "a", Component: () => null });
        ctx.registerOverlay({ id: "o", Component: () => null });
        ctx.reportCapability("editor.hover");
      },
    });
    ensureEditorExtensionsActivated();
    expect(findPreviewForPath("x.md")).not.toBeNull();
    expect(getOutlineProviders()).toHaveLength(1);
    expect(getOverlays()).toHaveLength(1);
    expect(getToolbarActions()).toHaveLength(1);

    unregisterEditorExtension("plugin.upper.editor");
    expect(getRegisteredEditorExtensions()).toHaveLength(0);
    expect(findPreviewForPath("x.md")).toBeNull();
    expect(getOutlineProviders()).toHaveLength(0);
    expect(getOverlays()).toHaveLength(0);
    expect(getToolbarActions()).toHaveLength(0);
    // Re-registering is a clean slate (contributions are not resurrected).
    let contributed = 0;
    registerEditorExtension({
      id: "plugin.upper.editor",
      title: "Upper",
      contribute: () => {
        contributed += 1;
      },
    });
    ensureEditorExtensionsActivated();
    expect(contributed).toBe(1);
  });
});

describe("editor extension registry — enable/disable filtering", () => {
  it("persists the toggle into app_settings (ADR 0010 blob)", async () => {
    const store = stubSettingsStore();
    await initEditorExtEnabledStore();
    registerEditorExtension(makeExt("builtin.a"));
    expect(isEditorExtensionEnabled("builtin.a")).toBe(true);
    setEditorExtensionEnabled("builtin.a", false);
    expect(isEditorExtensionEnabled("builtin.a")).toBe(false);
    const blob = JSON.parse(store.get("editorext.enabled.") ?? "{}") as Record<string, boolean>;
    expect(blob["builtin.a"]).toBe(false);
    // LocalStorage mirror is cleaned up once the IPC write succeeded.
    expect(localStorage.getItem("nuomi.editorExt.enabled.builtin.a")).toBeNull();
    setEditorExtensionEnabled("builtin.a", true);
    const blob2 = JSON.parse(store.get("editorext.enabled.") ?? "{}") as Record<string, boolean>;
    expect(blob2["builtin.a"]).toBe(true);
  });

  it("falls back to localStorage when IPC is unavailable", async () => {
    // No ipc doubles: the production binding throws in the test runtime.
    registerEditorExtension(makeExt("builtin.a"));
    setEditorExtensionEnabled("builtin.a", false);
    // The fallback write lands in a promise rejection handler.
    await vi.waitFor(() => {
      expect(localStorage.getItem("nuomi.editorExt.enabled.builtin.a")).toBe("0");
    });
    expect(isEditorExtensionEnabled("builtin.a")).toBe(false);
  });

  it("migrates legacy localStorage entries into app_settings once", async () => {
    const store = stubSettingsStore();
    localStorage.setItem("nuomi.editorExt.enabled.legacy.a", "0");
    await initEditorExtEnabledStore();
    expect(isEditorExtensionEnabled("legacy.a")).toBe(false);
    const blob = JSON.parse(store.get("editorext.enabled.") ?? "{}") as Record<string, boolean>;
    expect(blob["legacy.a"]).toBe(false);
    // Legacy keys are cleared so they cannot resurrect stale values.
    expect(localStorage.getItem("nuomi.editorExt.enabled.legacy.a")).toBeNull();
  });

  it("does not contribute disabled extensions", () => {
    const contributed: string[] = [];
    registerEditorExtension({
      id: "builtin.off",
      titleI18nKey: "editor.ext.off",
      contribute: () => contributed.push("off"),
    });
    registerEditorExtension({
      id: "builtin.on",
      titleI18nKey: "editor.ext.on",
      contribute: () => contributed.push("on"),
    });
    setEditorExtensionEnabled("builtin.off", false);
    ensureEditorExtensionsActivated();
    expect(contributed).toEqual(["on"]);
  });

  it("filters read-side contributions by enabled state", () => {
    registerEditorExtension({
      id: "builtin.p",
      titleI18nKey: "editor.ext.p",
      contribute: (ctx) => {
        ctx.registerPreview({ mode: "replace", matcher: (p) => p.endsWith(".md"), component: () => null });
        ctx.registerOutline({ id: "p.symbols", languages: ["*"], provideSymbols: () => [] });
        ctx.registerToolbarAction({ id: "p.action", Component: () => null });
        ctx.registerOverlay({ id: "p.overlay", Component: () => null });
      },
    });
    ensureEditorExtensionsActivated();
    expect(findPreviewForPath("README.md")).not.toBeNull();
    expect(getOutlineProviders()).toHaveLength(1);
    expect(getToolbarActions()).toHaveLength(1);
    expect(getOverlays()).toHaveLength(1);

    setEditorExtensionEnabled("builtin.p", false);
    expect(findPreviewForPath("README.md")).toBeNull();
    expect(getOutlineProviders()).toHaveLength(0);
    expect(getToolbarActions()).toHaveLength(0);
    expect(getOverlays()).toHaveLength(0);
  });
});

describe("editor extension registry — editor-ready binding", () => {
  it("binds every mounted editor instance, not only the first", () => {
    const bound: Monaco.editor.IStandaloneCodeEditor[] = [];
    registerEditorExtension({
      id: "builtin.ready",
      titleI18nKey: "editor.ext.ready",
      contribute: (ctx) =>
        ctx.registerEditorReady((_monaco, editor) => {
          bound.push(editor);
        }),
    });
    ensureEditorExtensionsActivated();

    // EditorArea remounts MonacoTab per open file (key={path}), so each
    // switch creates a NEW instance that needs its own F12/Alt+F1 bindings.
    const editorA = {} as Monaco.editor.IStandaloneCodeEditor;
    const editorB = {} as Monaco.editor.IStandaloneCodeEditor;
    const monaco = {} as typeof Monaco;
    attachMonacoProviders(monaco, editorA);
    attachMonacoProviders(monaco, editorB);
    attachMonacoProviders(monaco, editorA); // same instance again: no rebind
    expect(bound).toEqual([editorA, editorB]);
  });
});

describe("editor extension registry — version store", () => {
  it("bumps the version on registration and toggles (useSyncExternalStore source)", () => {
    const before = useEditorExtVersionSnapshot();
    registerEditorExtension(makeExt("builtin.v"));
    expect(useEditorExtVersionSnapshot()).toBeGreaterThan(before);
    const mid = useEditorExtVersionSnapshot();
    setEditorExtensionEnabled("builtin.v", false);
    expect(useEditorExtVersionSnapshot()).toBeGreaterThan(mid);
  });
});

function useEditorExtVersionSnapshot(): number {
  let value = 0;
  function Probe(): null {
    value = useEditorExtVersion();
    return null;
  }
  render(<Probe />);
  return value;
}
