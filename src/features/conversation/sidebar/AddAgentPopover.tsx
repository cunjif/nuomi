import type { ReactNode } from "react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../../lib/ipc/client";
import { groupAgentOptions } from "../../../lib/conversation/agentResolve";
import { describeError } from "../../../i18n";
import { toast } from "../../../lib/store/toastStore";

export interface AddAgentPopoverProps {
  sessionId: string;
  /** Agent keys already in the conversation, formatted as `${kind}:${id}`. */
  excludeAgentKeys: Set<string>;
  onAdded: () => void;
  onClose: () => void;
}

/**
 * Popover for adding a new agent to the conversation. Lists available
 * agents (CLI + Roles) minus those already participating. Selecting
 * triggers `addConversationAgent`, upgrading chat→group if needed.
 */
export function AddAgentPopover({
  sessionId,
  excludeAgentKeys,
  onAdded,
  onClose,
}: AddAgentPopoverProps): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [search, setSearch] = useState("");

  const { data: options } = useQuery({
    queryKey: ["agentOptions"],
    queryFn: () => ipc.listAgentOptions(),
    staleTime: 30_000,
  });

  const addMut = useMutation({
    mutationFn: (input: { kind: string; id: string }) =>
      ipc.addConversationAgent(sessionId, { kind: input.kind, id: input.id }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["conversation", sessionId] });
      onAdded();
    },
    onError: (e) => toast.error(t("conversation.addAgent.failed", { message: describeError(e) })),
  });

  const { cli, role } = useMemo(() => groupAgentOptions(options ?? []), [options]);

  const filterFn = (name: string): boolean =>
    search.length === 0 || name.toLowerCase().includes(search.toLowerCase());

  const notExcluded = (kind: string, id: string): boolean =>
    !excludeAgentKeys.has(`${kind}:${id}`);

  return (
    <div
      className="absolute left-0 top-full z-30 mt-1 w-64 rounded border border-ink-muted/40 bg-surface-raised shadow-lg"
      role="dialog"
      aria-label={t("conversation.addAgent.title")}
    >
      <div className="flex items-center gap-1 border-b border-ink-muted/30 p-1.5">
        <input
          type="text"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder={t("conversation.addAgent.search")}
          className="flex-1 bg-transparent text-sm text-ink placeholder:text-ink-muted focus-visible:outline-none"
          autoFocus
        />
        <button
          type="button"
          onClick={onClose}
          className="text-ink-muted hover:text-ink"
          aria-label={t("common.close")}
        >
          ✕
        </button>
      </div>
      <div className="max-h-48 overflow-y-auto py-1">
        {cli.filter((o) => notExcluded(o.kind, o.id) && filterFn(o.name)).length > 0 && (
          <div className="px-1.5">
            <div className="py-0.5 text-xs font-semibold text-ink-muted">CLI Agents</div>
            {cli.filter((o) => notExcluded(o.kind, o.id) && filterFn(o.name)).map((o) => (
              <button
                key={`${o.kind}:${o.id}`}
                type="button"
                disabled={addMut.isPending}
                onClick={() => addMut.mutate({ kind: o.kind, id: o.id })}
                className="flex w-full items-center gap-2 rounded px-1.5 py-1 text-left text-sm text-ink hover:bg-surface-overlay disabled:opacity-50"
              >
                <span className="flex-1 truncate">{o.name}</span>
                {!o.enabled && <span className="text-xs text-ink-muted">disabled</span>}
              </button>
            ))}
          </div>
        )}
        {role.filter((o) => notExcluded(o.kind, o.id) && filterFn(o.name)).length > 0 && (
          <div className="px-1.5">
            <div className="py-0.5 text-xs font-semibold text-ink-muted">Roles</div>
            {role.filter((o) => notExcluded(o.kind, o.id) && filterFn(o.name)).map((o) => (
              <button
                key={`${o.kind}:${o.id}`}
                type="button"
                disabled={addMut.isPending}
                onClick={() => addMut.mutate({ kind: o.kind, id: o.id })}
                className="flex w-full items-center gap-2 rounded px-1.5 py-1 text-left text-sm text-ink hover:bg-surface-overlay disabled:opacity-50"
              >
                <span className="flex-1 truncate">{o.name}</span>
                {o.builtin && <span className="text-xs text-ink-muted">builtin</span>}
              </button>
            ))}
          </div>
        )}
        {cli.filter((o) => notExcluded(o.kind, o.id) && filterFn(o.name)).length === 0 &&
          role.filter((o) => notExcluded(o.kind, o.id) && filterFn(o.name)).length === 0 && (
            <div className="px-1.5 py-2 text-sm text-ink-muted">{t("conversation.addAgent.empty")}</div>
          )}
      </div>
    </div>
  );
}
