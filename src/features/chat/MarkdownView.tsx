/**
 * Markdown rendering for chat bubbles (chat_rich_bind spec 5.1).
 *
 * Pipeline: `marked.parse` → `sanitizeMarkdownHtml` (allowlist sanitizer,
 * scripts / handlers / JS URLs stripped) → `dangerouslySetInnerHTML`.
 * Degradation ladder (spec 4.2-1, no white-screen ever):
 * - parse/sanitize throws → inline try/catch falls back to the raw text
 * - render throws anyway → ErrorBoundary falls back to the raw text
 * Both log only the text length (never the payload, spec 4.4-1).
 *
 * `<a>` clicks are intercepted at the container (delegation) and routed to
 * the system browser via `openExternalUrl` — structural safety (sanitizer)
 * plus behavioral safety (no in-app navigation).
 */
import { Component, memo, useMemo, type MouseEvent, type ReactNode } from "react";
import { marked } from "marked";
import { sanitizeMarkdownHtml } from "../../lib/editor-ext/builtin/sanitize";
import { openExternalUrl } from "../../lib/openUrl";

marked.setOptions({ gfm: true, breaks: true });

export interface MarkdownViewProps {
  /** Raw message text — also the source for copy actions (原文保留). */
  text: string;
  /** True while tokens are still streaming into this bubble. */
  streaming?: boolean;
}

interface BoundaryProps {
  text: string;
  children: ReactNode;
}

interface BoundaryState {
  failed: boolean;
}

class MarkdownErrorBoundary extends Component<BoundaryProps, BoundaryState> {
  state: BoundaryState = { failed: false };

  static getDerivedStateFromError(): BoundaryState {
    return { failed: true };
  }

  componentDidCatch(error: unknown): void {
    const { text } = this.props;
    // Degradation log: length only, never the raw text (spec 4.4-1).
    console.warn(
      `[markdown] render failed (len=${text.length}): ${error instanceof Error ? error.message : String(error)}`,
    );
  }

  render(): ReactNode {
    if (this.state.failed) {
      return <p className="whitespace-pre-wrap break-words">{this.props.text}</p>;
    }
    return this.props.children;
  }
}

const BUBBLE_MD_STYLES =
  "markdown-view break-words [&_a]:text-ink-accent [&_a]:underline [&_blockquote]:border-l-4 [&_blockquote]:border-ink-muted/40 [&_blockquote]:pl-3 [&_blockquote]:text-ink-muted [&_code]:rounded [&_code]:bg-surface-overlay [&_code]:px-1 [&_code]:font-mono [&_h1]:mb-1 [&_h1]:mt-2 [&_h1]:text-base [&_h1]:font-semibold [&_h2]:mb-1 [&_h2]:mt-2 [&_h2]:font-semibold [&_h3]:mb-1 [&_h3]:mt-1.5 [&_h3]:font-semibold [&_pre]:overflow-x-auto [&_pre]:rounded [&_pre]:bg-surface-overlay [&_pre]:p-2 [&_table]:border-collapse [&_td]:border [&_th]:border [&_td]:border-ink-muted/40 [&_th]:border-ink-muted/40 [&_td]:px-1.5 [&_th]:px-1.5 [&_li]:ml-4 [&_ul]:list-disc [&_ol]:list-decimal";

function MarkdownViewInner({ text, streaming = false }: MarkdownViewProps): ReactNode {
  const html = useMemo<string | null>(() => {
    try {
      const raw = marked.parse(text, { async: false });
      return sanitizeMarkdownHtml(raw);
    } catch (error) {
      // Degradation log: length only, never the raw text (spec 4.4-1).
      console.warn(
        `[markdown] parse failed (len=${text.length}): ${error instanceof Error ? error.message : String(error)}`,
      );
      return null;
    }
  }, [text]);

  const onClick = (event: MouseEvent<HTMLDivElement>): void => {
    const target = event.target as HTMLElement;
    const anchor = typeof target.closest === "function" ? target.closest("a") : null;
    if (anchor instanceof HTMLAnchorElement && event.currentTarget.contains(anchor)) {
      event.preventDefault();
      void openExternalUrl(anchor.getAttribute("href") ?? "");
    }
  };

  if (html === null) {
    return (
      <p className="whitespace-pre-wrap break-words">
        {text}
        {streaming && (
          <span aria-hidden="true" className="ml-0.5 inline-block h-3 w-1.5 animate-pulse bg-ink-muted align-middle" />
        )}
      </p>
    );
  }

  return (
    <>
      <div
        data-testid="markdown-view"
        onClick={onClick}
        // Content is sanitized above (scripts/handlers/JS URLs removed).
        dangerouslySetInnerHTML={{ __html: html }}
        className={BUBBLE_MD_STYLES}
      />
      {streaming && (
        <span aria-hidden="true" className="ml-0.5 inline-block h-3 w-1.5 animate-pulse bg-ink-muted align-middle" />
      )}
    </>
  );
}

/** Memoized: identical `text`/`streaming` props skip re-rendering — during
 * streaming only the active bubble re-renders (spec 5.1.1-2b). */
export const MarkdownView = memo(function MarkdownView(props: MarkdownViewProps) {
  return (
    <MarkdownErrorBoundary text={props.text}>
      <MarkdownViewInner {...props} />
    </MarkdownErrorBoundary>
  );
});
