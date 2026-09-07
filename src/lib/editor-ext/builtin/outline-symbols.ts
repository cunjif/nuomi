/**
 * Builtin symbol-outline extension: regex-based extraction of functions,
 * classes, structs and markdown headings per language, feeding the editor's
 * side outline panel and the hover-docs extension.
 *
 * The SymbolProvider interface is LSP-shaped on purpose — a tree-sitter or
 * real LSP backend can replace this provider as a same-interface plugin
 * (registerOutline) without any consumer change.
 */
import type { DocumentSymbol, EditorExtContext, EditorFileRef, SymbolProvider } from "../types";

export const EXT_ID = "builtin.outline-symbols";

interface Rule {
  kind: string;
  detail?: string;
  re: RegExp;
}

/** Per-language regex rules. Groups: 1 = name. Languages are Monaco ids. */
const RULES: Record<string, Rule[]> = {
  javascript: [
    { kind: "function", re: /^\s*(?:export\s+)?(?:async\s+)?function\s+(\w+)/ },
    { kind: "class", re: /^\s*(?:export\s+)?class\s+(\w+)/ },
    { kind: "function", re: /^\s*(?:export\s+)?(?:const|let|var)\s+(\w+)\s*=\s*(?:async\s*)?\(/ },
    { kind: "function", re: /^\s*(?:export\s+)?(?:const|let|var)\s+(\w+)\s*=\s*(?:async\s*)?[\w$]+\s*=>/ },
  ],
  typescript: [
    { kind: "function", re: /^\s*(?:export\s+)?(?:async\s+)?function\s+(\w+)/ },
    { kind: "class", re: /^\s*(?:export\s+)?(?:abstract\s+)?class\s+(\w+)/ },
    { kind: "interface", re: /^\s*(?:export\s+)?interface\s+(\w+)/ },
    { kind: "type", re: /^\s*(?:export\s+)?type\s+(\w+)\s*=/ },
    { kind: "function", re: /^\s*(?:export\s+)?(?:const|let|var)\s+(\w+)\s*=\s*(?:async\s*)?\(/ },
    { kind: "function", re: /^\s*(?:export\s+)?(?:const|let|var)\s+(\w+)\s*=\s*(?:async\s*)?[\w$]+\s*=>/ },
  ],
  rust: [
    { kind: "function", re: /^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+(\w+)/ },
    { kind: "struct", re: /^\s*(?:pub(?:\([^)]*\))?\s+)?struct\s+(\w+)/ },
    { kind: "enum", re: /^\s*(?:pub(?:\([^)]*\))?\s+)?enum\s+(\w+)/ },
    { kind: "trait", re: /^\s*(?:pub(?:\([^)]*\))?\s+)?trait\s+(\w+)/ },
    { kind: "impl", re: /^\s*impl(?:<[^>]*>)?\s+(?:\w+\s+for\s+)?(\w+)/ },
  ],
  python: [
    { kind: "function", re: /^\s*(?:async\s+)?def\s+(\w+)/ },
    { kind: "class", re: /^\s*class\s+(\w+)/ },
  ],
  markdown: [{ kind: "heading", re: /^(#{1,6})\s+(.*)$/ }],
};

const FALLBACK_RULES: Rule[] = [
  { kind: "function", re: /^\s*(?:export\s+)?(?:async\s+)?function\s+(\w+)/ },
  { kind: "class", re: /^\s*(?:export\s+)?class\s+(\w+)/ },
  { kind: "function", re: /^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+(\w+)/ },
  { kind: "struct", re: /^\s*(?:pub(?:\([^)]*\))?\s+)?struct\s+(\w+)/ },
  { kind: "function", re: /^\s*def\s+(\w+)/ },
  { kind: "class", re: /^\s*class\s+(\w+)/ },
];

/** Pure extraction — also the unit under test (outline-symbols.test). */
export function extractSymbols(language: string, content: string): DocumentSymbol[] {
  const rules = RULES[language] ?? FALLBACK_RULES;
  const symbols: DocumentSymbol[] = [];
  const lines = content.split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (line === undefined) continue;
    for (const rule of rules) {
      const m = rule.re.exec(line);
      if (!m) continue;
      // Markdown headings capture the level in group 1 and the title in 2;
      // every other rule captures the symbol name in group 1.
      const isHeading = rule.kind === "heading";
      const name = (isHeading ? m[2] ?? "" : m[1] ?? "").trim();
      if (name.length === 0) break;
      symbols.push({
        name,
        kind: rule.kind,
        detail: isHeading ? `h${m[1]?.length ?? 1}` : undefined,
        range: {
          start: { line: i, character: 0 },
          end: { line: i, character: line.length },
        },
        selectionRange: {
          start: { line: i, character: 0 },
          end: { line: i, character: line.length },
        },
      });
      break;
    }
  }
  return symbols;
}

const provider: SymbolProvider = {
  id: "builtin.regex.symbols",
  languages: ["*"],
  provideSymbols(doc: EditorFileRef): DocumentSymbol[] {
    return extractSymbols(doc.language, doc.content);
  },
};

/** Exported for tests: the registered provider is exactly this one. */
export function regexSymbolProvider(): SymbolProvider {
  return provider;
}

export const outlineSymbolsExtension = {
  id: EXT_ID,
  titleI18nKey: "editor.ext.outlineSymbols",
  contribute(ctx: EditorExtContext): void {
    ctx.registerOutline(provider);
    ctx.reportCapability("symbols");
  },
} as const;
