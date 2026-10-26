/**
 * TerminalView (ADR-0024 Part B): a single xterm.js terminal instance backed
 * by a Rust PTY (portable-pty). Creates the backend terminal on mount, pipes
 * stdin/stdout via IPC, and cleans up on unmount.
 */
import type { ReactNode } from "react";
import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { listen } from "@tauri-apps/api/event";
import { ipc } from "../../lib/ipc/client";
import "@xterm/xterm/css/xterm.css";

interface TerminalOutputEvent {
  id: string;
  data: string;
}

export function TerminalView({
  terminalId,
  cwd,
}: {
  terminalId: string;
  cwd?: string;
}): ReactNode {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);

  useEffect(() => {
    if (!containerRef.current) return;
    const term = new Terminal({
      fontSize: 13,
      fontFamily: "var(--font-mono, monospace)",
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

    // Create the backend terminal and wire stdin/stdout.
    void (async () => {
      try {
        backendId = await ipc.terminalCreate(cwd);
        if (disposed) {
          void ipc.terminalKill(backendId);
          return;
        }
        // Listen for output events — filter by our backend id.
        unlistenFn = await listen<TerminalOutputEvent>("terminal-output", (event) => {
          if (event.payload.id === backendId) {
            term.write(event.payload.data);
          }
        });
        // Pipe stdin to backend.
        term.onData((data) => {
          if (backendId) void ipc.terminalWrite(backendId, data);
        });
        // Pipe resize to backend.
        term.onResize(({ cols, rows }) => {
          if (backendId) void ipc.terminalResize(backendId, cols, rows);
        });
      } catch {
        term.writeln("\x1b[31mFailed to start terminal\x1b[0m");
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
      if (backendId) void ipc.terminalKill(backendId);
      term.dispose();
      termRef.current = null;
    };
  }, [terminalId, cwd]);

  return <div ref={containerRef} className="h-full w-full" />;
}
