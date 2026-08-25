/**
 * Event transport: production subscribes through the Tauri event API;
 * outside a Tauri runtime (vitest/jsdom, plain browser) it falls back to an
 * in-memory bus so components stay renderable and testable.
 */
import type { DomainEvent } from "./types";

type Listener = (event: DomainEvent) => void;
export type Unsubscribe = () => void;

const fallbackBus = new Map<string, Set<Listener>>();

function hasTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export async function subscribe(channel: string, listener: Listener): Promise<Unsubscribe> {
  if (!hasTauriRuntime()) {
    let set = fallbackBus.get(channel);
    if (!set) {
      set = new Set();
      fallbackBus.set(channel, set);
    }
    set.add(listener);
    const registered = set;
    return () => {
      registered.delete(listener);
    };
  }
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<DomainEvent>(channel, (e) => listener(e.payload));
  return () => {
    unlisten();
  };
}

/** Test-only: push an event through the in-memory fallback bus. */
export function emitTestEvent(channel: string, event: DomainEvent): void {
  for (const listener of [...(fallbackBus.get(channel) ?? [])]) listener(event);
}

/** Test-only: drop all fallback-bus listeners. */
export function resetTestBus(): void {
  fallbackBus.clear();
}
