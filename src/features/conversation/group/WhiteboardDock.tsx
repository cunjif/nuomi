import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { ipc } from "../../../lib/ipc/client";

export interface WhiteboardDockProps {
  sessionId: string;
}

/** Right-side collapsible whiteboard dock for group conversations. */
export function WhiteboardDock({ sessionId }: WhiteboardDockProps): ReactNode {
  const { t } = useTranslation();
  const [collapsed, setCollapsed] = useState(false);
  const notesQuery = useQuery({
    queryKey: ["whiteboard", sessionId],
    queryFn: () => ipc.listWhiteboardNotes(sessionId),
    refetchInterval: 5_000,
  });

  if (collapsed) {
    return (
      <button
        type="button"
        onClick={() => setCollapsed(false)}
        className="flex w-6 shrink-0 items-center justify-center border-l border-ink-muted/30 text-xs text-ink-muted"
      >
        {t("conversation.whiteboard")}
      </button>
    );
  }

  return (
    <aside className="flex w-48 shrink-0 flex-col border-l border-ink-muted/30">
      <div className="flex items-center justify-between border-b border-ink-muted/30 px-2 py-1">
        <span className="text-xs font-semibold text-ink-muted">{t("conversation.whiteboard")}</span>
        <button
          type="button"
          onClick={() => setCollapsed(true)}
          className="text-xs text-ink-muted hover:text-ink"
        >
          ▸
        </button>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-1">
        {(notesQuery.data ?? []).map((note) => (
          <div key={note.seq} className="rounded border border-ink-muted/20 p-1.5 text-xs text-ink">
            <span className="font-medium">{note.authorRoleId ?? "—"}</span>
            <p className="mt-0.5 text-ink-muted">{note.body}</p>
          </div>
        ))}
        {(notesQuery.data ?? []).length === 0 && (
          <p className="px-1 py-2 text-xs text-ink-muted">{t("conversation.whiteboardEmpty")}</p>
        )}
      </div>
    </aside>
  );
}
