/**
 * In-bubble entry point for the sandboxed HTML page preview (spec 5.1.1-4):
 * when the message text is detected as a full HTML document (D3 rules), the
 * bubble still renders it as regular sanitized Markdown AND shows this
 * preview affordance; clicking opens the HtmlPreviewDialog with the whole
 * page rendering.
 */
import { useMemo, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { detectHtmlDocument } from "./htmlDocument";
import { HtmlPreviewDialog } from "./HtmlPreviewDialog";

export interface HtmlPreviewEntryProps {
  /** Raw message text to scan for a full HTML document. */
  text: string;
}

export function HtmlPreviewEntry({ text }: HtmlPreviewEntryProps): ReactNode {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const detected = useMemo(() => detectHtmlDocument(text), [text]);

  if (detected === null) return null;

  return (
    <>
      <button
        type="button"
        data-testid="html-preview-entry"
        onClick={() => setOpen(true)}
        className="mt-1 inline-flex items-center gap-1 rounded-full bg-surface-overlay px-2 py-0.5 text-xs text-ink-accent transition-colors hover:bg-surface-raised"
      >
        <svg viewBox="0 0 16 16" className="h-3 w-3 fill-current" aria-hidden="true">
          <path d="M2 3h12v10H2V3zm1 2v7h10V5H3zm1 1h3v1H4V6zm0 2h3v1H4V8zm5-2h3v1H9V6zm0 2h3v1H9V8z" />
        </svg>
        {t("chat.htmlPreview.open")}
      </button>
      <HtmlPreviewDialog open={open} source={detected.source} onClose={() => setOpen(false)} />
    </>
  );
}
