import type { ReactNode } from "react";
import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import "../../lib/commands/builtin";
import {
  getCommands,
  parseInput,
  suggestCommands,
  type CommandContext,
  type ParsedInput,
  type SlashCommand,
} from "../../lib/commands/registry";
import { describeError } from "../../i18n";

interface ChatInputProps {
  disabled: boolean;
  pending: boolean;
  /** Services slash commands use; built by ChatView from existing state. */
  commandContext: CommandContext;
  /** Resolves on success; rejects keep the draft so user input is preserved. */
  onSubmit: (input: string) => Promise<void>;
}

/** Text before the caret forms a slash query only when it is still the first word. */
function activeSlashToken(value: string, caret: number): string | null {
  if (!value.startsWith("/")) return null;
  const before = value.slice(0, caret);
  if (/\s/.test(before)) return null;
  return before.slice(1).toLowerCase();
}

/**
 * Bottom composer with slash-command support (pi-style "commands as quick
 * input"): a leading "/" opens a completion panel (↑↓ select, Enter/Tab
 * complete, Esc dismiss, typing filters); submitting a registered command
 * runs its action instead of sending a message, an unknown one toasts the
 * closest matches. Enter submits, Shift+Enter inserts a newline; the draft
 * is only cleared when submission or the command succeeds.
 */
export function ChatInput({ disabled, pending, commandContext, onSubmit }: ChatInputProps): ReactNode {
  const { t } = useTranslation();
  const [value, setValue] = useState("");
  const [caret, setCaret] = useState(0);
  const [highlight, setHighlight] = useState(0);
  /** Esc dismisses the panel until the draft changes again. */
  const [dismissed, setDismissed] = useState(false);
  const inputRef = useRef<HTMLTextAreaElement | null>(null);
  const executingRef = useRef(false);

  const token = activeSlashToken(value, caret);
  const matches = token === null ? [] : getCommands().filter((c) => c.name.startsWith(token));
  const panelOpen = token !== null && !dismissed && matches.length > 0;
  const activeIndex = matches.length === 0 ? 0 : Math.min(highlight, matches.length - 1);
  const activeCommand = matches[activeIndex];

  const syncCaret = (): void => {
    setCaret(inputRef.current?.selectionStart ?? 0);
  };

  const resetCompletion = (): void => {
    setDismissed(false);
    setHighlight(0);
  };

  const complete = (cmd: SlashCommand): void => {
    const next = `/${cmd.name} `;
    setValue(next);
    resetCompletion();
    // Trailing space closes the panel; move the caret to the end on next paint.
    requestAnimationFrame(() => {
      const el = inputRef.current;
      if (el) el.setSelectionRange(next.length, next.length);
    });
  };

  const executeCommand = async (parsed: ParsedInput): Promise<void> => {
    const { t: ctxT, toast: cmdToast } = commandContext;
    if (!parsed.command) {
      const suggestions = suggestCommands(parsed.name)
        .map((c) => `/${c.name}`)
        .join(", ");
      cmdToast.error(
        suggestions.length > 0
          ? ctxT("commands.unknown", { name: parsed.name, suggestions })
          : ctxT("commands.unknownNone", { name: parsed.name }),
      );
      return;
    }
    try {
      await parsed.command.run(parsed.args, commandContext);
      setValue("");
      resetCompletion();
    } catch (e) {
      // Failure toast here; the draft stays so the command can be fixed.
      cmdToast.error(describeError(e));
    }
  };

  const submit = (): void => {
    const trimmed = value.trim();
    if (trimmed.length === 0 || pending || executingRef.current) return;
    const parsed = parseInput(trimmed);
    if (parsed !== null) {
      executingRef.current = true;
      void executeCommand(parsed).finally(() => {
        executingRef.current = false;
      });
      return;
    }
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
      <div className="relative flex min-h-0 flex-1">
        {panelOpen && (
          <ul
            role="listbox"
            aria-label={t("commands.panelLabel")}
            className="absolute bottom-full left-0 z-10 mb-1 max-h-48 w-full overflow-y-auto rounded border border-ink-muted/40 bg-surface-raised shadow-lg"
          >
            {matches.map((cmd, i) => (
              <li key={cmd.name} role="option" aria-selected={i === activeIndex}>
                <button
                  type="button"
                  // preventDefault keeps focus in the textarea on click.
                  onMouseDown={(e) => {
                    e.preventDefault();
                    complete(cmd);
                  }}
                  className={`flex w-full items-baseline gap-2 px-2 py-1 text-left text-sm focus-visible:ring-2 focus-visible:ring-ink-accent ${
                    i === activeIndex ? "bg-surface-overlay text-ink-accent" : "text-ink-muted"
                  }`}
                >
                  <span className="shrink-0 font-mono text-ink">
                    /{cmd.name}
                    {cmd.usage ? ` ${t(cmd.usage)}` : ""}
                  </span>
                  <span className="min-w-0 flex-1 truncate">{t(cmd.descriptionI18nKey)}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
        <textarea
          ref={inputRef}
          aria-label={t("chat.inputPlaceholder")}
          value={value}
          onChange={(e) => {
            setValue(e.target.value);
            resetCompletion();
            setCaret(e.target.selectionStart);
          }}
          onSelect={syncCaret}
          onKeyDown={(e) => {
            if (panelOpen) {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setHighlight((h) => (h + 1) % matches.length);
                return;
              }
              if (e.key === "ArrowUp") {
                e.preventDefault();
                setHighlight((h) => (h - 1 + matches.length) % matches.length);
                return;
              }
              if (e.key === "Tab") {
                e.preventDefault();
                if (activeCommand) complete(activeCommand);
                return;
              }
              if (e.key === "Escape") {
                e.preventDefault();
                setDismissed(true);
                return;
              }
            }
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              if (panelOpen) {
                e.preventDefault();
                if (activeCommand) complete(activeCommand);
                return;
              }
              e.preventDefault();
              submit();
            }
          }}
          rows={2}
          disabled={disabled || pending}
          placeholder={t("chat.inputPlaceholder")}
          className="min-h-0 flex-1 resize-none rounded border border-ink-muted/40 bg-surface-raised px-2 py-1.5 text-sm text-ink placeholder:text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
        />
      </div>
      <button
        type="submit"
        disabled={disabled || pending || value.trim().length === 0}
        className="pixel-fill-accent px-3 py-1.5 text-sm text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
      >
        {pending ? t("chat.running") : t("chat.send")}
      </button>
    </form>
  );
}
