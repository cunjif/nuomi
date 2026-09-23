import { useCallback, useEffect, useRef } from "react";
import type { RefObject } from "react";

/** Distance from the bottom (px) within which the view counts as "stuck". */
const STICK_THRESHOLD = 32;

interface UseStickToBottomOptions {
  /**
   * When the scroll element is swapped for a different DOM node (e.g. a
   * plain ⇄ virtualized branch switch), pass a value that changes here so
   * the scroll listener re-binds to the new node. Omit for a stable node.
   */
  rebindKey?: unknown;
}

interface UseStickToBottomResult {
  scrollRef: RefObject<HTMLDivElement>;
  /** Snap to bottom, but only if the user is currently following the tail. */
  scrollToBottomIfStuck: () => void;
}

/**
 * Keeps a scroll container pinned to the latest content while the user is
 * at the bottom, and stops forcing the view down once they scroll up to
 * read history. Attach `scrollRef` to the scroll element and call
 * `scrollToBottomIfStuck` whenever new content arrives.
 */
export function useStickToBottom(options?: UseStickToBottomOptions): UseStickToBottomResult {
  const scrollRef = useRef<HTMLDivElement>(null);
  const stickToBottomRef = useRef(true);
  const rebindKey = options?.rebindKey;

  useEffect(() => {
    const el = scrollRef.current;
    if (el === null) return;
    const onScroll = () => {
      stickToBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight < STICK_THRESHOLD;
    };
    el.addEventListener("scroll", onScroll, { passive: true });
    return () => el.removeEventListener("scroll", onScroll);
  }, [rebindKey]);

  const scrollToBottomIfStuck = useCallback(() => {
    const el = scrollRef.current;
    if (el === null || !stickToBottomRef.current) return;
    el.scrollTop = el.scrollHeight;
  }, []);

  return { scrollRef, scrollToBottomIfStuck };
}
