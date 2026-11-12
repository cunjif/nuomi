import { ipc } from "../../lib/ipc/client";

export type TerminalCwdResolution = {
  cwd: string;
  source: "workspace" | "home";
  fallbackReason?: string;
};

/**
 * Resolves the cwd for a new terminal (ADR-0024 B3: the terminal binds to the
 * open workspace and starts in its active work directory).
 *
 * Resolution order:
 * 1. the open workspace's active (focused) available work directory,
 * 2. any other available work directory of that workspace,
 * 3. the workspace's legacy root path,
 * 4. the user home dir.
 *
 * `workspaceId` scopes the lookup to the terminal area's workspace so a new
 * terminal never starts in a directory belonging to another workspace.
 */
export async function resolveTerminalCwd(
  workspaceId?: string,
): Promise<TerminalCwdResolution> {
  try {
    const open = await ipc.workspaceGetOpen();
    if (open !== null && (workspaceId === undefined || open.id === workspaceId)) {
      const dirs = open.directories ?? [];
      const mounted = dirs.filter((d) => d.mounted && d.available);
      const dir = mounted.find((d) => d.isActive) ?? mounted[0];
      if (dir !== undefined) return { cwd: dir.absPath, source: "workspace" };
      // Legacy fallback: a v1 workspace with no work-directory rows keeps its
      // real path in rootPath. The backend re-validates and falls back home.
      if (open.rootPath.trim() !== "") {
        return { cwd: open.rootPath, source: "workspace" };
      }
    }
  } catch {
    // fall through to the legacy probe below
  }
  try {
    const ws = await ipc.getWorkspace();
    if (ws.configured && ws.root.trim() !== "") {
      return { cwd: ws.root, source: "workspace" };
    }
  } catch {
    // fall through to home
  }
  const home = await ipc.resolveHomeDir();
  return { cwd: home, source: "home", fallbackReason: "workspace not available" };
}