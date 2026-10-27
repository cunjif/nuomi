export type SandboxUrlValidation = { valid: boolean; reason?: string };

export function validateSandboxUrl(url: string): SandboxUrlValidation {
  if (!url || url.trim() === "") {
    return { valid: false, reason: "url is empty" };
  }
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return { valid: false, reason: "url is not parseable" };
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    return { valid: false, reason: "protocol must be http or https" };
  }
  return { valid: true };
}
