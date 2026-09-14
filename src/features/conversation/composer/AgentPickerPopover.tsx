import type { ReactNode } from "react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { ipc } from "../../../lib/ipc/client";
import { groupAgentOptions } from "../../../lib/conversation/agentResolve";

export interface AgentPickerPopoverProps {
  sessionId: string;
  onSelect: (kind: string, id: string) => Promise<void>;
  onClose: () => void;
}

/**
 * Popover for picking an agent. Shows a search input and lists agents
 * grouped by CLI agents and Roles. Clicking an item calls onSelect.
 */
export function AgentPickerPopover({
  onSelect,
  onClose,
}: AgentPickerPopoverProps): ReactNode {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");

  const { data: options } = useQuery({
    queryKey: ["agentOptions"],
    queryFn: () => ipc.listAgentOptions(),
    staleTime: 30_000,
  });

  const { cli, role } = useMemo(() => groupAgentOptions(options ?? []), [options]);

  const filterFn = (name: string): boolean =>
    search.length === 0 || name.toLowerCase().includes(search.toLowerCase());

  return (
    <div
      className="absolute bottom-full left-0 z-20 mb-1 w-64 rounded border border-ink-muted/40 bg-surface-raised shadow-lg"
      role="dialog"
      aria-label={t("composer.agentPickerTitle")}
    >
      <div className="flex items-center gap-1 border-b border-ink-muted/30 p-1.5">
        <input
          type="text"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder={t("composer.agentPickerSearch")}
          className="flex-1 bg-transparent text-sm text-ink placeholder:text-ink-muted focus-visible:outline-none"
          autoFocus
        />
        <button
          type="button"
          onClick={onClose}
          className="text-ink-muted hover:text-ink"
          aria-label="Close"
        >
          ✕
        </button>
      </div>
      <div className="max-h-48 overflow-y-auto py-1">
        {cli.length > 0 && (
          <div className="px-1.5">
            <div className="py-0.5 text-xs font-semibold text-ink-muted">CLI Agents</div>
            {cli.filter((o) => filterFn(o.name)).map((o) => (
              <button
                key={`${o.kind}:${o.id}`}
                type="button"
                onClick={() => void onSelect(o.kind, o.id)}
                className="flex w-full items-center gap-2 rounded px-1.5 py-1 text-left text-sm text-ink hover:bg-surface-overlay"
              >
                <span className="flex-1 truncate">{o.name}</span>
                {!o.enabled && <span className="text-xs text-ink-muted">disabled</span>}
              </button>
            ))}
          </div>
        )}
        {role.length > 0 && (
          <div className="px-1.5">
            <div className="py-0.5 text-xs font-semibold text-ink-muted">Roles</div>
            {role.filter((o) => filterFn(o.name)).map((o) => (
              <button
                key={`${o.kind}:${o.id}`}
                type="button"
                onClick={() => void onSelect(o.kind, o.id)}
                className="flex w-full items-center gap-2 rounded px-1.5 py-1 text-left text-sm text-ink hover:bg-surface-overlay"
              >
                <span className="flex-1 truncate">{o.name}</span>
                {o.builtin && <span className="text-xs text-ink-muted">builtin</span>}
              </button>
            ))}
          </div>
        )}
        {cli.length === 0 && role.length === 0 && (
          <div className="px-1.5 py-2 text-sm text-ink-muted">{t("commands.noMatch")}</div>
        )}
      </div>
    </div>
  );
}
