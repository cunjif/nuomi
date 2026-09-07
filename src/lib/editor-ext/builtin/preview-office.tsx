/**
 * Builtin Office preview (Word/Excel/PPT): pragmatic "info panel" showing
 * recognized file type, size and path, with a copy-path fallback for
 * opening in the system default app.
 *
 * Deliberately NO mammoth/xlsx-style heavy deps (bundle size + maintenance):
 * rich in-app rendering is left to third-party editor extensions, which can
 * register a same-interface preview (registerPreview) and take over — see
 * the manager panel in the editor toolbar.
 *
 * "Open in system app": no opener command exists in the current IPC
 * bindings (bindings.gen has no open_path / plugin-opener import), so we
 * degrade to copying the absolute workspace path. When a Tauri opener
 * command lands, replace the copy handler with the invoke call.
 * Modification time: FileEntryDto carries only {name,isDir,size} — mtime is
 * shown as "—" until the backend adds it.
 */
import type { ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { EditorExtContext, EditorPreviewComponent } from "../types";
import { ipc } from "../../../lib/ipc/client";
import { toast } from "../../../lib/store/toastStore";

export const EXT_ID = "builtin.preview-office";

const OFFICE_RE = /\.(docx?|xlsx?|pptx?|odt|ods|odp)$/i;

function parentDir(path: string): string {
  const idx = path.lastIndexOf("/");
  return idx === -1 ? "" : path.slice(0, idx);
}

function officeKindLabel(ext: string, t: (key: string) => string): string {
  if (/^docx?$/.test(ext)) return t("editor.office.word");
  if (/^xlsx?$/.test(ext)) return t("editor.office.excel");
  if (/^pptx?$/.test(ext)) return t("editor.office.powerpoint");
  return t("editor.office.openDocument");
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

const OfficeInfoPanel: EditorPreviewComponent = ({ path }: { path: string; content: string }): ReactNode => {
  const { t } = useTranslation();
  const dir = parentDir(path);
  const name = path.slice(path.lastIndexOf("/") + 1);
  const ext = (path.match(/\.([^.]+)$/)?.[1] ?? "").toLowerCase();
  const entriesQuery = useQuery({ queryKey: ["dir", dir], queryFn: () => ipc.listDir(dir) });
  const size = entriesQuery.data?.find((e) => e.name === name)?.size ?? null;

  const copyPath = (): void => {
    void navigator.clipboard
      .writeText(path)
      .then(() => toast.success(t("editor.office.copied")))
      .catch(() => toast.error(t("editor.office.copyFailed")));
  };

  return (
    <div className="flex h-full min-w-0 flex-1 flex-col items-center justify-center gap-3 border-l border-ink-muted/30 bg-surface p-6">
      <span aria-hidden="true" className="text-3xl">
        📊
      </span>
      <h3 className="text-sm font-semibold text-ink">{name}</h3>
      <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs text-ink-muted">
        <dt className="text-right">{t("editor.office.kind")}</dt>
        <dd className="text-ink">{ext ? officeKindLabel(ext, t) : t("editor.office.unknown")}</dd>
        <dt className="text-right">{t("editor.office.size")}</dt>
        <dd className="text-ink">{size === null ? "—" : formatSize(size)}</dd>
        <dt className="text-right">{t("editor.office.modified")}</dt>
        <dd className="text-ink">—</dd>
      </dl>
      <p className="max-w-sm text-center text-xs text-ink-muted">{t("editor.office.openHint")}</p>
      <button
        type="button"
        onClick={copyPath}
        className="rounded border border-ink-muted px-3 py-1 text-xs text-ink hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
      >
        {t("editor.office.copyPath")}
      </button>
    </div>
  );
};

export const previewOfficeExtension = {
  id: EXT_ID,
  titleI18nKey: "editor.ext.previewOffice",
  contribute(ctx: EditorExtContext): void {
    ctx.registerPreview({
      mode: "replace",
      matcher: (path) => OFFICE_RE.test(path),
      component: OfficeInfoPanel,
    });
    ctx.reportCapability("preview.office");
  },
} as const;
