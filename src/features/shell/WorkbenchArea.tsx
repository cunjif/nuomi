import type { ReactNode } from "react";
import { EditorArea } from "./EditorArea";

/**
 * The workbench area: the single-workspace file editor (file tree + Monaco).
 * Rendered by Shell when `activeArea === "workbench"`.
 */
export function WorkbenchArea(): ReactNode {
  return <EditorArea />;
}
