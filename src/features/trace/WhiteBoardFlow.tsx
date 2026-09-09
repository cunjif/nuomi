import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

interface WhiteBoardFlowProps {
  notes: Array<{ seq: number; body: string }>;
}

/** Side stream of shared blackboard notes (empty until kernel emits them). */
export function WhiteBoardFlow({ notes }: WhiteBoardFlowProps): ReactNode {
  const { t } = useTranslation();
  return (
    <aside aria-label={t("trace.whiteboardHeading")} className="w-64 shrink-0 overflow-y-auto border-l border-ink-muted/30 p-3">
      <h3 className="text-title-hand mb-2 text-xs font-semibold uppercase tracking-wide text-ink-muted">
        {t("trace.whiteboardHeading")}
      </h3>
      {notes.length === 0 ? (
        <p className="text-xs text-ink-muted">{t("trace.noNotes")}</p>
      ) : (
        <ul className="flex flex-col gap-2">
          {notes.map((note) => (
            <li
              key={note.seq}
              className="torn-note rotate-[-0.4deg] p-2 font-note-hand text-xs text-ink"
            >
              {note.body}
            </li>
          ))}
        </ul>
      )}
    </aside>
  );
}
