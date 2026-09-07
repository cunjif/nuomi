import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";
import { describeError } from "../../i18n";
import { useUiStore } from "../../lib/store/uiStore";

/**
 * Workspace file tree contents (lazy: root listing + per-dir expansion).
 * Pure node list — chrome (header, width, border) and the scroll container
 * are owned by the Explorer sidebar in EditorArea (VSCode split: the tree
 * itself never owns scrolling).
 */
export function FileTree(): ReactNode {
  const { t } = useTranslation();
  const rootQuery = useQuery({ queryKey: ["dir", ""], queryFn: () => ipc.listDir("") });
  return (
    <>
      {rootQuery.isError && (
        <p className="text-xs text-state-danger">
          {t("files.loadFailedDir")}: {describeError(rootQuery.error)}
        </p>
      )}
      {rootQuery.isLoading && <p className="text-xs text-ink-muted">…</p>}
      {(rootQuery.data ?? []).map((entry) =>
        entry.isDir ? (
          <DirNode key={entry.name} parent="" name={entry.name} depth={0} />
        ) : (
          <LeafNode key={entry.name} parent="" name={entry.name} depth={0} />
        ),
      )}
      {!rootQuery.isLoading && (rootQuery.data?.length ?? 0) === 0 && !rootQuery.isError && (
        <p className="text-xs text-ink-muted">{t("files.dirEmpty")}</p>
      )}
    </>
  );
}

function DirNode({ parent, name, depth }: { parent: string; name: string; depth: number }): ReactNode {
  const [open, setOpen] = useState(false);
  const path = joinPath(parent, name);
  const childrenQuery = useQuery({
    queryKey: ["dir", path],
    queryFn: () => ipc.listDir(path),
    enabled: open,
  });
  return (
    <div>
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        className="block w-full truncate rounded text-left text-sm text-ink hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
        style={{ paddingLeft: depth * 12 + 8 }}
      >
        <span aria-hidden="true" className="mr-1 inline-block w-3 text-ink-muted">
          {open ? "▾" : "▸"}
        </span>
        {name}
      </button>
      {open && (
        <div>
          {childrenQuery.isError && (
            <p className="text-xs text-state-danger" style={{ paddingLeft: depth * 12 + 24 }}>
              {describeError(childrenQuery.error)}
            </p>
          )}
          {(childrenQuery.data ?? []).map((entry) =>
            entry.isDir ? (
              <DirNode key={entry.name} parent={path} name={entry.name} depth={depth + 1} />
            ) : (
              <LeafNode key={entry.name} parent={path} name={entry.name} depth={depth + 1} />
            ),
          )}
          {!childrenQuery.isLoading && (childrenQuery.data?.length ?? 0) === 0 && (
            <p className="text-xs text-ink-muted" style={{ paddingLeft: depth * 12 + 24 }}>
              —
            </p>
          )}
        </div>
      )}
    </div>
  );
}

function LeafNode({ parent, name, depth }: { parent: string; name: string; depth: number }): ReactNode {
  const openFile = useUiStore((s) => s.openFile);
  const activeFile = useUiStore((s) => s.activeFile);
  const path = joinPath(parent, name);
  return (
    <button
      type="button"
      onClick={() => openFile(path)}
      aria-current={activeFile === path ? "true" : undefined}
      className={`block w-full truncate rounded text-left text-sm focus-visible:ring-2 focus-visible:ring-ink-accent ${
        activeFile === path ? "bg-surface-overlay text-ink-accent" : "text-ink-muted hover:bg-surface-overlay"
      }`}
      style={{ paddingLeft: depth * 12 + 8 + 16 }}
    >
      {name}
    </button>
  );
}

function joinPath(parent: string, name: string): string {
  return parent === "" ? name : `${parent}/${name}`;
}
