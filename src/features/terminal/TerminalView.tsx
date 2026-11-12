/**
 * TerminalView (ADR-0024 Part B): a single xterm.js terminal instance backed
 * by a Rust PTY (portable-pty). Creates the backend terminal on mount, pipes
 * stdin/stdout via IPC, and cleans up on unmount.
 *
 * Startup ordering matters: the PTY starts pumping the shell banner/prompt
 * immediately, so the `terminal-output` listener is registered **before**
 * `terminalCreate`. Chunks that arrive before the backend id is known are
 * buffered per id and replayed once the create call resolves — otherwise the
 * first screenful is silently dropped and the terminal looks stuck while
 * initialising.
 */
import type { ReactNode } from "react";
import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { listen } from "@tauri-apps/api/event";
import { ipc } from "../../lib/ipc/client";
import { monoFontStack } from "../../lib/theme/monoFont";
import "@xterm/xterm/css/xterm.css";

interface TerminalOutputEvent {
  id: string;
  data: string;
}

export function TerminalView({
  terminalId,
  cwd,
  initialCommand,
  onBackendReady,
}: {
  terminalId: string;
  cwd?: string;
  initialCommand?: string;
  /** Reports the backend (PTY) id so the tab strip can kill the right process. */
  onBackendReady?: (backendId: string) => void;
}): ReactNode {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  // Kept in a ref so a new callback identity from the parent never re-creates
  // the PTY (which would drop the user's shell session).
  const onBackendReadyRef = useRef(onBackendReady);
  onBackendReadyRef.current = onBackendReady;

  useEffect(() => {
    if (!containerRef.current) return;
    const term = new Terminal({
      fontSize: 13,
      // xterm measures glyphs on a canvas: a var() reference is an invalid
      // font shorthand and renders with a fallback face. Read the concrete
      // stack from CSS instead (same constraint as Monaco, see monoFont.ts).
      fontFamily: monoFontStack(),
      cursorBlink: true,
      scrollback: 5000,
    });
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(containerRef.current);
    fitAddon.fit();
    termRef.current = term;

    let backendId: string | null = null;
    let unlistenFn: (() => void) | undefined;
    let disposed = false;
    // Output chunks that arrive before `terminalCreate` resolves, keyed by
    // backend id so concurrent terminals never cross-talk.
    const earlyChunks = new Map<string, string[]>();
    let live = false;

    void (async () => {
      try {
        // Register first: the shell may print its prompt before the create
        // round-trip completes.
        unlistenFn = await listen<TerminalOutputEvent>("terminal-output", (event) => {
          const { id, data } = event.payload;
          if (live) {
            if (id === backendId) term.write(data);
            return;
          }
          const buffered = earlyChunks.get(id);
          if (buffered) buffered.push(data);
          else earlyChunks.set(id, [data]);
        });

        backendId = (await ipc.terminalCreate(cwd)).id;
        if (disposed) {
          void ipc.terminalKill(backendId).catch(() => undefined);
          return;
        }
        onBackendReadyRef.current?.(backendId);

        // Wire stdin/resize BEFORE replaying buffered output. Windows ConPTY
        // emits `ESC[6n` (cursor position report) while starting the shell and
        // blocks until the terminal answers with `ESC[row;colR`; xterm emits
        // that answer through `onData`, so the subscription must exist before
        // the buffered query is written — otherwise the shell never reaches
        // its prompt and the terminal looks like it exited immediately.
        term.onData((data) => {
          if (backendId) void ipc.terminalWrite(backendId, data);
        });
        term.onResize(({ cols, rows }) => {
          if (backendId) void ipc.terminalResize(backendId, cols, rows);
        });

        // Replay anything the shell emitted during the create round-trip,
        // then switch to live filtering.
        live = true;
        const pending = earlyChunks.get(backendId);
        earlyChunks.clear();
        if (pending) for (const chunk of pending) term.write(chunk);

        // Push the size we fitted locally: the PTY starts at the 80x24
        // default and would otherwise wrap output at the wrong column.
        void ipc
          .terminalResize(backendId, term.cols, term.rows)
          .catch(() => undefined);
        // Execute initial command if provided (e.g. from run/build button).
        if (initialCommand) {
          void ipc.terminalWrite(backendId, initialCommand + "\r").catch(() => undefined);
        }
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        term.writeln(`\x1b[31mFailed to start terminal: ${message}\x1b[0m`);
      }
    })();

    // Handle container resize via ResizeObserver.
    const ro = new ResizeObserver(() => {
      try {
        fitAddon.fit();
      } catch {
        // Terminal not ready yet — ignore.
      }
    });
    ro.observe(containerRef.current);

    return () => {
      disposed = true;
      ro.disconnect();
      unlistenFn?.();
      if (backendId) void ipc.terminalKill(backendId).catch(() => undefined);
      term.dispose();
      termRef.current = null;
    };
  }, [terminalId, cwd, initialCommand]);

  return <div ref={containerRef} className="h-full w-full" />;
}