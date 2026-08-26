import { describe, expect, it } from "vitest";
import { languageForPath } from "./editorLanguage";

const CASES: ReadonlyArray<readonly [path: string, expected: string]> = [
  // Every mapped extension.
  ["main.ts", "typescript"],
  ["app.tsx", "typescript"],
  ["mod.mts", "typescript"],
  ["mod.cts", "typescript"],
  ["index.js", "javascript"],
  ["component.jsx", "javascript"],
  ["util.mjs", "javascript"],
  ["legacy.cjs", "javascript"],
  ["package.json", "json"],
  ["README.md", "markdown"],
  ["NOTES.markdown", "markdown"],
  ["tool.py", "python"],
  ["lib.rs", "rust"],
  ["Cargo.toml", "toml"],
  ["query.sql", "sql"],
  ["ci.yaml", "yaml"],
  ["ci.yml", "yaml"],
  ["index.html", "html"],
  ["page.htm", "html"],
  ["style.css", "css"],
  ["theme.scss", "scss"],
  ["run.sh", "shell"],
  ["profile.bash", "shell"],
  ["config.xml", "xml"],
  ["main.go", "go"],
  ["App.java", "java"],
  ["alloc.c", "c"],
  ["kernel.cpp", "cpp"],
  ["vector.cc", "cpp"],
  ["lexer.cxx", "cpp"],
  ["types.h", "cpp"],
  ["geometry.hpp", "cpp"],
  // Nested paths keep only the last segment in play.
  ["src/deep/nested/file.ts", "typescript"],
  ["a.b.c/notes.md", "markdown"],
  // Case-insensitive extension match.
  ["FILE.PY", "python"],
  ["ReadMe.MD", "markdown"],
  ["Cargo.TOML", "toml"],
  // Unknown / no extension / dotfiles fall back to plaintext.
  ["archive.tar.gz", "plaintext"],
  ["file.", "plaintext"],
  ["Makefile", "plaintext"],
  [".gitignore", "plaintext"],
  ["", "plaintext"],
  ["C:\\win\\path\\main.rs", "rust"],
];

describe("languageForPath — table-driven extension map", () => {
  it.each(CASES)("%s → %s", (path, expected) => {
    expect(languageForPath(path)).toBe(expected);
  });
});
