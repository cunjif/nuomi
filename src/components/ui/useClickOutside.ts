import { useEffect, useRef } from "react";
import type { RefObject } from "react";

interface UseClickOutsideOptions {
  /** Whether the listener is active. When false, no listener is bound. */
  enabled: boolean;
  /** Called on mousedown (or Escape) outside every provided ref. */
  onOutside: () => void;
  /** Also invoke `onOutside` on Escape keydown. Default false. */
  escape?: boolean;
}

/**
 * Invokes `onOutside` when a `mousedown` lands outside every provided ref.
 * Pass `enabled` (typically the panel's `open` state) so the listener is
 * only bound while the panel is visible — matching the `ContextUsageIndicator`
 * / `ContextMenu` pattern already used in this codebase.
 *
 * The trigger button ref should be included in `refs` so clicking it to
 * toggle does not immediately close the panel it just opened.
 */
export function useClickOutside(
  refs: ReadonlyArray<RefObject<Node | null>>,
  options: UseClickOutsideOptions,
): void {
  const { enabled, onOutside, escape = false } = options;
  // Keep latest callback/refs without re-binding the listener.
  const cbRef = useRef(onOutside);
  cbRef.current = onOutside;
  const refsRef = useRef(refs);
  refsRef.current = refs;

  useEffect(() => {
    if (!enabled) return;
    const onDown = (e: MouseEvent): void => {
      const target = e.target as Node | null;
      if (target === null) return;
      const inside = refsRef.current.some(
        (r) => r.current !== null && r.current.contains(target),
      );
      if (!inside) cbRef.current();
    };
    document.addEventListener("mousedown", onDown);
    const onKey = escape
      ? (e: KeyboardEvent): void => {
          if (e.key === "Escape") cbRef.current();
        }
      : null;
    if (onKey !== null) document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      if (onKey !== null) document.removeEventListener("keydown", onKey);
    };
  }, [enabled, escape]);
}
