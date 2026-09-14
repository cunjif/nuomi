import type { ReactNode } from "react";
import { useId } from "react";
import { useTranslation } from "react-i18next";

/** A single completion item — works for both `/` commands and `@` triggers. */
export interface CompletionItem {
  /** Unique key for React reconciliation. */
  key: string;
  /** Left-side monospace label (e.g. "/agent" or "@file"). */
  trigger: string;
  /** Optional usage hint after the trigger (e.g. "[name]"). */
  usage?: string;
  /** Human-readable description. */
  description: string;
  /** Optional category badge text. */
  badge?: string;
}

export interface CompletionMenuProps {
  items: CompletionItem[];
  /** Currently highlighted index. */
  activeIndex: number;
  /** Panel aria label. */
  label: string;
  /** Called when an item is clicked (mouse). */
  onSelect: (index: number) => void;
  /** True when no items match — shows empty state. */
  empty: boolean;
}

/**
 * Shared popup for `/` command and `@` trigger completion. Renders above the
 * composer textarea (`absolute bottom-full`), with keyboard contract handled
 * by the parent (Composer). Pure presentation + mouse click forwarding.
 */
export function CompletionMenu({
  items,
  activeIndex,
  label,
  onSelect,
  empty,
}: CompletionMenuProps): ReactNode {
  const { t } = useTranslation();
  const listboxId = useId();

  if (empty) {
    return (
      <div
        className="absolute bottom-full left-0 z-10 mb-1 w-full rounded border border-ink-muted/40 bg-surface-raised px-2 py-1.5 text-sm text-ink-muted shadow-lg"
        aria-live="polite"
      >
        {t("commands.noMatch")}
      </div>
    );
  }

  return (
    <ul
      id={listboxId}
      role="listbox"
      aria-label={label}
      className="absolute bottom-full left-0 z-10 mb-1 max-h-48 w-full overflow-y-auto rounded border border-ink-muted/40 bg-surface-raised shadow-lg"
    >
      {items.map((item, i) => (
        <li key={item.key} role="option" aria-selected={i === activeIndex}>
          <button
            type="button"
            onMouseDown={(e) => {
              e.preventDefault();
              onSelect(i);
            }}
            className={`flex w-full items-baseline gap-2 px-2 py-1 text-left text-sm focus-visible:ring-2 focus-visible:ring-ink-accent ${
              i === activeIndex ? "bg-surface-overlay text-ink-accent" : "text-ink-muted"
            }`}
          >
            <span className="shrink-0 font-mono text-ink">
              {item.trigger}
              {item.usage ? ` ${item.usage}` : ""}
            </span>
            <span className="min-w-0 flex-1 truncate">{item.description}</span>
            {item.badge && (
              <span className="shrink-0 rounded bg-ink-muted/20 px-1 text-xs text-ink-muted">
                {item.badge}
              </span>
            )}
          </button>
        </li>
      ))}
    </ul>
  );
}
