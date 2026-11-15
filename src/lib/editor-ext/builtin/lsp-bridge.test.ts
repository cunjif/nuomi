import { afterEach, describe, expect, it, vi } from "vitest";
import { injectIpcCommands, resetIpcCommands } from "../../ipc/client";
import { testDoubleCommands, tdState } from "../../ipc/test-double";
import { useUiStore } from "../../store/uiStore";
import {
  applyLspTextEdits,
  ensureLspEditorOpener,
  lspCodeActionsToMonaco,
  lspDocumentSymbolsToMonaco,
  lspUriToRelativePath,
  makeCodeActionProvider,
  makeDefinitionProvider,
  makeDocumentSymbolProvider,
  makeImplementationProvider,
  makeTypeDefinitionProvider,
  parseWorkspaceEdit,
  updateLspStatusCache,
  withTimeout,
} from "./lsp-bridge";
import type * as Monaco from "monaco-editor";

describe("lsp-bridge withTimeout", () => {
  it("returns the resolved value when the promise fulfills first", async () => {
    const result = await withTimeout(Promise.resolve("ok"), 1000);
    expect(result).toBe("ok");
  });

  it("returns null when the promise rejects (LS unavailable → fall back, no throw)", async () => {
    const result = await withTimeout(Promise.reject(new Error("jdtls: program not found")), 1000);
    expect(result).toBeNull();
  });

  it("returns null when the timeout fires before the promise resolves", async () => {
    const result = await withTimeout(new Promise<string>(() => {}), 10);
    expect(result).toBeNull();
  });
});

describe("lsp-bridge document symbol mapping", () => {
  it("maps the hierarchical DocumentSymbol tree with 0-based kind offset", () => {
    const tree = [
      {
        name: "Greeter",
        detail: "class",
        kind: 5, // LSP Class
        range: { start: { line: 0, character: 0 }, end: { line: 9, character: 1 } },
        selectionRange: { start: { line: 0, character: 6 }, end: { line: 0, character: 13 } },
        children: [
          {
            name: "greet",
            kind: 6, // LSP Method
            range: { start: { line: 2, character: 2 }, end: { line: 4, character: 3 } },
            selectionRange: { start: { line: 2, character: 7 }, end: { line: 2, character: 12 } },
          },
        ],
      },
    ];
    const mapped = lspDocumentSymbolsToMonaco(tree);
    expect(mapped).toHaveLength(1);
    const cls = mapped[0];
    expect(cls).toMatchObject({ name: "Greeter", detail: "class", kind: 4 }); // Monaco Class = 4
    expect(cls?.range).toEqual({ startLineNumber: 1, startColumn: 1, endLineNumber: 10, endColumn: 2 });
    const firstChild = cls?.children?.[0];
    expect(cls?.children).toHaveLength(1);
    expect(firstChild).toMatchObject({ name: "greet", kind: 5 }); // Monaco Method = 5
  });

  it("flattens SymbolInformation entries into top-level symbols", () => {
    const flat = [
      {
        name: "helper",
        kind: 12, // LSP Function
        location: {
          uri: "file:///x.ts",
          range: { start: { line: 3, character: 0 }, end: { line: 3, character: 10 } },
        },
      },
    ];
    const mapped = lspDocumentSymbolsToMonaco(flat);
    expect(mapped).toHaveLength(1);
    expect(mapped[0]).toMatchObject({
      name: "helper",
      kind: 11, // Monaco Function
      range: { startLineNumber: 4, startColumn: 1, endLineNumber: 4, endColumn: 11 },
    });
    expect(mapped[0]?.children).toHaveLength(0);
  });

  it("drops malformed entries instead of throwing", () => {
    const mapped = lspDocumentSymbolsToMonaco([
      null,
      { kind: 5 }, // no name
      { name: "x", selectionRange: {}, range: null }, // missing ranges
      "noise",
    ]);
    expect(mapped).toHaveLength(0);
  });
});

describe("lsp-bridge provider fast-fail (status cache)", () => {
  afterEach(() => {
    resetIpcCommands();
    updateLspStatusCache({});
    updateLspStatusCache({ typescript: "not_started" });
  });

  function fakeModel(content = ""): Monaco.editor.ITextModel {
    return {
      uri: { toString: () => "file:///C:/ws/a.ts" } as Monaco.Uri,
      getLanguageId: () => "typescript",
      getValue: () => content,
    } as unknown as Monaco.editor.ITextModel;
  }

  const noToken = {} as Monaco.CancellationToken;

  it("returns undefined from the definition provider without an IPC call when failed", async () => {
    updateLspStatusCache({ typescript: "failed" });
    const base = testDoubleCommands();
    const lspDefinition = vi.fn(base.lspDefinition);
    injectIpcCommands({ ...base, lspDefinition });
    const provider = makeDefinitionProvider({} as typeof Monaco, "typescript");
    const result = await provider.provideDefinition(fakeModel(), { lineNumber: 1, column: 1 } as Monaco.Position, noToken);
    expect(result).toBeUndefined();
    expect(lspDefinition).not.toHaveBeenCalled();
  });

  it("falls back to regex outline symbols when the LS is failed (quick outline stays alive)", async () => {
    updateLspStatusCache({ typescript: "disabled" });
    const base = testDoubleCommands();
    const lspDocumentSymbols = vi.fn(base.lspDocumentSymbols);
    injectIpcCommands({ ...base, lspDocumentSymbols });
    const provider = makeDocumentSymbolProvider("typescript");
    const result = await provider.provideDocumentSymbols(fakeModel("function greet() {}\n"), noToken);
    expect(lspDocumentSymbols).not.toHaveBeenCalled();
    expect(result?.map((s: Monaco.languages.DocumentSymbol) => s.name)).toContain("greet");
  });

  it("maps LSP symbols and falls back to regex when the LSP result is empty", async () => {
    tdState.workspaceRoot = "C:/ws";
    tdState.workspaceConfigured = true;
    const base = testDoubleCommands();
    const lspDocumentSymbols = vi.fn(base.lspDocumentSymbols);
    injectIpcCommands({
      ...base,
      lspDocumentSymbols: async (...args: Parameters<typeof base.lspDocumentSymbols>) => {
        await lspDocumentSymbols(...args);
        return {
          status: "ok" as const,
          data: [{ name: "LspSym", kind: 5, range: { start: { line: 0, character: 0 }, end: { line: 1, character: 0 } }, selectionRange: { start: { line: 0, character: 0 }, end: { line: 0, character: 6 } } }],
        };
      },
    });
    const provider = makeDocumentSymbolProvider("typescript");
    const result = await provider.provideDocumentSymbols(fakeModel(""), noToken);
    expect(lspDocumentSymbols).toHaveBeenCalledOnce();
    expect(result?.map((s: Monaco.languages.DocumentSymbol) => s.name)).toContain("LspSym");

    // Empty LSP result → regex outline fallback (never silent).
    injectIpcCommands({
      ...base,
      lspDocumentSymbols: async () => ({ status: "ok" as const, data: [] }),
    });
    const fallback = await makeDocumentSymbolProvider("typescript").provideDocumentSymbols(
      fakeModel("function greet() {}\n"),
      noToken,
    );
    expect(fallback?.map((s: Monaco.languages.DocumentSymbol) => s.name)).toContain("greet");
  });
});

describe("lsp-bridge rename plumbing (ADR-0031 M2)", () => {
  const range = (sl: number, sc: number, el: number, ec: number) => ({
    start: { line: sl, character: sc },
    end: { line: el, character: ec },
  });

  describe("applyLspTextEdits", () => {
    it("applies multiple edits from the end backwards without shift drift", () => {
      const content = "const alpha = 1;\nconst beta = 2;\nconst gamma = 3;\n";
      const next = applyLspTextEdits(content, [
        { range: range(0, 6, 0, 11), newText: "delta" },
        { range: range(2, 6, 2, 11), newText: "epsilon" },
      ]);
      expect(next).toBe("const delta = 1;\nconst beta = 2;\nconst epsilon = 3;\n");
    });

    it("handles multi-line insertions and deletions", () => {
      const content = "a\nb\nc";
      // Replace "a\nb" (lines 0-1 through the newline) with a single line.
      const next = applyLspTextEdits(content, [{ range: range(0, 0, 1, 1), newText: "ab" }]);
      expect(next).toBe("ab\nc");
      const inserted = applyLspTextEdits("ab", [{ range: range(0, 1, 0, 1), newText: "X\nY" }]);
      expect(inserted).toBe("aX\nYb");
    });

    it("works on CRLF content (offsets computed on the raw string)", () => {
      const content = "one\r\ntwo\r\n";
      const next = applyLspTextEdits(content, [{ range: range(1, 0, 1, 3), newText: "TWO" }]);
      expect(next).toBe("one\r\nTWO\r\n");
    });

    it("clamps out-of-range positions instead of throwing", () => {
      const next = applyLspTextEdits("hi", [{ range: range(99, 99, 99, 99), newText: "!" }]);
      expect(next).toBe("hi!");
    });
  });

  describe("parseWorkspaceEdit", () => {
    it("parses the changes map shape", () => {
      const we = {
        changes: {
          "file:///C:/ws/a.ts": [{ range: range(0, 0, 0, 3), newText: "new" }],
        },
      };
      expect(parseWorkspaceEdit(we)).toEqual([
        { uri: "file:///C:/ws/a.ts", edits: [{ range: range(0, 0, 0, 3), newText: "new" }] },
      ]);
    });

    it("parses the documentChanges shape and skips create/rename kinds", () => {
      const we = {
        documentChanges: [
          { textDocument: { uri: "file:///C:/ws/b.ts", version: 3 }, edits: [{ range: range(1, 0, 1, 2), newText: "x" }] },
          { kind: "create", uri: "file:///C:/ws/c.ts" },
        ],
      };
      expect(parseWorkspaceEdit(we)).toEqual([
        { uri: "file:///C:/ws/b.ts", edits: [{ range: range(1, 0, 1, 2), newText: "x" }] },
      ]);
    });

    it("drops malformed entries and non-object payloads", () => {
      expect(parseWorkspaceEdit(null)).toEqual([]);
      expect(parseWorkspaceEdit({ changes: { "file:///x": [null, { range: null }] } })).toEqual([]);
    });
  });

  describe("lspUriToRelativePath", () => {
    it("maps in-root paths case-insensitively (Windows)", () => {
      expect(lspUriToRelativePath("C:/Ws/sub/b.ts", "C:/ws")).toBe("sub/b.ts");
    });

    it("returns null outside the root and on empty roots", () => {
      expect(lspUriToRelativePath("D:/elsewhere/x.ts", "C:/ws")).toBeNull();
      expect(lspUriToRelativePath("C:/ws/a.ts", "")).toBeNull();
    });
  });
});

describe("lsp-bridge M3 — typeDefinition / implementation / codeAction", () => {
  afterEach(() => {
    resetIpcCommands();
    updateLspStatusCache({});
    updateLspStatusCache({ typescript: "not_started" });
  });

  const noopUriFactory = (uri: string) => ({ fsPath: uri } as unknown as Monaco.Uri);
  const noToken = {} as Monaco.CancellationToken;

  function fakeModel(content = ""): Monaco.editor.ITextModel {
    return {
      uri: { toString: () => "file:///C:/ws/a.ts" } as Monaco.Uri,
      getLanguageId: () => "typescript",
      getValue: () => content,
    } as unknown as Monaco.editor.ITextModel;
  }

  describe("lspCodeActionsToMonaco", () => {
    it("converts edit-bearing actions with the kind passthrough and flat edits", () => {
      const actions = lspCodeActionsToMonaco(
        [
          {
            title: "Add missing import",
            kind: "quickfix",
            edit: {
              changes: {
                "file:///C:/ws/a.ts": [{ range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } }, newText: "import x;\n" }],
              },
            },
          },
        ],
        noopUriFactory,
      );
      expect(actions).toHaveLength(1);
      expect(actions[0]).toMatchObject({ title: "Add missing import", kind: "quickfix" });
      expect(actions[0]?.edit?.edits).toHaveLength(1);
    });

    it("merges documentChanges files into one flat edit list", () => {
      const actions = lspCodeActionsToMonaco(
        [
          {
            title: "Refactor",
            kind: "refactor.extract",
            edit: {
              documentChanges: [
                { textDocument: { uri: "file:///a" }, edits: [{ range: { start: { line: 1, character: 0 }, end: { line: 1, character: 2 } }, newText: "y" }] },
                { textDocument: { uri: "file:///b" }, edits: [{ range: { start: { line: 2, character: 0 }, end: { line: 2, character: 2 } }, newText: "z" }] },
              ],
            },
          },
        ],
        noopUriFactory,
      );
      expect(actions).toHaveLength(1);
      expect(actions[0]?.edit?.edits).toHaveLength(2);
    });

    it("skips command-only and edit-less actions", () => {
      const actions = lspCodeActionsToMonaco(
        [
          { title: "Run command", command: { title: "run", command: "x.run" } },
          { title: "No edit here" },
          { title: "Empty edit", edit: { changes: {} } },
        ],
        noopUriFactory,
      );
      expect(actions).toHaveLength(0);
    });

    it("drops malformed entries", () => {
      expect(lspCodeActionsToMonaco([null, "noise", { edit: { changes: { "file:///x": [null] } } }], noopUriFactory)).toHaveLength(0);
    });
  });

  it("fails fast without IPC when the LS is failed (all three providers)", async () => {
    updateLspStatusCache({ typescript: "failed" });
    const base = testDoubleCommands();
    const spies = {
      lspTypeDefinition: vi.fn(base.lspTypeDefinition),
      lspImplementation: vi.fn(base.lspImplementation),
      lspCodeAction: vi.fn(base.lspCodeAction),
    };
    injectIpcCommands({ ...base, ...spies });
    const fakeMonaco = {
      editor: { getModelMarkers: () => [] },
    } as unknown as typeof Monaco;
    const position = { lineNumber: 1, column: 1 } as Monaco.Position;
    const modelRange = { startLineNumber: 1, startColumn: 1, endLineNumber: 1, endColumn: 5 } as unknown as Monaco.Range;
    expect(await makeTypeDefinitionProvider(fakeMonaco, "typescript").provideTypeDefinition(fakeModel(), position, noToken)).toBeUndefined();
    expect(await makeImplementationProvider(fakeMonaco, "typescript").provideImplementation(fakeModel(), position, noToken)).toBeUndefined();
    const actions = await makeCodeActionProvider(fakeMonaco, "typescript").provideCodeActions(fakeModel(), modelRange, { only: undefined, trigger: 0 } as never, noToken);
    expect(actions ?? null).toBeNull();
    expect(spies.lspTypeDefinition).not.toHaveBeenCalled();
    expect(spies.lspImplementation).not.toHaveBeenCalled();
    expect(spies.lspCodeAction).not.toHaveBeenCalled();
  });

  it("maps markers into the codeAction context diagnostics and returns converted actions", async () => {
    tdState.workspaceRoot = "C:/ws";
    tdState.workspaceConfigured = true;
    const base = testDoubleCommands();
    const lspCodeAction = vi.fn(base.lspCodeAction);
    injectIpcCommands({
      ...base,
      lspCodeAction: async (...args: Parameters<typeof base.lspCodeAction>) => {
        await lspCodeAction(...args);
        return {
          status: "ok" as const,
          data: [
            {
              title: "Fix typo",
              kind: "quickfix",
              edit: { changes: { "file:///C:/ws/a.ts": [{ range: { start: { line: 0, character: 0 }, end: { line: 0, character: 3 } }, newText: "fixed" }] } },
            },
          ],
        };
      },
    });
    const fakeMonaco = {
      editor: {
        getModelMarkers: () => [
          {
            startLineNumber: 1,
            startColumn: 1,
            endLineNumber: 1,
            endColumn: 4,
            message: "cannot find name",
            severity: 8,
            source: "ts",
          },
        ],
      },
      Uri: { parse: (u: string) => ({ fsPath: u }) },
    } as unknown as typeof Monaco;
    const modelRange = { startLineNumber: 1, startColumn: 1, endLineNumber: 1, endColumn: 4 } as unknown as Monaco.Range;
    const result = await makeCodeActionProvider(fakeMonaco, "typescript").provideCodeActions(
      fakeModel(),
      modelRange,
      { only: undefined, trigger: 0 } as never,
      noToken,
    );
    const list = result as { actions: Array<{ title: string; edit: unknown }>; dispose: () => void };
    expect(list.actions).toHaveLength(1);
    expect(list.actions[0]?.title).toBe("Fix typo");
    expect(lspCodeAction).toHaveBeenCalledWith(
      "typescript",
      "file:///C:/ws",
      "file:///C:/ws/a.ts",
      0,
      0,
      expect.arrayContaining([expect.objectContaining({ message: "cannot find name", severity: 1 })]),
    );
  });
});

describe("lsp-bridge editor opener (ADR-0031)", () => {
  afterEach(() => {
    resetIpcCommands();
  });

  it("maps file:// locations inside the workspace root onto requestOpenSymbol", async () => {
    tdState.workspaceRoot = "C:/ws";
    tdState.workspaceConfigured = true;
    useUiStore.setState({ jumpBackStack: [], jumpForwardStack: [], activeWorkspaceId: "ws-a" });
    injectIpcCommands(testDoubleCommands());
    const openers: Array<{ openCodeEditor: (src: unknown, resource: unknown, sel: unknown) => boolean | Promise<boolean> }> = [];
    ensureLspEditorOpener({
      editor: {
        registerEditorOpener: (o: (typeof openers)[number]) => {
          openers.push(o);
        },
      },
    } as unknown as typeof Monaco);
    expect(openers).toHaveLength(1); // idempotent across editor mounts
    const handler = openers[0];
    if (handler === undefined) throw new Error("opener not registered");

    // In-root location → jump recorded (pendingJump + openFile, not yet open).
    const handled = await handler.openCodeEditor(
      null,
      { scheme: "file", fsPath: "C:/ws/sub/b.ts" },
      { startLineNumber: 7, startColumn: 3 },
    );
    expect(handled).toBe(true);
    expect(useUiStore.getState().editorByWorkspace["ws-a"]?.openFiles).toContain("sub/b.ts");

    // Outside the workspace root → not ours, let other openers try.
    expect(
      await handler.openCodeEditor(null, { scheme: "file", fsPath: "D:/elsewhere/x.ts" }, { startLineNumber: 1 }),
    ).toBe(false);
    // Non-file scheme → passthrough.
    expect(await handler.openCodeEditor(null, { scheme: "https", fsPath: "x" }, undefined)).toBe(false);
  });
});
