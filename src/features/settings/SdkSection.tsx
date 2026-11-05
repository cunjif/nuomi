/**
 * SDK & Runtime configuration section for the Settings page.
 * Lets users configure the run command and arguments for each supported
 * language, and the build command for each detected project type.
 * Each language/build row is expandable to reveal advanced config.
 * When a command availability check fails, an error icon lets the user
 * ask the Steward agent for help via a dialog.
 */
import { useState, useCallback, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useSdkConfigs } from "../shell/run/useSdkConfigs";
import {
  DEFAULT_LANGUAGE_CONFIGS,
  DEFAULT_BUILD_CONFIGS,
} from "../shell/run/sdkConfigs";
import { ipc } from "../../lib/ipc/client";
import { Icon } from "../../components/ui/Icon/Icon";
import { StewardDialog } from "./StewardDialog";

const LANGUAGE_LABELS: Record<string, string> = {
  python: "Python",
  javascript: "JavaScript",
  typescript: "TypeScript",
  go: "Go",
  rust: "Rust",
  java: "Java",
  c: "C",
  cpp: "C++",
  shell: "Shell",
};

const BUILD_LABELS: Record<string, string> = {
  node: "Node.js",
  cargo: "Rust (Cargo)",
  go: "Go",
  maven: "Java (Maven)",
  gradle: "Java (Gradle)",
  make: "Makefile",
  cmake: "CMake",
};

type CheckKey = string;

export function SdkSection(): ReactNode {
  const { t } = useTranslation();
  const { configs, updateLanguage, updateBuild, resetToDefaults } = useSdkConfigs();
  const [unavailableCommands, setUnavailableCommands] = useState<Set<CheckKey>>(new Set());
  const [checking, setChecking] = useState<Set<CheckKey>>(new Set());
  const [expandedRows, setExpandedRows] = useState<Set<string>>(new Set());
  const [stewardOpen, setStewardOpen] = useState(false);
  const [stewardMessage, setStewardMessage] = useState("");

  const toggleRow = useCallback((rowKey: string) => {
    setExpandedRows((prev) => {
      const next = new Set(prev);
      if (next.has(rowKey)) {
        next.delete(rowKey);
      } else {
        next.add(rowKey);
      }
      return next;
    });
  }, []);

  const openSteward = useCallback((message: string) => {
    setStewardMessage(message);
    setStewardOpen(true);
  }, []);

  const handleToggleLanguage = useCallback(
    (lang: string, command: string, currentlyEnabled: boolean) => {
      updateLanguage(lang, { enabled: !currentlyEnabled });
      if (!currentlyEnabled && command.trim().length > 0) {
        const key = `lang:${lang}`;
        setChecking((prev) => new Set(prev).add(key));
        ipc
          .checkCommandAvailable(command.trim())
          .then((result) => {
            setUnavailableCommands((prev) => {
              const next = new Set(prev);
              if (result.ok) {
                next.delete(key);
              } else {
                next.add(key);
              }
              return next;
            });
          })
          .catch(() => {
            setUnavailableCommands((prev) => new Set(prev).add(key));
          })
          .finally(() => {
            setChecking((prev) => {
              const next = new Set(prev);
              next.delete(key);
              return next;
            });
          });
      } else {
        const key = `lang:${lang}`;
        setUnavailableCommands((prev) => {
          const next = new Set(prev);
          next.delete(key);
          return next;
        });
      }
    },
    [updateLanguage],
  );

  const handleToggleBuild = useCallback(
    (buildKey: string, command: string, currentlyEnabled: boolean) => {
      updateBuild(buildKey, { enabled: !currentlyEnabled });
      if (!currentlyEnabled && command.trim().length > 0) {
        const key = `build:${buildKey}`;
        setChecking((prev) => new Set(prev).add(key));
        ipc
          .checkCommandAvailable(command.trim())
          .then((result) => {
            setUnavailableCommands((prev) => {
              const next = new Set(prev);
              if (result.ok) {
                next.delete(key);
              } else {
                next.add(key);
              }
              return next;
            });
          })
          .catch(() => {
            setUnavailableCommands((prev) => new Set(prev).add(key));
          })
          .finally(() => {
            setChecking((prev) => {
              const next = new Set(prev);
              next.delete(key);
              return next;
            });
          });
      } else {
        const key = `build:${buildKey}`;
        setUnavailableCommands((prev) => {
          const next = new Set(prev);
          next.delete(key);
          return next;
        });
      }
    },
    [updateBuild],
  );

  const languageKeys = Object.keys(DEFAULT_LANGUAGE_CONFIGS);
  const buildKeys = Object.keys(DEFAULT_BUILD_CONFIGS);

  return (
    <section className="space-y-4">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-sm font-semibold text-ink">{t("settings.sdk.heading")}</h2>
          <p className="text-xs text-ink-muted">{t("settings.sdk.hint")}</p>
        </div>
        <button
          type="button"
          onClick={resetToDefaults}
          className="rounded border border-ink-muted/40 px-2 py-1 text-xs text-ink-muted hover:bg-surface-overlay hover:text-ink-accent"
        >
          {t("settings.sdk.reset")}
        </button>
      </div>

      <div>
        <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-ink-muted">
          {t("settings.sdk.languagesHeading")}
        </h3>
        <ul className="space-y-1.5" data-testid="sdk-language-list">
          {languageKeys.map((lang) => {
            const cfg = configs.languages[lang];
            if (cfg === undefined) return null;
            const checkKey = `lang:${lang}`;
            const rowKey = `lang-row:${lang}`;
            const isUnavailable = unavailableCommands.has(checkKey);
            const isChecking = checking.has(checkKey);
            const isExpanded = expandedRows.has(rowKey);
            const label = LANGUAGE_LABELS[lang] ?? lang;
            return (
              <li
                key={lang}
                className="rounded border border-ink-muted/30 bg-surface"
                data-testid={`sdk-lang-${lang}-row`}
              >
                <div className="flex items-center gap-2 px-3 py-2">
                  <button
                    type="button"
                    onClick={() => toggleRow(rowKey)}
                    aria-label={isExpanded ? t("settings.sdk.collapse") : t("settings.sdk.expand")}
                    aria-expanded={isExpanded}
                    className="shrink-0 rounded p-0.5 text-ink-muted hover:bg-surface-overlay hover:text-ink"
                    data-testid={`sdk-lang-${lang}-expand`}
                  >
                    <Icon name="chevron-down" size={14} className={isExpanded ? "" : "-rotate-90"} />
                  </button>
                  <span className="w-20 shrink-0 text-sm text-ink">{label}</span>
                  <input
                    type="text"
                    value={cfg.command}
                    onChange={(e) => updateLanguage(lang, { command: e.target.value })}
                    className="min-w-0 flex-[2] rounded border border-ink-muted/30 bg-surface-raised px-2 py-1 text-xs text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
                    aria-label={`${label} command`}
                    data-testid={`sdk-lang-${lang}-command`}
                  />
                  <input
                    type="text"
                    value={cfg.args}
                    onChange={(e) => updateLanguage(lang, { args: e.target.value })}
                    className="min-w-0 flex-1 rounded border border-ink-muted/30 bg-surface-raised px-2 py-1 text-xs text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
                    placeholder={t("settings.sdk.argsPlaceholder")}
                    aria-label={`${label} args`}
                    data-testid={`sdk-lang-${lang}-args`}
                  />
                  <button
                    type="button"
                    onClick={() => handleToggleLanguage(lang, cfg.command, cfg.enabled)}
                    disabled={isChecking}
                    aria-pressed={cfg.enabled}
                    aria-label={`${label} ${t("settings.sdk.enabled")}`}
                    className={`shrink-0 rounded border px-2 py-1 text-xs focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50 ${
                      cfg.enabled
                        ? "border-state-ok/50 text-state-ok"
                        : "border-ink-muted/40 text-ink-muted"
                    }`}
                    data-testid={`sdk-lang-${lang}-toggle`}
                  >
                    {isChecking
                      ? "..."
                      : cfg.enabled
                        ? t("settings.sdk.enabled")
                        : t("settings.sdk.disabled")}
                  </button>
                  {isUnavailable && (
                    <button
                      type="button"
                      onClick={() =>
                        openSteward(
                          `${label} 运行命令 \`${cfg.command}\` 在 PATH 中不可用，请提供修复建议`,
                        )
                      }
                      aria-label={t("settings.sdk.askSteward")}
                      className="shrink-0 rounded p-0.5 text-state-warning hover:bg-surface-overlay"
                      data-testid={`sdk-lang-${lang}-error`}
                    >
                      <Icon name="warning" size={16} />
                    </button>
                  )}
                </div>
                {isExpanded && (
                  <div
                    className="space-y-2 border-t border-ink-muted/20 px-3 py-2"
                    data-testid={`sdk-lang-${lang}-advanced`}
                  >
                    <div className="flex items-center gap-2">
                      <label className="w-28 shrink-0 text-xs text-ink-muted">
                        {t("settings.sdk.preRunCommand")}
                      </label>
                      <input
                        type="text"
                        value={cfg.preRunCommand ?? ""}
                        onChange={(e) => updateLanguage(lang, { preRunCommand: e.target.value })}
                        className="min-w-0 flex-1 rounded border border-ink-muted/30 bg-surface-raised px-2 py-1 text-xs text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
                        placeholder={t("settings.sdk.preRunPlaceholder")}
                        data-testid={`sdk-lang-${lang}-preRun`}
                      />
                    </div>
                    <div className="flex items-center gap-2">
                      <label className="w-28 shrink-0 text-xs text-ink-muted">
                        {t("settings.sdk.workingDir")}
                      </label>
                      <input
                        type="text"
                        value={cfg.workingDir ?? ""}
                        onChange={(e) => updateLanguage(lang, { workingDir: e.target.value })}
                        className="min-w-0 flex-1 rounded border border-ink-muted/30 bg-surface-raised px-2 py-1 text-xs text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
                        placeholder={t("settings.sdk.workingDirPlaceholder")}
                        data-testid={`sdk-lang-${lang}-workdir`}
                      />
                    </div>
                    <div className="flex items-center gap-2">
                      <label className="w-28 shrink-0 text-xs text-ink-muted">
                        {t("settings.sdk.fileExtensions")}
                      </label>
                      <input
                        type="text"
                        value={(cfg.fileExtensions ?? []).join(", ")}
                        onChange={(e) =>
                          updateLanguage(lang, {
                            fileExtensions: e.target.value
                              .split(",")
                              .map((s) => s.trim())
                              .filter((s) => s.length > 0),
                          })
                        }
                        className="min-w-0 flex-1 rounded border border-ink-muted/30 bg-surface-raised px-2 py-1 text-xs text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
                        placeholder=".py, .pyw"
                        data-testid={`sdk-lang-${lang}-extensions`}
                      />
                    </div>
                    <div className="flex items-start gap-2">
                      <label className="w-28 shrink-0 pt-1 text-xs text-ink-muted">
                        {t("settings.sdk.envVars")}
                      </label>
                      <textarea
                        value={Object.entries(cfg.envVars ?? {})
                          .map(([k, v]) => `${k}=${v}`)
                          .join("\n")}
                        onChange={(e) => {
                          const envVars: Record<string, string> = {};
                          for (const line of e.target.value.split("\n")) {
                            const eqIdx = line.indexOf("=");
                            if (eqIdx > 0) {
                              envVars[line.slice(0, eqIdx).trim()] = line.slice(eqIdx + 1).trim();
                            }
                          }
                          updateLanguage(lang, { envVars });
                        }}
                        className="min-w-0 flex-1 rounded border border-ink-muted/30 bg-surface-raised px-2 py-1 text-xs text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
                        placeholder="KEY=value"
                        rows={2}
                        data-testid={`sdk-lang-${lang}-envvars`}
                      />
                    </div>
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      </div>

      <div>
        <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-ink-muted">
          {t("settings.sdk.buildsHeading")}
        </h3>
        <ul className="space-y-1.5" data-testid="sdk-build-list">
          {buildKeys.map((buildKey) => {
            const cfg = configs.builds[buildKey];
            if (cfg === undefined) return null;
            const checkKey = `build:${buildKey}`;
            const rowKey = `build-row:${buildKey}`;
            const isUnavailable = unavailableCommands.has(checkKey);
            const isChecking = checking.has(checkKey);
            const isExpanded = expandedRows.has(rowKey);
            const label = BUILD_LABELS[buildKey] ?? buildKey;
            return (
              <li
                key={buildKey}
                className="rounded border border-ink-muted/30 bg-surface"
                data-testid={`sdk-build-${buildKey}-row`}
              >
                <div className="flex items-center gap-2 px-3 py-2">
                  <button
                    type="button"
                    onClick={() => toggleRow(rowKey)}
                    aria-label={isExpanded ? t("settings.sdk.collapse") : t("settings.sdk.expand")}
                    aria-expanded={isExpanded}
                    className="shrink-0 rounded p-0.5 text-ink-muted hover:bg-surface-overlay hover:text-ink"
                    data-testid={`sdk-build-${buildKey}-expand`}
                  >
                    <Icon name="chevron-down" size={14} className={isExpanded ? "" : "-rotate-90"} />
                  </button>
                  <span className="w-28 shrink-0 text-sm text-ink">{label}</span>
                  <span className="w-28 shrink-0 text-xs text-ink-muted">{cfg.manifestFile}</span>
                  <input
                    type="text"
                    value={cfg.command}
                    onChange={(e) => updateBuild(buildKey, { command: e.target.value })}
                    className="w-40 shrink-0 rounded border border-ink-muted/30 bg-surface-raised px-2 py-1 text-xs text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
                    aria-label={`${label} build command`}
                    data-testid={`sdk-build-${buildKey}-command`}
                  />
                  <button
                    type="button"
                    onClick={() => handleToggleBuild(buildKey, cfg.command, cfg.enabled)}
                    disabled={isChecking}
                    aria-pressed={cfg.enabled}
                    aria-label={`${label} ${t("settings.sdk.enabled")}`}
                    className={`shrink-0 rounded border px-2 py-1 text-xs focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50 ${
                      cfg.enabled
                        ? "border-state-ok/50 text-state-ok"
                        : "border-ink-muted/40 text-ink-muted"
                    }`}
                    data-testid={`sdk-build-${buildKey}-toggle`}
                  >
                    {isChecking
                      ? "..."
                      : cfg.enabled
                        ? t("settings.sdk.enabled")
                        : t("settings.sdk.disabled")}
                  </button>
                  {isUnavailable && (
                    <button
                      type="button"
                      onClick={() =>
                        openSteward(
                          `${label} 构建命令 \`${cfg.command}\` 在 PATH 中不可用，请提供修复建议`,
                        )
                      }
                      aria-label={t("settings.sdk.askSteward")}
                      className="shrink-0 rounded p-0.5 text-state-warning hover:bg-surface-overlay"
                      data-testid={`sdk-build-${buildKey}-error`}
                    >
                      <Icon name="warning" size={16} />
                    </button>
                  )}
                </div>
                {isExpanded && (
                  <div
                    className="space-y-2 border-t border-ink-muted/20 px-3 py-2"
                    data-testid={`sdk-build-${buildKey}-advanced`}
                  >
                    <div className="flex items-center gap-2">
                      <label className="w-28 shrink-0 text-xs text-ink-muted">
                        {t("settings.sdk.buildArgs")}
                      </label>
                      <input
                        type="text"
                        value={cfg.buildArgs ?? ""}
                        onChange={(e) => updateBuild(buildKey, { buildArgs: e.target.value })}
                        className="min-w-0 flex-1 rounded border border-ink-muted/30 bg-surface-raised px-2 py-1 text-xs text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
                        placeholder={t("settings.sdk.buildArgsPlaceholder")}
                        data-testid={`sdk-build-${buildKey}-buildargs`}
                      />
                    </div>
                    <div className="flex items-center gap-2">
                      <label className="w-28 shrink-0 text-xs text-ink-muted">
                        {t("settings.sdk.outputDir")}
                      </label>
                      <input
                        type="text"
                        value={cfg.outputDir ?? ""}
                        onChange={(e) => updateBuild(buildKey, { outputDir: e.target.value })}
                        className="min-w-0 flex-1 rounded border border-ink-muted/30 bg-surface-raised px-2 py-1 text-xs text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
                        placeholder="dist"
                        data-testid={`sdk-build-${buildKey}-outputdir`}
                      />
                    </div>
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      </div>

      <StewardDialog
        open={stewardOpen}
        onClose={() => setStewardOpen(false)}
        autoMessage={stewardMessage}
      />
    </section>
  );
}
