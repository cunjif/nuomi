import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type * as Monaco from "monaco-editor";
import { stubLocalStorage } from "../../test/stubStorage";
import {
  attachMonacoProviders,
  getRegisteredEditorExtensions,
  isEditorExtensionEnabled,
  findPreviewForPath,
  getOutlineProviders,
  getToolbarActions,
  getOverlays,
  ensureEditorExtensionsActivated,
  registerEditorExtension,
  setEditorExtensionEnabled,
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
});

afterEach(() => {
  vi.unstubAllGlobals();
});

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
});

describe("editor extension registry — enable/disable filtering", () => {
  it("defaults to enabled and persists the toggle in localStorage", () => {
    registerEditorExtension(makeExt("builtin.a"));
    expect(isEditorExtensionEnabled("builtin.a")).toBe(true);
    setEditorExtensionEnabled("builtin.a", false);
    expect(isEditorExtensionEnabled("builtin.a")).toBe(false);
    expect(localStorage.getItem("nuomi.editorExt.enabled.builtin.a")).toBe("0");
    setEditorExtensionEnabled("builtin.a", true);
    expect(localStorage.getItem("nuomi.editorExt.enabled.builtin.a")).toBe("1");
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
