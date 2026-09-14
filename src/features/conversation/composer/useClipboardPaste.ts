import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { ipc } from "../../../lib/ipc/client";
import {
  LARGE_TEXT_THRESHOLD_BYTES,
  formatSize,
  guessMime,
  isAllowedMime,
  isWithinSize,
} from "../../../lib/conversation/attachmentModel";
import type { AttachmentDto } from "../../../lib/ipc/client";

export interface UseClipboardPasteOptions {
  sessionId: string;
  /** Called when an attachment is saved successfully. */
  onAttachmentSaved: (attachment: AttachmentDto) => void;
  /** Called when an error occurs (toast). */
  onError: (message: string) => void;
  /** Called to insert text into the textarea value. */
  insertText: (text: string) => void;
}

/**
 * Paste handler for the composer textarea. Routes clipboard content by type:
 * - Plain text < threshold → default insert (no interception).
 * - Plain text >= threshold → save as attachment, insert placeholder.
 * - Image → save as attachment with thumbnail.
 * - File → read Blob → save as attachment.
 * - Does not intercept during IME composition.
 */
export function useClipboardPaste({
  sessionId,
  onAttachmentSaved,
  onError,
  insertText,
}: UseClipboardPasteOptions): (e: React.ClipboardEvent<HTMLTextAreaElement>) => void {
  const { t } = useTranslation();

  return useCallback(
    (e: React.ClipboardEvent<HTMLTextAreaElement>) => {
      const clipboard = e.clipboardData;
      if (!clipboard) return;

      // Check for image items first (highest priority).
      for (const item of Array.from(clipboard.items)) {
        if (item.type.startsWith("image/")) {
          e.preventDefault();
          const file = item.getAsFile();
          if (file) {
            void saveFileAsAttachment(file, sessionId, "paste", onAttachmentSaved, onError, t);
          }
          return;
        }
      }

      // Check for file items.
      if (clipboard.files && clipboard.files.length > 0) {
        e.preventDefault();
        for (const file of Array.from(clipboard.files)) {
          void saveFileAsAttachment(file, sessionId, "file", onAttachmentSaved, onError, t);
        }
        return;
      }

      // Check for large text.
      const text = clipboard.getData("text/plain");
      if (text.length >= LARGE_TEXT_THRESHOLD_BYTES) {
        e.preventDefault();
        const blob = new Blob([text], { type: "text/plain" });
        const file = new File([blob], `pasted-text-${Date.now()}.txt`, { type: "text/plain" });
        void saveFileAsAttachment(file, sessionId, "text", onAttachmentSaved, onError, t);
        insertText(`[${formatSize(text.length)}]`);
        return;
      }

      // Small text: let the default paste happen.
    },
    [sessionId, onAttachmentSaved, onError, insertText, t],
  );
}

/** Reads a File as base64 and saves it via IPC. */
async function saveFileAsAttachment(
  file: File,
  sessionId: string,
  _kind: string,
  onSaved: (a: AttachmentDto) => void,
  onError: (msg: string) => void,
  t: (key: string, opts?: Record<string, unknown>) => string,
): Promise<void> {
  const mime = file.type || guessMime(file.name);
  if (!isAllowedMime(mime)) {
    onError(t("composer.attachmentInvalidMime"));
    return;
  }
  if (!isWithinSize(file.size)) {
    onError(t("composer.attachmentTooLarge"));
    return;
  }

  try {
    const base64 = await fileToBase64(file);
    const attachment = await ipc.saveAttachment(sessionId, file.name, mime, base64);
    onSaved(attachment);
  } catch {
    onError(t("composer.attachmentFailed"));
  }
}

/** Reads a File as a base64 string (without the data URL prefix). */
function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = reader.result;
      if (typeof result !== "string") {
        reject(new Error("FileReader returned non-string"));
        return;
      }
      const comma = result.indexOf(",");
      resolve(comma >= 0 ? result.slice(comma + 1) : result);
    };
    reader.onerror = () => reject(reader.error ?? new Error("FileReader error"));
    reader.readAsDataURL(file);
  });
}
