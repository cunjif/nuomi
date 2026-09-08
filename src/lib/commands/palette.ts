/**
 * Global command registry powering the Ctrl+Shift+P command palette
 * (VSCode-style). Commands are self-contained `{ id, title, run }` entries;
 * builtin commands register from QuickOpen.tsx module scope, future
 * extensions can contribute theirs via registerPaletteCommand too.
 *
 * Reactivity mirrors the editor-ext registry: version counter +
 * useSyncExternalStore hook, so the palette list re-renders when commands
 * register/unregister.
 */
import { useSyncExternalStore } from "react";

export interface PaletteCommand {
  /** Globally unique, e.g. "shell.gotoFile". */
  id: string;
  /** i18n key of the display title (resolved at render time). */
  titleKey?: string;
  /** Literal title fallback when no i18n key fits (e.g. dynamic titles). */
  title?: string;
  /** Optional one-line hint shown on the right (shortcut, scope…). */
  hint?: string;
  run: () => void;
}

const commands = new Map<string, PaletteCommand>();
const listeners = new Set<() => void>();
let version = 0;

function notify(): void {
  version += 1;
  listeners.forEach((l) => l());
}

/** Register (or replace) a palette command. */
export function registerPaletteCommand(cmd: PaletteCommand): void {
  commands.set(cmd.id, cmd);
  notify();
}

export function getPaletteCommands(): PaletteCommand[] {
  return [...commands.values()];
}

export function subscribePaletteCommands(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function getPaletteCommandsVersion(): number {
  return version;
}

/** Reactive command list for the palette UI. */
export function usePaletteCommands(): PaletteCommand[] {
  useSyncExternalStore(subscribePaletteCommands, getPaletteCommandsVersion, getPaletteCommandsVersion);
  return getPaletteCommands();
}
