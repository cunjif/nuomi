/**
 * Monaco language id resolution from a file path. Pure and monaco-free so it
 * is trivially unit-testable and safe to call during render (status bar).
 */
const PLAINTEXT = "plaintext";

/** Extension → monaco language id. Lookups are case-insensitive; see languageForPath. */
const EXTENSION_LANGUAGES: Record<string, string> = {
  ts: "typescript",
  tsx: "typescript",
  mts: "typescript",
  cts: "typescript",
  js: "javascript",
  jsx: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  json: "json",
  md: "markdown",
  markdown: "markdown",
  py: "python",
  rs: "rust",
  toml: "toml",
  sql: "sql",
  yaml: "yaml",
  yml: "yaml",
  html: "html",
  htm: "html",
  css: "css",
  scss: "scss",
  sh: "shell",
  bash: "shell",
  xml: "xml",
  go: "go",
  java: "java",
  c: "c",
  cpp: "cpp",
  cc: "cpp",
  cxx: "cpp",
  h: "cpp",
  hpp: "cpp",
};

/** Monaco language id for a path by extension (case-insensitive); unknown/no extension → plaintext. */
export function languageForPath(path: string): string {
  const fileName = path.split(/[\\/]/).pop() ?? "";
  const dot = fileName.lastIndexOf(".");
  // dot <= 0 covers no-extension names and dotfiles like .gitignore.
  if (dot <= 0) return PLAINTEXT;
  return EXTENSION_LANGUAGES[fileName.slice(dot + 1).toLowerCase()] ?? PLAINTEXT;
}
