/**
 * Workspace code index tests: pure builders (doc extraction, definition/
 * reference maps, container resolution) plus an IPC-driven reindex and the
 * incremental save-path update, with injected listDir/readFile doubles.
 */
import { beforeEach, describe, expect, it } from "vitest";
import { injectIpcCommands, resetIpcCommands } from "../../ipc/client";
import { tdSeedFiles, testDoubleCommands } from "../../ipc/test-double";
import {
  ensureEditorExtensionsActivated,
  getOutlineProviders,
  registerEditorExtension,
  resetEditorExtensionsForTest,
} from "../registry";
import { regexSymbolProvider } from "../builtin/outline-symbols";
import type { EditorExtension } from "../types";
import {
  buildIndexFromFiles,
  extractDocComment,
  getWorkspaceIndex,
  reindexWorkspace,
  updateFileIndex,
  type IndexedFile,
} from "./workspace-index";

function registerRealProvider(): void {
  const ext: EditorExtension = {
    id: "test.symbols-ext",
    titleI18nKey: "test",
    contribute: (ctx) => ctx.registerOutline(regexSymbolProvider()),
  };
  registerEditorExtension(ext);
  ensureEditorExtensionsActivated();
}

describe("workspace-index — extractDocComment", () => {
  it("collects contiguous line comments above the symbol", () => {
    const lines = ["// greets the user", "// by name", "function greet() {}"];
    expect(extractDocComment(lines, 2)).toBe("greets the user\nby name");
  });

  it("collects multi-line and single-line block comments", () => {
    const block = ["/**", " * Parses roots.", " * Tolerates noise.", " */", "fn parse() {}"];
    expect(extractDocComment(block, 4)).toBe("Parses roots.\nTolerates noise.");
    const oneLine = ["/** 仅用于测试 */", "fn parse() {}"];
    expect(extractDocComment(oneLine, 1)).toBe("仅用于测试");
  });

  it("ignores rust attributes and bare declarations", () => {
    expect(extractDocComment(["#[derive(Debug)]", "struct A;"], 1)).toBeNull();
    expect(extractDocComment(["let x = 1;"], 0)).toBeNull();
  });
});

describe("workspace-index — buildIndexFromFiles", () => {
  const files: IndexedFile[] = [
    {
      path: "a.ts",
      language: "typescript",
      content: ["/** Greets */", "function greet() {}", "const x = 1;", "greet();"].join("\n"),
    },
    { path: "b.ts", language: "typescript", content: "greet();\n" },
    {
      path: "c.rs",
      language: "rust",
      content: ["struct Foo;", "impl Foo {", "  fn new() {}", "}"].join("\n"),
    },
  ];

  it("indexes definitions with signature, doc and container", () => {
    const index = buildIndexFromFiles(files, [regexSymbolProvider()]);
    const greet = index.definitions.get("greet");
    expect(greet).toHaveLength(1);
    expect(greet?.[0]).toMatchObject({
      name: "greet",
      kind: "function",
      path: "a.ts",
      line: 1,
      signature: "function greet() {}",
      doc: "Greets",
      container: null,
    });
    expect(index.definitions.get("new")?.[0]).toMatchObject({ path: "c.rs", line: 2, container: "Foo" });
  });

  it("scans references across files with line addresses and snippets", () => {
    const index = buildIndexFromFiles(files, [regexSymbolProvider()]);
    // The declaration line itself counts as an occurrence; each hit carries
    // a trimmed snippet for the navigation panel.
    expect(index.references.get("greet")).toEqual([
      { path: "a.ts", line: 1, text: "function greet() {}" },
      { path: "a.ts", line: 3, text: "greet();" },
      { path: "b.ts", line: 0, text: "greet();" },
    ]);
    expect(index.fileCount).toBe(3);
  });

  it("indexes java definitions with javadoc so hover shows the doc, not just refs", () => {
    const java: IndexedFile[] = [
      {
        path: "src/Greeter.java",
        language: "java",
        content: [
          "/**",
          " * Greets people warmly.",
          " */",
          "public class Greeter {",
          "    /**",
          "     * Builds a hello line.",
          "     */",
          "    public String greet(String name) {",
          "        return \"hello \" + name;",
          "    }",
          "}",
        ].join("\n"),
      },
    ];
    const index = buildIndexFromFiles(java, [regexSymbolProvider()]);
    expect(index.definitions.get("Greeter")?.[0]).toMatchObject({ kind: "class", doc: "Greets people warmly." });
    expect(index.definitions.get("greet")?.[0]).toMatchObject({
      kind: "method",
      container: "Greeter",
      doc: "Builds a hello line.",
    });
    // The hover can render a doc section even when the method has no refs
    // beyond its declaration line — the ref count is a footer, not the body.
    expect(index.definitions.get("greet")?.[0]?.doc).not.toBeNull();
  });
});

describe("workspace-index — reindexWorkspace over IPC", () => {
  beforeEach(() => {
    resetEditorExtensionsForTest();
    resetIpcCommands();
    registerRealProvider();
  });

  it("walks the workspace (skipping ignored dirs) and lands a ready index", async () => {
    injectIpcCommands(testDoubleCommands());
    tdSeedFiles({
      "a.ts": "/** Greets */\nfunction greet() {}\n",
      "sub/b.ts": "greet();\n",
      "node_modules/pkg/index.js": "junk();\n",
    });

    await reindexWorkspace();

    const { status, data } = getWorkspaceIndex();
    expect(status).toBe("ready");
    expect(data?.definitions.get("greet")?.map((d) => d.path)).toEqual(["a.ts"]);
    expect(data?.references.get("greet")?.map((r) => r.path)).toEqual(["a.ts", "sub/b.ts"]);
    expect(data?.fileCount).toBe(2);
    expect(getOutlineProviders().length).toBeGreaterThan(0);

    // Saving a.ts without greet drops its declaration but keeps b.ts's use.
    updateFileIndex("a.ts", "const x = 1;\n");
    expect(getWorkspaceIndex().data?.definitions.has("greet")).toBe(false);
    expect(getWorkspaceIndex().data?.references.get("greet")).toEqual([
      { path: "sub/b.ts", line: 0, text: "greet();" },
    ]);
  });
});
