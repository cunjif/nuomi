import type { ReactNode } from "react";

function shortenCwd(cwd: string): string {
  const parts = cwd.replace(/\\/g, "/").split("/").filter(Boolean);
  if (parts.length <= 2) return cwd;
  return "…/" + parts.slice(-2).join("/");
}

export function TerminalCwdBadge({
  cwd,
  cwdSource,
}: {
  cwd?: string;
  cwdSource?: "workspace" | "home" | "explicit";
}): ReactNode {
  if (cwd == null) return null;
  const label = shortenCwd(cwd);
  const sourceTag = cwdSource === "home" ? " ~" : "";
  return (
    <span title={cwd} className="ml-1 font-mono text-[10px] text-ink-muted">
      {label}
      {sourceTag}
    </span>
  );
}
