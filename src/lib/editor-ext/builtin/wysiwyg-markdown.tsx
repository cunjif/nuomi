/**
 * Typora-style WYSIWYG markdown editor (builtin extension surface).
 *
 * Design: "source-while-focused, rendered-while-blurred". The document is
 * split into top-level markdown blocks via marked's lexer. Blurred blocks
 * render as sanitized HTML; the FOCUSED block swaps to a plain-text textarea
 * holding its raw markdown — exactly Typora's behavior of revealing the
 * markdown source of the line under the caret (## markers, ** marks …).
 *
 * Key consequences of this model (deliberate trade-offs):
 * - Raw markdown is the single source of truth: no HTML→markdown back-
 *   serialization at all, so no turndown, no escaping churn, and typing any
 *   syntax renders the moment the block loses focus (blur re-lexes the raw
 *   and can split it into several blocks).
 * - Clicking a rendered block maps the plain-text caret offset into the raw
 *   source (line-prefix aware: #/ > /- /1. /```) so the caret lands close to
 *   where the user clicked.
 * - Cross-block undo is not continuous; rich-text paste degrades to plain
 *   text (the focused surface is a textarea).
 */
import type { ReactNode } from "react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { marked } from "marked";
import type { EditorWysiwygApi, EditorWysiwygProps } from "../types";
import { sanitizeMarkdownHtml } from "./sanitize";

interface BlockData {
  /** Markdown source of this block (concatenation reproduces the document). */
  raw: string;
  /** Blank-line token: rendered as a non-editable spacer. */
  space: boolean;
}

marked.setOptions({ gfm: true, breaks: true });

/** Split a markdown document into top-level block raws (pure, unit-tested). */
export function lexBlocks(source: string): string[] {
  return marked.lexer(source).map((t) => t.raw);
}

function toBlock(raw: string): BlockData {
  return { raw, space: false };
}

/** Nearest non-blank block index to `i` (forward first, then backward). */
function nearestEditable(blocks: BlockData[], i: number): number {
  for (let k = i + 1; k < blocks.length; k += 1) {
    if (blocks[k]?.space === false) return k;
  }
  for (let k = i - 1; k >= 0; k -= 1) {
    if (blocks[k]?.space === false) return k;
  }
  return -1;
}

/** Lex source into block data, preserving blank-line space tokens. */
function lexBlockData(source: string): BlockData[] {
  const blocks = marked
    .lexer(source)
    .map((t) => ({ raw: t.raw, space: t.type === "space" || t.raw.trim() === "" }));
  return blocks.length > 0 ? blocks : [toBlock("")];
}

function toBlocks(source: string): BlockData[] {
  const blocks = lexBlockData(source);
  // An empty document still needs somewhere to type.
  return blocks.every((b) => b.space) ? [toBlock("")] : blocks;
}

/** Rendered HTML for one blurred block (sanitized at the boundary). */
function renderBlockHtml(raw: string): string {
  return sanitizeMarkdownHtml(marked.parse(raw, { async: false }));
}

/** Leading markdown line prefix: heading / quote / list item / fence. */
const MD_PREFIX_RE = /^(\s*(?:#{1,6}\s+|>\s?|[-*+]\s+|\d+[.)]\s+|```))/;

function stripPrefix(line: string): string {
  return line.replace(MD_PREFIX_RE, "");
}

/** Map a plain-text caret offset in the rendered block to its raw source. */
function rawCaretOffset(raw: string, plain: number): number {
  const prefix = MD_PREFIX_RE.exec(raw)?.[1]?.length ?? 0;
  return Math.min(plain + prefix, raw.length);
}

/** Current caret offset within `el`'s plain text (0 when unavailable). */
function plainCaretOffset(el: HTMLElement): number {
  const sel = window.getSelection();
  if (sel === null || sel.rangeCount === 0) return 0;
  const anchor = sel.anchorNode;
  if (anchor === null || !el.contains(anchor)) return 0;
  const pre = document.createRange();
  pre.selectNodeContents(el);
  try {
    pre.setEnd(anchor, sel.anchorOffset);
  } catch {
    return 0;
  }
  return pre.toString().length;
}

export const WysiwygMarkdownEditor = ({
  content,
  onChange,
  onReady,
  toolbarExtra,
  toolbarContainer,
}: EditorWysiwygProps): ReactNode => {
  const [blocks, setBlocks] = useState<BlockData[]>(() => toBlocks(content));
  // The focused block renders as raw markdown source (Typora model).
  const [focusIdx, setFocusIdx] = useState<number | null>(null);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const pendingCaretRef = useRef<number | null>(null);
  const pendingScrollRef = useRef(false);
  // Latest-value indirections so handlers stay stable across renders.
  const blocksRef = useRef(blocks);
  blocksRef.current = blocks;
  const focusIdxRef = useRef(focusIdx);
  focusIdxRef.current = focusIdx;
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  const onReadyRef = useRef(onReady);
  onReadyRef.current = onReady;
  // Start line (0-based) of each block: cumulative newline count of the
  // preceding raws. Lets outline symbols (LSP line positions) target blocks.
  const blockStartLinesRef = useRef<number[]>([]);

  // External content changes (query refresh, save round-trip) re-lex — but
  // never while our own onChange echo comes back (joined === content).
  useEffect(() => {
    const joined = blocksRef.current.map((b) => b.raw).join("");
    if (joined !== content) {
      setBlocks(toBlocks(content));
      setFocusIdx(null);
    }
  }, [content]);

  // Recompute block → start-line mapping whenever blocks change.
  useEffect(() => {
    const starts: number[] = [];
    let acc = 0;
    for (const b of blocks) {
      starts.push(acc);
      acc += (b.raw.match(/\n/g) ?? []).length;
    }
    blockStartLinesRef.current = starts;
  }, [blocks]);

  // Whenever a block becomes focused: focus its textarea, place the caret
  // (mapped from the click), auto-size, and optionally center-scroll.
  useLayoutEffect(() => {
    if (focusIdx === null) return;
    const ta = textareaRef.current;
    if (ta === null) return;
    ta.focus();
    const pos = pendingCaretRef.current ?? ta.value.length;
    pendingCaretRef.current = null;
    ta.setSelectionRange(pos, pos);
    ta.style.height = "auto";
    ta.style.height = `${ta.scrollHeight}px`;
    if (pendingScrollRef.current) {
      pendingScrollRef.current = false;
      if (typeof ta.scrollIntoView === "function") ta.scrollIntoView({ block: "center" });
    }
  }, [focusIdx]);

  // Hand the host (MonacoTab outline) an imperative reveal API once mounted,
  // and take it back on unmount (host must not keep a dead handle).
  useEffect(() => {
    const api: EditorWysiwygApi = {
      revealLine(line: number): void {
        const starts = blockStartLinesRef.current;
        // Last non-space block whose start line is at or before the target —
        // blank-line blocks are non-editable and never a reveal target.
        let idx = -1;
        for (let i = 0; i < starts.length; i += 1) {
          const block = blocksRef.current[i];
          if ((starts[i] ?? 0) <= line && block !== undefined && !block.space) idx = i;
        }
        if (idx < 0) return;
        if (focusIdxRef.current === idx) {
          // Already the open source block: just scroll to it.
          const el = containerRef.current?.querySelector<HTMLElement>(`[data-block-index="${idx}"]`);
          if (el !== null && el !== undefined && typeof el.scrollIntoView === "function") {
            el.scrollIntoView({ block: "center" });
          }
          return;
        }
        pendingCaretRef.current = 0;
        pendingScrollRef.current = true;
        setFocusIdx(idx);
      },
    };
    onReadyRef.current?.(api);
    return () => onReadyRef.current?.(null);
  }, []);

  const join = (list: BlockData[]): string => list.map((b) => b.raw).join("");

  /** Push a raw edit for block i into state + the parent draft pipeline. */
  const editBlock = (i: number, raw: string): void => {
    const next = blocksRef.current.map((b, idx) => (idx === i ? { ...b, raw } : b));
    setBlocks(next);
    onChangeRef.current(join(next));
    const ta = textareaRef.current;
    if (ta !== null) {
      ta.style.height = "auto";
      ta.style.height = `${ta.scrollHeight}px`;
    }
  };

  /** On blur: re-lex the block raw — typed syntax renders, and multi-block
   * source splits into several blocks. */
  const commitBlock = (i: number): void => {
    const ta = textareaRef.current;
    if (ta === null) return;
    const replacement = lexBlockData(ta.value);
    const next = [...blocksRef.current];
    next.splice(i, 1, ...replacement);
    setBlocks(next);
    setFocusIdx(null);
    onChangeRef.current(join(next));
  };

  const onMouseDown = (e: React.MouseEvent): void => {
    const blockEl = (e.target as HTMLElement).closest<HTMLElement>("[data-block-index]");
    if (blockEl === null || blockEl.tagName === "TEXTAREA") return;
    e.preventDefault();
    // Leaving an open block commits it first (blur will skip: focus moved).
    let list = blocksRef.current;
    let replacementCount = 1;
    const open = focusIdxRef.current;
    if (open !== null && textareaRef.current !== null) {
      const replacement = lexBlockData(textareaRef.current.value);
      replacementCount = replacement.length;
      list = [...list];
      list.splice(open, 1, ...replacement);
      setBlocks(list);
      onChangeRef.current(join(list));
      setFocusIdx(null);
    }
    let i = Number(blockEl.dataset.blockIndex);
    if (open !== null && open < i) i += replacementCount - 1;
    // Blank-line spacers are non-editable (aria-hidden): clicking one lands
    // on the nearest real block instead of doing nothing.
    if (list[i]?.space === true) {
      i = nearestEditable(list, i);
      if (i < 0) return;
    }
    const block = list[i];
    if (block === undefined) return;
    pendingCaretRef.current = rawCaretOffset(block.raw, plainCaretOffset(blockEl));
    setFocusIdx(i);
  };

  const onSourceKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>, i: number): void => {
    const ta = e.currentTarget;
    if (e.key === "Escape") {
      // Render the block immediately (Typora: Esc leaves source view).
      e.preventDefault();
      ta.blur();
      return;
    }
    if (e.key === "Backspace" && ta.selectionStart === 0 && ta.selectionEnd === 0) {
      // Empty block + Backspace at start: remove it and focus the previous one.
      const cur = blocksRef.current[i];
      if (cur !== undefined && cur.raw === "" && blocksRef.current.length > 1) {
        e.preventDefault();
        const next = blocksRef.current.filter((_, idx) => idx !== i);
        const prev = i - 1;
        if (blocksRef.current[prev]?.space === true) next.splice(prev, 1);
        let target = -1;
        for (let k = Math.min(prev, next.length - 1); k >= 0; k -= 1) {
          if (next[k]?.space === false) {
            target = k;
            break;
          }
        }
        setBlocks(next);
        onChangeRef.current(join(next));
        if (target >= 0) {
          pendingCaretRef.current = next[target]?.raw.length ?? 0;
          setFocusIdx(target);
        } else {
          setFocusIdx(null);
        }
      }
      return;
    }
    if (e.key === "Tab") {
      // Two-space indent, list friendly.
      e.preventDefault();
      ta.setRangeText("  ", ta.selectionStart, ta.selectionEnd, "end");
      editBlock(i, ta.value);
    }
  };

  // ── Toolbar: markdown string transforms over the focused source block ──
  const withTextarea = (run: (ta: HTMLTextAreaElement, i: number) => void): void => {
    const i = focusIdxRef.current;
    const ta = textareaRef.current;
    if (i === null || ta === null) return;
    run(ta, i);
    // Re-focus after React applies the new raw value.
    requestAnimationFrame(() => {
      ta.focus();
      ta.style.height = "auto";
      ta.style.height = `${ta.scrollHeight}px`;
    });
  };

  const wrapSelection = (mark: string): void => {
    withTextarea((ta, i) => {
      const s = ta.selectionStart;
      const e = ta.selectionEnd;
      const raw = ta.value;
      const inner = raw.slice(s, e);
      const nextRaw = raw.slice(0, s) + mark + inner + mark + raw.slice(e);
      ta.value = nextRaw;
      editBlock(i, nextRaw);
      const caret = inner === "" ? s + mark.length : e + mark.length * 2;
      requestAnimationFrame(() => ta.setSelectionRange(caret, caret));
    });
  };

  const prefixLines = (prefix: (line: string) => string): void => {
    withTextarea((ta, i) => {
      const raw = ta.value;
      const nextRaw = raw.split("\n").map(prefix).join("\n");
      ta.value = nextRaw;
      editBlock(i, nextRaw);
    });
  };

  const setFirstLineHeading = (level: 0 | 1 | 2 | 3): void => {
    withTextarea((ta, i) => {
      const lines = ta.value.split("\n");
      const first = lines[0] ?? "";
      lines[0] = level === 0 ? stripPrefix(first) : `${"#".repeat(level)} ${stripPrefix(first)}`;
      const nextRaw = lines.join("\n");
      ta.value = nextRaw;
      editBlock(i, nextRaw);
    });
  };

  const tools: Array<{ id: string; label: string; run: () => void }> = [
    { id: "bold", label: "B", run: () => wrapSelection("**") },
    { id: "italic", label: "I", run: () => wrapSelection("*") },
    { id: "code", label: "</>", run: () => wrapSelection("`") },
    { id: "h1", label: "H1", run: () => setFirstLineHeading(1) },
    { id: "h2", label: "H2", run: () => setFirstLineHeading(2) },
    { id: "h3", label: "H3", run: () => setFirstLineHeading(3) },
    { id: "p", label: "¶", run: () => setFirstLineHeading(0) },
    { id: "ul", label: "• list", run: () => prefixLines((line) => (line.trim() === "" ? line : `- ${stripPrefix(line)}`)) },
    {
      id: "ol",
      label: "1. list",
      run: () =>
        prefixLines((line) => {
          if (line.trim() === "") return line;
          const m = /^(\d+)[.)]\s+/.exec(line);
          const n = m === null ? 1 : Number(m[1]) + 1;
          return `${n}. ${stripPrefix(line)}`;
        }),
    },
    { id: "quote", label: "❝", run: () => prefixLines((line) => (line.trim() === "" ? line : `> ${stripPrefix(line)}`)) },
  ];

  const formatToolbar = (
    <>
      {tools.map((tool) => (
        <button
          key={tool.id}
          type="button"
          title={tool.id}
          // Prevent toolbar clicks from stealing the textarea selection.
          onMouseDown={(e) => e.preventDefault()}
          onClick={tool.run}
          className="sketch-btn min-w-7 px-1.5 py-0.5 text-xs text-ink-muted"
        >
          {tool.label}
        </button>
      ))}
      {toolbarExtra !== undefined && <div className="ml-auto flex items-center gap-1">{toolbarExtra}</div>}
    </>
  );

  const blockList = blocks.map((block, i) =>
    block.space ? (
      <div key={i} data-block-index={i} className="mdw-spacer" aria-hidden="true" />
    ) : focusIdx === i ? (
      <textarea
        key={i}
        ref={textareaRef}
        data-block-index={i}
        className="mdw-source"
        value={block.raw}
        rows={1}
        spellCheck={false}
        onChange={(e) => editBlock(i, e.target.value)}
        onBlur={() => {
          if (focusIdxRef.current === i) commitBlock(i);
        }}
        onKeyDown={(e) => onSourceKeyDown(e, i)}
      />
    ) : (
      <div
        key={i}
        data-block-index={i}
        className="mdw-block outline-none"
        // Rendered from the raw above; sanitizeMarkdownHtml strips
        // executable content at the boundary.
        dangerouslySetInnerHTML={{ __html: renderBlockHtml(block.raw) }}
      />
    ),
  );

  // Portal mode: the host renders the format toolbar across the full tab
  // width (above side panels too) — this component renders content only.
  if (toolbarContainer !== undefined) {
    return (
      <div className="md-wysiwyg flex h-full min-h-0 min-w-0 flex-col">
        {toolbarContainer !== null && createPortal(formatToolbar, toolbarContainer)}
        <div
          ref={containerRef}
          data-testid="wysiwyg-editor"
          aria-label="editor"
          className="min-h-0 flex-1 overflow-y-auto px-6 py-4"
          onMouseDown={onMouseDown}
        >
          {blockList}
        </div>
      </div>
    );
  }

  return (
    <div className="md-wysiwyg flex h-full min-h-0 min-w-0 flex-col">
      <div className="flex shrink-0 flex-wrap items-center gap-0.5 border-b border-ink-muted/30 px-2 py-1">
        {formatToolbar}
      </div>
      <div
        ref={containerRef}
        data-testid="wysiwyg-editor"
        aria-label="editor"
        className="min-h-0 flex-1 overflow-y-auto px-6 py-4"
        onMouseDown={onMouseDown}
      >
        {blockList}
      </div>
    </div>
  );
};
