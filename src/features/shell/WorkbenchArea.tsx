import type { ReactNode } from "react";
import { useUiStore } from "../../lib/store/uiStore";
import { WorkbenchNav } from "./WorkbenchNav";
import { WorkspaceListPanel } from "./WorkspaceListPanel";
import { EditorArea } from "./EditorArea";
import { ConversationsList } from "../conversation/ConversationsList";

/**
 * The workbench area: a second-level nav (WorkbenchNav) plus the active
 * sub-panel. Rendered by Shell when `activeArea === "workbench"`.
 *
 * Sub-tabs:
 * - `workspaceList` → upper WorkspaceListPanel + lower ConversationsList
 * - `editor` → EditorArea (single-workspace file editor + file tree)
 */
export function WorkbenchArea(): ReactNode {
  const workbenchSubTab = useUiStore((s) => s.workbenchSubTab);

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <WorkbenchNav />
      <div className="min-h-0 flex-1 overflow-hidden">
        {workbenchSubTab === "editor" ? (
          <EditorArea />
        ) : (
          <div className="flex h-full flex-col overflow-hidden">
            <div className="min-h-0 shrink-0 overflow-hidden" style={{ flexBasis: "45%" }}>
              <WorkspaceListPanel />
            </div>
            <div className="min-h-0 flex-1 overflow-y-auto border-t border-ink-muted/30">
              <ConversationsList />
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
