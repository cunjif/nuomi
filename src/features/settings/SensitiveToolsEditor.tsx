import type { ReactNode } from "react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";

/** Tag editor for the sensitive-tool approval allowlist. */
export function SensitiveToolsEditor(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const query = useQuery({ queryKey: ["sensitiveTools"], queryFn: ipc.getSensitiveTools });
  const [draft, setDraft] = useState("");
  const [patterns, setPatterns] = useState<string[]>([]);

  // Seed local editable copy once the server value arrives (null = unset).
  useEffect(() => {
    if (query.data !== undefined && query.data !== null) setPatterns(query.data);
  }, [query.data]);

  const saveMut = useMutation({
    mutationFn: () => ipc.setSensitiveTools(patterns),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["sensitiveTools"] });
      toast.success(t("settings.sensitiveSaved"));
    },
    onError: (e) => toast.error(`${t("settings.sensitiveSaveFailed")}: ${describeError(e)}`),
  });

  return (
    <section aria-label={t("settings.sensitiveHeading")} className="mb-3 rounded border border-ink-muted/40 bg-surface-raised p-3">
      <h3 className="text-sm font-semibold text-ink">{t("settings.sensitiveHeading")}</h3>
      <p className="mb-2 text-xs text-ink-muted">{t("settings.sensitiveHint")}</p>
      <ul className="mb-2 flex flex-wrap gap-1">
        {patterns.map((pattern) => (
          <li key={pattern} className="flex items-center gap-1 rounded bg-surface-overlay px-2 py-0.5 font-mono text-xs text-ink">
            {pattern}
            <button
              type="button"
              aria-label={`${t("common.remove")} ${pattern}`}
              onClick={() => setPatterns((p) => p.filter((x) => x !== pattern))}
              className="text-ink-muted hover:text-state-danger focus-visible:ring-2 focus-visible:ring-ink-accent"
            >
              ×
            </button>
          </li>
        ))}
      </ul>
      <div className="flex items-center gap-2">
        <input
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          placeholder="write_file*"
          className="w-48 rounded border border-ink-muted/40 bg-surface px-2 py-1 font-mono text-xs text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
          onKeyDown={(e) => {
            if (e.key === "Enter" && draft.trim().length > 0) {
              e.preventDefault();
              setPatterns((p) => (p.includes(draft.trim()) ? p : [...p, draft.trim()]));
              setDraft("");
            }
          }}
          aria-label={t("settings.sensitiveHeading")}
        />
        <button
          type="button"
          onClick={() => {
            if (draft.trim().length === 0) return;
            setPatterns((p) => (p.includes(draft.trim()) ? p : [...p, draft.trim()]));
            setDraft("");
          }}
          className="rounded border border-ink-muted px-2 py-1 text-xs text-ink hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
        >
          {t("common.add")}
        </button>
        <button
          type="button"
          disabled={saveMut.isPending}
          onClick={() => saveMut.mutate()}
          className="rounded bg-ink-accent px-3 py-1 text-xs text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
        >
          {t("common.save")}
        </button>
      </div>
    </section>
  );
}
