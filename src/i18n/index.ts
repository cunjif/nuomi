/**
 * i18n bootstrap — zh-CN is the default resource language with a parallel
 * `en` bundle (typescript-react rule: every user-facing string goes through
 * i18n resources).
 */
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import en from "./locales/en.json";
import zhCn from "./locales/zh-CN.json";

export const i18n = i18next.createInstance();

void i18n.use(initReactI18next).init({
  lng: "zh-CN",
  fallbackLng: "zh-CN",
  resources: {
    "zh-CN": { translation: zhCn },
    en: { translation: en },
  },
  interpolation: { escapeValue: false },
  react: { useSuspense: false },
});

/** Human-readable message for any thrown error, mapping stable IPC codes. */
export function describeError(error: unknown): string {
  const code = (error as { code?: unknown })?.code;
  if (typeof code === "string") {
    const key = `errors.${code.replaceAll(".", "_")}`;
    if (i18n.exists(key)) return i18n.t(key);
  }
  const message = error instanceof Error ? error.message : String(error);
  return i18n.t("errors.generic", { message });
}
