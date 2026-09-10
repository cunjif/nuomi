/**
 * Node ≥22 ships a disabled-by-default global `localStorage` (needs
 * --localstorage-file), which shadows jsdom's implementation under vitest.
 * Tests stub it with an in-memory Map so persistence assertions are
 * deterministic; pair with vi.unstubAllGlobals() in afterEach.
 */
import { vi } from "vitest";

export function stubLocalStorage(): Map<string, string> {
  const backing = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string): string | null => backing.get(key) ?? null,
    setItem: (key: string, value: string): void => {
      backing.set(key, value);
    },
    removeItem: (key: string): void => {
      backing.delete(key);
    },
    clear: (): void => {
      backing.clear();
    },
    key: (index: number): string | null => [...backing.keys()][index] ?? null,
    get length(): number {
      return backing.size;
    },
  });
  return backing;
}
