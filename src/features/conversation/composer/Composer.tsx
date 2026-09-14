import type { ReactNode } from "react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import "../../../lib/commands/builtin";
import {
  commandDescription,
  getCommands,
  parseInput,
  suggestCommands,
  type CommandContext,
  type ParsedInput,
  type SlashCommand,
} from "../../../lib/commands/registry";
import { ipc } from "../../../lib/ipc/client";
import { describeError } from "../../../i18n";
import { toast } from "../../../lib/store/toastStore";
import {
  isAllowedMime,
  isWithinSize,
  guessMime,
} from "../../../lib/conversation/attachmentModel";
import { CompletionMenu, type CompletionItem } from "./CompletionMenu";
import { useComposerTriggers } from "./useComposerTriggers";
import { ComposerToolbar } from "./ComposerToolbar";
import { AttachmentShelf } from "./AttachmentShelf";

export interface ComposerProps {
  disabled: boolean;
  pending: boolean;
  /** Services slash commands use; built by parent from existing state. */
  commandContext: CommandContext;
  /** Resolves on success; rejects keep the draft so user input is preserved. */
  onSubmit: (input: string) => Promise<void>;
  /** Optional node rendered left of the send button (AgentChip). */
  leftSlot?: ReactNode;
  /** Optional node rendered above the textarea (AttachmentShelf). */
  topSlot?: ReactNode;
  /** Session ID for attachment persistence; when null, attachment button is disabled. */
  sessionId?: string | null;
}

/**
 * Unified composer with `/` command and `@` mention support.
 *
 * - `/` at input start → command completion panel.
 * - `@` at word boundary → mention completion panel (file/agent/session).
 * - Enter submits, Shift+Enter inserts a newline.
 * - ↑↓ navigate, Tab/Enter completes, Esc dismisses.
 * - The draft is only cleared when submission or the command succeeds.
 */
export function Composer({
  disabled,
  pending,
  commandContext,
  onSubmit,
  leftSlot,
  topSlot,
  sessionId = null,
}: ComposerProps): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [value, setValue] = useState("");
  const [caret, setCaret] = useState(0);
  const [highlight, setHighlight] = useState(0);
  const [dismissed, setDismissed] = useState(false);
  const [voiceActive, setVoiceActive] = useState(false);
  const inputRef = useRef<HTMLTextAreaElement | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const executingRef = useRef(false);

  // Auto-grow: adjust textarea height to fit content, capped at ~12 rows.
  useEffect(() => {
    const el = inputRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 300)}px`;
  }, [value]);

  // Attachment list for this session (shown in AttachmentShelf).
  const { data: attachments } = useQuery({
    queryKey: ["attachments", sessionId],
    queryFn: () => (sessionId ? ipc.listAttachments(sessionId) : Promise.resolve([])),
    enabled: sessionId !== null,
    staleTime: 5_000,
  });

  const attachMut = useMutation({
    mutationFn: (input: { name: string; mime: string; dataBase64: string }) =>
      sessionId
        ? ipc.saveAttachment(sessionId, input.name, input.mime, input.dataBase64)
        : Promise.reject(new Error("no session")),
    onSuccess: () => {
      if (sessionId) void qc.invalidateQueries({ queryKey: ["attachments", sessionId] });
    },
    onError: (e) => toast.error(`${t("composer.attachmentFailed")}: ${describeError(e)}`),
  });

  const onFileSelected = (file: File): void => {
    if (!sessionId) return;
    if (!isWithinSize(file.size)) {
      toast.error(t("composer.attachmentTooLarge"));
      return;
    }
    const mime = file.type || guessMime(file.name);
    if (!isAllowedMime(mime)) {
      toast.error(t("composer.attachmentInvalidMime"));
      return;
    }
    const reader = new FileReader();
    reader.onload = (): void => {
      const result = reader.result;
      if (typeof result !== "string") return;
      const base64 = result.split(",")[1] ?? "";
      attachMut.mutate({ name: file.name, mime, dataBase64: base64 });
    };
    reader.onerror = (): void => toast.error(t("composer.attachmentFailed"));
    reader.readAsDataURL(file);
  };

  const trigger = useComposerTriggers(value, caret);

  // Build completion items from the active trigger mode.
  const items: CompletionItem[] = (() => {
    if (!trigger || dismissed) return [];
    if (trigger.mode === "command") {
      return getCommands()
        .filter((c) => c.name.startsWith(trigger.query))
        .map<CompletionItem>((cmd) => ({
          key: cmd.name,
          trigger: `/${cmd.name}`,
          usage: cmd.usage ? t(cmd.usage) : undefined,
          description: commandDescription(cmd, t),
          badge: cmd.category,
        }));
    }
    // Mention mode — P1.3/P1.4 will populate with real data sources.
    // For now, show namespace hints.
    if (trigger.mode === "mention") {
      const namespaces = [
        { key: "file", desc: t("composer.mentionFile") },
        { key: "agent", desc: t("composer.mentionAgent") },
        { key: "session", desc: t("composer.mentionSession") },
      ];
      return namespaces
        .filter((ns) => !trigger.namespace || ns.key.startsWith(trigger.namespace))
        .map<CompletionItem>((ns) => ({
          key: ns.key,
          trigger: `@${ns.key}`,
          description: ns.desc,
          badge: ns.key,
        }));
    }
    return [];
  })();

  const panelOpen = trigger !== null && !dismissed && items.length > 0;
  const activeIndex = items.length === 0 ? 0 : Math.min(highlight, items.length - 1);
  const empty = trigger !== null && !dismissed && items.length === 0;

  const syncCaret = (): void => {
    setCaret(inputRef.current?.selectionStart ?? 0);
  };

  const resetCompletion = (): void => {
    setDismissed(false);
    setHighlight(0);
  };

  const completeCommand = (cmd: SlashCommand): void => {
    const next = `/${cmd.name} `;
    setValue(next);
    resetCompletion();
    requestAnimationFrame(() => {
      const el = inputRef.current;
      if (el) el.setSelectionRange(next.length, next.length);
    });
  };

  const completeMention = (item: CompletionItem): void => {
    if (!trigger) return;
    const before = value.slice(0, trigger.start);
    const after = value.slice(trigger.end);
    const next = `${before}${item.trigger} ${after}`;
    setValue(next);
    resetCompletion();
    requestAnimationFrame(() => {
      const el = inputRef.current;
      if (el) {
        const pos = trigger.start + item.trigger.length + 1;
        el.setSelectionRange(pos, pos);
      }
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
      className="flex shrink-0 flex-col gap-0 border-t border-ink-muted/30 p-2"
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
    >
      {topSlot}
      {sessionId && attachments && attachments.length > 0 && (
        <AttachmentShelf
          attachments={attachments}
          onRemove={async (id) => {
            try {
              await ipc.deleteAttachment(id);
              void qc.invalidateQueries({ queryKey: ["attachments", sessionId] });
            } catch (e) {
              toast.error(describeError(e));
            }
          }}
        />
      )}
      <div className="flex items-end gap-2">
        {leftSlot}
        <div className="relative flex min-h-0 flex-1">
          {(panelOpen || empty) && trigger?.mode === "command" && (
            <CompletionMenu
              items={items}
              activeIndex={activeIndex}
              label={t("commands.panelLabel")}
              onSelect={(i) => {
                const cmd = getCommands().find((c) => c.name === items[i]?.key);
                if (cmd) completeCommand(cmd);
              }}
              empty={empty}
            />
          )}
          {(panelOpen || empty) && trigger?.mode === "mention" && (
            <CompletionMenu
              items={items}
              activeIndex={activeIndex}
              label={t("composer.mentionPanelLabel")}
              onSelect={(i) => { const item = items[i]; if (item) completeMention(item); }}
              empty={empty}
            />
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
                  setHighlight((h) => (h + 1) % items.length);
                  return;
                }
                if (e.key === "ArrowUp") {
                  e.preventDefault();
                  setHighlight((h) => (h - 1 + items.length) % items.length);
                  return;
                }
                if (e.key === "Tab") {
                  e.preventDefault();
                  if (trigger?.mode === "command") {
                    const cmd = getCommands().find((c) => c.name === items[activeIndex]?.key);
                    if (cmd) completeCommand(cmd);
                  } else if (trigger?.mode === "mention") {
                    const item = items[activeIndex];
                    if (item) completeMention(item);
                  }
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
                  if (trigger?.mode === "command") {
                    const cmd = getCommands().find((c) => c.name === items[activeIndex]?.key);
                    if (cmd) completeCommand(cmd);
                  } else if (trigger?.mode === "mention") {
                    const item = items[activeIndex];
                    if (item) completeMention(item);
                  }
                  return;
                }
                e.preventDefault();
                submit();
              }
            }}
            rows={4}
            disabled={disabled || pending}
            placeholder={t("chat.inputPlaceholder")}
            className="min-h-0 w-full resize-none rounded border border-ink-muted/40 bg-surface-raised px-2 pb-9 pt-1.5 text-sm text-ink placeholder:text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
          />
          <div className="absolute bottom-1 left-1 z-10">
            <ComposerToolbar
              onVoiceClick={() => setVoiceActive((v) => !v)}
              onFunctionMenuClick={() => { /* function menu — task group 5 */ }}
              onAttachmentClick={() => fileInputRef.current?.click()}
              voiceActive={voiceActive}
            />
          </div>
          <button
            type="submit"
            disabled={disabled || pending || value.trim().length === 0}
            className="pixel-fill-accent absolute bottom-1 right-1 z-10 px-3 py-1 text-sm text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
          >
            {pending ? t("chat.running") : t("chat.send")}
          </button>
        </div>
      </div>
      <input
        ref={fileInputRef}
        type="file"
        className="hidden"
        onChange={(e) => {
          const file = e.target.files?.[0];
          if (file) onFileSelected(file);
          e.target.value = "";
        }}
      />
    </form>
  );
}

