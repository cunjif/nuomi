import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { AgentProfileDto, AgentProfileInput, CliFlavorDto } from "../../lib/ipc/bindings.gen";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { fieldClass as field } from "../../components/ui/Field";

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

/** Serialize an env record back into the KEY=VALUE textarea format. */
function stringifyEnv(env: Record<string, string | undefined>): string {
  return Object.entries(env)
    .filter(([, v]) => v !== undefined)
    .map(([k, v]) => `${k}=${v}`)
    .join("\n");
}

const FLAVORS: Array<{ value: CliFlavorDto; labelKey: string }> = [
  { value: "claude_code", labelKey: "settings.cliAgents.flavorClaudeCode" },
  { value: "codex", labelKey: "settings.cliAgents.flavorCodex" },
  { value: "plain", labelKey: "settings.cliAgents.flavorPlain" },
];

/** Add/edit form for CLI agent profiles (`name` is the backend idempotency key).
 *  Pass `initial` to prefill the form for editing an existing profile; the parent
 *  should remount this component (e.g. via `key`) when the edit target changes so
 *  the internal state re-initializes from the new `initial`. */
export function CliAgentForm({
  initial = null,
  onDone,
}: {
  initial?: AgentProfileDto | null;
  onDone?: () => void;
} = {}): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [name, setName] = useState(initial?.name ?? "");
  const [flavor, setFlavor] = useState<CliFlavorDto>(initial?.flavor ?? "claude_code");
  const [command, setCommand] = useState(initial?.command ?? "");
  const [modelId, setModelId] = useState(initial?.modelId ?? "");
  const [argsText, setArgsText] = useState(initial ? initial.args.join("\n") : "");
  const [envText, setEnvText] = useState(initial ? stringifyEnv(initial.env) : "");
  const [workingDir, setWorkingDir] = useState(initial?.workingDir ?? "");
  const [resumeArgs, setResumeArgs] = useState(initial?.resumeArgs ?? "");
  const [enabled, setEnabled] = useState(initial?.enabled ?? true);
  const [envIgnored, setEnvIgnored] = useState(0);
  const isEditing = initial !== null;

  const resetForm = () => {
    setName("");
    setFlavor("claude_code");
    setCommand("");
    setModelId("");
    setArgsText("");
    setEnvText("");
    setWorkingDir("");
    setResumeArgs("");
    setEnabled(true);
  };

  const saveMut = useMutation({
    mutationFn: (input: AgentProfileInput) => ipc.upsertAgentProfile(input),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["agentProfiles"] });
      toast.success(t("settings.cliAgents.saved"));
      if (isEditing) {
        onDone?.();
      } else {
        resetForm();
      }
    },
    onError: (e) => {
      // Inputs stay as-is so the user can fix and resubmit.
      toast.error(`${t("settings.cliAgents.saveFailed")}: ${describeError(e)}`);
    },
  });


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
          modelId: modelId.trim().length > 0 ? modelId.trim() : null,
          resumeArgs: resumeArgs.trim().length > 0 ? resumeArgs.trim() : null,
        });
      }}
    >
      <p className="mb-2 text-xs text-ink-muted">{t("settings.cliAgents.idempotentHint")}</p>
      <div className="flex flex-wrap items-end gap-2">
        <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
          {t("settings.cliAgents.name")}
          <input value={name} onChange={(e) => setName(e.target.value)} required className={`${field} bg-surface text-sm w-40`} />
        </label>
        <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
          {t("settings.cliAgents.flavor")}
          <select
            value={flavor}
            onChange={(e) => setFlavor(e.target.value as CliFlavorDto)}
            className={`${field} bg-surface text-sm`}
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
          <input value={command} onChange={(e) => setCommand(e.target.value)} required className={`${field} bg-surface text-sm w-56 font-mono`} />
        </label>
        <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
          {t("settings.cliAgents.modelId")}
          <input
            value={modelId}
            onChange={(e) => setModelId(e.target.value)}
            placeholder="sonnet"
            className={`${field} bg-surface text-sm w-40 font-mono`}
          />
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
            className={`${field} bg-surface text-sm w-56 font-mono`}
          />
        </label>
        <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
          {t("settings.cliAgents.resumeArgs")}
          <input
            value={resumeArgs}
            onChange={(e) => setResumeArgs(e.target.value)}
            placeholder={t("settings.cliAgents.resumeArgsPlaceholder")}
            className={`${field} bg-surface text-sm w-56 font-mono`}
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
            className={`${field} bg-surface text-sm font-mono`}
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
            className={`${field} bg-surface text-sm font-mono`}
          />
          {envIgnored > 0 && (
            <span role="alert" className="text-[10px] text-state-warn">
              {t("settings.cliAgents.envIgnoredWarning", { count: envIgnored })}
            </span>
          )}
        </label>
      </div>
      <div className="mt-2 flex flex-wrap items-center gap-2">
        <button
          type="submit"
          disabled={saveMut.isPending}
          className="pixel-fill-accent px-3 py-1.5 text-sm text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
        >
          {saveMut.isPending ? t("settings.cliAgents.saving") : t("settings.cliAgents.save")}
        </button>
        {isEditing && (
          <button
            type="button"
            onClick={onDone}
            className="rounded border border-ink-muted px-3 py-1.5 text-sm text-ink hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            {t("common.cancel")}
          </button>
        )}
      </div>
    </form>
  );
}
