/**
 * LSP (Language Server Protocol) configuration section for the Settings page.
 * Lets users configure the Language Server command and arguments for each
 * supported language, toggle LSP on/off per language, and view/start/stop
 * server status. Configurations are persisted via the lsp_set_config IPC command.
 */
import { useState, useEffect, useCallback, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ipc } from "../../lib/ipc/client";
import type { LspServerConfigDto, LspServerStatusDto } from "../../lib/ipc/client";
import { Spinner } from "../../components/ui/Spinner";

const LANGUAGE_LABELS: Record<string, string> = {
  typescript: "TypeScript",
  javascript: "JavaScript",
  python: "Python",
  java: "Java",
  go: "Go",
  rust: "Rust",
  c: "C",
  cpp: "C++",
};

const STATUS_POLL_MS = 2000;

type StatusKind = "not_started" | "initializing" | "ready" | "failed" | "disabled";

function classifyStatus(status: string): StatusKind {
  if (status.startsWith("failed")) return "failed";
  if (status === "ready") return "ready";
  if (status === "initializing") return "initializing";
  if (status === "disabled") return "disabled";
  return "not_started";
}

const STATUS_DOT_CLASS: Record<StatusKind, string> = {
  not_started: "bg-ink-muted/40",
  initializing: "bg-yellow-500",
  ready: "bg-green-500",
  failed: "bg-red-500",
  disabled: "bg-ink-muted/20",
};

const STATUS_I18N_KEY: Record<StatusKind, string> = {
  not_started: "settings.lsp.statusNotStarted",
  initializing: "settings.lsp.statusInitializing",
  ready: "settings.lsp.statusReady",
  failed: "settings.lsp.statusFailed",
  disabled: "settings.lsp.statusDisabled",
};

export function LspSection(): ReactNode {
  const { t } = useTranslation();
  const [configs, setConfigs] = useState<LspServerConfigDto[]>([]);
  const [statuses, setStatuses] = useState<LspServerStatusDto[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState<string | null>(null);

  const loadConfigs = useCallback(async () => {
    try {
      const result = await ipc.lspGetConfigs();
      setConfigs(result);
    } catch {
      setConfigs([]);
    } finally {
      setLoading(false);
    }
  }, []);

  const loadStatuses = useCallback(async () => {
    try {
      const result = await ipc.lspGetStatuses();
      setStatuses(result);
    } catch {
      // ignore — status polling is best-effort
    }
  }, []);

  useEffect(() => {
    void loadConfigs();
  }, [loadConfigs]);

  useEffect(() => {
    void loadStatuses();
    const timer = setInterval(() => void loadStatuses(), STATUS_POLL_MS);
    return () => clearInterval(timer);
  }, [loadStatuses]);

  const updateConfig = useCallback(
    async (language: string, patch: Partial<LspServerConfigDto>): Promise<void> => {
      const existing = configs.find((c) => c.language === language);
      if (existing === undefined) return;
      const updated = { ...existing, ...patch };
      setSaving(language);
      setConfigs((prev) => prev.map((c) => (c.language === language ? updated : c)));
      try {
        await ipc.lspSetConfig(updated);
      } catch {
        void loadConfigs();
      } finally {
        setSaving(null);
      }
    },
    [configs, loadConfigs],
  );

  const startServer = useCallback(async (language: string): Promise<void> => {
    try {
      await ipc.lspStart(language, "");
      void loadStatuses();
    } catch {
      // ignore — user will see status remain not_started/failed
    }
  }, [loadStatuses]);

  const stopServer = useCallback(async (language: string): Promise<void> => {
    try {
      await ipc.lspStop(language);
      void loadStatuses();
    } catch {
      // ignore
    }
  }, [loadStatuses]);

  if (loading) {
    return (
      <div className="p-4">
        <Spinner label={t("settings.lsp.loading")} />
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col overflow-y-auto p-4">
      <h2 className="mb-1 text-lg font-semibold text-ink">{t("settings.lsp.heading")}</h2>
      <p className="mb-4 text-sm text-ink-muted">{t("settings.lsp.hint")}</p>

      <div className="space-y-3">
        {configs.map((cfg) => {
          const label = LANGUAGE_LABELS[cfg.language] ?? cfg.language;
          const isSaving = saving === cfg.language;
          const status = statuses.find((s) => s.language === cfg.language)?.status ?? "not_started";
          const kind = classifyStatus(status);
          return (
            <div
              key={cfg.language}
              className="rounded border border-ink-muted/30 bg-surface p-3"
            >
              <div className="mb-2 flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <span className="font-medium text-ink">{label}</span>
                  <span
                    className={`inline-block h-2 w-2 rounded-full ${STATUS_DOT_CLASS[kind]}`}
                    title={t(STATUS_I18N_KEY[kind])}
                  />
                  <span className="text-xs text-ink-muted">{t(STATUS_I18N_KEY[kind])}</span>
                  {isSaving && (
                    <span className="text-xs text-ink-muted">{t("settings.lsp.saving")}</span>
                  )}
                </div>
                <div className="flex items-center gap-3">
                  {kind === "ready" || kind === "initializing" ? (
                    <button
                      type="button"
                      onClick={() => void stopServer(cfg.language)}
                      className="rounded border border-ink-muted/40 px-2 py-0.5 text-xs text-ink-muted hover:bg-ink-muted/10"
                    >
                      {t("settings.lsp.stop")}
                    </button>
                  ) : (
                    <button
                      type="button"
                      onClick={() => void startServer(cfg.language)}
                      disabled={kind === "disabled"}
                      className="rounded border border-ink-muted/40 px-2 py-0.5 text-xs text-ink-muted hover:bg-ink-muted/10 disabled:cursor-not-allowed disabled:opacity-40"
                    >
                      {t("settings.lsp.start")}
                    </button>
                  )}
                  <label className="flex cursor-pointer items-center gap-1.5 text-sm text-ink-muted">
                    <input
                      type="checkbox"
                      checked={cfg.enabled ?? false}
                      onChange={(e) => void updateConfig(cfg.language, { enabled: e.target.checked })}
                      className="h-4 w-4"
                    />
                    {(cfg.enabled ?? false) ? t("settings.lsp.enabled") : t("settings.lsp.disabled")}
                  </label>
                </div>
              </div>
              <div className="grid grid-cols-[1fr_1fr] gap-2">
                <input
                  type="text"
                  value={cfg.command}
                  onChange={(e) => void updateConfig(cfg.language, { command: e.target.value })}
                  placeholder="command"
                  className="rounded border border-ink-muted/40 bg-surface px-2 py-1 text-sm text-ink focus:border-ink-accent focus:outline-none"
                />
                <input
                  type="text"
                  value={(cfg.args ?? []).join(" ")}
                  onChange={(e) =>
                    void updateConfig(cfg.language, {
                      args: e.target.value.split(/\s+/).filter((s) => s.length > 0),
                    })
                  }
                  placeholder={t("settings.lsp.argsPlaceholder")}
                  className="rounded border border-ink-muted/40 bg-surface px-2 py-1 text-sm text-ink focus:border-ink-accent focus:outline-none"
                />
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
