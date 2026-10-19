import type { WorkspaceEntryDto } from "../../lib/ipc/bindings.gen";

/**
 * What the workspace path bar shows for a workspace (ADR-0021 compat read):
 * a workspace with exactly one available directory shows that directory's
 * absolute path (the bar's original "current path" semantics); multi-
 * directory, stale or anomalous workspaces fall back to the derived
 * `labelFull` (the rootPath column is a uuid placeholder for v2 rows).
 */
export function workspaceDisplayPath(ws: WorkspaceEntryDto): string | null {
  const available = (ws.directories ?? []).filter((d) => d.available);
  const [single] = available;
  if (available.length === 1 && single !== undefined) {
    return single.absPath;
  }
  return ws.labelFull ?? ws.rootPath;
}
