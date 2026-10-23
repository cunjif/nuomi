/**
 * One-shot read-only viewer for an EXTERNAL file (absolute path outside
 * every workspace sandbox — the source-navigation "external file" branch,
 * chat_rich_bind spec 5.2.1-6 / D10). Memory-only tab state: destroyed on
 * workspace switch and app exit; never added to work_directories or the
 * persistent editor buckets.
 */
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";
import { useUiStore } from "../../lib/store/uiStore";

export function TempFileTab(): ReactNode {
  const { t } = useTranslation();
  const tempFileTab = useUiStore((s) => s.tempFileTab);
  const closeTempFile = useUiStore((s) => s.closeTempFile);

  const contentQuery = useQuery({
    queryKey: ["externalFile", tempFileTab?.absPath],
    queryFn: () => ipc.readExternalFile(tempFileTab?.absPath ?? ""),
    enabled: tempFileTab !== null,
    retry: false,
  });

  if (tempFileTab === null) return null;

  return (
    <div data-testid="temp-file-tab" className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center justify-between gap-2 border-b border-ink-muted/30 px-3 py-1.5">
        <div className="flex min-w-0 items-center gap-2">
          <span className="rounded-sm bg-surface-overlay px-1 py-0.5 text-[10px] uppercase text-ink-muted">
            {t("chat.sources.tempFile")}
          </span>
          <span className="truncate text-sm text-ink" title={tempFileTab.absPath}>
            {tempFileTab.name}
          </span>
        </div>
        <button
          type="button"
          aria-label={t("chat.sources.closeTempFile")}
          onClick={closeTempFile}
          className="rounded-full p-1 text-ink-muted hover:bg-surface hover:text-ink"
        >
          <svg viewBox="0 0 16 16" className="h-3.5 w-3.5 fill-current">
            <path d="M3.7 2.3L2.3 3.7 6.6 8l-4.3 4.3 1.4 1.4L8 9.4l4.3 4.3 1.4-1.4L9.4 8l4.3-4.3-1.4-1.4L8 6.6 3.7 2.3z" />
          </svg>
        </button>
      </div>
      {contentQuery.isLoading ? (
        <div className="p-4 text-sm text-ink-muted">{t("chat.sources.loading")}</div>
      ) : contentQuery.isError ? (
        <div className="p-4 text-sm text-ink-muted">{t("chat.sources.readFailed")}</div>
      ) : (
        <pre className="min-h-0 flex-1 overflow-auto p-3 text-xs leading-5 text-ink">
          {contentQuery.data}
        </pre>
      )}
    </div>
  );
}
