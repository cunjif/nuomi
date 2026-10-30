/**
 * Path classification for the temp-workspace drag handler
 * (workbench_enhance spec 5.2.1 / design 2.2.2). Pure helpers +
 * IPC-backed probes: file/directory type, outside-workspace check, and
 * the 10MB size cap (spec 6.2.3).
 */
import { ipc } from "../../../lib/ipc/client";

/** Max external file size accepted into the temp workspace (spec 6.2.3). */
export const MAX_TEMP_FILE_BYTES = 10 * 1024 * 1024;

/** Classify an absolute path as a file or directory via IPC probe. */
export async function classifyPath(absPath: string): Promise<"file" | "directory"> {
  try {
    await ipc.listDir(absPath);
    return "directory";
  } catch {
    return "file";
  }
}

/**
 * True when `absPath` does not reside under any of the given workspace
 * roots. Path containment is prefix-based with a separator boundary so
 * `C:\foo` does not contain `C:\foobar`.
 */
export function isOutsideWorkspaces(absPath: string, workspaceRoots: string[]): boolean {
  const normalized = absPath.replace(/[\\/]+$/, "");
  return workspaceRoots.every((root) => {
    const r = root.replace(/[\\/]+$/, "");
    if (normalized === r) return false;
    const prefix = r + (r.includes("/") ? "/" : "\\");
    return !normalized.startsWith(prefix);
  });
}

/** Check the external file size against the 10MB cap (spec 6.2.3). */
export async function checkFileSize(absPath: string): Promise<{ ok: boolean; size: number }> {
  try {
    const content = await ipc.readExternalFile(absPath);
    const size = content.length;
    return { ok: size <= MAX_TEMP_FILE_BYTES, size };
  } catch {
    return { ok: false, size: 0 };
  }
}
