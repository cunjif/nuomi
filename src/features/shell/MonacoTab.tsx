import type { ReactNode } from "react";
import { lazy, Suspense, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Spinner } from "../../components/ui/Spinner";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { useTheme } from "../../lib/store/useTheme";
import { toast } from "../../lib/store/toastStore";
import { useUiStore } from "../../lib/store/uiStore";
import { NUOMI_MONACO_DARK, NUOMI_MONACO_LIGHT, defineNuomiThemes } from "./monacoThemes";
import { languageForPath } from "./editorLanguage";

// Self-hosted Monaco: bundle the editor locally instead of the default CDN
// loader so the desktop app works fully offline. The setup module is pulled
// in lazily together with the editor chunk.
const MonacoEditor = lazy(async () => {
  await import("./monacoSetup");
  return import("@monaco-editor/react");
});

interface MonacoTabProps {
  path: string;
}

/** One open file: lazy Monaco editor + Ctrl/Cmd+S save with toast feedback. */
export function MonacoTab({ path }: MonacoTabProps): ReactNode {
  const { t } = useTranslation();
  const { theme } = useTheme();
  const qc = useQueryClient();
  const markDirty = useUiStore((s) => s.markDirty);
  const fileQuery = useQuery({ queryKey: ["file", path], queryFn: () => ipc.readFile(path) });
  const [draft, setDraft] = useState<string | null>(null);
  const value = draft ?? fileQuery.data ?? "";

  const saveMut = useMutation({
    mutationFn: () => ipc.writeFile(path, value),
    onSuccess: () => {
      setDraft(null);
      markDirty(path, false);
      void qc.invalidateQueries({ queryKey: ["file", path] });
      toast.success(t("files.saved"));
    },
    onError: (e) => toast.error(describeError(e)),
  });

  const save = (): void => {
    if (!saveMut.isPending && !fileQuery.isLoading) saveMut.mutate();
  };

  const onKeyDown = (e: React.KeyboardEvent): void => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
      e.preventDefault();
      save();
    }
  };

  return (
    <section aria-label={path} onKeyDown={onKeyDown} className="flex min-w-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center justify-between gap-2 border-b border-ink-muted/30 px-2 py-1">
        <span className="truncate font-mono text-xs text-ink-muted" title={path}>
          {path}
        </span>
        <button
          type="button"
          onClick={save}
          disabled={saveMut.isPending || fileQuery.isLoading}
          className="shrink-0 rounded border border-ink-muted px-2 py-0.5 text-xs text-ink hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
        >
          {t("files.save")}
        </button>
      </div>
      <div className="min-h-0 flex-1">
        {fileQuery.isError ? (
          <p className="p-3 text-sm text-state-danger">{describeError(fileQuery.error)}</p>
        ) : fileQuery.isLoading ? (
          <div className="p-3">
            <Spinner label={t("files.loadingFile")} />
          </div>
        ) : (
          <Suspense
            fallback={
              <div className="p-3">
                <Spinner />
              </div>
            }
          >
            <MonacoEditor
              height="100%"
              defaultLanguage={languageForPath(path)}
              beforeMount={defineNuomiThemes}
              theme={theme === "dark" ? NUOMI_MONACO_DARK : NUOMI_MONACO_LIGHT}
              value={value}
              path={path}
              onChange={(v) => {
                const next = v ?? "";
                setDraft(next);
                markDirty(path, next !== (fileQuery.data ?? ""));
              }}
              options={{
                fontSize: 13,
                minimap: { enabled: false },
                tabSize: 2,
                renderWhitespace: "selection",
                smoothScrolling: true,
                scrollBeyondLastLine: false,
              }}
            />
          </Suspense>
        )}
      </div>
      <div className="flex shrink-0 items-center justify-between gap-2 border-t border-ink-muted/30 px-2 py-0.5 text-[10px] text-ink-muted">
        <span>
          {t("editor.languageLabel")}: <span className="font-mono">{languageForPath(path)}</span>
        </span>
        <span>{t("editor.saveHint")}</span>
      </div>
    </section>
  );
}
