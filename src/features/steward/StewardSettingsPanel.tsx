import { useEffect } from "react";
import type { ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";
import { Icon } from "../../components/ui/Icon/Icon";
import { SoulEditor } from "./SoulEditor";
import { BindingEditor } from "./BindingEditor";
import { ChatRecordsManager } from "./ChatRecordsManager";
import { RunStatusView } from "./RunStatusView";
import { TodoView } from "./TodoView";

export interface StewardSettingsPanelProps {
  sessionId: string;
  open: boolean;
  onClose: () => void;
}

const SECTION_TITLES = ["SOUL 人设", "Provider 绑定", "聊天记录", "运行状态", "待办"] as const;

/** Section-level error placeholder (spec 5.4.3-1): the panel shell and the
 *  five titled cards stay mounted; each failed section offers its own retry
 *  so a store hiccup never white-screens the panel. */
function SettingsSectionError({
  title,
  onRetry,
}: {
  title: string;
  onRetry: () => void;
}): ReactNode {
  return (
    <section aria-label={title} className="sketch-card bg-surface-raised p-3">
      <h3 className="mb-2 text-sm font-medium text-ink">{title}</h3>
      <button
        type="button"
        className="sketch-btn px-2 py-0.5 text-xs text-state-danger focus-visible:ring-2 focus-visible:ring-ink-accent"
        onClick={onRetry}
      >
        加载失败，点击重试
      </button>
    </section>
  );
}

/** Steward settings slide-over: five sketch-card sections over a scrim,
 *  aligned with the app's drawer paradigm (AgentSidebar/RunDrawer) — the
 *  aside stays mounted and slides via translate-x so both open and close
 *  animate; Esc and scrim click dismiss. */
export function StewardSettingsPanel({
  sessionId,
  open,
  onClose,
}: StewardSettingsPanelProps): ReactNode {
  const settingsQuery = useQuery({
    queryKey: ["steward", "settings", sessionId],
    queryFn: () => ipc.stewardGetSettings(sessionId),
    enabled: open,
  });

  const messagesQuery = useQuery({
    queryKey: ["steward", "messages", sessionId],
    queryFn: () => ipc.stewardListSessionMessages(sessionId, 200),
    enabled: open,
  });

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  return (
    <>
      {open && (
        <div
          className="fixed inset-0 z-40 bg-surface-scrim"
          onClick={onClose}
          role="presentation"
        />
      )}
      <aside
        className={`fixed right-0 top-0 z-40 flex h-full w-96 flex-col border-l border-ink-muted/40 bg-surface-raised shadow-sketch-md transition-transform duration-200 ease-out ${
          open ? "translate-x-0" : "translate-x-full"
        }`}
        role="complementary"
        aria-label="管家设置"
        aria-hidden={!open}
      >
        <div className="flex shrink-0 items-center justify-between border-b border-divider px-3 py-2">
          <h2 className="text-title-hand text-base font-semibold text-ink">管家设置</h2>
          <button
            type="button"
            aria-label="关闭"
            className="sketch-btn px-2 py-0.5 text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent"
            onClick={onClose}
          >
            <Icon name="close" size={16} />
          </button>
        </div>

        <div className="min-h-0 flex-1 space-y-3 overflow-y-auto p-3">
          {settingsQuery.isLoading && (
            <div className="text-sm text-ink-muted">加载中...</div>
          )}

          {settingsQuery.isError &&
            SECTION_TITLES.map((title) => (
              <SettingsSectionError
                key={title}
                title={title}
                onRetry={() => void settingsQuery.refetch()}
              />
            ))}

          {settingsQuery.data && (
            <>
              <section aria-label="SOUL 人设" className="sketch-card bg-surface-raised p-3">
                <SoulEditor
                  sessionId={sessionId}
                  soul={settingsQuery.data.soul}
                />
              </section>

              <section aria-label="Provider 绑定" className="sketch-card bg-surface-raised p-3">
                <BindingEditor
                  sessionId={sessionId}
                  binding={settingsQuery.data.binding}
                />
              </section>

              <section aria-label="聊天记录" className="sketch-card bg-surface-raised p-3">
                <ChatRecordsManager
                  sessionId={sessionId}
                  messageStats={settingsQuery.data.messageStats}
                  messages={messagesQuery.data ?? []}
                />
              </section>

              <section aria-label="运行状态" className="sketch-card bg-surface-raised p-3">
                <h3 className="mb-2 text-sm font-medium text-ink">运行状态</h3>
                <RunStatusView
                  state={settingsQuery.data.runStatus.state}
                  label={settingsQuery.data.runStatus.label}
                />
              </section>

              <section aria-label="待办" className="sketch-card bg-surface-raised p-3">
                <h3 className="mb-2 text-sm font-medium text-ink">待办</h3>
                <TodoView todos={settingsQuery.data.todoList} />
              </section>
            </>
          )}
        </div>
      </aside>
    </>
  );
}
