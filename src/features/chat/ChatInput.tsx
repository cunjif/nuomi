import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

interface ChatInputProps {
  disabled: boolean;
  pending: boolean;
  /** Resolves on success; rejects keep the draft so user input is preserved. */
  onSubmit: (input: string) => Promise<void>;
}

/**
 * Bottom composer. Enter submits, Shift+Enter inserts a newline; the draft
 * is only cleared when submission succeeds.
 */
export function ChatInput({ disabled, pending, onSubmit }: ChatInputProps): ReactNode {
  const { t } = useTranslation();
  const [value, setValue] = useState("");
  const submit = (): void => {
    const trimmed = value.trim();
    if (trimmed.length === 0 || pending) return;
    void onSubmit(trimmed)
      .then(() => setValue(""))
      .catch(() => {
        /* failure toast is raised by the parent; draft stays */
      });
  };
  return (
    <form
      className="flex shrink-0 items-end gap-2 border-t border-ink-muted/30 p-2"
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
    >
      <textarea
        aria-label={t("chat.inputPlaceholder")}
        value={value}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
            e.preventDefault();
            submit();
          }
        }}
        rows={2}
        disabled={disabled || pending}
        placeholder={t("chat.inputPlaceholder")}
        className="min-h-0 flex-1 resize-none rounded border border-ink-muted/40 bg-surface-raised px-2 py-1.5 text-sm text-ink placeholder:text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
      />
      <button
        type="submit"
        disabled={disabled || pending || value.trim().length === 0}
        className="rounded bg-ink-accent px-3 py-1.5 text-sm text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
      >
        {pending ? t("chat.running") : t("chat.send")}
      </button>
    </form>
  );
}
