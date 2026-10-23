/**
 * Sandboxed full-page HTML preview dialog (spec 5.1.1-4/5, design D2/D3).
 *
 * The agent's HTML document renders inside an iframe with `sandbox=""` (no
 * scripts, no same-origin access, no plugins) fed via `srcdoc`, with a CSP
 * meta injected ahead of the content (no network, inline styles only, data:
 * images). The dialog can be closed, dragged-resized via the corner handle,
 * and degrades to a source-text view when rendering fails (iframe error or
 * the 3s first-frame timeout — spec 5.1.3-2).
 */
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { injectPreviewCsp } from "./htmlDocument";

export interface HtmlPreviewDialogProps {
  open: boolean;
  /** Raw HTML document source (already extracted by detectHtmlDocument). */
  source: string;
  onClose: () => void;
}

const MIN_WIDTH = 320;
const MIN_HEIGHT = 240;
const DEFAULT_WIDTH = 720;
const DEFAULT_HEIGHT = 480;
const FIRST_FRAME_TIMEOUT_MS = 3_000;

export function HtmlPreviewDialog({ open, source, onClose }: HtmlPreviewDialogProps): ReactNode {
  const { t } = useTranslation();
  const [size, setSize] = useState({ width: DEFAULT_WIDTH, height: DEFAULT_HEIGHT });
  const [failed, setFailed] = useState(false);
  const [showSource, setShowSource] = useState(false);
  const frameRef = useRef<HTMLIFrameElement | null>(null);
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const resizing = useRef(false);

  const srcdoc = useMemo(() => injectPreviewCsp(source), [source]);

  useEffect(() => {
    if (!open) {
      setFailed(false);
      setShowSource(false);
      setSize({ width: DEFAULT_WIDTH, height: DEFAULT_HEIGHT });
    }
  }, [open]);

  // First-frame watchdog: onload must fire within the timeout; a native
  // error listener on the frame element drives the failure branch (React's
  // synthetic onError is unreliable for resource errors in jsdom).
  useEffect(() => {
    if (!open) return;
    let loaded = false;
    const markFailed = (): void => setFailed(true);
    const timer = window.setTimeout(() => {
      if (!loaded) setFailed(true);
    }, FIRST_FRAME_TIMEOUT_MS);
    const frame = frameRef.current;
    if (frame !== null) {
      const markLoaded = (): void => {
        loaded = true;
      };
      frame.addEventListener("load", markLoaded);
      frame.addEventListener("error", markFailed);
      return () => {
        frame.removeEventListener("load", markLoaded);
        frame.removeEventListener("error", markFailed);
        window.clearTimeout(timer);
      };
    }
    return () => window.clearTimeout(timer);
  }, [open, srcdoc]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  const onResizeStart = useCallback(
    (event: React.MouseEvent<HTMLDivElement>) => {
      event.preventDefault();
      resizing.current = true;
      const startX = event.clientX;
      const startY = event.clientY;
      const startW = size.width;
      const startH = size.height;
      const onMove = (e: MouseEvent): void => {
        if (!resizing.current) return;
        const nextW = Math.max(MIN_WIDTH, startW + (e.clientX - startX));
        const nextH = Math.max(MIN_HEIGHT, startH + (e.clientY - startY));
        setSize({ width: nextW, height: nextH });
      };
      const onUp = (): void => {
        resizing.current = false;
        window.removeEventListener("mousemove", onMove);
        window.removeEventListener("mouseup", onUp);
      };
      window.addEventListener("mousemove", onMove);
      window.addEventListener("mouseup", onUp);
    },
    [size.width, size.height],
  );

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-surface-scrim p-4"
      onClick={onClose}
      role="presentation"
    >
      <div
        ref={dialogRef}
        role="dialog"
        aria-modal="true"
        aria-label={t("chat.htmlPreview.title")}
        data-testid="html-preview-dialog"
        onClick={(e) => e.stopPropagation()}
        style={{ width: size.width, height: size.height }}
        className="flex flex-col overflow-hidden rounded-lg bg-surface-raised shadow-sketch-lg"
      >
        <div className="flex shrink-0 items-center justify-between gap-2 border-b border-ink-muted/30 px-3 py-2">
          <h2 className="font-title-hand text-sm text-ink">{t("chat.htmlPreview.title")}</h2>
          <button
            type="button"
            aria-label={t("chat.htmlPreview.close")}
            onClick={onClose}
            className="rounded-full p-1 text-ink-muted hover:bg-surface hover:text-ink"
          >
            <svg viewBox="0 0 16 16" className="h-3.5 w-3.5 fill-current">
              <path d="M3.7 2.3L2.3 3.7 6.6 8l-4.3 4.3 1.4 1.4L8 9.4l4.3 4.3 1.4-1.4L9.4 8l4.3-4.3-1.4-1.4L8 6.6 3.7 2.3z" />
            </svg>
          </button>
        </div>
        <div className="relative min-h-0 flex-1 bg-surface">
          {failed ? (
            <div className="flex h-full flex-col items-center justify-center gap-3 p-4 text-center">
              <p className="text-sm text-ink-muted">{t("chat.htmlPreview.renderFailed")}</p>
              <button
                type="button"
                onClick={() => setShowSource(true)}
                className="rounded bg-surface-raised px-3 py-1.5 text-sm text-ink hover:bg-surface-overlay"
              >
                {t("chat.htmlPreview.viewSource")}
              </button>
              {showSource && (
                <pre
                  data-testid="html-preview-source"
                  className="max-h-[60%] w-full overflow-auto rounded bg-surface-overlay p-2 text-left text-xs text-ink"
                >
                  {source}
                </pre>
              )}
            </div>
          ) : (
            <iframe
              ref={frameRef}
              title={t("chat.htmlPreview.title")}
              data-testid="html-preview-frame"
              sandbox=""
              srcDoc={srcdoc}
              className="h-full w-full border-0 bg-white"
            />
          )}
        </div>
        <div
          onMouseDown={onResizeStart}
          aria-label={t("chat.htmlPreview.resize")}
          data-testid="html-preview-resize-handle"
          className="absolute bottom-0 right-0 h-4 w-4 cursor-nwse-resize"
          style={{
            background:
              "linear-gradient(135deg, transparent 50%, rgba(127,127,127,0.5) 50%)",
          }}
        />
      </div>
    </div>
  );
}
