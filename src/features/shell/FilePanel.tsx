import type { ReactNode } from "react";
import { FileTabs } from "./FileTabs";
import { FileTree } from "./FileTree";
import { MonacoTab } from "./MonacoTab";
import { useUiStore } from "../../lib/store/uiStore";

/**
 * U10 right pane: workspace tree + open-file tabs + lazy Monaco editor.
 * Widens once a tab is open so the editor stays usable.
 */
export function FilePanel(): ReactNode {
  const openFiles = useUiStore((s) => s.openFiles);
  const activeFile = useUiStore((s) => s.activeFile);
  return (
    <aside
      className={`flex min-h-0 shrink-0 flex-col border-l border-ink-muted/30 bg-surface-raised transition-[width] ${
        openFiles.length > 0 ? "w-[42rem]" : "w-60"
      }`}
    >
      {openFiles.length > 0 && <FileTabs />}
      <div className="flex min-h-0 flex-1">
        <FileTree />
        {activeFile !== null && openFiles.includes(activeFile) && <MonacoTab key={activeFile} path={activeFile} />}
      </div>
    </aside>
  );
}
