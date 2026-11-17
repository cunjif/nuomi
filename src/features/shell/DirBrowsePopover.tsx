import { useEffect, useRef, type RefObject, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useOpenWorkspace } from "../../lib/store/workspaceQueries";
import { FileTree } from "./FileTree";
import type { WorkDirectoryDto } from "../../lib/ipc/bindings.gen";

interface DirBrowsePopoverProps {
  workspaceId: string;
  /** The directory whose button was pressed (fresh from the parent render). */
  dir: WorkDirectoryDto;
  /** The trigger button — clicking it again must toggle, not re-open. */
  buttonRef: RefObject<HTMLButtonElement | null>;
  onClose: () => void;
}

/**
 * Local browse popover anchored next to a dock directory badge: shows the
 * full path on top and the complete FileTree (context menu / rename / delete
 * / run — same widget as the expanded sidebar) below. Clicking the badge
 * already focuses the directory, and the FileTree root lists the ACTIVE work
 * directory, so the tree renders only after focus lands on this one — a
 * short loading state hides the stale-directory flash during the async
 * focus IPC + invalidation.
 *
 * Owns its outside-click/Escape handling instead of useClickOutside: the
 * FileTree's ContextMenu and DeleteConfirmDialog escape this subtree
 * (ContextMenu portals to body, dialogs are subtree-internal but must keep
 * the popover open while they handle Escape themselves), so any mousedown /
 * Escape landing inside a [role="menu"] or [role="dialog"] must NOT close
 * this popover — otherwise the menu unmounts before its click fires.
 */
export function DirBrowsePopover({ workspaceId, dir, buttonRef, onClose }: DirBrowsePopoverProps): ReactNode {
  const { t } = useTranslation();
  const panelRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onDown = (e: MouseEvent): void => {
      const target = e.target as Node | null;
      if (target === null) return;
      // Inside the popover panel, the trigger button, or any open portal
      // layer (context menu / confirm dialog) keeps the popover open.
      const inLayer =
        (panelRef.current !== null && panelRef.current.contains(target)) ||
        (buttonRef.current !== null && buttonRef.current.contains(target)) ||
        target.parentElement?.closest('[role="menu"], [role="dialog"]') !== null;
      if (!inLayer) onClose();
    };
    const onKey = (e: KeyboardEvent): void => {
      if (e.key !== "Escape") return;
      // Inner layers handle their own Escape (menu first, then dialog);
      // the popover closes only when none is open.
      if (document.querySelector('[role="menu"], [role="dialog"]') !== null) return;
      onClose();
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  });

  // Live check (not the captured prop): the focus IPC lands asynchronously,
  // and the workspace query refetch flips isActive afterwards.
  const openWs = useOpenWorkspace().data ?? null;
  const liveDir = (openWs?.directories ?? []).find((d) => d.id === dir.id);
  const focused = liveDir?.isActive ?? false;

  return (
    <div
      ref={panelRef}
      data-testid="dock-browse-popover"
      className="absolute left-full top-0 z-30 ml-1 flex h-80 w-72 flex-col overflow-hidden rounded border border-ink-muted/40 bg-surface-raised text-xs shadow-lg"
    >
      <div className="shrink-0 border-b border-ink-muted/30 px-2 py-1.5">
        <p className="truncate font-semibold text-ink">{dir.displayName}</p>
        <p className="truncate font-mono text-ink-muted" title={dir.absPath}>
          {dir.absPath}
        </p>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-1.5">
        {focused ? (
          <FileTree workspaceId={workspaceId} />
        ) : (
          <p className="px-1 py-2 text-ink-muted">{t("workspace.dirBrowseLoading")}</p>
        )}
      </div>
    </div>
  );
}
