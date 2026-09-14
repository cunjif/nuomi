import type { ReactNode } from "react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { ConversationDto } from "../../../lib/ipc/client";
import { ipc } from "../../../lib/ipc/client";
import { describeError } from "../../../i18n";
import { toast } from "../../../lib/store/toastStore";

export interface ConversationInfoPanelProps {
  conversation: ConversationDto;
  sessionId: string;
}

const MAX_TITLE = 100;
const MAX_GOAL = 500;

/**
 * Conversation info panel: inline-editable title and goal, read-only Todo
 * list, main agent + route mode, whiteboard route mode display.
 */
export function ConversationInfoPanel({
  conversation,
  sessionId,
}: ConversationInfoPanelProps): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();

  const [editingTitle, setEditingTitle] = useState(false);
  const [titleDraft, setTitleDraft] = useState(conversation.title);
  const [editingGoal, setEditingGoal] = useState(false);
  const [goalDraft, setGoalDraft] = useState(conversation.goal ?? "");

  const titleInputRef = useRef<HTMLInputElement | null>(null);
  const goalInputRef = useRef<HTMLTextAreaElement | null>(null);

  useEffect(() => {
    if (editingTitle) titleInputRef.current?.focus();
  }, [editingTitle]);
  useEffect(() => {
    if (editingGoal) goalInputRef.current?.focus();
  }, [editingGoal]);

  const updateMut = useMutation({
    mutationFn: (input: { title: string | null; goal: string | null }) =>
      ipc.updateConversation(sessionId, input),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["conversation", sessionId] });
    },
    onError: (e) => toast.error(describeError(e)),
  });

  const saveTitle = (): void => {
    const trimmed = titleDraft.trim();
    if (trimmed.length === 0 || trimmed === conversation.title) {
      setTitleDraft(conversation.title);
      setEditingTitle(false);
      return;
    }
    if (trimmed.length > MAX_TITLE) {
      toast.error(t("conversation.info.titleTooLong", { max: MAX_TITLE }));
      return;
    }
    updateMut.mutate({ title: trimmed, goal: null });
    setEditingTitle(false);
  };

  const saveGoal = (): void => {
    const trimmed = goalDraft.trim();
    if (trimmed === (conversation.goal ?? "")) {
      setGoalDraft(conversation.goal ?? "");
      setEditingGoal(false);
      return;
    }
    if (trimmed.length > MAX_GOAL) {
      toast.error(t("conversation.info.goalTooLong", { max: MAX_GOAL }));
      return;
    }
    updateMut.mutate({ title: null, goal: trimmed });
    setEditingGoal(false);
  };

  const routeModeLabel = (mode: string | null): string => {
    if (mode === null) return t("conversation.info.notSet");
    if (mode === "orchestrator_worker") return t("conversation.info.routeOrchestratorWorker");
    if (mode === "master_slave") return t("conversation.info.routeMasterSlave");
    return mode;
  };

  const wbRouteModeLabel = (mode: string | null): string => {
    if (mode === null) return t("conversation.info.notSet");
    if (mode === "preemptive") return t("conversation.info.wbRoutePreemptive");
    if (mode === "concurrent") return t("conversation.info.wbRouteConcurrent");
    return mode;
  };

  return (
    <div className="space-y-3 p-3">
      {/* Title */}
      <div>
        <div className="text-xs text-ink-muted">{t("conversation.info.title")}</div>
        {editingTitle ? (
          <input
            ref={titleInputRef}
            type="text"
            value={titleDraft}
            maxLength={MAX_TITLE}
            onChange={(e) => setTitleDraft(e.target.value)}
            onBlur={saveTitle}
            onKeyDown={(e) => {
              if (e.key === "Enter") { e.preventDefault(); saveTitle(); }
              if (e.key === "Escape") { setTitleDraft(conversation.title); setEditingTitle(false); }
            }}
            className="mt-0.5 w-full rounded border border-ink-accent/50 bg-surface px-1.5 py-1 text-sm text-ink focus-visible:outline-none"
          />
        ) : (
          <button
            type="button"
            onClick={() => { setTitleDraft(conversation.title); setEditingTitle(true); }}
            className="mt-0.5 w-full truncate text-left text-sm text-ink hover:text-ink-accent"
          >
            {conversation.title}
          </button>
        )}
      </div>

      {/* Goal */}
      <div>
        <div className="text-xs text-ink-muted">{t("conversation.info.goal")}</div>
        {editingGoal ? (
          <textarea
            ref={goalInputRef}
            value={goalDraft}
            maxLength={MAX_GOAL}
            rows={3}
            onChange={(e) => setGoalDraft(e.target.value)}
            onBlur={saveGoal}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); saveGoal(); }
              if (e.key === "Escape") { setGoalDraft(conversation.goal ?? ""); setEditingGoal(false); }
            }}
            className="mt-0.5 w-full resize-none rounded border border-ink-accent/50 bg-surface px-1.5 py-1 text-sm text-ink focus-visible:outline-none"
          />
        ) : (
          <button
            type="button"
            onClick={() => { setGoalDraft(conversation.goal ?? ""); setEditingGoal(true); }}
            className="mt-0.5 w-full text-left text-sm text-ink hover:text-ink-accent"
          >
            {conversation.goal ?? t("conversation.info.goalEmpty")}
          </button>
        )}
      </div>

      {/* Todo list (read-only) */}
      {conversation.todoList.length > 0 && (
        <div>
          <div className="text-xs text-ink-muted">{t("conversation.info.todoList")}</div>
          <ul className="mt-0.5 space-y-1">
            {conversation.todoList.map((todo) => (
              <li key={todo.id} className="flex items-start gap-1.5 text-sm text-ink">
                <span className={todo.completed ? "text-state-ok" : "text-ink-muted"}>
                  {todo.completed ? "☑" : "☐"}
                </span>
                <span className={todo.completed ? "line-through text-ink-muted" : ""}>
                  {todo.description}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {/* Main agent + route mode */}
      <div>
        <div className="text-xs text-ink-muted">{t("conversation.info.mainAgent")}</div>
        <div className="text-sm text-ink">
          {conversation.mainAgentId ?? t("conversation.info.notSet")}
        </div>
        <div className="mt-1 text-xs text-ink-muted">{t("conversation.info.routeMode")}</div>
        <div className="text-sm text-ink">{routeModeLabel(conversation.routeMode)}</div>
      </div>

      {/* Whiteboard route mode */}
      <div>
        <div className="text-xs text-ink-muted">{t("conversation.info.wbRouteMode")}</div>
        <div className="text-sm text-ink">{wbRouteModeLabel(conversation.whiteboardRouteMode)}</div>
      </div>
    </div>
  );
}
