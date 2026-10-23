import { describe, expect, it } from "vitest";
import {
  ARCHIVE_EXTENSIONS,
  attachmentCategory,
  guessMime,
  isAllowedMime,
  isArchiveName,
} from "./attachmentModel";

describe("attachmentModel — archive whitelist (chat_rich_bind 5.7 / D6)", () => {
  it("admits zip/tar/gz/7z and rejects rar", () => {
    for (const ext of ARCHIVE_EXTENSIONS) {
      expect(isAllowedMime(guessMime(`bundle.${ext}`))).toBe(true);
      expect(isArchiveName(`bundle.${ext}`)).toBe(true);
    }
    expect(guessMime("bundle.rar")).toBe("application/octet-stream");
    expect(isAllowedMime(guessMime("bundle.rar"))).toBe(false);
    expect(isArchiveName("bundle.rar")).toBe(false);
  });

  it("classifies attachments into image/document/archive (D8 runtime derivation)", () => {
    expect(attachmentCategory("image/png", "shot.png")).toBe("image");
    expect(attachmentCategory("image/png", "shot.txt")).toBe("image");
    expect(attachmentCategory("application/pdf", "doc.pdf")).toBe("document");
    expect(attachmentCategory("text/plain", "notes.txt")).toBe("document");
    expect(attachmentCategory("application/zip", "bundle.zip")).toBe("archive");
    expect(attachmentCategory("application/octet-stream", "bundle.7z")).toBe("archive");
    // Unknown mime + known image extension falls back to the extension.
    expect(attachmentCategory("application/octet-stream", "photo.webp")).toBe("image");
  });

  it("keeps the existing image/document whitelist intact (spec 5.7.1-6a)", () => {
    expect(isAllowedMime("image/png")).toBe(true);
    expect(isAllowedMime("text/markdown")).toBe(true);
    expect(isAllowedMime("application/pdf")).toBe(true);
    expect(isAllowedMime("application/json")).toBe(true);
    expect(isAllowedMime("application/x-executable")).toBe(false);
  });
});
