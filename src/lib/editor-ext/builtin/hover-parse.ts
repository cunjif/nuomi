/**
 * Pure parsers for enhanced hover content (workbench_enhance spec 5.3 /
 * design 2.2.2). Extract structured parameter + return-value info from
 * a declaration signature line and its doc comment, across the 8 builtin
 * languages. Then composeHoverContent assembles the per-kind Markdown
 * sequence for Monaco's hover widget.
 */
import type * as Monaco from "monaco-editor";

export interface ParameterInfo {
  name: string;
  type: string;
  doc: string | null;
}

export interface ReturnInfo {
  type: string;
  doc: string | null;
}

export interface HoverContentInput {
  kind: string;
  signature: string;
  doc: string | null;
  params: ParameterInfo[];
  returnValue: ReturnInfo | null;
  location: string | null;
  container: string | null;
  refCount: number;
  language: string;
}

/** Extract the text inside the first (...) pair of `signature`. */
function extractParamBlock(signature: string): string {
  const open = signature.indexOf("(");
  if (open === -1) return "";
  let depth = 0;
  for (let i = open; i < signature.length; i++) {
    const ch = signature[i];
    if (ch === "(") depth++;
    else if (ch === ")") {
      depth--;
      if (depth === 0) return signature.slice(open + 1, i);
    }
  }
  return "";
}

/** Split a parameter block on top-level commas (depth 0 for <> and []). */
function splitParams(block: string): string[] {
  if (block.trim() === "") return [];
  const parts: string[] = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i < block.length; i++) {
    const ch = block[i];
    if (ch === "<" || ch === "[" || ch === "(") depth++;
    else if (ch === ">" || ch === "]" || ch === ")") depth--;
    else if (ch === "," && depth === 0) {
      parts.push(block.slice(start, i).trim());
      start = i + 1;
    }
  }
  parts.push(block.slice(start).trim());
  return parts.filter((p) => p !== "");
}

/** Parse `@param name doc` / `@param {type} name doc` lines from a doc comment. */
function parseParamDocs(doc: string | null): Map<string, string> {
  const map = new Map<string, string>();
  if (doc === null) return map;
  for (const line of doc.split(/\r?\n/)) {
    const m = /^\s*@param\s+(?:\{[^}]*\}\s+)?(\w+)\s+(.*)$/.exec(line);
    if (m !== null) map.set(m[1] ?? "", (m[2] ?? "").trim());
  }
  return map;
}

/** Parse `@returns` / `@return` line from a doc comment. */
function parseReturnDoc(doc: string | null): string | null {
  if (doc === null) return null;
  for (const line of doc.split(/\r?\n/)) {
    const m = /^\s*@returns?\s+(.*)$/.exec(line);
    if (m !== null) return (m[1] ?? "").trim();
  }
  return null;
}

/**
 * Extract structured parameter info from a declaration signature + doc
 * comment. Covers TS/JS/Rust/Python/Java/Go/C/C++ (spec 5.3.1-3).
 */
export function parseParameters(signature: string, doc: string | null, language: string): ParameterInfo[] {
  const block = extractParamBlock(signature);
  const rawParts = splitParams(block);
  if (rawParts.length === 0) return [];
  const paramDocs = parseParamDocs(doc);

  return rawParts.map((part) => {
    const docEntry = paramDocs.get(part.split(/[\s:]/)[0] ?? "");
    return parseSingleParam(part, language, docEntry ?? null);
  });
}

function parseSingleParam(part: string, language: string, doc: string | null): ParameterInfo {
  // Go: `name type` (e.g. `ctx context.Context`)
  if (language === "go") {
    const m = /^(\w+)\s+(.+)$/.exec(part);
    if (m !== null) return { name: m[1] ?? "", type: (m[2] ?? "").trim(), doc };
  }
  // TS/JS/Rust/Python/Java/C/C++: `name: type` or `name: type` or `type name`
  const colonIdx = part.indexOf(":");
  if (colonIdx !== -1) {
    const name = part.slice(0, colonIdx).trim();
    const type = part.slice(colonIdx + 1).trim();
    return { name, type, doc };
  }
  // C/Java style: `type name`
  const tokens = part.split(/\s+/);
  if (tokens.length >= 2) {
    const name = tokens[tokens.length - 1] ?? "";
    const type = tokens.slice(0, -1).join(" ");
    return { name, type, doc };
  }
  return { name: part, type: "", doc };
}

/**
 * Extract the return type + doc from a declaration signature + doc
 * comment. Returns null when the signature has no return type (spec 5.3.1-3).
 */
export function parseReturnValue(signature: string, doc: string | null, language: string): ReturnInfo | null {
  const returnDoc = parseReturnDoc(doc);

  // Rust / Python: `-> Type`
  if (language === "rust" || language === "python") {
    const m = /->\s*(.+?)(?:\s*[\{:]|\s*$)/.exec(signature);
    if (m !== null) return { type: (m[1] ?? "").trim(), doc: returnDoc };
    return null;
  }
  // Go: `) Type` after the param block
  if (language === "go") {
    const after = signature.slice(signature.indexOf(")") + 1).trim();
    if (after !== "" && after !== "{" ) return { type: after.replace(/\{.*$/, "").trim(), doc: returnDoc };
    return null;
  }
  // TS/JS: `): Type`
  if (language === "typescript" || language === "javascript") {
    const close = signature.lastIndexOf(")");
    if (close !== -1) {
      const after = signature.slice(close + 1).trim();
      const m = /^:\s*(.+?)(?:\s*=>|\s*\{|\s*$)/.exec(after);
      if (m !== null) return { type: (m[1] ?? "").trim(), doc: returnDoc };
    }
    return null;
  }
  // Java/C/C++: return type is before the method name (best-effort)
  const parenOpen = signature.indexOf("(");
  if (parenOpen !== -1) {
    const before = signature.slice(0, parenOpen).trim();
    const tokens = before.split(/\s+/);
    if (tokens.length >= 2) {
      const type = tokens[tokens.length - 2] ?? "";
      if (type !== "" && !["if", "while", "for", "switch"].includes(type)) {
        return { type, doc: returnDoc };
      }
    }
  }
  return null;
}

const CLASS_KINDS = new Set(["class", "struct", "interface", "trait", "enum", "impl"]);
const METHOD_KINDS = new Set(["method", "function"]);

/**
 * Compose the hover Markdown content sequence per symbol kind
 * (spec 5.3.1-2). Classes omit params; methods show params + returns;
 * properties show type/returns. Signature leads as a code block; the
 * container is an independent line; refs show only when > 0.
 */
export function composeHoverContent(input: HoverContentInput): Monaco.IMarkdownString[] {
  const contents: Monaco.IMarkdownString[] = [];
  const { kind, signature, doc, params, returnValue, location, container, refCount, language } = input;

  // Signature code block on top (VSCode style).
  if (signature.length > 0) {
    contents.push({ value: `\`\`\`${language}\n${signature}\n\`\`\`` });
  }

  // Doc comment.
  if (doc !== null && doc.length > 0) {
    contents.push({ value: doc });
  }

  // Method: params + returns.
  if (METHOD_KINDS.has(kind)) {
    if (params.length > 0) {
      const lines = params.map((p) => `- \`${p.name}\`${p.type !== "" ? `: \`${p.type}\`` : ""}${p.doc !== null ? ` — ${p.doc}` : ""}`);
      contents.push({ value: `**${labelParams()}**\n${lines.join("\n")}` });
    }
    if (returnValue !== null) {
      contents.push({ value: `**${labelReturns()}** \`${returnValue.type}\`${returnValue.doc !== null ? ` — ${returnValue.doc}` : ""}` });
    }
  }

  // Property: type/returns only.
  if (!CLASS_KINDS.has(kind) && !METHOD_KINDS.has(kind) && returnValue !== null) {
    contents.push({ value: `**${labelReturns()}** \`${returnValue.type}\`${returnValue.doc !== null ? ` — ${returnValue.doc}` : ""}` });
  }

  // Location.
  if (location !== null) {
    contents.push({ value: `\`📍 ${location}\`` });
  }

  // Container as an independent line.
  if (container !== null && container.length > 0) {
    contents.push({ value: `*${labelContainer(container)}*` });
  }

  // References (only when > 0).
  if (refCount > 0) {
    contents.push({ value: `---\n${refCount} refs` });
  }

  return contents;
}

function labelParams(): string {
  return "Parameters";
}
function labelReturns(): string {
  return "Returns";
}
function labelContainer(container: string): string {
  return `container: ${container}`;
}
