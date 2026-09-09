import type { ReactNode } from "react";

/**
 * Hand-drawn tab strip (review §6.2). A row of pill buttons; the active tab
 * firms its dashed border and gains an accent underline. Keyboard navigable via
 * the native button semantics.
 */
export interface TabItem {
  id: string;
  label: ReactNode;
  icon?: ReactNode;
}

export interface TabsProps {
  items: ReadonlyArray<TabItem>;
  value: string;
  onChange: (id: string) => void;
  className?: string;
  "aria-label"?: string;
}

export function Tabs({ items, value, onChange, className = "", ...aria }: TabsProps): ReactNode {
  return (
    <div
      role="tablist"
      className={`inline-flex flex-wrap gap-1 ${className}`}
      aria-label={aria["aria-label"]}
    >
      {items.map((item) => {
        const active = item.id === value;
        return (
          <button
            key={item.id}
            type="button"
            role="tab"
            aria-selected={active}
            onClick={() => onChange(item.id)}
            className={`inline-flex items-center gap-1.5 rounded-full border border-dashed px-3 py-1 font-note-hand text-sm transition-colors ${
              active
                ? "border-ink-accent text-ink-accent shadow-[inset_0_-2px_0_0_var(--nuomi-accent)]"
                : "border-ink-muted text-ink-muted hover:border-ink hover:text-ink"
            }`}
          >
            {item.icon}
            {item.label}
          </button>
        );
      })}
    </div>
  );
}
