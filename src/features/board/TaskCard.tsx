import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { useDraggable } from "@dnd-kit/core";
import type { TaskDto } from "../../lib/ipc/bindings.gen";
import { Spinner } from "../../components/ui/Spinner";
import { ipc } from "../../lib/ipc/client";
import { statusLabelKey, TASK_STATUSES, type TaskStatus } from "./taskStatuses";

interface TaskCardProps {
  task: TaskDto;
  onOpenRuns: (taskId: string) => void;
  onMove: (taskId: string, status: TaskStatus) => void;
  onDelete: (taskId: string) => void;
  onRunWithTeam: (taskId: string, teamId: string) => void;
  onAutoFormRun: (taskId: string) => void;
}

const menuButton =
  "block w-full px-3 py-1 text-left text-xs text-ink hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent";

/** Draggable card with an equivalent keyboard menu (a11y rule for drag). */
export function TaskCard({ task, onOpenRuns, onMove, onDelete, onRunWithTeam, onAutoFormRun }: TaskCardProps): ReactNode {
  const { t } = useTranslation();
  const [menuOpen, setMenuOpen] = useState(false);
  const [teamListOpen, setTeamListOpen] = useState(false);
  /** two-step delete (IntegrationsSection pattern): armed by the first click */
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  const teamsQuery = useQuery({ queryKey: ["teams"], queryFn: ipc.listTeams });
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({ id: task.id });

  const closeMenu = (): void => {
    setMenuOpen(false);
    setTeamListOpen(false);
    setConfirmingDelete(false);
  };

  return (
    <div
      ref={setNodeRef}
      {...listeners}
      {...attributes}
      className={`relative rounded border border-ink-muted/40 bg-surface-overlay p-2 ${
        isDragging ? "opacity-50" : ""
      }`}
    >
      <button
        type="button"
        onClick={() => onOpenRuns(task.id)}
        className="block w-full text-left text-sm font-medium text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
        title={task.description}
      >
        {task.title}
      </button>
      {task.status === "running" && (
        <div className="mt-1">
          <Spinner />
        </div>
      )}
      <button
        type="button"
        aria-label={t("board.cardMenu")}
        aria-expanded={menuOpen}
        onClick={() => {
          setMenuOpen((o) => !o);
          setTeamListOpen(false);
        }}
        onPointerDown={(e) => e.stopPropagation()}
        className="absolute right-1 top-1 rounded px-1.5 text-ink-muted hover:bg-surface focus-visible:ring-2 focus-visible:ring-ink-accent"
      >
        ⋯
      </button>
      {menuOpen && (
        <ul className="absolute right-0 top-6 z-10 w-40 rounded border border-ink-muted/40 bg-surface-raised py-1 shadow-lg">
          {TASK_STATUSES.filter((s) => s !== task.status).map((status) => (
            <li key={status}>
              <button
                type="button"
                onClick={() => {
                  closeMenu();
                  onMove(task.id, status);
                }}
                onPointerDown={(e) => e.stopPropagation()}
                className={menuButton}
              >
                {t("board.moveTo", { status: t(statusLabelKey(status)) })}
              </button>
            </li>
          ))}
          <li>
            <button
              type="button"
              aria-expanded={teamListOpen}
              onClick={() => setTeamListOpen((o) => !o)}
              onPointerDown={(e) => e.stopPropagation()}
              className={menuButton}
            >
              {t("board.runWithTeam")}
            </button>
            {teamListOpen && (
              <ul>
                {(teamsQuery.data ?? []).map((team) => (
                  <li key={team.id}>
                    <button
                      type="button"
                      onClick={() => {
                        closeMenu();
                        onRunWithTeam(task.id, team.id);
                      }}
                      onPointerDown={(e) => e.stopPropagation()}
                      className={`${menuButton} pl-6`}
                    >
                      {team.name}
                    </button>
                  </li>
                ))}
                {(teamsQuery.data?.length ?? 0) === 0 && (
                  <li className="px-6 py-1 text-[10px] text-ink-muted" role="note">
                    {t("board.noTeamsForRun")}
                  </li>
                )}
              </ul>
            )}
          </li>
          <li>
            <button
              type="button"
              onClick={() => {
                closeMenu();
                onAutoFormRun(task.id);
              }}
              onPointerDown={(e) => e.stopPropagation()}
              className={menuButton}
            >
              {t("board.autoFormRun")}
            </button>
          </li>
          {task.status !== "running" && (
            <li>
              {confirmingDelete ? (
                <button
                  type="button"
                  onClick={() => {
                    closeMenu();
                    onDelete(task.id);
                  }}
                  onPointerDown={(e) => e.stopPropagation()}
                  aria-label={`${t("board.deleteConfirm")} ${task.title}`}
                  className={`${menuButton} text-state-danger`}
                >
                  {t("board.deleteConfirm")}
                </button>
              ) : (
                <button
                  type="button"
                  onClick={() => setConfirmingDelete(true)}
                  onPointerDown={(e) => e.stopPropagation()}
                  aria-label={`${t("board.delete")} ${task.title}`}
                  className={menuButton}
                >
                  {t("board.delete")}
                </button>
              )}
            </li>
          )}
        </ul>
      )}
    </div>
  );
}
