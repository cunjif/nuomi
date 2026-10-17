import type { ReactNode } from "react";
import type { StewardRunStatusStateDto } from "../../lib/ipc/bindings.gen";

export interface RunStatusViewProps {
  state: StewardRunStatusStateDto;
  label: string;
}

export function RunStatusView({ state, label }: RunStatusViewProps): ReactNode {
  const colorClass: string = state === "running"
    ? "text-state-warn"
    : state === "awaiting_approval"
    ? "text-state-ok"
    : state === "failed"
    ? "text-state-danger"
    : "text-ink-muted";

  const dotClass: string = state === "running"
    ? "bg-state-warn"
    : state === "awaiting_approval"
    ? "bg-state-ok"
    : state === "failed"
    ? "bg-state-danger"
    : "bg-ink-muted";

  const labelText: string = state === "idle"
    ? "空闲"
    : state === "running"
    ? "运行中"
    : state === "awaiting_approval"
    ? "等待审批"
    : "已失败";

  return (
    <div className="flex items-center gap-2 text-sm">
      <span className={`inline-block h-2 w-2 rounded-full ${dotClass}`} />
      <span className={colorClass}>{labelText}</span>
      <span className="text-xs text-ink-muted">({label})</span>
    </div>
  );
}
