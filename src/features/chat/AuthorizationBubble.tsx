/**
 * In-flow authorization bubble (chat_rich_bind spec 5.4.1-4a/8a, 5.4.3):
 * pending requests present the three options (grant once / grant for
 * session / deny); terminal rows render their outcome; a still-pending row
 * past its deadline shows the timeout reminder (derived UI state). `now`
 * is a controlled clock so tests freeze time deterministically.
 */
import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { i18n } from "../../i18n";
import type { AuthorizationRequest } from "./useAuthorizationStream";

export type AuthorizationDecision = "grant_once" | "grant_session" | "deny";

export interface AuthorizationBubbleProps {
  request: AuthorizationRequest;
  onResolve?: (decision: AuthorizationDecision) => void;
  /** Controlled clock base for the timeout-reminder derivation. */
  now?: number;
}

function formatTime(ms: number): string {
  return new Date(ms).toLocaleTimeString();
}

export function AuthorizationBubble({
  request,
  onResolve,
  now,
}: AuthorizationBubbleProps): ReactNode {
  const { t } = useTranslation();
  const [submitting, setSubmitting] = useState(false);
  const isPending = request.state === "pending";
  const overdue =
    isPending && (now ?? Date.now()) > request.expiresAt;

  const submit = (decision: AuthorizationDecision): void => {
    setSubmitting(true);
    void ipc
      .resolveAuthorization({ authorizationId: request.authorizationId, decision })
      .then(() => {
        toast.success(
          decision === "deny"
            ? t("chat.authorization.denied")
            : t("chat.authorization.grantedOnce"),
        );
        onResolve?.(decision);
      })
      .catch(() => {
        toast.error(i18n.t("chat.authorization.resolveFailed"));
      })
      .finally(() => setSubmitting(false));
  };

  const terminalLabel = (): string | null => {
    switch (request.state) {
      case "granted_once":
        return t("chat.authorization.grantedOnce");
      case "granted_session":
        return t("chat.authorization.grantedSession");
      case "denied":
        return t("chat.authorization.denied");
      case "timeout_default":
        return t("chat.authorization.timeoutDefault");
      case "voided":
        return t("chat.authorization.voided");
      default:
        return null;
    }
  };

  const terminal = terminalLabel();

  return (
    <div
      data-testid="authorization-bubble"
      data-state={request.state}
      className="mx-3 my-1 rounded-lg border border-ink-muted/40 bg-surface px-3 py-2 text-sm text-ink"
    >
      <div className="flex items-center gap-1.5 font-medium">
        <svg viewBox="0 0 16 16" className="h-3.5 w-3.5 fill-current text-ink-accent" aria-hidden="true">
          <path d="M8 1a3 3 0 0 0-3 3v2H4v9h8V6h-1V4a3 3 0 0 0-3-3zm1 8v2H7V9h2zM8 3a1 1 0 0 1 1 1v2H7V4a1 1 0 0 1 1-1z" />
        </svg>
        {t("chat.authorization.title")}
      </div>
      <dl className="mt-1 grid grid-cols-[max-content_1fr] gap-x-2 gap-y-0.5 text-xs">
        <dt className="text-ink-muted">{t("chat.authorization.requestedBy")}</dt>
        <dd>{request.roleDisplayName}</dd>
        <dt className="text-ink-muted">{t("chat.authorization.operation")}</dt>
        <dd>
          {request.op === "delete"
            ? t("chat.authorization.delete")
            : t("chat.authorization.write")}
        </dd>
        <dt className="text-ink-muted">{t("chat.authorization.target")}</dt>
        <dd className="truncate font-mono" title={request.targetPath}>
          {request.targetPath}
        </dd>
        {isPending && (
          <>
            <dt className="text-ink-muted">{t("chat.authorization.deadline")}</dt>
            <dd>{formatTime(request.expiresAt)}</dd>
          </>
        )}
      </dl>
      {overdue && (
        <p data-testid="authorization-timeout-reminder" className="mt-1 text-xs text-ink-accent">
          {t("chat.authorization.timeoutReminder")}
        </p>
      )}
      {isPending ? (
        <div className="mt-2 flex flex-wrap gap-1.5">
          <button
            type="button"
            disabled={submitting}
            onClick={() => submit("grant_once")}
            className="rounded bg-surface-overlay px-2.5 py-1 text-xs text-ink-accent transition-colors hover:bg-surface-raised disabled:opacity-40"
          >
            {t("chat.authorization.grantOnce")}
          </button>
          <button
            type="button"
            disabled={submitting}
            onClick={() => submit("grant_session")}
            className="rounded bg-surface-overlay px-2.5 py-1 text-xs text-ink-accent transition-colors hover:bg-surface-raised disabled:opacity-40"
          >
            {t("chat.authorization.grantSession")}
          </button>
          <button
            type="button"
            disabled={submitting}
            onClick={() => submit("deny")}
            className="rounded bg-surface-overlay px-2.5 py-1 text-xs text-ink-muted transition-colors hover:bg-surface-raised disabled:opacity-40"
          >
            {t("chat.authorization.deny")}
          </button>
        </div>
      ) : (
        terminal !== null && (
          <p className="mt-1.5 text-xs text-ink-muted" data-testid="authorization-terminal">
            {terminal}
          </p>
        )
      )}
    </div>
  );
}
