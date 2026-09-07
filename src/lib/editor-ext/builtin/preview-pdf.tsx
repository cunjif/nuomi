/**
 * Builtin PDF preview. LIMITATION (documented degradation): the intended
 * implementation is <iframe src="data:application/pdf;base64,...">, but the
 * workspace readFile IPC returns a UTF-8 string (bindings.gen:
 * readFile(path) => Result<string>) and cannot deliver the binary bytes a
 * base64 data URL needs. Until the backend gains a binary read command this
 * extension renders an explicit placeholder panel. The registration
 * (matcher + component) is the integration point — replacing the
 * placeholder with the iframe needs no consumer change.
 */
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { EditorExtContext, EditorPreviewComponent } from "../types";

export const EXT_ID = "builtin.preview-pdf";

const PDF_RE = /\.pdf$/i;

const PdfPlaceholder: EditorPreviewComponent = (): ReactNode => {
  const { t } = useTranslation();
  return (
    <div
      role="status"
      className="flex h-full min-w-0 flex-1 flex-col items-center justify-center gap-2 border-l border-ink-muted/30 bg-surface p-6 text-center text-sm text-ink-muted"
    >
      <span aria-hidden="true" className="text-2xl">
        📄
      </span>
      <p>{t("editor.previewBinaryUnsupported")}</p>
    </div>
  );
};

export const previewPdfExtension = {
  id: EXT_ID,
  titleI18nKey: "editor.ext.previewPdf",
  contribute(ctx: EditorExtContext): void {
    ctx.registerPreview({
      mode: "replace",
      matcher: (path) => PDF_RE.test(path),
      component: PdfPlaceholder,
    });
    ctx.reportCapability("preview.pdf");
  },
} as const;
