/**
 * Attachment model: size limits, MIME whitelist, formatting utilities.
 */

/** Max attachment size (25 MB). */
export const MAX_ATTACHMENT_BYTES = 25 * 1024 * 1024;

/** Max attachments per message. */
export const MAX_ATTACHMENTS_PER_MESSAGE = 20;

/** Large-text threshold: text >= this becomes an attachment instead of inline. */
export const LARGE_TEXT_THRESHOLD_BYTES = 8 * 1024;

/** MIME whitelist prefix patterns. */
const MIME_PREFIX_WHITELIST = ["image/", "text/"];
const MIME_EXACT_WHITELIST = [
  "application/pdf",
  "application/json",
  "application/x-yaml",
  "application/yaml",
  "application/javascript",
  "application/typescript",
  "application/x-sh",
  "application/x-python",
  "application/x-rust",
  "application/x-go",
  "application/x-toml",
  "application/xml",
  "application/csv",
];

/** Returns true when the MIME type is allowed. */
export function isAllowedMime(mime: string): boolean {
  if (MIME_EXACT_WHITELIST.includes(mime)) return true;
  return MIME_PREFIX_WHITELIST.some((p) => mime.startsWith(p));
}

/** Returns true when the byte size is within limits. */
export function isWithinSize(sizeBytes: number): boolean {
  return sizeBytes > 0 && sizeBytes <= MAX_ATTACHMENT_BYTES;
}

/** Human-readable file size. */
export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

  /** UTF-8-safe base64 of a text payload (chunked to avoid arg limits). */
  export function toBase64(text: string): string {
    const bytes = new TextEncoder().encode(text);
    let binary = "";
    const CHUNK = 0x8000;
    for (let i = 0; i < bytes.length; i += CHUNK) {
      binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
    }
    return btoa(binary);
  }

  /** File extension from a filename (lowercase, no dot). */
  export function fileExt(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot < 0 ? "bin" : name.slice(dot + 1).toLowerCase();
}

/** Guess MIME from filename extension. */
export function guessMime(name: string): string {
  const ext = fileExt(name);
  const map: Record<string, string> = {
    png: "image/png",
    jpg: "image/jpeg",
    jpeg: "image/jpeg",
    gif: "image/gif",
    webp: "image/webp",
    svg: "image/svg+xml",
    txt: "text/plain",
    md: "text/markdown",
    json: "application/json",
    pdf: "application/pdf",
    yaml: "application/x-yaml",
    yml: "application/x-yaml",
    ts: "text/typescript",
    tsx: "text/typescript",
    js: "text/javascript",
    rs: "text/x-rust",
    py: "text/x-python",
    go: "text/x-go",
    sh: "application/x-sh",
    toml: "application/x-toml",
    xml: "application/xml",
    csv: "application/csv",
  };
  return map[ext] ?? "application/octet-stream";
}
