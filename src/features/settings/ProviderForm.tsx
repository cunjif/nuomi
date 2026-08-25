import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { ProviderInput, ProviderProtocolDto } from "../../lib/ipc/bindings.gen";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";

interface ProviderFormProps {
  onDone: () => void;
}

/** Provider form; the API key goes straight to the OS keyring backend. */
export function ProviderForm({ onDone }: ProviderFormProps): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [name, setName] = useState("");
  const [protocol, setProtocol] = useState<ProviderProtocolDto>("open_ai_compatible");
  const [baseUrl, setBaseUrl] = useState("");
  const [capabilities, setCapabilities] = useState("");
  const [isMaster, setIsMaster] = useState(false);
  const [apiKey, setApiKey] = useState("");

  const saveMut = useMutation({
    mutationFn: (input: ProviderInput) => ipc.upsertProvider(input),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["providers"] });
      toast.success(t("settings.providerSaved"));
      setApiKey("");
      onDone();
    },
    onError: (e) => toast.error(`${t("settings.providerSaveFailed")}: ${describeError(e)}`),
  });

  const field =
    "rounded border border-ink-muted/40 bg-surface px-2 py-1 text-sm text-ink focus-visible:ring-2 focus-visible:ring-ink-accent";

  return (
    <form
      aria-label={t("settings.providersHeading")}
      className="mb-3 flex flex-wrap items-end gap-2 rounded border border-ink-muted/40 bg-surface-raised p-3"
      onSubmit={(e) => {
        e.preventDefault();
        if (!name.trim() || !baseUrl.trim()) return;
        saveMut.mutate({
          name: name.trim(),
          protocol,
          baseUrl: baseUrl.trim(),
          capabilities: capabilities
            .split(",")
            .map((c) => c.trim())
            .filter((c) => c.length > 0),
          isMaster,
          apiKey: apiKey.length > 0 ? apiKey : null,
        });
      }}
    >
      <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
        {t("settings.providerName")}
        <input value={name} onChange={(e) => setName(e.target.value)} required className={field} />
      </label>
      <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
        {t("settings.protocol")}
        <select
          value={protocol}
          onChange={(e) => setProtocol(e.target.value as ProviderProtocolDto)}
          className={field}
        >
          <option value="open_ai_compatible">{t("settings.protocolOpenAi")}</option>
          <option value="anthropic_compatible">{t("settings.protocolAnthropic")}</option>
        </select>
      </label>
      <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
        {t("settings.baseUrl")}
        <input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} required type="url" className={`${field} w-64`} />
      </label>
      <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
        {t("settings.capabilities")}
        <input value={capabilities} onChange={(e) => setCapabilities(e.target.value)} placeholder="chat, tools" className={field} />
      </label>
      <label className="flex items-center gap-1 pb-1 text-xs text-ink-muted">
        <input type="checkbox" checked={isMaster} onChange={(e) => setIsMaster(e.target.checked)} className="accent-[var(--nuomi-accent)]" />
        {t("settings.isMaster")}
      </label>
      <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
        {t("settings.apiKey")}
        <input
          type="password"
          value={apiKey}
          onChange={(e) => setApiKey(e.target.value)}
          autoComplete="off"
          className={field}
        />
        <span className="text-[10px]">{t("settings.apiKeyHint")}</span>
      </label>
      <button
        type="submit"
        disabled={saveMut.isPending}
        className="rounded bg-ink-accent px-3 py-1.5 text-sm text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
      >
        {t("settings.saveProvider")}
      </button>
      <button
        type="button"
        onClick={onDone}
        className="rounded border border-ink-muted px-3 py-1.5 text-sm text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
      >
        {t("common.cancel")}
      </button>
    </form>
  );
}
