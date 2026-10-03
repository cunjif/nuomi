import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";

/** Parent directory of a workspace-relative path ("" = root). */
export function parentOf(path: string): string {
  const idx = path.lastIndexOf("/");
  return idx === -1 ? "" : path.slice(0, idx);
}

/** Basename of a workspace-relative path. */
export function baseName(path: string): string {
  const idx = path.lastIndexOf("/");
  return idx === -1 ? path : path.slice(idx + 1);
}

/** Join a parent directory and a name into a workspace-relative path. */
export function joinPath(parent: string, name: string): string {
  return parent === "" ? name : `${parent}/${name}`;
}

/** Invalidate the directory listing cache for one dir (and root as fallback). */
function invalidateDir(qc: ReturnType<typeof useQueryClient>, dir: string): void {
  qc.invalidateQueries({ queryKey: ["dir", dir] });
  if (dir !== "") qc.invalidateQueries({ queryKey: ["dir", ""] });
}

/** Create a file (parents auto-created). */
export function useCreateFile() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ path, content = "" }: { path: string; content?: string }) =>
      ipc.createFile(path, content),
    onSuccess: (_data, { path }) => invalidateDir(qc, parentOf(path)),
  });
}

/** Create a directory (idempotent, nested). */
export function useCreateDir() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (path: string) => ipc.createDir(path),
    onSuccess: (_data, path) => invalidateDir(qc, parentOf(path)),
  });
}

/** Delete a single path (file or directory). */
export function useDelete() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (path: string) => ipc.deletePath(path),
    onSuccess: (_data, path) => invalidateDir(qc, parentOf(path)),
  });
}

/** Rename/move a single path. */
export function useRename() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ from, to }: { from: string; to: string }) =>
      ipc.renamePath(from, to),
    onSuccess: (_data, { from, to }) => {
      invalidateDir(qc, parentOf(from));
      invalidateDir(qc, parentOf(to));
    },
  });
}

/** Copy a single path. */
export function useCopy() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: ({ from, to }: { from: string; to: string }) =>
      ipc.copyPath(from, to),
    onSuccess: (_data, { to }) => invalidateDir(qc, parentOf(to)),
  });
}

/**
 * Batch delete: sequentially deletes each path (serial to avoid concurrent
 * fs ops on the same tree). Returns when all succeed; throws on first error.
 */
export async function batchDelete(paths: string[]): Promise<void> {
  for (const p of paths) {
    await ipc.deletePath(p);
  }
}

/**
 * Batch copy: sequentially copies each path into `destDir` (serial to avoid
 * name collisions during concurrent copy). Returns the list of copied paths.
 */
export async function batchCopyInto(
  sources: string[],
  destDir: string,
): Promise<string[]> {
  const copied: string[] = [];
  for (const src of sources) {
    const name = baseName(src);
    const dst = joinPath(destDir, name);
    await ipc.copyPath(src, dst);
    copied.push(dst);
  }
  return copied;
}

/**
 * Batch move (cut): sequentially renames each path into `destDir`.
 */
export async function batchMoveInto(
  sources: string[],
  destDir: string,
): Promise<string[]> {
  const moved: string[] = [];
  for (const src of sources) {
    const name = baseName(src);
    const dst = joinPath(destDir, name);
    await ipc.renamePath(src, dst);
    moved.push(dst);
  }
  return moved;
}
