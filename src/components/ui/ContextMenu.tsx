import type { ReactNode } from "react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Icon } from "./Icon/Icon";
import type { IconName } from "./Icon/Icon";

export type ContextMenuItem =
  | { type: "separator" }
  | {
      type: "item";
      id: string;
      label: string;
      icon?: IconName;
      disabled?: boolean;
      danger?: boolean;
      onSelect: () => void;
    };

export interface ContextMenuProps {
  open: boolean;
  x: number;
  y: number;
  items: ContextMenuItem[];
  onClose: () => void;
}

/**
 * Hand-drawn context menu (review §6.2). Portals to document.body and
 * positions at (x, y) with viewport-boundary flip. Closes on Esc, outside
 * click, or scroll. Menu items render as a sketch-styled list; separators
 * are dashed dividers.
 */
export function ContextMenu({ open, x, y, items, onClose }: ContextMenuProps): ReactNode {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: x, top: y });

  useLayoutEffect(() => {
    if (!open || !ref.current) return;
    const rect = ref.current.getBoundingClientRect();
    let left = x;
    let top = y;
    if (left + rect.width > window.innerWidth - 4) left = window.innerWidth - rect.width - 4;
    if (top + rect.height > window.innerHeight - 4) top = window.innerHeight - rect.height - 4;
    if (left < 4) left = 4;
    if (top < 4) top = 4;
    setPos({ left, top });
  }, [open, x, y]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") onClose();
    };
    const onPointer = (e: PointerEvent): void => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    const onScroll = (): void => onClose();
    window.addEventListener("keydown", onKey);
    window.addEventListener("pointerdown", onPointer, true);
    window.addEventListener("scroll", onScroll, true);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointerdown", onPointer, true);
      window.removeEventListener("scroll", onScroll, true);
    };
  }, [open, onClose]);

  if (!open) return null;

  return createPortal(
    <div
      ref={ref}
      role="menu"
      style={{ left: pos.left, top: pos.top }}
      className="sketch-card fixed z-50 min-w-[160px] animate-draw-in bg-surface-raised p-1 shadow-sketch-lg"
    >
      {items.map((item, idx) =>
        item.type === "separator" ? (
          <div key={`sep-${idx}`} className="my-1 border-t border-dashed border-ink-muted/30" />
        ) : (
          <button
            key={item.id}
            type="button"
            role="menuitem"
            disabled={item.disabled}
            onClick={() => {
              if (item.disabled) return;
              item.onSelect();
              onClose();
            }}
            className={`flex w-full items-center gap-2 rounded px-2 py-1 text-left text-sm focus-visible:ring-2 focus-visible:ring-ink-accent ${
              item.disabled
                ? "cursor-not-allowed text-ink-muted/40"
                : item.danger
                  ? "text-state-danger hover:bg-state-danger/10"
                  : "text-ink hover:bg-surface-overlay"
            }`}
          >
            {item.icon && <Icon name={item.icon} size={14} />}
            <span className="flex-1 truncate">{item.label}</span>
          </button>
        ),
      )}
    </div>,
    document.body,
  );
}
