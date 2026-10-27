import { ipc } from "../../lib/ipc/client";

export type TerminalCwdResolution = {
  cwd: string;
  source: "workspace" | "home";
  fallbackReason?: string;
};

/**
 * Resolves the cwd for a new terminal: prefers the open workspace root,
 * falls back to the user home dir when no workspace is open or the
 * workspace query fails. Sensitive-dir rejection is delegated to the
 * backend `terminal_create` command.
 */
export async function resolveTerminalCwd(): Promise<TerminalCwdResolution> {
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
