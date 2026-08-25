import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";

/** Narrow top strip showing IPC connection health (probed via listSessions). */
export function TopBar(): ReactNode {
  const { t } = useTranslation();
  const probe = useQuery({ queryKey: ["sessions"], queryFn: ipc.listSessions, staleTime: 10_000 });
  const status = probe.isPending ? "checking" : probe.isError ? "disconnected" : "connected";
  const color =
    status === "connected" ? "bg-state-ok" : status === "disconnected" ? "bg-state-danger" : "bg-state-warn";
  return (
    <header className="flex h-8 shrink-0 items-center justify-between border-b border-ink-muted/30 bg-surface-raised px-3">
      <span className="text-sm font-semibold">{t("shell.appName")}</span>
      <span className="flex items-center gap-2 text-xs text-ink-muted" role="status">
        <span aria-hidden="true" className={`inline-block size-2 rounded-full ${color}`} />
        {t(`shell.${status}`)}
      </span>
    </header>
  );
}
