import type { Ref, ReactNode } from "react";
import { Icon, type IconName } from "../../components/ui/Icon/Icon";

export interface DockIconButtonProps {
  icon: IconName;
  label: string;
  active?: boolean;
  onClick: () => void;
  /** Optional anchor ref (floating panels need the trigger excluded from
   * click-outside detection). */
  buttonRef?: Ref<HTMLButtonElement>;
}

/** Square icon button used inside the collapsed-panel docks; mirrors the
 * LeftRail nav item styling (hand-drawn rounded outline, active ring). */
export function DockIconButton({ icon, label, active = false, onClick, buttonRef }: DockIconButtonProps): ReactNode {
  return (
    <button
      ref={buttonRef}
      type="button"
      aria-label={label}
      title={label}
      aria-current={active ? "page" : undefined}
      onClick={onClick}
      className={`flex size-7 items-center justify-center rounded-[12px_255px_15px_225px/225px_15px_255px_12px] focus-visible:ring-2 focus-visible:ring-ink-accent ${
        active
          ? "bg-surface-overlay text-ink-accent ring-1 ring-inset ring-ink-muted/40"
          : "text-ink-muted hover:bg-surface-overlay hover:text-ink"
      }`}
    >
      <Icon name={icon} size={16} />
    </button>
  );
}
