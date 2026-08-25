import type { ReactNode } from "react";

interface ToolCardProps {
  label: string;
  tool: string;
  body: string;
}

/** Collapsed-by-default tool_call / tool_result card (semantic details). */
export function ToolCard({ label, tool, body }: ToolCardProps): ReactNode {
  return (
    <details className="mx-3 my-1 max-w-[80%] rounded border border-ink-muted/40 bg-surface-raised text-sm">
      <summary className="cursor-pointer px-3 py-1.5 text-xs text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent">
        {label}
        {tool !== "" && <span className="ml-2 font-mono text-ink-accent">{tool}</span>}
      </summary>
      <pre className="overflow-x-auto px-3 pb-2 font-mono text-xs text-ink-muted">{body}</pre>
    </details>
  );
}
