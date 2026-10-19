/**
 * Steward attachment gate (task 4.4): frontend pre-check mirroring the
 * backend whitelist in `nuomi-core/src/steward/attachments.rs`
 * (`STEWARD_ALLOWED_MIMES`). The backend stays authoritative — this only
 * gives immediate feedback in the composer before a save round-trip.
 */

/**
 * Default per-file cap (design §2.1: `steward_attachment_max_bytes`, 10 MB).
 * Fallback only — the live cap comes from `stewardGetAttachmentLimit`
 * (app_settings); the backend save stays authoritative either way.
 */
export const STEWARD_MAX_ATTACHMENT_BYTES = 10 * 1024 * 1024;

/** Steward MIME whitelist — exact matches only (mirrors backend, 25 entries). */
const STEWARD_MIME_WHITELIST: ReadonlySet<string> = new Set([
  "text/plain",
  "text/markdown",
  "application/pdf",
  "application/msword",
  "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
  "application/vnd.ms-excel",
  "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
  "application/vnd.ms-powerpoint",
  "application/vnd.openxmlformats-officedocument.presentationml.presentation",
  "application/json",
  "application/yaml",
  "application/x-yaml",
  "application/javascript",
  "application/typescript",
  "application/x-sh",
  "application/x-python",
  "application/x-rust",
  "application/x-go",
  "application/x-toml",
  "application/xml",
  "application/csv",
  "image/png",
  "image/jpeg",
  "image/gif",
  "image/webp",
]);

/**
 * Extension → whitelisted MIME. Extension wins over the browser-reported
 * type: browsers map `.ts` to `video/mp2t` and leave `.rs`/`.go`/`.toml`
 * empty, while the steward whitelist is exact-match.
 */
const EXT_TO_STEWARD_MIME: Record<string, string> = {
  txt: "text/plain",
  md: "text/markdown",
  markdown: "text/markdown",
  pdf: "application/pdf",
  doc: "application/msword",
  docx: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
  xls: "application/vnd.ms-excel",
  xlsx: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
  ppt: "application/vnd.ms-powerpoint",
  pptx: "application/vnd.openxmlformats-officedocument.presentationml.presentation",
  json: "application/json",
  yaml: "application/x-yaml",
  yml: "application/x-yaml",
  js: "application/javascript",
  mjs: "application/javascript",
  jsx: "application/javascript",
  ts: "application/typescript",
  tsx: "application/typescript",
  sh: "application/x-sh",
  py: "application/x-python",
  rs: "application/x-rust",
  go: "application/x-go",
  toml: "application/x-toml",
  xml: "application/xml",
  csv: "application/csv",
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
};

/** True when the MIME type is on the steward whitelist (exact match). */
export function isStewardAllowedMime(mime: string): boolean {
  return STEWARD_MIME_WHITELIST.has(mime);
}

function fileExt(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot < 0 ? "" : name.slice(dot + 1).toLowerCase();
}

/**
 * Resolve the steward MIME for a picked file, or `null` when unsupported.
 * Extension mapping first (deterministic for source-code extensions the
 * browser misreports), then the browser-reported type if it is whitelisted
 * (covers whitelist members without a mapping table entry, e.g. `.log`
 * reported as `text/plain`).
 */
export function guessStewardMime(name: string, browserType: string): string | null {
  const mapped = EXT_TO_STEWARD_MIME[fileExt(name)];
  if (mapped) return mapped;
  if (browserType !== "" && isStewardAllowedMime(browserType)) return browserType;
  return null;
}
