import { describe, expect, it, beforeEach } from "vitest";
import type * as Monaco from "monaco-editor";
import type { EditorExtContext } from "../types";
import {
  activateBuiltinEditorExtensions,
  isEditorExtensionEnabled,
  resetEditorExtensionsForTest,
  setEditorExtensionEnabled,
} from "../index";
import { hoverDocsExtension } from "./hover-docs";

// Register builtin extensions so outline-symbols feeds the hover provider.
activateBuiltinEditorExtensions();

// Minimal stub model for the hover provider (jsdom has no Monaco).
function makeModel(content: string, language: string, wordAt: { word: string; column: number } | null): Monaco.editor.ITextModel {
  return {
    getValue: () => content,
    getLanguageId: () => language,
    getWordAtPosition: () => wordAt,
  } as unknown as Monaco.editor.ITextModel;
}

const pos = { lineNumber: 1, column: 1 } as Monaco.Position;
const cancelToken = { isCancellationRequested: false, onCancellationRequested: () => ({ dispose: () => {} }) } as Monaco.CancellationToken;

function captureProvider(): Monaco.languages.HoverProvider {
  let captured: Monaco.languages.HoverProvider | null = null;
  const ctx = {
    registerHoverProvider: (_lang: string, p: Monaco.languages.HoverProvider) => {
      captured = p;
    },
    reportCapability: () => {},
  } as unknown as EditorExtContext;
  hoverDocsExtension.contribute(ctx);
  return captured as unknown as Monaco.languages.HoverProvider;
}

describe("EnhancedHoverProvider", () => {
  it("returns null when no word is under the cursor", () => {
    const model = makeModel("let x = 1;", "typescript", null);
    const provider = captureProvider();
    expect(provider.provideHover(model, pos, cancelToken)).toBeNull();
  });

  it("composes structured content for a method symbol", () => {
    const model = makeModel("function add(a: number, b: number): number { return a + b; }", "typescript", {
      word: "add",
      column: 10,
    });
    const provider = captureProvider();
    const result = provider.provideHover(model, pos, cancelToken) as Monaco.languages.Hover | null;
    expect(result).not.toBeNull();
    const joined = (result?.contents ?? []).map((c) => c.value).join("\n");
    expect(joined).toContain("function add");
  });

  it("returns null for an unrecognised symbol with no refs", () => {
    const model = makeModel("let x = 1;", "typescript", { word: "nonexistent", column: 1 });
    const provider = captureProvider();
    const result = provider.provideHover(model, pos, cancelToken);
    expect(result).toBeNull();
  });
});

describe("EnhancedHoverProvider — toggle fallback", () => {
  beforeEach(() => {
    resetEditorExtensionsForTest();
    activateBuiltinEditorExtensions();
  });

  it("falls back to basic content when workbench.enhancedHover is disabled", () => {
    setEditorExtensionEnabled("workbench.enhancedHover", false);
    expect(isEditorExtensionEnabled("workbench.enhancedHover")).toBe(false);

    const model = makeModel("function add(a: number, b: number): number { return a + b; }", "typescript", {
      word: "add",
      column: 10,
    });
    const provider = captureProvider();
    const result = provider.provideHover(model, pos, cancelToken) as Monaco.languages.Hover | null;
    expect(result).not.toBeNull();
    const joined = (result?.contents ?? []).map((c) => c.value).join("\n");
    // Basic content still includes the signature.
    expect(joined).toContain("function add");
  });

  it("uses enhanced content when workbench.enhancedHover is enabled (default)", () => {
    expect(isEditorExtensionEnabled("workbench.enhancedHover")).toBe(true);

    const model = makeModel("function add(a: number, b: number): number { return a + b; }", "typescript", {
      word: "add",
      column: 10,
    });
    const provider = captureProvider();
    const result = provider.provideHover(model, pos, cancelToken) as Monaco.languages.Hover | null;
    expect(result).not.toBeNull();
    const joined = (result?.contents ?? []).map((c) => c.value).join("\n");
    expect(joined).toContain("function add");
  });
});
