import type { ReactNode } from "react";
import { ErrorBoundary } from "../../components/ui/ErrorBoundary";
import { useUiStore, type View } from "../../lib/store/uiStore";
import { ApprovalsView } from "../approvals/ApprovalsView";
import { BoardView } from "../board/BoardView";
import { ChatView } from "../chat/ChatView";
import { SchedulerView } from "../scheduler/SchedulerView";
import { SettingsView } from "../settings/SettingsView";
import { TraceView } from "../trace/TraceView";
import { GitView } from "../git/GitView";
import { FilePanel } from "./FilePanel";
import { LeftRail } from "./LeftRail";
import { TopBar } from "./TopBar";

/** U8 three-pane shell: top status bar, left rail, center view, file panel. */
export function Shell(): ReactNode {
  const view = useUiStore((s) => s.view);
  return (
    <div className="flex h-screen flex-col bg-surface text-ink">
      <TopBar />
      <div className="flex min-h-0 flex-1">
        <LeftRail />
        <main className="min-w-0 flex-1 overflow-hidden">
          <ErrorBoundary key={view}>{renderView(view)}</ErrorBoundary>
        </main>
        <FilePanel />
      </div>
    </div>
  );
}

function renderView(view: View): ReactNode {
  switch (view) {
    case "chat":
      return <ChatView />;
    case "board":
      return <BoardView />;
    case "trace":
      return <TraceView />;
    case "git":
      return <GitView />;
    case "approvals":
      return <ApprovalsView />;
    case "scheduler":
      return <SchedulerView />;
    case "settings":
      return <SettingsView />;
  }
}
