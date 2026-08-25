import { Component, type ErrorInfo, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
}

/**
 * Route-level error boundary. Class component is the only React-supported
 * way to catch render errors (function-component equivalents do not exist);
 * everything else in this codebase stays function components.
 */
export class ErrorBoundary extends Component<Props, State> {
  override state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  override componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error("Unhandled render error", error, info.componentStack);
  }

  override render(): ReactNode {
    if (this.state.error === null) return this.props.children;
    return <Fallback error={this.state.error} />;
  }
}

function Fallback({ error }: { error: Error }): ReactNode {
  const { t } = useTranslation();
  return (
    <div role="alert" className="flex h-full flex-col items-center justify-center gap-3 p-6">
      <p className="text-state-danger">{t("common.renderError")}</p>
      <p className="max-w-md font-mono text-xs text-ink-muted">{error.message}</p>
    </div>
  );
}
