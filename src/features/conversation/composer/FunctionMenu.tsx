import type { ReactNode } from "react";
import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../../lib/ipc/client";
import { describeError } from "../../../i18n";
import { toast } from "../../../lib/store/toastStore";
import { ARCHIVE_EXTENSIONS, IMAGE_EXTENSIONS } from "../../../lib/conversation/attachmentModel";

export interface FunctionMenuProps {
  sessionId: string;
  onClose: () => void;
  /** File handoff for the three attachment entries (chat_rich_bind 5.7):
   * the Composer runs the shared validation + upload chain. */
  onFilesSelected: (files: FileList | null, category: "image" | "document" | "archive") => void;
}

type MenuMode = "root" | "session" | "rule" | "prompt";

const IMAGE_ACCEPT = (IMAGE_EXTENSIONS as readonly string[]).map((e) => `.${e}`).join(",");
const ARCHIVE_ACCEPT = (ARCHIVE_EXTENSIONS as readonly string[]).map((e) => `.${e}`).join(",");

/**
 * Function menu: the three attachment entries (image / document / archive —
 * chat_rich_bind 5.7, each bound to an accept-filtered hidden file input),
 * plus import session info, apply rule, or inject custom SystemPrompt.
 * 150ms expand animation.
 */
export function FunctionMenu({ sessionId, onClose, onFilesSelected }: FunctionMenuProps): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [mode, setMode] = useState<MenuMode>("root");
  const [customPrompt, setCustomPrompt] = useState("");
  const [confirmed, setConfirmed] = useState(false);
  const imageInputRef = useRef<HTMLInputElement | null>(null);
  const documentInputRef = useRef<HTMLInputElement | null>(null);
  const archiveInputRef = useRef<HTMLInputElement | null>(null);

  const sessionsQuery = useQuery({
    queryKey: ["injectableSessions"],
    queryFn: () => ipc.listInjectableSessions(),
    staleTime: 30_000,
    enabled: mode === "session",
  });

  const rulesQuery = useQuery({
    queryKey: ["injectableRules"],
    queryFn: () => ipc.listInjectableRules(),
    staleTime: 30_000,
    enabled: mode === "rule",
  });

  const injectMut = useMutation({
    mutationFn: (input: { type: string; refId: string | null; text: string | null }) =>
      ipc.injectContext(sessionId, input),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["contextInjections", sessionId] });
      toast.success(t("composer.functionMenu.injected"));
      onClose();
    },
    onError: (e) => toast.error(describeError(e)),
  });

  return (
    <div
      className="absolute bottom-full left-0 z-20 mb-1 w-72 rounded border border-ink-muted/40 bg-surface-raised shadow-lg transition-opacity duration-150"
      role="dialog"
      aria-label={t("composer.functionMenu.title")}
    >
      <div className="flex items-center justify-between border-b border-ink-muted/30 px-2 py-1.5">
        <span className="text-sm font-medium text-ink">
          {mode === "root" ? t("composer.functionMenu.title") : t(`composer.functionMenu.${mode}`)}
        </span>
        <button type="button" onClick={onClose} className="text-ink-muted hover:text-ink" aria-label={t("common.close")}>
          ✕
        </button>
      </div>
      <div className="max-h-56 overflow-y-auto p-1.5">
        {mode === "root" && (
          <div className="space-y-0.5">
            <button
              type="button"
              onClick={() => imageInputRef.current?.click()}
              className="block w-full rounded px-1.5 py-1 text-left text-sm text-ink hover:bg-surface-overlay"
            >
              {t("composer.addImage")}
            </button>
            <button
              type="button"
              onClick={() => documentInputRef.current?.click()}
              className="block w-full rounded px-1.5 py-1 text-left text-sm text-ink hover:bg-surface-overlay"
            >
              {t("composer.addDocument")}
            </button>
            <button
              type="button"
              onClick={() => archiveInputRef.current?.click()}
              className="block w-full rounded px-1.5 py-1 text-left text-sm text-ink hover:bg-surface-overlay"
            >
              {t("composer.addArchive")}
            </button>
            <button
              type="button"
              onClick={() => setMode("session")}
              className="block w-full rounded px-1.5 py-1 text-left text-sm text-ink hover:bg-surface-overlay"
            >
              {t("composer.functionMenu.importSession")}
            </button>
            <button
              type="button"
              onClick={() => setMode("rule")}
              className="block w-full rounded px-1.5 py-1 text-left text-sm text-ink hover:bg-surface-overlay"
            >
              {t("composer.functionMenu.useRule")}
            </button>
            <button
              type="button"
              onClick={() => setMode("prompt")}
              className="block w-full rounded px-1.5 py-1 text-left text-sm text-ink hover:bg-surface-overlay"
            >
              {t("composer.functionMenu.customPrompt")}
            </button>
          </div>
        )}
        {mode === "session" && (
          <div className="space-y-0.5">
            {(sessionsQuery.data ?? []).map((s) => (
              <button
                key={s.id}
                type="button"
                disabled={injectMut.isPending}
                onClick={() => injectMut.mutate({ type: "session_ref", refId: s.id, text: null })}
                className="block w-full truncate rounded px-1.5 py-1 text-left text-sm text-ink hover:bg-surface-overlay disabled:opacity-50"
              >
                {s.title}
              </button>
            ))}
            {(sessionsQuery.data ?? []).length === 0 && (
              <div className="px-1.5 py-1 text-sm text-ink-muted">{t("composer.functionMenu.empty")}</div>
            )}
          </div>
        )}
        {mode === "rule" && (
          <div className="space-y-0.5">
            {(rulesQuery.data ?? []).map((r) => (
              <button
                key={r.id}
                type="button"
                disabled={injectMut.isPending}
                onClick={() => injectMut.mutate({ type: "rule", refId: r.id, text: null })}
                className="block w-full truncate rounded px-1.5 py-1 text-left text-sm text-ink hover:bg-surface-overlay disabled:opacity-50"
              >
                {r.name}
              </button>
            ))}
            {(rulesQuery.data ?? []).length === 0 && (
              <div className="px-1.5 py-1 text-sm text-ink-muted">{t("composer.functionMenu.empty")}</div>
            )}
          </div>
        )}
        {mode === "prompt" && (
          <div className="space-y-1.5">
            <textarea
              value={customPrompt}
              onChange={(e) => setCustomPrompt(e.target.value)}
              rows={4}
              placeholder={t("composer.functionMenu.promptPlaceholder")}
              className="w-full resize-none rounded border border-ink-muted/40 bg-surface px-1.5 py-1 text-sm text-ink placeholder:text-ink-muted focus-visible:outline-none"
            />
            {!confirmed ? (
              <button
                type="button"
                onClick={() => setConfirmed(true)}
                disabled={customPrompt.trim().length === 0}
                className="w-full rounded px-1.5 py-1 text-sm text-ink-accent hover:bg-surface-overlay disabled:opacity-50"
              >
                {t("composer.functionMenu.confirmPrompt")}
              </button>
            ) : (
              <div className="flex gap-1">
                <button
                  type="button"
                  onClick={() => injectMut.mutate({ type: "custom_prompt", refId: null, text: customPrompt })}
                  disabled={injectMut.isPending}
                  className="flex-1 rounded bg-ink-accent px-1.5 py-1 text-sm text-surface disabled:opacity-50"
                >
                  {t("common.save")}
                </button>
                <button
                  type="button"
                  onClick={() => setConfirmed(false)}
                  className="rounded px-1.5 py-1 text-sm text-ink-muted hover:text-ink"
                >
                  {t("common.cancel")}
                </button>
              </div>
            )}
          </div>
        )}
      </div>
      {/* Three category-filtered hidden inputs (chat_rich_bind 5.7.1-2a):
       * the accept filter is the first gate; the Composer's runtime
       * validation is the authority. */}
      <input
        ref={imageInputRef}
        type="file"
        multiple
        accept={IMAGE_ACCEPT}
        className="hidden"
        onChange={(e) => {
          onFilesSelected(e.target.files, "image");
          e.target.value = "";
        }}
      />
      <input
        ref={documentInputRef}
        type="file"
        multiple
        className="hidden"
        onChange={(e) => {
          onFilesSelected(e.target.files, "document");
          e.target.value = "";
        }}
      />
      <input
        ref={archiveInputRef}
        type="file"
        multiple
        accept={ARCHIVE_ACCEPT}
        className="hidden"
        onChange={(e) => {
          onFilesSelected(e.target.files, "archive");
          e.target.value = "";
        }}
      />
    </div>
  );
}
