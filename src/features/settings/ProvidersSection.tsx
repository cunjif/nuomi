import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import type { ProviderDto } from "../../lib/ipc/bindings.gen";
import { Button } from "../../components/ui/Button";
import { ipc } from "../../lib/ipc/client";
import {
  ProviderForm,
  providerAvatarStyle,
  providerInitials,
  type ProviderTestStatus,
} from "./ProviderForm";

type DotStatus = ProviderTestStatus | "untested";

const DOT_CLASSES: Record<DotStatus, string> = {
  ok: "bg-state-ok",
  error: "bg-state-danger",
  untested: "bg-ink-muted/40",
};

interface ListRowProps {
  provider: ProviderDto;
  status: DotStatus;
  active: boolean;
  onSelect: () => void;
}

/** One provider row: initial avatar + name + current model + status dot. */
function ListRow({ provider, status, active, onSelect }: ListRowProps): ReactNode {
  const { t } = useTranslation();
  const currentModel = provider.settings.defaultModel ?? provider.settings.models?.[0]?.id ?? null;
  return (
    <li>
      <button
        type="button"
        onClick={onSelect}
        aria-current={active ? "true" : undefined}
        className={`flex w-full items-center gap-2 rounded px-2 py-1.5 text-left hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent ${
          active ? "bg-surface-overlay" : ""
        }`}
      >
        <span
          aria-hidden
          style={providerAvatarStyle(provider.name)}
          className="flex h-7 w-7 shrink-0 items-center justify-center rounded-[155px_12px_155px_12px/12px_155px_12px_155px] border border-dashed border-ink-muted/50 text-[11px] font-semibold text-ink"
        >
          {providerInitials(provider.name)}
        </span>
        <span className="min-w-0 flex-1">
          <span className="block truncate text-xs font-medium text-ink">{provider.name}</span>
          <span className="block truncate text-[10px] text-ink-muted">
            {currentModel === null
              ? t("provider.subtitleNoModel")
              : t("provider.subtitleModel", { model: currentModel })}
          </span>
        </span>
        <span
          role="img"
          aria-label={t(
            status === "ok"
              ? "provider.statusConnected"
              : status === "error"
                ? "provider.statusError"
                : "provider.statusUntested",
          )}
          className={`h-2 w-2 shrink-0 rounded-[40%_60%_55%_45%/50%_45%_55%_50%] ${DOT_CLASSES[status]}`}
        />
      </button>
    </li>
  );
}

/** Provider master-detail: searchable list on the left, editor on the right. */
export function ProvidersSection(): ReactNode {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [statuses, setStatuses] = useState<Record<string, ProviderTestStatus>>({});

  const providersQuery = useQuery({ queryKey: ["providers"], queryFn: ipc.listProviders });
  const providers = providersQuery.data ?? [];
  const needle = search.trim().toLowerCase();
  const filtered = providers.filter((p) => p.name.toLowerCase().includes(needle));
  const selected = providers.find((p) => p.id === selectedId) ?? null;

  function handleTested(providerId: string, status: ProviderTestStatus): void {
    setStatuses((prev) => ({ ...prev, [providerId]: status }));
  }

  return (
    <section aria-label={t("provider.heading")} className="mb-3">
      <div className="grid grid-cols-[240px_minmax(0,1fr)] gap-3">
        {/* Left: list */}
        <div className="sketch-card flex flex-col self-start bg-surface-raised p-2">
          <div className="mb-2 flex items-center justify-between px-1">
            <h3 className="text-title-hand text-sm font-semibold text-ink">{t("provider.heading")}</h3>
            <span className="rounded bg-ink-accent/20 px-1.5 py-0.5 text-[10px] text-ink-accent">
              {t("provider.count", { count: providers.length })}
            </span>
          </div>
          <input
            type="search"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={t("provider.searchPlaceholder")}
            aria-label={t("provider.searchPlaceholder")}
            className="sketch-input mb-2 w-full bg-surface-overlay px-2 py-1 text-xs text-ink placeholder:text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent"
          />
          {providersQuery.isLoading ? (
            <p className="px-1 py-2 text-xs text-ink-muted">{t("common.empty")}</p>
          ) : providersQuery.error ? (
            <p className="px-1 py-2 text-xs text-state-danger">{t("common.loadFailed")}</p>
          ) : (
            <ul className="flex flex-col gap-0.5">
              {filtered.map((provider) => (
                <ListRow
                  key={provider.id}
                  provider={provider}
                  status={statuses[provider.id] ?? "untested"}
                  active={provider.id === selectedId && !creating}
                  onSelect={() => {
                    setSelectedId(provider.id);
                    setCreating(false);
                  }}
                />
              ))}
            </ul>
          )}
          {!providersQuery.isLoading && providersQuery.error === null && filtered.length === 0 && (
            <p className="px-1 py-2 text-xs text-ink-muted">
              {providers.length === 0 ? t("provider.empty") : t("provider.noSearchMatch")}
            </p>
          )}
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              setCreating(true);
              setSelectedId(null);
            }}
            className="mt-2 w-full"
          >
            {t("provider.add")}
          </Button>
        </div>

        {/* Right: detail */}
        <div className="sketch-card min-h-[16rem] bg-surface-raised p-3">
          {creating || selected !== null ? (
            <ProviderForm
              key={creating ? "new" : (selected?.id ?? "new")}
              provider={creating ? null : selected}
              onDone={() => setCreating(false)}
              onTested={handleTested}
              onDeleted={() => setSelectedId(null)}
            />
          ) : (
            <p className="text-xs text-ink-muted">{t("provider.selectHint")}</p>
          )}
        </div>
      </div>
    </section>
  );
}
