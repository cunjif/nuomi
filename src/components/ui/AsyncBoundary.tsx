import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Spinner } from "./Spinner";

interface AsyncBoundaryProps {
  isLoading: boolean;
  error: unknown;
  isEmpty: boolean;
  /** Overrides the generic empty message. */
  emptyLabel?: string;
  /** Called by the retry button after an error. */
  onRetry?: () => void;
  children: ReactNode;
}

/**
 * The async-surface triad (loading / empty / error) in one place so every
 * IPC-backed region ships all three states in the same change.
 */
export function AsyncBoundary({ isLoading, error, isEmpty, emptyLabel, onRetry, children }: AsyncBoundaryProps): ReactNode {
  const { t } = useTranslation();
  if (isLoading) {
    return (
      <div className="flex h-full items-center justify-center p-6 text-ink-muted">
        <Spinner />
      </div>
    );
  }
  if (error !== null && error !== undefined) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 p-6">
        <p className="text-state-danger">{t("common.loadFailed")}</p>
        <ErrorDetail error={error} />
        {onRetry !== undefined && (
          <button
            type="button"
            onClick={onRetry}
            className="rounded border border-ink-muted px-3 py-1 text-sm text-ink hover:bg-surface-raised focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            {t("common.retry")}
          </button>
        )}
      </div>
    );
  }
  if (isEmpty) {
    return (
      <div className="flex h-full items-center justify-center p-6 text-sm text-ink-muted">
        {emptyLabel ?? t("common.empty")}
      </div>
    );
  }
  return <>{children}</>;
}

function ErrorDetail({ error }: { error: unknown }): ReactNode {
  const message = error instanceof Error ? error.message : String(error);
  return <p className="max-w-md truncate font-mono text-xs text-ink-muted">{message}</p>;
}
