import type { ReactNode } from "react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { Icon } from "../../components/ui/Icon/Icon";
import { formatSize } from "../../lib/conversation/attachmentModel";
import { ContextUsageIndicator } from "../conversation/composer/ContextUsageIndicator";
import { OptimizeCompareBar } from "../conversation/composer/OptimizeCompareBar";
import { usePromptOptimize } from "../conversation/composer/usePromptOptimize";
import {
  STEWARD_MAX_ATTACHMENT_BYTES,
  guessStewardMime,
} from "./stewardAttachmentGate";

export interface StewardPendingAttachment {
  id: string;
  name: string;
  mime: string;
  sizeBytes: number;
  /** Data URL for image previews; documents render as chips. */
  previewUrl: string | null;
}

export interface StewardComposerProps {
  sessionId: string | null;
  /** True while a send is in flight (mirrors the chat mutation). */
  disabled: boolean;
  /**
   * Sends the draft. Resolves `true` when the turn was accepted (composer
   * clears the draft and attachments); `false` keeps everything so the user
   * can edit and retry (spec 5.1.3 / 5.3.3-1c).
   */
  onSend: (text: string, attachmentIds: string[]) => Promise<boolean>;
}

/**
 * Steward input area (task 4.4, spec 5.3.1), aligned to the unified
 * RoleAgent composer skeleton: auto-growing textarea, in-frame tool
 * buttons (attachment + AI optimize), pixel send button, toast notices.
 * Steward semantics kept: attachment whitelist pre-check, multi-file
 * picker, local pending attachments that survive a failed send, and the
 * `onSend → boolean` draft-clearing contract.
 */
export function StewardComposer({
  sessionId,
  disabled,
  onSend,
}: StewardComposerProps): ReactNode {
  const { t } = useTranslation();
  const [input, setInput] = useState("");
  const [attachments, setAttachments] = useState<StewardPendingAttachment[]>([]);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const inputRef = useRef<HTMLTextAreaElement | null>(null);
  const [attaching, setAttaching] = useState(false);
  const { optimize, runOptimize, clearOptimize, describeOptimizeError } =
    usePromptOptimize(sessionId);

  // Live attachment cap from app_settings (steward_attachment_max_bytes);
  // the exported constant is only the fallback until the query settles.
  const { data: attachmentLimit } = useQuery({
    queryKey: ["steward", "attachment-limit"],
    queryFn: () => ipc.stewardGetAttachmentLimit(),
  });
  const maxBytes = attachmentLimit ?? STEWARD_MAX_ATTACHMENT_BYTES;

  // Auto-grow: adjust textarea height to fit content, capped at ~12 rows
  // (same contract as the unified Composer).
  useEffect(() => {
    const el = inputRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 300)}px`;
  }, [input]);

  const onFilesSelected = useCallback(
    async (files: FileList | null): Promise<void> => {
      if (!files || files.length === 0 || !sessionId) return;
      setAttaching(true);
      try {
        for (const file of Array.from(files)) {
          const mime = guessStewardMime(file.name, file.type);
          if (mime === null) {
            toast.error(t("steward.composer.unsupportedType", { name: file.name }));
            continue;
          }
          if (file.size > maxBytes) {
            toast.error(
              t("steward.composer.tooLarge", {
                name: file.name,
                limit: formatSize(maxBytes),
              }),
            );
            continue;
          }
          if (file.size === 0) {
            toast.error(t("steward.composer.emptyFile", { name: file.name }));
            continue;
          }
          const dataUrl = await readFileAsDataUrl(file);
          const base64 = dataUrl.split(",")[1] ?? "";
          try {
            const dto = await ipc.stewardSaveAttachment(sessionId, file.name, mime, base64);
            setAttachments((prev) => [
              ...prev,
              {
                id: dto.id,
                name: file.name,
                mime,
                sizeBytes: file.size,
                previewUrl: mime.startsWith("image/") ? dataUrl : null,
              },
            ]);
          } catch (e) {
            // Backend rejection names the file (mime/size/session errors).
            toast.error(
              t("steward.composer.saveFailed", {
                message: e instanceof Error ? e.message : String(e),
              }),
            );
          }
        }
      } finally {
        setAttaching(false);
      }
    },
    [sessionId, t, maxBytes],
  );

  const removeAttachment = useCallback((id: string): void => {
    setAttachments((prev) => prev.filter((a) => a.id !== id));
  }, []);

  // Prompt AI optimize (task 4.7, spec 5.3.1-4, session-generic IPC): the
  // draft is never replaced without an explicit adopt — compare-first,
  // adopt/discard.
  const handleOptimize = useCallback(
    async (draft: string): Promise<void> => {
      try {
        await runOptimize(draft);
      } catch (e) {
        // Timeout/failure keeps the draft untouched (spec 5.3.3-3c).
        toast.error(describeOptimizeError(e));
      }
    },
    [runOptimize, describeOptimizeError],
  );

  const handleSend = useCallback(async (): Promise<void> => {
    const trimmed = input.trim();
    if (!trimmed || disabled) return;
    const ids = attachments.map((a) => a.id);
    const sent = await onSend(trimmed, ids);
    if (sent) {
      setInput("");
      setAttachments([]);
      clearOptimize();
    }
  }, [input, disabled, attachments, onSend, clearOptimize]);

  const busy = disabled || attaching;
  const optimizeRunning = optimize.state === "running";

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        void handleSend();
      }}
      className="flex shrink-0 flex-col gap-0 border-t border-ink-muted/30 p-2"
    >
      {attachments.length > 0 && (
        <div
          role="list"
          aria-label={t("steward.composer.pendingAttachments")}
          className="flex gap-1 overflow-x-auto pb-1"
        >
          {attachments.map((att) => (
            <div
              key={att.id}
              role="listitem"
              className="group relative flex shrink-0 items-center gap-1 rounded border border-ink-muted/30 bg-surface-raised px-1.5 py-1 text-xs text-ink"
            >
              {att.previewUrl !== null ? (
                <img
                  src={att.previewUrl}
                  alt={att.name}
                  className="h-8 w-8 rounded object-cover"
                />
              ) : (
                <span aria-hidden="true" className="text-sm">📎</span>
              )}
              <span className="max-w-32 truncate">{att.name}</span>
              <span className="text-ink-muted">{formatSize(att.sizeBytes)}</span>
              <button
                type="button"
                onClick={() => removeAttachment(att.id)}
                className="ml-0.5 text-ink-muted opacity-0 transition-opacity hover:text-ink group-hover:opacity-100 focus-visible:opacity-100"
                aria-label={t("composer.attachmentRemove")}
              >
                ✕
              </button>
            </div>
          ))}
        </div>
      )}
      {optimize.state === "compare" && (
        <OptimizeCompareBar
          original={optimize.original}
          optimized={optimize.optimized}
          busy={optimizeRunning}
          onAdopt={(optimized) => {
            setInput(optimized);
            clearOptimize();
          }}
          onRetry={(original) => {
            void handleOptimize(original);
          }}
          onDiscard={clearOptimize}
        />
      )}
      <div className="flex items-end gap-2">
        <div className="relative flex min-h-0 flex-1">
          <textarea
            ref={inputRef}
            aria-label={t("steward.composer.placeholder")}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                void handleSend();
              }
            }}
            rows={4}
            disabled={disabled}
            placeholder={t("steward.composer.placeholder")}
            className="min-h-0 w-full resize-none rounded border border-ink-muted/40 bg-surface-raised px-2 pb-9 pt-1.5 text-sm text-ink placeholder:text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
          />
          <div className="absolute bottom-1 left-1 z-10 flex items-center gap-0.5">
            <button
              type="button"
              onClick={() => fileInputRef.current?.click()}
              className="flex h-7 w-7 items-center justify-center rounded p-1 text-ink-muted hover:bg-surface-overlay hover:text-ink-accent disabled:opacity-50"
              aria-label={t("composer.toolbar.attachment")}
              title={t("composer.toolbar.attachment")}
              disabled={busy || sessionId === null}
            >
              <Icon name="file" size={16} />
            </button>
            <button
              type="button"
              onClick={() => void handleOptimize(input)}
              className="flex h-7 w-7 items-center justify-center rounded p-1 text-ink-muted hover:bg-surface-overlay hover:text-ink-accent disabled:opacity-50"
              aria-label={t("composer.optimize")}
              title={t(optimizeRunning ? "composer.optimizeRunning" : "composer.optimize")}
              disabled={
                busy ||
                sessionId === null ||
                optimizeRunning ||
                input.trim().length === 0
              }
            >
              <Icon
                name={optimizeRunning ? "spinner" : "wand"}
                size={16}
                className={optimizeRunning ? "animate-spin" : undefined}
              />
            </button>
          </div>
          <div className="absolute bottom-1 right-1 z-10">
            <button
              type="submit"
              disabled={disabled || attaching || !input.trim()}
              className="pixel-fill-accent px-3 py-1 text-sm text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
            >
              {t("chat.send")}
            </button>
          </div>
        </div>
      </div>
      <div className="mt-1 flex justify-end">
        <ContextUsageIndicator sessionId={sessionId} />
      </div>
      <input
        ref={fileInputRef}
        type="file"
        multiple
        className="hidden"
        onChange={(e) => {
          void onFilesSelected(e.target.files);
          e.target.value = "";
        }}
      />
    </form>
  );
}

function readFileAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = (): void => {
      if (typeof reader.result === "string") resolve(reader.result);
      else reject(new Error("read failed"));
    };
    reader.onerror = (): void => reject(new Error("read failed"));
    reader.readAsDataURL(file);
  });
}
