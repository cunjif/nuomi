import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { HandoffChain as ChainModel } from "./traceModel";

interface HandoffChainViewProps {
  chain: ChainModel;
}

/** Visualizes control transfer A→B→…; cycle detection renders a red alarm. */
export function HandoffChainView({ chain }: HandoffChainViewProps): ReactNode {
  const { t } = useTranslation();
  return (
    <section aria-label={t("trace.handoffHeading")} className="border-b border-ink-muted/30 p-3">
      <h3 className="mb-1 text-xs font-semibold uppercase tracking-wide text-ink-muted">
        {t("trace.handoffHeading")}
      </h3>
      {chain.cycle && (
        <p role="alert" className="mb-1 rounded border border-state-danger px-2 py-1 text-xs text-state-danger">
          ⚠ {t("trace.cycleWarning")}
        </p>
      )}
      {chain.edges.length === 0 ? (
        <p className="text-xs text-ink-muted">{t("trace.noHandoff")}</p>
      ) : (
        <ol className="flex flex-wrap items-center gap-2 font-mono text-xs" aria-label={t("trace.handoffHeading")}>
          {chain.edges.map((edge, i) => (
            <li key={`${edge.from}-${edge.to}-${i}`} className="flex items-center gap-1">
              <span className="text-ink-accent">{edge.from}</span>
              <span aria-hidden="true" className="text-ink-muted">
                →
              </span>
              <span>{edge.to}</span>
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}
