import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { IntegrationInput, IntegrationKindDto } from "../../lib/ipc/bindings.gen";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { fieldClass as field } from "../../components/ui/Field";

/** Fixed trigger-topic whitelist offered by the form (SPEC bots-telemetry-m1 D8). */
export const INTEGRATION_EVENT_TOPICS: readonly string[] = [
  "run.state_changed",
  "task.status_changed",
  "team.formed",
  "approval.requested",
];

const KINDS: Array<{ value: IntegrationKindDto; labelKey: string }> = [
  { value: "feishu_bot", labelKey: "settings.integrations.kindFeishuBot" },
  { value: "qq_webhook", labelKey: "settings.integrations.kindQqWebhook" },
  { value: "telemetry", labelKey: "settings.integrations.kindTelemetry" },
];

/** Add/edit form for integrations (`name` is the backend idempotency key). */
export function IntegrationForm(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [name, setName] = useState("");
  const [kind, setKind] = useState<IntegrationKindDto>("feishu_bot");
  const [webhookUrl, setWebhookUrl] = useState("");
  const [secret, setSecret] = useState("");
  const [enabled, setEnabled] = useState(true);
  const [selectedTopics, setSelectedTopics] = useState<ReadonlySet<string>>(new Set());

  const saveMut = useMutation({
    mutationFn: (input: IntegrationInput) => ipc.upsertIntegration(input),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["integrations"] });
      toast.success(t("settings.integrations.saved"));
      setName("");
      setKind("feishu_bot");
      setWebhookUrl("");
      setSecret("");
      setEnabled(true);
      setSelectedTopics(new Set());
    },
    onError: (e) => {
      // Inputs stay as-is so the user can fix and resubmit.
      toast.error(`${t("settings.integrations.saveFailed")}: ${describeError(e)}`);
    },
  });


  return (
    <form
      aria-label={t("settings.integrations.heading")}
      className="mb-3 rounded border border-ink-muted/40 bg-surface-raised p-3"
      onSubmit={(e) => {
        e.preventDefault();
        if (!name.trim() || !webhookUrl.trim() || saveMut.isPending) return;
        saveMut.mutate({
          name: name.trim(),
          kind,
          webhookUrl: webhookUrl.trim(),
          secret: secret.trim().length > 0 ? secret.trim() : null,
          headers: null,
          events: INTEGRATION_EVENT_TOPICS.filter((topic) => selectedTopics.has(topic)),
          enabled,
        });
      }}
    >
      <p className="mb-2 text-xs text-ink-muted">{t("settings.integrations.idempotentHint")}</p>
      <div className="flex flex-wrap items-end gap-2">
        <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
          {t("settings.integrations.name")}
          <input value={name} onChange={(e) => setName(e.target.value)} required className={`${field} bg-surface text-sm w-40`} />
        </label>
        <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
          {t("settings.integrations.kind")}
          <select
            value={kind}
            onChange={(e) => {
              const next = e.target.value;
              if (next === "feishu_bot" || next === "qq_webhook" || next === "telemetry") setKind(next);
            }}
            className={`${field} bg-surface text-sm`}
          >
            {KINDS.map((k) => (
              <option key={k.value} value={k.value}>
                {t(k.labelKey)}
              </option>
            ))}
          </select>
        </label>
        <label className="flex min-w-56 flex-1 flex-col gap-0.5 text-xs text-ink-muted">
          {t("settings.integrations.webhookUrl")}
          <input
            value={webhookUrl}
            onChange={(e) => setWebhookUrl(e.target.value)}
            required
            placeholder={t("settings.integrations.webhookUrlPlaceholder")}
            spellCheck={false}
            className={`${field} bg-surface text-sm font-mono`}
          />
        </label>
        <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
          {t("settings.integrations.secret")}
          <input
            type="password"
            value={secret}
            onChange={(e) => setSecret(e.target.value)}
            autoComplete="new-password"
            className={`${field} bg-surface text-sm w-48 font-mono`}
          />
        </label>
        <label className="flex items-center gap-1 pb-1 text-xs text-ink-muted">
          <input
            type="checkbox"
            checked={enabled}
            onChange={(e) => setEnabled(e.target.checked)}
            className="accent-[var(--nuomi-accent)]"
          />
          {t("settings.integrations.enabled")}
        </label>
      </div>
      <p className="mt-0.5 text-[10px] text-ink-muted">{t("settings.integrations.secretHint")}</p>
      <div className="mt-2">
        <p className="text-xs text-ink-muted">{t("settings.integrations.events")}</p>
        <div className="mt-1 flex flex-wrap gap-x-4 gap-y-1">
          {INTEGRATION_EVENT_TOPICS.map((topic) => (
            <label key={topic} className="flex items-center gap-1 text-xs text-ink-muted">
              <input
                type="checkbox"
                checked={selectedTopics.has(topic)}
                onChange={(e) =>
                  setSelectedTopics((prev) => {
                    const next = new Set(prev);
                    if (e.target.checked) next.add(topic);
                    else next.delete(topic);
                    return next;
                  })
                }
                className="accent-[var(--nuomi-accent)]"
              />
              <span className="font-mono">{topic}</span>
            </label>
          ))}
        </div>
        <p className="mt-0.5 text-[10px] text-ink-muted">{t("settings.integrations.eventsAllHint")}</p>
      </div>
      <button
        type="submit"
        disabled={saveMut.isPending}
        className="mt-2 pixel-fill-accent px-3 py-1.5 text-sm text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
      >
        {saveMut.isPending ? t("settings.integrations.saving") : t("settings.integrations.save")}
      </button>
    </form>
  );
}
