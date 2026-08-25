import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useDraggable } from "@dnd-kit/core";
import type { TaskDto } from "../../lib/ipc/bindings.gen";
import { Spinner } from "../../components/ui/Spinner";
import { statusLabelKey, TASK_STATUSES, type TaskStatus } from "./taskStatuses";

interface TaskCardProps {
  task: TaskDto;
  onOpenRuns: (taskId: string) => void;
  onMove: (taskId: string, status: TaskStatus) => void;
}

/** Draggable card with an equivalent keyboard menu (a11y rule for drag). */
export function TaskCard({ task, onOpenRuns, onMove }: TaskCardProps): ReactNode {
  const { t } = useTranslation();
  const [menuOpen, setMenuOpen] = useState(false);
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({ id: task.id });

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
        onClick={() => setMenuOpen((o) => !o)}
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
                  setMenuOpen(false);
                  onMove(task.id, status);
                }}
                onPointerDown={(e) => e.stopPropagation()}
                className="block w-full px-3 py-1 text-left text-xs text-ink hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
              >
                {t("board.moveTo", { status: t(statusLabelKey(status)) })}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
