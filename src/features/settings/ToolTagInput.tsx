import type { KeyboardEvent, ReactNode } from "react";
import { useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { fieldClass as field } from "../../components/ui/Field";

interface ToolTagInputProps {
  value: string[];
  onChange: (next: string[]) => void;
  candidates: string[];
  placeholder?: string;
  emptyHintLabel?: string;
}

const MAX_SUGGESTIONS = 20;

/** Tag input with fuzzy-match autocomplete for tool allowlists. */
export function ToolTagInput({
  value,
  onChange,
  candidates,
  placeholder,
  emptyHintLabel,
}: ToolTagInputProps): ReactNode {
  const { t } = useTranslation();
  const [input, setInput] = useState("");
  const [highlight, setHighlight] = useState(-1);
  const [expanded, setExpanded] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  const suggestions = useMemo(() => {
    const q = input.trim().toLowerCase();
    return candidates
      .filter((c) => !value.includes(c))
      .filter((c) => q.length === 0 || c.toLowerCase().includes(q))
      .slice(0, MAX_SUGGESTIONS);
  }, [input, candidates, value]);

  const addTag = (tag: string) => {
    const trimmed = tag.trim();
    if (trimmed.length === 0 || value.includes(trimmed)) return;
    onChange([...value, trimmed]);
    setInput("");
    setHighlight(-1);
    setExpanded(false);
  };

  const removeTag = (tag: string) => {
    onChange(value.filter((v) => v !== tag));
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      if (highlight >= 0 && highlight < suggestions.length) {
        const sug = suggestions[highlight];
        if (sug !== undefined) addTag(sug);
      } else if (input.trim().length > 0) {
        addTag(input);
      }
    } else if (e.key === "Backspace" && input.length === 0 && value.length > 0) {
      const last = value[value.length - 1];
      if (last !== undefined) removeTag(last);
    } else if (e.key === "ArrowDown" && suggestions.length > 0) {
      e.preventDefault();
      setHighlight((h) => Math.min(h + 1, suggestions.length - 1));
    } else if (e.key === "ArrowUp" && suggestions.length > 0) {
      e.preventDefault();
      setHighlight((h) => Math.max(h - 1, 0));
    } else if (e.key === "Escape") {
      setExpanded(false);
      setHighlight(-1);
    }
  };

  const hint = emptyHintLabel ?? t("settings.roles.toolTagInputNoRestriction");

  return (
    <div className="flex flex-col gap-1">
      <div
        className={`flex flex-wrap items-center gap-1 rounded border bg-surface p-1.5 ${field}`}
        onClick={() => inputRef.current?.focus()}
      >
        {value.map((tag) => (
          <span
            key={tag}
            className="flex items-center gap-0.5 rounded bg-ink-accent/20 px-1.5 py-0.5 text-xs text-ink"
          >
            {tag}
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                removeTag(tag);
              }}
              className="text-ink-muted hover:text-state-danger focus-visible:ring-1 focus-visible:ring-ink-accent"
              aria-label={`${t("common.delete")} ${tag}`}
            >
              ×
            </button>
          </span>
        ))}
        <input
          ref={inputRef}
          value={input}
          onChange={(e) => {
            setInput(e.target.value);
            setExpanded(true);
            setHighlight(-1);
          }}
          onKeyDown={onKeyDown}
          onFocus={() => setExpanded(true)}
          onBlur={() => setTimeout(() => setExpanded(false), 150)}
          placeholder={value.length === 0 ? (placeholder ?? "") : ""}
          className="flex-1 bg-transparent text-xs text-ink outline-none placeholder:text-ink-muted"
          role="combobox"
          aria-expanded={expanded && suggestions.length > 0}
          aria-autocomplete="list"
        />
      </div>
      {expanded && suggestions.length > 0 && (
        <ul
          className="rounded border border-ink-muted/40 bg-surface-raised py-1 text-xs shadow-sm"
          role="listbox"
        >
          {suggestions.map((s, i) => (
            <li
              key={s}
              role="option"
              aria-selected={i === highlight}
              onMouseDown={(e) => {
                e.preventDefault();
                addTag(s);
              }}
              className={`cursor-pointer px-2 py-0.5 ${
                i === highlight ? "bg-ink-accent/20 text-ink" : "text-ink-muted hover:bg-surface-overlay"
              }`}
            >
              {s}
            </li>
          ))}
        </ul>
      )}
      {value.length === 0 && (
        <span className="text-[10px] text-ink-muted">{hint}</span>
      )}
    </div>
  );
}
