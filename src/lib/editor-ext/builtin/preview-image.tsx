/**
 * Builtin image preview. LIMITATION (documented degradation): the workspace
 * readFile IPC returns a UTF-8 string (bindings.gen: readFile(path) =>
 * Result<string>), so binary image bytes cannot be recovered to build a
 * base64 data URL — until the backend gains a binary read command this
 * extension renders an explicit "needs binary read support" placeholder
 * instead of a broken <img>. The registration (matcher + component) is the
 * integration point: swap the placeholder for a data-URL <img> once binary
 * IPC lands, no consumer change required.
 */
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { EditorExtContext, EditorPreviewComponent } from "../types";

export const EXT_ID = "builtin.preview-image";

const IMAGE_RE = /\.(png|jpe?g|gif|bmp|webp|svg|ico|avif)$/i;

/** Non-SVG images need binary bytes; SVG is text and CAN be rendered safely? No — SVG can carry scripts, so it goes through the same placeholder. */
const BinaryPlaceholder: EditorPreviewComponent = (): ReactNode => {
  const { t } = useTranslation();
  return (
    <div
      role="status"
      className="flex h-full min-w-0 flex-1 flex-col items-center justify-center gap-2 border-l border-ink-muted/30 bg-surface p-6 text-center text-sm text-ink-muted"
    >
      <span aria-hidden="true" className="text-2xl">
        🖼
      </span>
      <p>{t("editor.previewBinaryUnsupported")}</p>
    </div>
  );
};

export const previewImageExtension = {
  id: EXT_ID,
  titleI18nKey: "editor.ext.previewImage",
  contribute(ctx: EditorExtContext): void {
    ctx.registerPreview({
      mode: "replace",
      matcher: (path) => IMAGE_RE.test(path),
      component: BinaryPlaceholder,
    });
    ctx.reportCapability("preview.image");
  },
} as const;
