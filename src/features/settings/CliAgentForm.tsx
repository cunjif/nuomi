import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { AgentProfileInput, CliFlavorDto } from "../../lib/ipc/bindings.gen";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";

/** One trimmed, non-empty line per argument; blank lines are skipped. */
export function parseArgs(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
}

/** First `=` splits KEY=VALUE; blank lines skipped, malformed lines counted. */
export function parseEnv(text: string): { env: Record<string, string>; ignored: number } {
  const env: Record<string, string> = {};
  let ignored = 0;
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (line.length === 0) continue;
    const eq = line.indexOf("=");
    if (eq <= 0) {
      ignored += 1;
      continue;
    }
    env[line.slice(0, eq)] = line.slice(eq + 1);
  }
  return { env, ignored };
}

const FLAVORS: Array<{ value: CliFlavorDto; labelKey: string }> = [
  { value: "claude_code", labelKey: "settings.cliAgents.flavorClaudeCode" },
  { value: "codex", labelKey: "settings.cliAgents.flavorCodex" },
  { value: "plain", labelKey: "settings.cliAgents.flavorPlain" },
];

/** Add/edit form for CLI agent profiles (`name` is the backend idempotency key). */
export function CliAgentForm(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [name, setName] = useState("");
  const [flavor, setFlavor] = useState<CliFlavorDto>("claude_code");
  const [command, setCommand] = useState("");
  const [argsText, setArgsText] = useState("");
  const [envText, setEnvText] = useState("");
  const [workingDir, setWorkingDir] = useState("");
  const [enabled, setEnabled] = useState(true);
  const [envIgnored, setEnvIgnored] = useState(0);

  const saveMut = useMutation({
    mutationFn: (input: AgentProfileInput) => ipc.upsertAgentProfile(input),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["agentProfiles"] });
      toast.success(t("settings.cliAgents.saved"));
      setName("");
      setFlavor("claude_code");
      setCommand("");
      setArgsText("");
      setEnvText("");
      setWorkingDir("");
      setEnabled(true);
    },
    onError: (e) => {
      // Inputs stay as-is so the user can fix and resubmit.
      toast.error(`${t("settings.cliAgents.saveFailed")}: ${describeError(e)}`);
    },
  });

  const field =
    "rounded border border-ink-muted/40 bg-surface px-2 py-1 text-sm text-ink focus-visible:ring-2 focus-visible:ring-ink-accent";

  return (
    <form
      aria-label={t("settings.cliAgents.heading")}
      className="mb-3 rounded border border-ink-muted/40 bg-surface-raised p-3"
      onSubmit={(e) => {
        e.preventDefault();
        if (!name.trim() || !command.trim() || saveMut.isPending) return;
        const parsed = parseEnv(envText);
        setEnvIgnored(parsed.ignored);
        saveMut.mutate({
          name: name.trim(),
          flavor,
          command: command.trim(),
          args: parseArgs(argsText),
          env: parsed.env,
          workingDir: workingDir.trim().length > 0 ? workingDir.trim() : null,
          enabled,
        });
      }}
    >
      <p className="mb-2 text-xs text-ink-muted">{t("settings.cliAgents.idempotentHint")}</p>
      <div className="flex flex-wrap items-end gap-2">
        <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
          {t("settings.cliAgents.name")}
          <input value={name} onChange={(e) => setName(e.target.value)} required className={`${field} w-40`} />
        </label>
        <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
          {t("settings.cliAgents.flavor")}
          <select
            value={flavor}
            onChange={(e) => setFlavor(e.target.value as CliFlavorDto)}
            className={field}
          >
            {FLAVORS.map((f) => (
              <option key={f.value} value={f.value}>
                {t(f.labelKey)}
              </option>
            ))}
          </select>
        </label>
        <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
          {t("settings.cliAgents.command")}
          <input value={command} onChange={(e) => setCommand(e.target.value)} required className={`${field} w-56 font-mono`} />
        </label>
        <label className="flex items-center gap-1 pb-1 text-xs text-ink-muted">
          <input
            type="checkbox"
            checked={enabled}
            onChange={(e) => setEnabled(e.target.checked)}
            className="accent-[var(--nuomi-accent)]"
          />
          {t("settings.cliAgents.enabled")}
        </label>
        <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
          {t("settings.cliAgents.workingDir")}
          <input
            value={workingDir}
            onChange={(e) => setWorkingDir(e.target.value)}
            placeholder="C:\work\project"
            className={`${field} w-56 font-mono`}
          />
        </label>
      </div>
      <div className="mt-2 flex flex-wrap gap-2">
        <label className="flex min-w-48 flex-1 flex-col gap-0.5 text-xs text-ink-muted">
          {t("settings.cliAgents.args")}
          <textarea
            value={argsText}
            onChange={(e) => setArgsText(e.target.value)}
            rows={3}
            spellCheck={false}
            className={`${field} font-mono`}
          />
        </label>
        <label className="flex min-w-48 flex-1 flex-col gap-0.5 text-xs text-ink-muted">
          {t("settings.cliAgents.env")}
          <textarea
            value={envText}
            onChange={(e) => {
              setEnvText(e.target.value);
              setEnvIgnored(0);
            }}
            rows={3}
            spellCheck={false}
            className={`${field} font-mono`}
          />
          {envIgnored > 0 && (
            <span role="alert" className="text-[10px] text-state-warn">
              {t("settings.cliAgents.envIgnoredWarning", { count: envIgnored })}
            </span>
          )}
        </label>
      </div>
      <button
        type="submit"
        disabled={saveMut.isPending}
        className="mt-2 pixel-fill-accent px-3 py-1.5 text-sm text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
      >
        {saveMut.isPending ? t("settings.cliAgents.saving") : t("settings.cliAgents.save")}
      </button>
    </form>
  );
}
