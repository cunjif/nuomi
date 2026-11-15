import { useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useUiStore } from "../../lib/store/uiStore";
import { useClickOutside } from "../../components/ui/useClickOutside";
import { DockIconButton } from "./DockIconButton";
import { NAV_ITEMS } from "./LeftRail";
import { ConversationsList } from "../conversation/ConversationsList";
import { NewConversationDialog } from "../conversation/new/NewConversationDialog";

/**
 * Icon dock shown when the left rail is collapsed: the session-list entry
 * (floating panel), the new-conversation entry, every navigation view, plus
 * expand / fully-hide controls — so all rail functions stay reachable
 * without expanding the panel.
 */
export function LeftPanelDock(): ReactNode {
  const { t } = useTranslation();
  const view = useUiStore((s) => s.view);
  const setView = useUiStore((s) => s.setView);
  const selectedSessionId = useUiStore((s) => s.selectedSessionId);
  const setLeftPanel = useUiStore((s) => s.setLeftPanel);
  const [sessionsOpen, setSessionsOpen] = useState(false);
  const [newDialogOpen, setNewDialogOpen] = useState(false);
  const sessionsBtnRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  useClickOutside([sessionsBtnRef, panelRef], {
    enabled: sessionsOpen,
    onOutside: () => setSessionsOpen(false),
    escape: true,
  });

  // Picking a session is the whole point of the floating panel — close it
  // as soon as the selection changes (opening with a fresh ref value so the
  // mount-time selection never closes it immediately).
  const selectedAtOpen = useRef<string | null | undefined>(undefined);
  useEffect(() => {
    if (!sessionsOpen) {
      selectedAtOpen.current = undefined;
      return;
    }
    if (selectedAtOpen.current === undefined) {
      selectedAtOpen.current = selectedSessionId;
      return;
    }
    if (selectedSessionId !== selectedAtOpen.current) {
      setSessionsOpen(false);
      selectedAtOpen.current = undefined;
    }
  }, [sessionsOpen, selectedSessionId]);

  return (
    <nav
      aria-label={t("shell.appName")}
      data-testid="left-panel-dock"
      className="flex w-10 shrink-0 flex-col items-center gap-1 border-r border-ink-muted/30 bg-surface-raised py-2"
    >
      <div className="relative flex flex-col items-center">
        <DockIconButton
          icon="list"
          label={t("sessions.heading")}
          active={sessionsOpen}
          onClick={() => setSessionsOpen((v) => !v)}
          buttonRef={sessionsBtnRef}
        />
        {sessionsOpen && (
          <div
            ref={panelRef}
            data-testid="dock-sessions-panel"
            className="absolute left-full top-0 z-30 flex h-full max-h-[70vh] w-72 min-h-56 flex-col overflow-hidden rounded-lg border border-ink-muted/40 bg-surface-raised shadow-lg"
          >
            <ConversationsList />
          </div>
        )}
      </div>
      <DockIconButton icon="plus" label={t("sessions.newSession")} onClick={() => setNewDialogOpen(true)} />
      <div className="my-1 h-px w-6 bg-ink-muted/30" aria-hidden="true" />
      {NAV_ITEMS.map((item) => (
        <DockIconButton
          key={item.view}
          icon={item.icon}
          label={t(item.labelKey)}
          active={view === item.view}
          onClick={() => {
            // setView already releases the editor area (see uiStore).
            setView(item.view);
          }}
        />
      ))}
      <div className="mt-auto flex flex-col items-center gap-1 pt-2">
        <DockIconButton icon="eye-off" label={t("shell.panelHide")} onClick={() => setLeftPanel({ hidden: true })} />
        <DockIconButton icon="panel-left" label={t("shell.panelExpand")} onClick={() => setLeftPanel({ collapsed: false })} />
      </div>
      {newDialogOpen && <NewConversationDialog kind="chat" onClose={() => setNewDialogOpen(false)} />}
    </nav>
  );
}
