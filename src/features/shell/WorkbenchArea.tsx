import type { ReactNode } from "react";
import { EditorArea } from "./EditorArea";

/**
 * The workbench area: the per-workspace file editor (file tree + Monaco).
 * Rendered by Shell when `activeArea === "workbench"`. Each workspace gets
 * its own editor bucket (ADR 0017) — in split layout two WorkbenchArea
 * instances live side-by-side, each scoped to its `workspaceId`.
 */
export function WorkbenchArea({ workspaceId, paneIndex }: { workspaceId: string; paneIndex?: number }): ReactNode {
  return <EditorArea workspaceId={workspaceId} paneIndex={paneIndex} />;
}
