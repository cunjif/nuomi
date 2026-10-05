/**
 * Workspace code index (需求重构 2): an automatic, in-app code index that
 * powers VSCode-style navigation — go-to-definition, find-references and
 * hover info. The workspace is walked over the existing listDir/readFile
 * IPC (no Agent, no external MCP); symbols come from the registered
 * SymbolProviders (outline-symbols regexes today, tree-sitter/LSP tomorrow)
 * and references from a word scan.
 *
 * Reactivity mirrors the editor-ext registry: a version counter + subscribe
 * API, so consumers re-render when a rebuild lands.
 */
import { ipc } from "../../ipc/client";
import { languageForPath } from "../../../features/shell/editorLanguage";
import { useUiStore } from "../../store/uiStore";
import { getOutlineProviders } from "../registry";
import type { DocumentSymbol, SymbolProvider } from "../types";

export interface IndexedSymbol {
  name: string;
  kind: string;
  /** Workspace-relative file path. */
  path: string;
  /** 0-based declaration line. */
  line: number;
  /** Trimmed declaration line (the hover signature). */
  signature: string;
  /** Doc comment attached above the declaration (注释), if any. */
  doc: string | null;
  /** Enclosing class-like symbol name, if any ("Declared in X"). */
  container: string | null;
}

export interface ReferenceHit {
  path: string;
  line: number;
  /** Trimmed line snippet (≤120 chars) for reference-list display. */
  text: string;
}

export interface WorkspaceIndexData {
  definitions: Map<string, IndexedSymbol[]>;
  references: Map<string, ReferenceHit[]>;
  fileCount: number;
  builtAt: number;
}

export type WorkspaceIndexStatus = "idle" | "indexing" | "ready" | "error";

const SKIP_DIRS = new Set([
  "node_modules",
  ".git",
  "target",
  "dist",
  "build",
  "coverage",
  ".next",
  ".vite",
  "__pycache__",
]);
const MAX_FILES = 3000;
const MAX_FILE_CHARS = 400_000;
const MAX_REFS_PER_WORD = 1000;
const WORD_RE = /[A-Za-z_$][\w$]*/g;
const READ_CONCURRENCY = 16;

export interface IndexedFile {
  path: string;
  language: string;
  content: string;
}

// --- pure builders (unit-tested) ---------------------------------------------

/**
 * Doc comment attached to the symbol at `symbolLine`: either a contiguous
 * `//`/`///`/`#` comment block directly above, or a `/** ... *​/` block.
 */
export function extractDocComment(lines: string[], symbolLine: number): string | null {
  let i = symbolLine - 1;
  if (i < 0) return null;
  const above = (lines[i] ?? "").trim();
  if (above.endsWith("*/")) {
    const parts: string[] = [];
    if (/^\/\*+/.test(above)) return stripBlockComment(above) || null;
    while (i >= 0 && symbolLine - i <= 40) {
      const l = (lines[i] ?? "").trim();
      parts.unshift(stripBlockComment(l));
      if (/^\/\*+/.test(l)) return parts.join("\n").trim() || null;
      i -= 1;
    }
    return parts.join("\n").trim() || null;
  }
  const parts: string[] = [];
  while (i >= 0 && symbolLine - i <= 40) {
    const l = lines[i] ?? "";
    const m = /^\s*(?:\/\/+|#(?!\[))\s?(.*)$/.exec(l);
    if (m === null) break;
    parts.unshift(m[1] ?? "");
    i -= 1;
  }
  const doc = parts.join("\n").trim();
  return doc === "" ? null : doc;
}

function stripBlockComment(line: string): string {
  return line
    .replace(/\*\/\s*$/, "")
    .replace(/^\/?\*+\s?/, "")
    .trim();
}

const CONTAINER_KINDS = new Set(["class", "struct", "interface", "trait", "enum", "impl"]);

/** Build the definition/reference maps from a set of in-memory files. */
export function buildIndexFromFiles(files: IndexedFile[], providers: SymbolProvider[]): WorkspaceIndexData {
  const definitions = new Map<string, IndexedSymbol[]>();
  const references = new Map<string, ReferenceHit[]>();
  const pushDefinition = (entry: IndexedSymbol): void => {
    const arr = definitions.get(entry.name);
    if (arr === undefined) definitions.set(entry.name, [entry]);
    else arr.push(entry);
  };
  const pushReference = (hit: ReferenceHit, word: string): void => {
    const arr = references.get(word);
    if (arr === undefined) references.set(word, [hit]);
    else if (arr.length < MAX_REFS_PER_WORD) arr.push(hit);
  };

  for (const file of files) {
    const lines = file.content.split(/\r?\n/);
    const active = providers.filter((p) => p.languages.includes("*") || p.languages.includes(file.language));
    const symbols = active.flatMap((p) =>
      p.provideSymbols({ path: file.path, language: file.language, content: file.content }),
    );
    const codeSymbols = symbols.filter((s) => s.kind !== "heading");
    const containers = codeSymbols.filter((s) => CONTAINER_KINDS.has(s.kind));
    for (const sym of codeSymbols) {
      const line = sym.range.start.line;
      const nearest = containers.reduce<DocumentSymbol | null>((best, c) => {
        if (c.range.start.line >= line || c.name === sym.name) return best;
        if (best === null || c.range.start.line > best.range.start.line) return c;
        return best;
      }, null);
      pushDefinition({
        name: sym.name,
        kind: sym.kind,
        path: file.path,
        line,
        signature: (lines[line] ?? "").trim(),
        doc: extractDocComment(lines, line),
        container: nearest?.name ?? null,
      });
    }
    // Reference scan: every identifier occurrence, line-addressed with a
    // trimmed snippet for list display.
    for (let ln = 0; ln < lines.length; ln += 1) {
      const line = lines[ln] ?? "";
      for (const m of line.matchAll(WORD_RE)) {
        const word = m[0];
        if (word.length < 2) continue;
        pushReference({ path: file.path, line: ln, text: line.trim().slice(0, 120) }, word);
      }
    }
  }
  return { definitions, references, fileCount: files.length, builtAt: Date.now() };
}

// --- reactive store ------------------------------------------------------------

let status: WorkspaceIndexStatus = "idle";
let data: WorkspaceIndexData | null = null;
const listeners = new Set<() => void>();
let version = 0;
let rerunPending = false;

function notify(): void {
  version += 1;
  listeners.forEach((l) => l());
}

export function subscribeWorkspaceIndex(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function getWorkspaceIndexVersion(): number {
  return version;
}

export function getWorkspaceIndex(): { status: WorkspaceIndexStatus; data: WorkspaceIndexData | null } {
  return { status, data };
}

/**
 * Walk the workspace over listDir IPC and return every file path (dirs in
 * SKIP_DIRS excluded, MAX_FILES cap). Shared by the indexer (which filters
 * plaintext before reading) and the Ctrl+P quick-open palette (which shows
 * everything).
 */
export async function listWorkspaceFiles(): Promise<string[]> {
  const paths: string[] = [];
  const queue: string[] = [""];
  while (queue.length > 0 && paths.length < MAX_FILES) {
    const dir = queue.shift() ?? "";
    let entries;
    try {
      entries = await ipc.listDir(dir);
    } catch {
      continue; // unreadable dir — skip
    }
    for (const entry of entries) {
      const path = dir === "" ? entry.name : `${dir}/${entry.name}`;
      if (entry.isDir) {
        if (!SKIP_DIRS.has(entry.name)) queue.push(path);
        continue;
      }
      if (paths.length >= MAX_FILES) break;
      paths.push(path);
    }
  }
  return paths;
}

/**
 * Rebuild the whole index from the workspace over IPC. Safe to call
 * concurrently: a call landing while a build runs schedules exactly one
 * rerun afterwards.
 */
export async function reindexWorkspace(): Promise<void> {
  if (status === "indexing") {
    rerunPending = true;
    return;
  }
  status = "indexing";
  notify();
  try {
    const files: IndexedFile[] = (await listWorkspaceFiles())
      .map((path) => ({ path, language: languageForPath(path), content: "" }))
      .filter((f) => f.language !== "plaintext");
    // Bounded-concurrency reads.
    let cursor = 0;
    const readNext = async (): Promise<void> => {
      while (cursor < files.length) {
        const file = files[cursor];
        cursor += 1;
        if (file === undefined) return;
        try {
          const content = await ipc.readFile(file.path);
          if (content.length <= MAX_FILE_CHARS) file.content = content;
        } catch {
          // Unreadable/binary file — drop it.
        }
      }
    };
    await Promise.all(Array.from({ length: READ_CONCURRENCY }, () => readNext()));
    const readable = files.filter((f) => f.content !== "");
    data = buildIndexFromFiles(readable, getOutlineProviders());
    status = "ready";
  } catch {
    status = "error";
  }
  notify();
  if (rerunPending) {
    rerunPending = false;
    void reindexWorkspace();
  }
}

/** Incremental refresh after an in-app save: recompute just this file. */
export function updateFileIndex(path: string, content: string): void {
  if (data === null) return;
  for (const [name, arr] of data.definitions) {
    const filtered = arr.filter((d) => d.path !== path);
    if (filtered.length === 0) data.definitions.delete(name);
    else data.definitions.set(name, filtered);
  }
  for (const [word, arr] of data.references) {
    const filtered = arr.filter((r) => r.path !== path);
    if (filtered.length === 0) data.references.delete(word);
    else data.references.set(word, filtered);
  }
  const fresh = buildIndexFromFiles([{ path, language: languageForPath(path), content }], getOutlineProviders());
  for (const [name, arr] of fresh.definitions) {
    const existing = data.definitions.get(name);
    data.definitions.set(name, existing === undefined ? arr : [...existing, ...arr]);
  }
  for (const [word, arr] of fresh.references) {
    const existing = data.references.get(word);
    data.references.set(word, existing === undefined ? arr : [...existing, ...arr]);
  }
  data.fileCount = Math.max(data.fileCount, 1);
  data.builtAt = Date.now();
  notify();
}

// --- cross-file jump + find plumbing --------------------------------------------

let activeEditor: {
  reveal: (line: number) => void;
  path: string | null;
  find?: () => void;
} | null = null;
let pendingJump: { path: string; line: number } | null = null;

/** MonacoTab registers the live editor so jumps land without remounts. */
export function registerActiveEditor(
  path: string | null,
  reveal: (line: number) => void,
  actions?: { find(): void },
): void {
  activeEditor = { path, reveal, find: actions?.find };
}

/** Open a workspace file at a 0-based line (go-to-definition / references). */
export function requestOpenSymbol(path: string, line: number): void {
  if (activeEditor !== null && activeEditor.path === path) {
    activeEditor.reveal(line);
    return;
  }
  pendingJump = { path, line };
  const s = useUiStore.getState();
  if (s.activeWorkspaceId !== null) s.openFile(path, s.activeWorkspaceId);
}

/** MonacoTab consumes the jump targeted at its file once mounted. */
export function consumePendingJump(path: string): number | null {
  if (pendingJump !== null && pendingJump.path === path) {
    const line = pendingJump.line;
    pendingJump = null;
    return line;
  }
  return null;
}

/**
 * Global Ctrl+F: run the find-in-file action of the live Monaco editor.
 * Returns false when no editor surface registered one (browser default).
 */
export function triggerActiveEditorFind(): boolean {
  const find = activeEditor?.find;
  if (find === undefined) return false;
  find();
  return true;
}
