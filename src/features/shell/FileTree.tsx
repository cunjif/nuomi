import type { ReactNode } from "react";
import {
  createContext,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";
import { describeError } from "../../i18n";
import { useUiStore } from "../../lib/store/uiStore";
import { ContextMenu, type ContextMenuItem } from "../../components/ui/ContextMenu";
import { DeleteConfirmDialog } from "./DeleteConfirmDialog";
import {
  batchDelete,
  batchCopyInto,
  batchMoveInto,
  joinPath,
  parentOf,
  useCreateFile,
  useCreateDir,
  useRename,
} from "./fileOps";

const ILLEGAL_NAME_CHARS = /[/\\:*?"<>|]/;

interface NodeContext {
  selectedPaths: string[];
  renaming: string | null;
  creating: { parent: string; isDir: boolean } | null;
  onNodeClick: (e: React.MouseEvent, path: string, isDir: boolean) => void;
  onNodeContextMenu: (e: React.MouseEvent, path: string, isDir: boolean) => void;
  onRenameSubmit: (from: string, to: string) => void;
  onRenameCancel: () => void;
  onCreateSubmit: (parent: string, name: string, isDir: boolean) => void;
  onCreateCancel: () => void;
}

const NodeCtx = createContext<NodeContext | null>(null);

function useNodeCtx(): NodeContext {
  const ctx = useContext(NodeCtx);
  if (!ctx) throw new Error("FileTree node used outside FileTree");
  return ctx;
}

export function FileTree(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const rootQuery = useQuery({ queryKey: ["dir", ""], queryFn: () => ipc.listDir("") });

  const selectedPaths = useUiStore((s) => s.selectedPaths);
  const lastSelectedPath = useUiStore((s) => s.lastSelectedPath);
  const setSelectedPaths = useUiStore((s) => s.setSelectedPaths);
  const toggleSelected = useUiStore((s) => s.toggleSelected);
  const selectRange = useUiStore((s) => s.selectRange);
  const clearSelection = useUiStore((s) => s.clearSelection);
  const clipboardPaths = useUiStore((s) => s.clipboardPaths);
  const clipboardMode = useUiStore((s) => s.clipboardMode);
  const setClipboard = useUiStore((s) => s.setClipboard);
  const clearClipboard = useUiStore((s) => s.clearClipboard);
  const openFile = useUiStore((s) => s.openFile);

  const createFileMut = useCreateFile();
  const createDirMut = useCreateDir();
  const renameMut = useRename();

  const [menu, setMenu] = useState<{ x: number; y: number; path: string; isDir: boolean } | null>(null);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [creating, setCreating] = useState<{ parent: string; isDir: boolean } | null>(null);
  const [error, setError] = useState<string | null>(null);

  function getVisiblePaths(): string[] {
    const el = document.querySelector("[data-filetree-root]");
    if (!el) return [];
    return Array.from(el.querySelectorAll<HTMLElement>("[data-path]"))
      .map((n) => n.dataset.path ?? "")
      .filter(Boolean);
  }

  function handleNodeClick(e: React.MouseEvent, path: string, isDir: boolean): void {
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      toggleSelected(path);
      return;
    }
    if (e.shiftKey && lastSelectedPath) {
      e.preventDefault();
      selectRange(lastSelectedPath, path, getVisiblePaths());
      return;
    }
    setSelectedPaths([path]);
    if (!isDir) openFile(path);
  }

  function handleNodeContextMenu(e: React.MouseEvent, path: string, isDir: boolean): void {
    e.preventDefault();
    if (!selectedPaths.includes(path)) setSelectedPaths([path]);
    setMenu({ x: e.clientX, y: e.clientY, path, isDir });
  }

  function onRenameSubmit(from: string, to: string): void {
    if (from === to) {
      setRenaming(null);
      return;
    }
    renameMut.mutate(
      { from, to },
      {
        onSuccess: () => setRenaming(null),
        onError: (err) => {
          setError(describeError(err));
          setRenaming(null);
        },
      },
    );
  }

  function onCreateSubmit(parent: string, name: string, isDir: boolean): void {
    if (!name || ILLEGAL_NAME_CHARS.test(name)) {
      setError(t("files.invalidName"));
      return;
    }
    const path = joinPath(parent, name);
    if (isDir) {
      createDirMut.mutate(path, {
        onSuccess: () => setCreating(null),
        onError: (err) => {
          setError(describeError(err));
          setCreating(null);
        },
      });
    } else {
      createFileMut.mutate(
        { path },
        {
          onSuccess: () => setCreating(null),
          onError: (err) => {
            setError(describeError(err));
            setCreating(null);
          },
        },
      );
    }
  }

  const ctxValue: NodeContext = {
    selectedPaths,
    renaming,
    creating,
    onNodeClick: handleNodeClick,
    onNodeContextMenu: handleNodeContextMenu,
    onRenameSubmit,
    onRenameCancel: () => setRenaming(null),
    onCreateSubmit,
    onCreateCancel: () => setCreating(null),
  };

  const menuPath = menu?.path ?? "";
  const menuIsDir = menu?.isDir ?? true;
  const menuParent = menuPath === "" ? "" : parentOf(menuPath);
  const targetDir = selectedPaths.length === 1 ? (menuIsDir ? menuPath : menuParent) : "";
  const singleSelected = selectedPaths.length === 1 ? selectedPaths[0] : null;

  function buildMenuItems(): ContextMenuItem[] {
    const items: ContextMenuItem[] = [];
    const canCreate = singleSelected !== null;
    const canPaste = singleSelected !== null && clipboardPaths.length > 0;
    items.push({
      type: "item",
      id: "newFile",
      label: t("files.newFile"),
      icon: "plus",
      disabled: !canCreate,
      onSelect: () => setCreating({ parent: targetDir, isDir: false }),
    });
    items.push({
      type: "item",
      id: "newFolder",
      label: t("files.newFolder"),
      icon: "plus",
      disabled: !canCreate,
      onSelect: () => setCreating({ parent: targetDir, isDir: true }),
    });
    items.push({ type: "separator" });
    items.push({
      type: "item",
      id: "rename",
      label: t("files.rename"),
      disabled: singleSelected === null,
      onSelect: () => singleSelected && setRenaming(singleSelected),
    });
    items.push({
      type: "item",
      id: "delete",
      label: t("files.delete"),
      icon: "trash",
      danger: true,
      disabled: selectedPaths.length === 0,
      onSelect: () => {
        if (selectedPaths.length > 0) setDeleteOpen(true);
      },
    });
    items.push({ type: "separator" });
    items.push({
      type: "item",
      id: "copy",
      label: t("files.copy"),
      icon: "copy",
      disabled: selectedPaths.length === 0,
      onSelect: () => setClipboard(selectedPaths, "copy"),
    });
    items.push({
      type: "item",
      id: "cut",
      label: t("files.cut"),
      disabled: selectedPaths.length === 0,
      onSelect: () => setClipboard(selectedPaths, "cut"),
    });
    items.push({
      type: "item",
      id: "paste",
      label: t("files.paste"),
      disabled: !canPaste,
      onSelect: async () => {
        if (!targetDir || clipboardPaths.length === 0) return;
        try {
          if (clipboardMode === "cut") {
            await batchMoveInto(clipboardPaths, targetDir);
            clearClipboard();
          } else {
            await batchCopyInto(clipboardPaths, targetDir);
          }
          qc.invalidateQueries({ queryKey: ["dir"] });
        } catch (err) {
          setError(describeError(err));
        }
      },
    });
    items.push({ type: "separator" });
    items.push({
      type: "item",
      id: "copyPath",
      label: t("files.copyPath"),
      icon: "copy",
      disabled: selectedPaths.length === 0,
      onSelect: () => {
        const text = selectedPaths.join("\n");
        void navigator.clipboard?.writeText(text).catch(() => {});
      },
    });
    return items;
  }

  async function confirmDelete(): Promise<void> {
    try {
      await batchDelete(selectedPaths);
      qc.invalidateQueries({ queryKey: ["dir"] });
      clearSelection();
      setDeleteOpen(false);
    } catch (err) {
      setError(describeError(err));
      setDeleteOpen(false);
    }
  }

  return (
    <NodeCtx.Provider value={ctxValue}>
      <div
        data-filetree-root="true"
        onContextMenu={(e) => {
          if (e.target === e.currentTarget) {
            e.preventDefault();
            setMenu({ x: e.clientX, y: e.clientY, path: "", isDir: true });
          }
        }}
      >
        {error && (
          <p className="mb-1 text-xs text-state-danger" onClick={() => setError(null)}>
            {t("files.opFailed")}: {error}
          </p>
        )}
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
        {creating && creating.parent === "" && (
          <InlineNameInput
            depth={0}
            isDir={creating.isDir}
            onCancel={() => setCreating(null)}
            onSubmit={(name) => onCreateSubmit("", name, creating.isDir)}
          />
        )}
        {!rootQuery.isLoading && (rootQuery.data?.length ?? 0) === 0 && !rootQuery.isError && (
          <p className="text-xs text-ink-muted">{t("files.dirEmpty")}</p>
        )}
      </div>
      <ContextMenu
        open={menu !== null}
        x={menu?.x ?? 0}
        y={menu?.y ?? 0}
        items={buildMenuItems()}
        onClose={() => setMenu(null)}
      />
      <DeleteConfirmDialog
        open={deleteOpen}
        paths={selectedPaths}
        onConfirm={() => void confirmDelete()}
        onCancel={() => setDeleteOpen(false)}
      />
    </NodeCtx.Provider>
  );
}

function DirNode({ parent, name, depth }: { parent: string; name: string; depth: number }): ReactNode {
  const ctx = useNodeCtx();
  const [open, setOpen] = useState(false);
  const path = joinPath(parent, name);
  const childrenQuery = useQuery({
    queryKey: ["dir", path],
    queryFn: () => ipc.listDir(path),
    enabled: open,
  });
  const selected = ctx.selectedPaths.includes(path);
  const isRenaming = ctx.renaming === path;

  return (
    <div>
      {isRenaming ? (
        <InlineRenameInput
          depth={depth}
          initialName={name}
          onCancel={ctx.onRenameCancel}
          onSubmit={(newName) => ctx.onRenameSubmit(path, joinPath(parent, newName))}
        />
      ) : (
        <button
          type="button"
          data-path={path}
          onClick={(e) => {
            ctx.onNodeClick(e, path, true);
            if (!e.ctrlKey && !e.metaKey && !e.shiftKey) setOpen((o) => !o);
          }}
          onContextMenu={(e) => ctx.onNodeContextMenu(e, path, true)}
          aria-expanded={open}
          className={`block w-full truncate rounded text-left text-sm focus-visible:ring-2 focus-visible:ring-ink-accent ${
            selected ? "bg-surface-overlay text-ink-accent" : "text-ink hover:bg-surface-overlay"
          }`}
          style={{ paddingLeft: depth * 12 + 8 }}
        >
          <span aria-hidden="true" className="mr-1 inline-block w-3 text-ink-muted">
            {open ? "▾" : "▸"}
          </span>
          {name}
        </button>
      )}
      {open && (
        <div>
          {childrenQuery.isError && (
            <p className="text-xs text-state-danger" style={{ paddingLeft: depth * 12 + 24 }}>
              {describeError(childrenQuery.error)}
            </p>
          )}
          {ctx.creating && ctx.creating.parent === path && (
            <InlineNameInput
              depth={depth + 1}
              isDir={ctx.creating.isDir}
              onCancel={ctx.onCreateCancel}
              onSubmit={(nm) => ctx.onCreateSubmit(path, nm, ctx.creating!.isDir)}
            />
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
  const ctx = useNodeCtx();
  const activeFile = useUiStore((s) => s.activeFile);
  const path = joinPath(parent, name);
  const selected = ctx.selectedPaths.includes(path);
  const isRenaming = ctx.renaming === path;
  const isActive = activeFile === path;

  if (isRenaming) {
    return (
      <InlineRenameInput
        depth={depth}
        initialName={name}
        onCancel={ctx.onRenameCancel}
        onSubmit={(newName) => ctx.onRenameSubmit(path, joinPath(parent, newName))}
      />
    );
  }

  return (
    <button
      type="button"
      data-path={path}
      onClick={(e) => ctx.onNodeClick(e, path, false)}
      onContextMenu={(e) => ctx.onNodeContextMenu(e, path, false)}
      aria-current={isActive ? "true" : undefined}
      className={`block w-full truncate rounded text-left text-sm focus-visible:ring-2 focus-visible:ring-ink-accent ${
        selected
          ? "bg-surface-overlay text-ink-accent"
          : isActive
            ? "bg-surface-overlay text-ink-accent"
            : "text-ink-muted hover:bg-surface-overlay"
      } ${isActive ? "border-l-2 border-ink-accent" : ""}`}
      style={{ paddingLeft: depth * 12 + 8 + (isActive ? 14 : 16) }}
    >
      {name}
    </button>
  );
}

function InlineNameInput({
  depth,
  isDir,
  onCancel,
  onSubmit,
}: {
  depth: number;
  isDir: boolean;
  onCancel: () => void;
  onSubmit: (name: string) => void;
}): ReactNode {
  const { t } = useTranslation();
  const [name, setName] = useState("");
  const [invalid, setInvalid] = useState(false);

  function commit(): void {
    if (!name.trim()) {
      onCancel();
      return;
    }
    if (ILLEGAL_NAME_CHARS.test(name)) {
      setInvalid(true);
      return;
    }
    onSubmit(name.trim());
  }

  return (
    <div className="flex items-center" style={{ paddingLeft: depth * 12 + 8 }}>
      <span aria-hidden="true" className="mr-1 inline-block w-3 text-ink-muted">
        {isDir ? "▸" : ""}
      </span>
      <input
        autoFocus
        type="text"
        value={name}
        onChange={(e) => {
          setName(e.target.value);
          setInvalid(false);
        }}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            commit();
          } else if (e.key === "Escape") {
            e.preventDefault();
            onCancel();
          }
        }}
        placeholder={isDir ? t("files.newFolder") : t("files.newFile")}
        className={`w-full rounded border bg-surface px-1 py-0.5 text-sm text-ink focus-visible:ring-2 focus-visible:ring-ink-accent ${
          invalid ? "border-state-danger" : "border-ink-muted/40"
        }`}
      />
    </div>
  );
}

function InlineRenameInput({
  depth,
  initialName,
  onCancel,
  onSubmit,
}: {
  depth: number;
  initialName: string;
  onCancel: () => void;
  onSubmit: (newName: string) => void;
}): ReactNode {
  const [name, setName] = useState(initialName);
  const [invalid, setInvalid] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (inputRef.current) {
      inputRef.current.focus();
      const dot = initialName.lastIndexOf(".");
      if (dot > 0) inputRef.current.setSelectionRange(0, dot);
      else inputRef.current.select();
    }
  }, [initialName]);

  function commit(): void {
    if (!name.trim() || name.trim() === initialName) {
      onCancel();
      return;
    }
    if (ILLEGAL_NAME_CHARS.test(name)) {
      setInvalid(true);
      return;
    }
    onSubmit(name.trim());
  }

  return (
    <div className="flex items-center" style={{ paddingLeft: depth * 12 + 8 }}>
      <input
        ref={inputRef}
        type="text"
        value={name}
        onChange={(e) => {
          setName(e.target.value);
          setInvalid(false);
        }}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            commit();
          } else if (e.key === "Escape") {
            e.preventDefault();
            onCancel();
          }
        }}
        className={`w-full rounded border bg-surface px-1 py-0.5 text-sm text-ink focus-visible:ring-2 focus-visible:ring-ink-accent ${
          invalid ? "border-state-danger" : "border-ink-muted/40"
        }`}
      />
    </div>
  );
}
