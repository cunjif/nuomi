/**
 * VSCode-style palette overlay (需求: 全局快捷键):
 * - Ctrl/Cmd+P        → quick file open (workspace file search);
 * - Ctrl/Cmd+Shift+P  → global command palette;
 * - Ctrl/Cmd+F        → find within the open file (only when a Monaco editor
 *   surface registered a find action — see triggerActiveEditorFind).
 *
 * One modal, two modes (paletteStore.mode). Builtin commands self-register
 * at module scope; other modules can contribute via registerPaletteCommand.
 * Shortcuts are handled at window keydown CAPTURE phase so neither Monaco
 * keybindings nor browser defaults (print / find-in-page) observe them —
 * same 不可覆盖 pattern as the Alt+H/Alt+E nav chords in Shell.
 */
import type { ReactNode } from "react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { listWorkspaceFiles, triggerActiveEditorFind } from "../../lib/editor-ext/indexer/workspace-index";
import { registerPaletteCommand, usePaletteCommands, type PaletteCommand } from "../../lib/commands/palette";
import { usePaletteStore } from "../../lib/store/paletteStore";
import { THEME_STORAGE_KEY, nextTheme, useUiStore } from "../../lib/store/uiStore";

const MAX_RESULTS = 50;

type PaletteItem = { kind: "file"; path: string } | { kind: "command"; cmd: PaletteCommand };

// ── builtin commands ─────────────────────────────────────────────────────

/** useTheme-free theme cycle (runs outside React render/hook context). */
function toggleTheme(): void {
  const current = useUiStore.getState().theme;
  const next = nextTheme(current);
  useUiStore.getState().setTheme(next);
  const root = document.documentElement;
  root.dataset.theme = next;
  root.classList.toggle("dark", next === "chalkboard-dark" || next === "high-contrast");
  try {
    localStorage.setItem(THEME_STORAGE_KEY, next);
  } catch {
    // Storage unavailable — theme still applies for this session.
  }
}

let builtinRegistered = false;

function registerBuiltinCommands(): void {
  if (builtinRegistered) return;
  builtinRegistered = true;
  const palette = usePaletteStore.getState;
  const ui = useUiStore.getState;
  registerPaletteCommand({
    id: "shell.gotoFile",
    titleKey: "palette.cmd.gotoFile",
    hint: "Ctrl+P",
    run: () => palette().open("files"),
  });
  registerPaletteCommand({
    id: "shell.commandPalette",
    titleKey: "palette.cmd.commandPalette",
    hint: "Ctrl+Shift+P",
    run: () => palette().open("commands"),
  });
  registerPaletteCommand({ id: "shell.toggleTheme", titleKey: "palette.cmd.toggleTheme", run: toggleTheme });
  registerPaletteCommand({
    id: "shell.gotoChat",
    titleKey: "palette.cmd.gotoChat",
    run: () => ui().setActiveArea("chat"),
  });
  registerPaletteCommand({
    id: "shell.gotoEditor",
    titleKey: "palette.cmd.gotoEditor",
    run: () => {
      const store = ui();
      store.setActiveArea("workbench");
      store.setWorkbenchSubTab("editor");
    },
  });
  registerPaletteCommand({
    id: "shell.closeFile",
    titleKey: "palette.cmd.closeFile",
    run: () => {
      const { activeFile, closeFile } = ui();
      if (activeFile !== null) closeFile(activeFile);
    },
  });
}
registerBuiltinCommands();

// ── global shortcuts ─────────────────────────────────────────────────────

/**
 * Ctrl/Cmd+P and Ctrl/Cmd+Shift+P toggle/switch the palette; Ctrl/Cmd+F
 * finds in the open file (only intercepted when a Monaco surface registered
 * a find action, otherwise the browser default is left alone).
 */
export function useGlobalPaletteShortcuts(): void {
  const handler = useRef<(e: KeyboardEvent) => void>(() => {});
  handler.current = (e: KeyboardEvent): void => {
    if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
    const palette = usePaletteStore.getState();
    if (e.code === "KeyP") {
      e.preventDefault();
      e.stopImmediatePropagation();
      if (e.shiftKey) {
        if (palette.mode === "commands") palette.close();
        else palette.open("commands");
      } else {
        if (palette.mode === "files") palette.close();
        else palette.open("files");
      }
      return;
    }
    if (e.code === "KeyF" && !e.shiftKey) {
      // Palette owns the keyboard while open — swallow the chord.
      if (palette.mode !== null) {
        e.preventDefault();
        e.stopImmediatePropagation();
        return;
      }
      if (triggerActiveEditorFind()) {
        e.preventDefault();
        e.stopImmediatePropagation();
      }
    }
  };
  useEffect(() => {
    const listener = (e: KeyboardEvent): void => handler.current(e);
    window.addEventListener("keydown", listener, true);
    return () => window.removeEventListener("keydown", listener, true);
  }, []);
}

// ── filtering ─────────────────────────────────────────────────────────────

/** Match score: basename prefix < basename hit < path hit < subsequence. */
function scorePath(path: string, q: string): number | null {
  const lower = path.toLowerCase();
  const base = lower.split(/[\\/]/).pop() ?? lower;
  if (base.startsWith(q)) return 0;
  if (base.includes(q)) return 1;
  if (lower.includes(q)) return 2;
  let i = 0;
  for (const ch of lower) {
    if (ch === q[i]) i += 1;
    if (i === q.length) return 3;
  }
  return i === q.length ? 3 : null;
}

// ── overlay ───────────────────────────────────────────────────────────────

export function QuickOpen(): ReactNode {
  const { t } = useTranslation();
  const mode = usePaletteStore((s) => s.mode);
  const close = usePaletteStore((s) => s.close);
  const openFile = useUiStore((s) => s.openFile);
  const commands = usePaletteCommands();
  const [query, setQuery] = useState("");
  const [activeIdx, setActiveIdx] = useState(0);
  const inputRef = useRef<HTMLInputElement | null>(null);

  const filesQuery = useQuery({
    queryKey: ["workspace-files"],
    queryFn: listWorkspaceFiles,
    enabled: mode === "files",
    staleTime: 60_000,
  });

  // Reset per open and per keystroke (VSCode resets selection on type).
  useEffect(() => {
    setQuery("");
    setActiveIdx(0);
    if (mode !== null) inputRef.current?.focus();
  }, [mode]);
  useEffect(() => {
    setActiveIdx(0);
  }, [query]);

  const items = useMemo<PaletteItem[]>(() => {
    if (mode === "files") {
      const paths = filesQuery.data ?? [];
      const q = query.trim().toLowerCase();
      if (q === "") return paths.slice(0, MAX_RESULTS).map((path) => ({ kind: "file" as const, path }));
      const scored: Array<{ path: string; score: number }> = [];
      for (const path of paths) {
        const score = scorePath(path, q);
        if (score !== null) scored.push({ path, score });
      }
      scored.sort((a, b) => a.score - b.score || a.path.length - b.path.length || a.path.localeCompare(b.path));
      return scored.slice(0, MAX_RESULTS).map(({ path }) => ({ kind: "file" as const, path }));
    }
    if (mode === "commands") {
      const q = query.trim().toLowerCase();
      return commands
        .filter((c) => {
          if (q === "") return true;
          const title = (c.titleKey !== undefined ? t(c.titleKey) : (c.title ?? c.id)).toLowerCase();
          return title.includes(q) || c.id.toLowerCase().includes(q);
        })
        .map((cmd) => ({ kind: "command" as const, cmd }));
    }
    return [];
  }, [mode, filesQuery.data, query, commands, t]);

  const clampedIdx = Math.min(activeIdx, Math.max(0, items.length - 1));

  const runItem = (item: PaletteItem): void => {
    close();
    if (item.kind === "file") openFile(item.path);
    else item.cmd.run();
  };

  const onKeyDown = (e: React.KeyboardEvent): void => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActiveIdx((i) => Math.min(i + 1, Math.max(0, items.length - 1)));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActiveIdx((i) => Math.max(i - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const item = items[clampedIdx];
      if (item !== undefined) runItem(item);
    } else if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  };

  if (mode === null) return null;
  return (
    <div
      role="dialog"
      aria-label={mode === "files" ? t("palette.filePlaceholder") : t("palette.commandPlaceholder")}
      className="fixed inset-0 z-50 flex items-start justify-center"
    >
      {/* Backdrop: click dismisses (tabIndex -1 keeps tab order inside panel). */}
      <div
        aria-hidden="true"
        className="absolute inset-0 bg-black/40"
        onMouseDown={(e) => {
          e.preventDefault();
          close();
        }}
      />
      <div
        data-testid="quick-open"
        className="sketch-panel relative mt-[10vh] w-full max-w-xl overflow-hidden border border-ink-muted/40 bg-surface-raised shadow-2xl"
      >
        <input
          ref={inputRef}
          data-testid="quick-open-input"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder={mode === "files" ? t("palette.filePlaceholder") : t("palette.commandPlaceholder")}
          className="w-full border-b border-ink-muted/30 bg-transparent px-3 py-2 text-sm text-ink outline-none placeholder:text-ink-muted"
        />
        <ul role="listbox" aria-label={t("palette.filePlaceholder")} className="max-h-80 overflow-y-auto p-1">
          {items.length === 0 && (
            <li className="px-2 py-3 text-center text-xs text-ink-muted" role="option" aria-selected={false}>
              {t("palette.noResults")}
            </li>
          )}
          {items.map((item, i) =>
            item.kind === "file" ? (
              <li key={item.path}>
                <button
                  type="button"
                  role="option"
                  aria-selected={i === clampedIdx}
                  onMouseDown={(e) => e.preventDefault()}
                  onMouseMove={() => setActiveIdx(i)}
                  onClick={() => runItem(item)}
                  className={`flex w-full items-center justify-between gap-3 rounded px-2 py-1.5 text-left text-sm ${
                    i === clampedIdx ? "bg-surface-overlay text-ink" : "text-ink-muted"
                  }`}
                >
                  <span className="truncate">{item.path.split(/[\\/]/).pop() ?? item.path}</span>
                  <span className="shrink-0 truncate font-mono text-[10px] opacity-70">{item.path}</span>
                </button>
              </li>
            ) : (
              <li key={item.cmd.id}>
                <button
                  type="button"
                  role="option"
                  aria-selected={i === clampedIdx}
                  onMouseDown={(e) => e.preventDefault()}
                  onMouseMove={() => setActiveIdx(i)}
                  onClick={() => runItem(item)}
                  className={`flex w-full items-center justify-between gap-3 rounded px-2 py-1.5 text-left text-sm ${
                    i === clampedIdx ? "bg-surface-overlay text-ink" : "text-ink-muted"
                  }`}
                >
                  <span className="truncate">
                    {item.cmd.titleKey !== undefined ? t(item.cmd.titleKey) : (item.cmd.title ?? item.cmd.id)}
                  </span>
                  {item.cmd.hint !== undefined && (
                    <span className="shrink-0 font-mono text-[10px] opacity-70">{item.cmd.hint}</span>
                  )}
                </button>
              </li>
            ),
          )}
        </ul>
        <div className="border-t border-ink-muted/30 px-3 py-1 text-[10px] text-ink-muted">{t("palette.hint")}</div>
      </div>
    </div>
  );
}
