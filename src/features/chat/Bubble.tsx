import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ToolCard } from "./ToolCard";
import type { ChatEntry } from "./useSessionStream";

interface BubbleProps {
  entry: ChatEntry;
  /** True while tokens are still streaming into this bubble. */
  streaming?: boolean;
}

/** User bubbles right, assistant left; tool traffic renders as folded cards. */
export function Bubble({ entry, streaming = false }: BubbleProps): ReactNode {
  const { t } = useTranslation();
  if (entry.kind === "tool_call") {
    return <ToolCard label={t("chat.toolCall")} tool={entry.tool ?? ""} body={entry.argsJson ?? "{}"} />;
  }
  if (entry.kind === "tool_result") {
    return <ToolCard label={t("chat.toolResult")} tool="" body={entry.content ?? ""} />;
  }
  const isUser = entry.role === "user";
  const speaker = entry.roleName ?? entry.role ?? "assistant";
  return (
    <div className={`flex ${isUser ? "justify-end" : "justify-start"} px-3 py-1`}>
      <div
        className={`max-w-[80%] rounded-lg px-3 py-2 text-sm ${
          isUser ? "bg-ink-accent/20 text-ink" : "bg-surface-raised text-ink"
        }`}
      >
        {entry.roleName !== undefined && <p className="mb-0.5 text-xs font-semibold text-ink-accent">{speaker}</p>}
        <p className="whitespace-pre-wrap break-words">
          {entry.text || t("chat.emptyResponse")}
          {streaming && (
            <span aria-hidden="true" className="ml-0.5 inline-block h-3 w-1.5 animate-pulse bg-ink-muted align-middle" />
          )}
        </p>
      </div>
    </div>
  );
}
