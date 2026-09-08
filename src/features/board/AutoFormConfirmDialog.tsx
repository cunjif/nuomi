import type { KeyboardEvent, ReactNode } from "react";
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { TeamPlanDto, TeamTopologyDto } from "../../lib/ipc/bindings.gen";

const MAX_SUMMARY = 40;

const FOCUSABLE_SELECTOR =
  'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

const TOPOLOGY_LABEL_KEYS: Record<TeamTopologyDto, string> = {
  pipeline: "settings.teams.topologyPipeline",
  router: "settings.teams.topologyRouter",
  group_chat: "settings.teams.topologyGroupChat",
};

/** Task summary fits one line; longer text is ellipsised. */
function truncateSummary(text: string): string {
  return text.length > MAX_SUMMARY ? `${text.slice(0, MAX_SUMMARY)}…` : text;
}

export type AutoFormConfirmDialogProps = {
  taskTitle: string;
  plan: TeamPlanDto;
  onConfirm: () => void;
  onCancel: () => void;
};

/** Dry-run confirm dialog before an auto-formed team is built (打磨③b). */
export function AutoFormConfirmDialog({
  taskTitle,
  plan,
  onConfirm,
  onCancel,
}: AutoFormConfirmDialogProps): ReactNode {
  const { t } = useTranslation();
  const cancelRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const summary = truncateSummary(taskTitle);

  useEffect(() => {
    cancelRef.current?.focus();
  }, []);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>): void => {
    if (e.key === "Escape") {
      e.stopPropagation();
      onCancel();
      return;
    }
    if (e.key !== "Tab") return;
    const panel = panelRef.current;
    if (!panel) return;
    const focusables = Array.from(panel.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR));
    if (focusables.length === 0) return;
    const first = focusables[0];
    const last = focusables[focusables.length - 1];
    if (!(first && last)) return;
    const active = document.activeElement;
    if (e.shiftKey && active === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && active === last) {
      e.preventDefault();
      first.focus();
    }
  };

  const badge =
    "rounded bg-surface-overlay px-1.5 py-0.5 text-[10px] leading-4 text-ink-muted";

  return (
    <div
      className="fixed inset-0 z-40 flex items-center justify-center bg-surface-scrim p-4"
      onClick={onCancel}
      onKeyDown={onKeyDown}
    >
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-label={t("board.autoFormDialogTitle", { title: summary })}
        className="flex w-full max-w-md flex-col rounded border border-ink-muted/40 bg-surface-raised p-3 shadow-lg"
        onClick={(e) => e.stopPropagation()}
      >
        <h2 className="truncate text-sm font-semibold" title={taskTitle}>
          {summary}
        </h2>
        <div className="mt-2 flex flex-wrap items-center gap-2">
          <span className="text-xs text-ink-muted">{t("board.topologyLabel")}</span>
          <span className={badge}>{t(TOPOLOGY_LABEL_KEYS[plan.topology])}</span>
          {plan.maxRounds !== null && (
            <span className={badge}>
              {t("settings.teams.maxRounds")}: {plan.maxRounds}
            </span>
          )}
        </div>
        <h3 className="mt-3 text-xs font-semibold uppercase tracking-wide text-ink-muted">
          {t("board.autoFormMemberHeading")}
        </h3>
        <ul className="mt-1 flex flex-col gap-1">
          {plan.members.map((member) => (
            <li
              key={`${member.kind}:${member.refId}`}
              className="flex flex-wrap items-center gap-2 rounded border border-ink-muted/30 bg-surface px-2 py-1 text-xs"
            >
              <span className="font-medium text-ink">{member.name}</span>
              <span className={badge}>{member.kind}</span>
              {member.willCreateRole && (
                <span className="rounded bg-state-ok/20 px-1.5 py-0.5 text-[10px] leading-4 text-state-ok">
                  {t("board.willCreateRoleBadge")}
                </span>
              )}
            </li>
          ))}
        </ul>
        <h3 className="mt-3 text-xs font-semibold uppercase tracking-wide text-ink-muted">
          {t("board.rationaleHeading")}
        </h3>
        <p className="mt-1 text-xs text-ink">{plan.rationale}</p>
        <div className="mt-4 flex justify-end gap-2">
          <button
            ref={cancelRef}
            type="button"
            onClick={onCancel}
            className="rounded border border-ink-muted/40 px-3 py-1 text-xs text-ink hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            {t("common.cancel")}
          </button>
          <button
            type="button"
            onClick={onConfirm}
            className="pixel-fill-accent px-3 py-1 text-xs text-surface focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            {t("board.confirmRun")}
          </button>
        </div>
      </div>
    </div>
  );
}
