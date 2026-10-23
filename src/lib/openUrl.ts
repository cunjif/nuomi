/**
 * External URL opening channel — single seam for every "open link outside
 * the app" call site (MarkdownView link interception, source navigation).
 *
 * The concrete implementation is injected by the tauri-plugin-opener wiring
 * at app bootstrap (task 3.6). Failures surface one shared error toast
 * (spec 5.2.3-3) and resolve `false` so callers can keep the URL for
 * copying. In-app rendering of arbitrary external pages is forbidden
 * (spec 4.3-4) — only the system browser may load them.
 */
import { i18n } from "../i18n";
import { toast } from "./store/toastStore";

type OpenUrlImpl = (url: string) => Promise<boolean>;

let impl: OpenUrlImpl = async () => false;

/** Installs the opener implementation (called once at bootstrap). */
export function setOpenUrlImpl(next: OpenUrlImpl): void {
  impl = next;
}

/** Opens `url` with the system browser; toasts and resolves `false` on
 * failure (the URL stays in the source list for manual copying). */
export async function openExternalUrl(url: string): Promise<boolean> {
  const ok = await impl(url);
  if (!ok) {
    toast.error(i18n.t("chat.sources.openFailed"));
  }
  return ok;
}
