import type { ReactNode } from "react";
import { EditorArea } from "./EditorArea";

/**
 * The workbench area: the per-workspace file editor (file tree + Monaco).
 * Rendered by Shell when `activeArea === "workbench"`. The workspace gets
 * its own editor bucket (ADR 0017); split layout retired with ADR-0021.
 */
export function WorkbenchArea({ workspaceId }: { workspaceId: string }): ReactNode {
  return <EditorArea workspaceId={workspaceId} />;
}
