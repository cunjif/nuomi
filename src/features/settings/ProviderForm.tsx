import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import type {
  ProviderDto,
  ProviderInput,
  ProviderProtocolDto,
  TestProviderConnectionInput,
} from "../../lib/ipc/bindings.gen";
import { describeError } from "../../i18n";
import { IpcCommandError, ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";

/** Persisted per-provider connectivity outcome (drives the list dot). */
export type ProviderTestStatus = "ok" | "error";

/** Two-letter avatar text ("DeepSeek" → "DE"). */
export function providerInitials(name: string): string {
  const points = [...name.trim().toUpperCase()];
  const first = points[0] ?? "?";
  const second = points[1] ?? "";
  return `${first}${second}`;
}

/** Deterministic blue→purple avatar tint derived from the name hash. */
export function providerAvatarStyle(name: string): { backgroundColor: string } {
  let hash = 0;
  for (const ch of name) hash = (hash * 31 + (ch.codePointAt(0) ?? 0)) % 360;
  return { backgroundColor: `hsl(${220 + (hash % 80)} 55% 45% / 0.35)` };
}

const ROLE_KEYS = ["code", "review", "docs", "plan"] as const;
type RoleKey = (typeof ROLE_KEYS)[number];
const ROLE_LABEL_KEYS: Record<RoleKey, string> = {
  code: "provider.roleCode",
  review: "provider.roleReview",
  docs: "provider.roleDocs",
  plan: "provider.rolePlan",
};

type TypeKey = "openai" | "anthropic" | "deepseek" | "ollama" | "custom";
const TYPE_OPTIONS: ReadonlyArray<{ key: TypeKey; labelKey: string }> = [
  { key: "openai", labelKey: "provider.typeOpenAi" },
  { key: "anthropic", labelKey: "provider.typeAnthropic" },
  { key: "deepseek", labelKey: "provider.typeDeepSeek" },
  { key: "ollama", labelKey: "provider.typeOllama" },
  { key: "custom", labelKey: "provider.typeCustom" },
];
const PRESET_URLS: Partial<Record<TypeKey, string>> = {
  deepseek: "https://api.deepseek.com",
  ollama: "http://localhost:11434/v1",
};

function protocolOf(key: TypeKey): ProviderProtocolDto {
  return key === "anthropic" ? "anthropic_compatible" : "open_ai_compatible";
}

function typeKeyOf(protocol: ProviderProtocolDto): TypeKey {
  if (protocol === "anthropic_compatible") return "anthropic";
  return "openai";
}

function numberOrNull(raw: string): number | null {
  const trimmed = raw.trim();
  if (trimmed.length === 0) return null;
  const parsed = Number(trimmed);
  return Number.isFinite(parsed) ? parsed : null;
}

const field =
  "w-full rounded border border-ink-muted/40 bg-surface-overlay px-2 py-1 text-xs text-ink placeholder:text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent";

interface ProviderFormProps {
  /** `null` creates a new provider; otherwise edits the row in place. */
  provider: ProviderDto | null;
  onDone: () => void;
  onTested: (providerId: string, status: ProviderTestStatus) => void;
  onDeleted: () => void;
}

/** Provider detail editor: connection, models, advanced params, routing. */
export function ProviderForm({ provider, onDone, onTested, onDeleted }: ProviderFormProps): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const settings = provider?.settings;

  const [name, setName] = useState(provider?.name ?? "");
  const [typeKey, setTypeKey] = useState<TypeKey>(
    typeKeyOf(provider?.protocol ?? "open_ai_compatible"),
  );
  const [baseUrl, setBaseUrl] = useState(provider?.baseUrl ?? "");
  const [apiKey, setApiKey] = useState("");
  const [showKey, setShowKey] = useState(false);
  const [models, setModels] = useState<string[]>(settings?.models ?? []);
  const [modelInput, setModelInput] = useState("");
  const [defaultModel, setDefaultModel] = useState<string>(settings?.defaultModel ?? "");
  const [temperature, setTemperature] = useState<number>(settings?.temperature ?? 0.7);
  const [topP, setTopP] = useState<number>(settings?.topP ?? 1);
  const [maxTokens, setMaxTokens] = useState<string>(settings?.maxTokens?.toString() ?? "");
  const [timeoutSecs, setTimeoutSecs] = useState<string>(settings?.timeoutSecs?.toString() ?? "");
  const [retry, setRetry] = useState<string>(settings?.retry?.toString() ?? "");
  const [maxConcurrency, setMaxConcurrency] = useState<string>(
    settings?.maxConcurrency?.toString() ?? "",
  );
  const [priority, setPriority] = useState<number>(settings?.priority ?? 5);
  const [roles, setRoles] = useState<string[]>(settings?.roles ?? []);
  const [enabled, setEnabled] = useState<boolean>(settings?.enabled ?? true);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [testResult, setTestResult] = useState<{
    status: ProviderTestStatus;
    message: string;
  } | null>(null);

  const saveMut = useMutation({
    mutationFn: (input: ProviderInput) => ipc.upsertProvider(input),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["providers"] });
      toast.success(t("provider.saved"));
      onDone();
    },
    onError: (e) => toast.error(`${t("provider.saveFailed")}: ${describeError(e)}`),
  });

  const testMut = useMutation({
    mutationFn: (input: TestProviderConnectionInput) => ipc.testProviderConnection(input),
    onSuccess: (result, input) => {
      if (input.providerId !== null) {
        onTested(input.providerId, result.ok ? "ok" : "error");
      }
      setTestResult(
        result.ok
          ? { status: "ok", message: t("provider.testOk", { ms: result.latencyMs ?? 0 }) }
          : { status: "error", message: result.error ?? t("provider.testFailed") },
      );
    },
  });

  const deleteMut = useMutation({
    mutationFn: (id: string) => ipc.deleteProvider(id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["providers"] });
      toast.success(t("provider.deleted"));
      onDeleted();
      onDone();
    },
    onError: (e) => {
      const message =
        e instanceof IpcCommandError && e.code === "store.conflict"
          ? t("provider.deleteConflict")
          : describeError(e);
      toast.error(`${t("provider.deleteFailed")}: ${message}`);
    },
  });

  function handleTypeChange(next: TypeKey): void {
    setTypeKey(next);
    const preset = PRESET_URLS[next];
    if (preset !== undefined && baseUrl.trim().length === 0) setBaseUrl(preset);
  }

  function addModel(): void {
    const id = modelInput.trim();
    if (id.length > 0 && !models.includes(id)) setModels([...models, id]);
    setModelInput("");
  }

  function toggleRole(role: RoleKey): void {
    setRoles((prev) =>
      prev.includes(role) ? prev.filter((r) => r !== role) : [...prev, role],
    );
  }

  const saveDisabled =
    saveMut.isPending || name.trim().length === 0 || baseUrl.trim().length === 0;

  return (
    <form
      aria-label={t("provider.heading")}
      className="flex flex-col gap-3 text-xs"
      onSubmit={(e) => {
        e.preventDefault();
        if (saveDisabled) return;
        saveMut.mutate({
          id: provider?.id ?? null,
          name: name.trim(),
          protocol: protocolOf(typeKey),
          baseUrl: baseUrl.trim(),
          capabilities: provider?.capabilities ?? [],
          isMaster: provider?.isMaster ?? false,
          apiKey: apiKey.length > 0 ? apiKey : null,
          settings: {
            models,
            defaultModel: defaultModel.length > 0 ? defaultModel : null,
            temperature,
            topP,
            maxTokens: numberOrNull(maxTokens),
            timeoutSecs: numberOrNull(timeoutSecs),
            retry: numberOrNull(retry),
            maxConcurrency: numberOrNull(maxConcurrency),
            priority,
            roles,
            enabled,
          },
        });
      }}
    >
      {/* Header: avatar + name + protocol badge + enable switch */}
      <div className="flex items-start justify-between gap-2">
        <div className="flex items-center gap-2">
          <span
            aria-hidden
            style={providerAvatarStyle(name || provider?.name || "")}
            className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full text-sm font-semibold text-ink"
          >
            {providerInitials(name || provider?.name || "")}
          </span>
          <div>
            <p className="text-sm font-medium text-ink">
              {provider?.name || t("provider.untitled")}
            </p>
            <span className="mt-0.5 inline-block rounded bg-ink-accent/20 px-1.5 py-0.5 text-[10px] text-ink-accent">
              {t(TYPE_OPTIONS.find((o) => o.key === typeKey)?.labelKey ?? "provider.typeCustom")}
            </span>
          </div>
        </div>
        <label className="flex items-center gap-1.5 text-ink-muted">
          <input
            type="checkbox"
            checked={enabled}
            onChange={(e) => setEnabled(e.target.checked)}
            className="accent-[var(--nuomi-accent)]"
          />
          {t("provider.enabled")}
        </label>
      </div>

      {/* Connection */}
      <fieldset className="rounded border border-ink-muted/30 p-2">
        <legend className="px-1 font-medium text-ink-muted">{t("provider.connection")}</legend>
        <div className="grid grid-cols-2 gap-2">
          <label className="flex flex-col gap-0.5 text-ink-muted">
            {t("provider.providerType")}
            <select
              value={typeKey}
              onChange={(e) => handleTypeChange(e.target.value as TypeKey)}
              className={field}
            >
              {TYPE_OPTIONS.map((option) => (
                <option key={option.key} value={option.key}>
                  {t(option.labelKey)}
                </option>
              ))}
            </select>
          </label>
          <label className="flex flex-col gap-0.5 text-ink-muted">
            {t("provider.displayName")}
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              required
              className={field}
            />
          </label>
          <label className="col-span-2 flex flex-col gap-0.5 text-ink-muted">
            {t("provider.baseUrl")}
            <input
              type="url"
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              required
              placeholder="https://api.example.com/v1"
              className={field}
            />
          </label>
          <div className="col-span-2 flex items-end gap-2">
            <label className="flex flex-1 flex-col gap-0.5 text-ink-muted">
              {t("provider.apiKey")}
              <span className="flex items-center gap-1">
                <input
                  type={showKey ? "text" : "password"}
                  value={apiKey}
                  onChange={(e) => setApiKey(e.target.value)}
                  autoComplete="off"
                  placeholder={provider?.hasKey ? t("provider.hasKeyStored") : undefined}
                  className={field}
                />
                <button
                  type="button"
                  onClick={() => setShowKey((v) => !v)}
                  className="shrink-0 rounded border border-ink-muted/40 px-2 py-1 text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
                >
                  {showKey ? t("provider.hide") : t("provider.show")}
                </button>
              </span>
              <span className="text-[10px]">{t("provider.apiKeyHint")}</span>
            </label>
            <button
              type="button"
              disabled={testMut.isPending || baseUrl.trim().length === 0}
              onClick={() =>
                testMut.mutate({
                  providerId: provider?.id ?? null,
                  protocol: protocolOf(typeKey),
                  baseUrl: baseUrl.trim(),
                  apiKey: apiKey.length > 0 ? apiKey : null,
                  model: defaultModel.length > 0 ? defaultModel : (models[0] ?? null),
                })
              }
              className="shrink-0 rounded border border-ink-muted/40 px-2 py-1 text-ink hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
            >
              {testMut.isPending ? t("provider.testing") : t("provider.testConnection")}
            </button>
          </div>
        </div>
        {testResult !== null && (
          <p
            role="status"
            className={`mt-1 ${testResult.status === "ok" ? "text-state-ok" : "text-state-danger"}`}
          >
            {testResult.message}
          </p>
        )}
      </fieldset>

      {/* Models */}
      <fieldset className="rounded border border-ink-muted/30 p-2">
        <legend className="px-1 font-medium text-ink-muted">{t("provider.models")}</legend>
        <div className="flex flex-wrap items-center gap-1">
          {models.map((model) => (
            <span
              key={model}
              className="flex items-center gap-1 rounded bg-surface-overlay px-1.5 py-0.5 text-ink"
            >
              {model}
              <button
                type="button"
                aria-label={`${t("common.remove")} ${model}`}
                onClick={() => setModels(models.filter((m) => m !== model))}
                className="text-ink-muted hover:text-state-danger focus-visible:ring-2 focus-visible:ring-ink-accent"
              >
                ×
              </button>
            </span>
          ))}
          <input
            value={modelInput}
            onChange={(e) => setModelInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                addModel();
              }
            }}
            onBlur={addModel}
            placeholder={t("provider.addModelPlaceholder")}
            aria-label={t("provider.addModelPlaceholder")}
            className="w-40 border border-dashed border-ink-muted/60 bg-transparent px-1.5 py-0.5 text-ink placeholder:text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent"
          />
        </div>
        <label className="mt-2 flex w-48 flex-col gap-0.5 text-ink-muted">
          {t("provider.defaultModel")}
          <select
            value={defaultModel}
            onChange={(e) => setDefaultModel(e.target.value)}
            className={field}
          >
            <option value="">{t("provider.defaultModelNone")}</option>
            {models.map((model) => (
              <option key={model} value={model}>
                {model}
              </option>
            ))}
          </select>
        </label>
      </fieldset>

      {/* Advanced (collapsible) */}
      <div className="rounded border border-ink-muted/30 p-2">
        <button
          type="button"
          aria-expanded={advancedOpen}
          onClick={() => setAdvancedOpen((o) => !o)}
          className="font-medium text-ink-muted hover:text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
        >
          {advancedOpen ? "▾" : "▸"} {t("provider.advanced")}
        </button>
        {advancedOpen && (
          <div className="grid grid-cols-2 gap-x-3 gap-y-2 pt-2">
            <label className="flex flex-col gap-0.5 text-ink-muted">
              {t("provider.temperature")}
              <span className="flex items-center gap-2">
                <input
                  type="range"
                  min={0}
                  max={2}
                  step={0.1}
                  value={temperature}
                  onChange={(e) => setTemperature(Number(e.target.value))}
                  className="flex-1 accent-[var(--nuomi-accent)]"
                />
                <span className="w-8 text-right tabular-nums text-ink">
                  {temperature.toFixed(1)}
                </span>
              </span>
            </label>
            <label className="flex flex-col gap-0.5 text-ink-muted">
              {t("provider.topP")}
              <span className="flex items-center gap-2">
                <input
                  type="range"
                  min={0}
                  max={1}
                  step={0.05}
                  value={topP}
                  onChange={(e) => setTopP(Number(e.target.value))}
                  className="flex-1 accent-[var(--nuomi-accent)]"
                />
                <span className="w-8 text-right tabular-nums text-ink">{topP.toFixed(2)}</span>
              </span>
            </label>
            <label className="flex flex-col gap-0.5 text-ink-muted">
              {t("provider.maxTokens")}
              <input
                type="number"
                min={1}
                value={maxTokens}
                onChange={(e) => setMaxTokens(e.target.value)}
                className={field}
              />
            </label>
            <label className="flex flex-col gap-0.5 text-ink-muted">
              {t("provider.timeoutSecs")}
              <input
                type="number"
                min={1}
                value={timeoutSecs}
                onChange={(e) => setTimeoutSecs(e.target.value)}
                className={field}
              />
            </label>
            <label className="flex flex-col gap-0.5 text-ink-muted">
              {t("provider.retry")}
              <input
                type="number"
                min={0}
                value={retry}
                onChange={(e) => setRetry(e.target.value)}
                className={field}
              />
            </label>
            <label className="flex flex-col gap-0.5 text-ink-muted">
              {t("provider.maxConcurrency")}
              <input
                type="number"
                min={1}
                value={maxConcurrency}
                onChange={(e) => setMaxConcurrency(e.target.value)}
                className={field}
              />
            </label>
          </div>
        )}
      </div>

      {/* Routing & priority */}
      <fieldset className="rounded border border-ink-muted/30 p-2">
        <legend className="px-1 font-medium text-ink-muted">{t("provider.routing")}</legend>
        <label className="flex flex-col gap-0.5 text-ink-muted">
          {t("provider.priority")}
          <span className="flex items-center gap-2">
            <input
              type="range"
              min={0}
              max={10}
              step={1}
              value={priority}
              onChange={(e) => setPriority(Number(e.target.value))}
              className="flex-1 accent-[var(--nuomi-accent)]"
            />
            <span className="w-8 text-right tabular-nums text-ink">{priority}</span>
          </span>
        </label>
        <div className="mt-2 flex flex-wrap gap-1.5">
          {ROLE_KEYS.map((role) => {
            const active = roles.includes(role);
            return (
              <label
                key={role}
                className={`flex cursor-pointer items-center gap-1 rounded-full border px-2 py-0.5 focus-within:ring-2 focus-within:ring-ink-accent ${
                  active
                    ? "border-ink-accent bg-ink-accent/20 text-ink"
                    : "border-ink-muted/40 text-ink-muted hover:bg-surface-overlay"
                }`}
              >
                <input
                  type="checkbox"
                  className="sr-only"
                  checked={active}
                  onChange={() => toggleRole(role)}
                />
                {t(ROLE_LABEL_KEYS[role])}
              </label>
            );
          })}
        </div>
      </fieldset>

      {/* Footer */}
      <div className="flex items-center justify-between border-t border-ink-muted/30 pt-2">
        {provider !== null ? (
          confirmDelete ? (
            <button
              type="button"
              disabled={deleteMut.isPending}
              onClick={() => deleteMut.mutate(provider.id)}
              className="rounded border border-state-danger px-2 py-1 text-state-danger hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
            >
              {t("provider.deleteConfirm")}
            </button>
          ) : (
            <button
              type="button"
              onClick={() => setConfirmDelete(true)}
              className="text-state-danger hover:underline focus-visible:ring-2 focus-visible:ring-ink-accent"
            >
              {t("common.delete")}
            </button>
          )
        ) : (
          <span />
        )}
        <div className="flex gap-2">
          <button
            type="button"
            onClick={onDone}
            className="rounded border border-ink-muted px-3 py-1 text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            {t("common.cancel")}
          </button>
          <button
            type="submit"
            disabled={saveDisabled}
            className="rounded bg-ink-accent px-3 py-1 text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
          >
            {t("common.save")}
          </button>
        </div>
      </div>
    </form>
  );
}
