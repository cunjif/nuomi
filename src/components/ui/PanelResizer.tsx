import { useRef, type MouseEvent as ReactMouseEvent, type ReactNode } from "react";

export interface PanelResizerProps {
  /** Which side the resizable panel sits on: "left" panel → handle on its
   * right edge (drag right = wider); "right" panel → handle on its left edge
   * (drag left = wider). */
  side: "left" | "right";
  width: number;
  minWidth: number;
  maxWidth: number;
  onWidthChange: (width: number) => void;
  /** Collapse the panel to its icon dock. Lives on a small chevron button
   * centered on the handle (click ≠ drag). */
  onCollapse: () => void;
  /** Double-click resets the width to the panel default. */
  onReset: () => void;
  resizeLabel: string;
  collapseLabel: string;
}

/**
 * Edge-mounted resize handle for the dockable side panels. Renders as an
 * absolutely-positioned hot zone over the panel border (the host panel must
 * be `relative`): drag to resize, double-click to reset, click the chevron
 * to collapse to the icon dock.
 */
export function PanelResizer({
  side,
  width,
  minWidth,
  maxWidth,
  onWidthChange,
  onCollapse,
  onReset,
  resizeLabel,
  collapseLabel,
}: PanelResizerProps): ReactNode {
  const dragging = useRef(false);

  const handleMouseDown = (e: ReactMouseEvent<HTMLDivElement>): void => {
    if (e.button !== 0) return;
    e.preventDefault();
    const start = { x: e.clientX, width };
    dragging.current = true;
    const onMouseMove = (ev: globalThis.MouseEvent): void => {
      const delta = side === "left" ? ev.clientX - start.x : start.x - ev.clientX;
      onWidthChange(Math.min(maxWidth, Math.max(minWidth, start.width + delta)));
    };
    const onMouseUp = (): void => {
      dragging.current = false;
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
    };
    window.addEventListener("mousemove", onMouseMove);
    window.addEventListener("mouseup", onMouseUp);
  };

  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={resizeLabel}
      data-testid={`panel-resizer-${side}`}
      className={`group absolute inset-y-0 z-20 flex w-2 cursor-col-resize justify-center ${
        side === "left" ? "right-0" : "left-0"
      }`}
      onMouseDown={handleMouseDown}
      onDoubleClick={onReset}
    >
      <div className="w-px bg-transparent transition-colors group-hover:bg-ink-accent/60" />
      <button
        type="button"
        aria-label={collapseLabel}
        title={collapseLabel}
        data-testid={`panel-collapse-${side}`}
        onMouseDown={(e) => e.stopPropagation()}
        onDoubleClick={(e) => e.stopPropagation()}
        onClick={onCollapse}
        className="absolute top-1/2 flex h-10 w-3 -translate-y-1/2 items-center justify-center rounded border border-ink-muted/40 bg-surface-raised text-[10px] text-ink-muted/70 transition-colors hover:text-ink-accent focus-visible:text-ink-accent"
      >
        {side === "left" ? "◂" : "▸"}
      </button>
    </div>
  );
}
