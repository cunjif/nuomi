import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { MemoryEntryDto } from "../../lib/ipc/bindings.gen";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { describeError } from "../../i18n";
import { AsyncBoundary } from "../../components/ui/AsyncBoundary";

export function MemorySection(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();

  const listQuery = useQuery({
    queryKey: ["memory"],
    queryFn: () => ipc.listMemory(null, 100),
  });

  const [content, setContent] = useState("");
  const [tags, setTags] = useState("");
  const [kind, setKind] = useState("note");
  const [roleId, setRoleId] = useState("");
  const [userProfile, setUserProfile] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState<MemoryEntryDto[] | null>(null);
  const [confirmingId, setConfirmingId] = useState<string | null>(null);

  const rememberMut = useMutation({
    mutationFn: () =>
      ipc.rememberMemory(
        content,
        tags.split(",").map((s) => s.trim()).filter(Boolean),
        kind,
        userProfile,
        roleId || null,
        null,
      ),
    onSuccess: () => {
      setContent("");
      setTags("");
      setKind("note");
      setRoleId("");
      setUserProfile(false);
      void qc.invalidateQueries({ queryKey: ["memory"] });
      toast.success(t("settings.memory.remembered"));
    },
    onError: (e) => toast.error(`${t("settings.memory.rememberFailed")}: ${describeError(e)}`),
  });

  const recallMut = useMutation({
    mutationFn: () => ipc.recallMemory(searchQuery || null, null, null, 20),
    onSuccess: (results) => setSearchResults(results),
    onError: (e) => toast.error(`${t("settings.memory.recallFailed")}: ${describeError(e)}`),
  });

  const deleteMut = useMutation({
    mutationFn: (id: string) => ipc.deleteMemory(id),
    onSuccess: () => {
      setConfirmingId(null);
      void qc.invalidateQueries({ queryKey: ["memory"] });
      toast.success(t("settings.memory.deleted"));
    },
    onError: (e) => toast.error(`${t("settings.memory.deleteFailed")}: ${describeError(e)}`),
  });

  return (
    <section aria-label={t("settings.memory.heading")} className="flex flex-col gap-4">
      <h3 className="text-sm font-semibold text-ink">{t("settings.memory.heading")}</h3>

      <div className="rounded border border-ink-muted/30 p-3">
        <h4 className="mb-2 text-xs font-medium text-ink-muted">{t("settings.memory.rememberTitle")}</h4>
        <textarea
          value={content}
          onChange={(e) => setContent(e.target.value)}
          placeholder={t("settings.memory.contentPlaceholder")}
          rows={3}
          className="mb-2 w-full resize-none rounded border border-ink-muted/40 bg-surface-raised px-2 py-1 text-sm text-ink placeholder:text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent"
        />
        <div className="mb-2 flex flex-wrap gap-2">
          <input
            value={tags}
            onChange={(e) => setTags(e.target.value)}
            placeholder={t("settings.memory.tagsPlaceholder")}
            className="min-w-[120px] flex-1 rounded border border-ink-muted/40 bg-surface-raised px-2 py-1 text-sm text-ink placeholder:text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent"
          />
          <input
            value={kind}
            onChange={(e) => setKind(e.target.value)}
            placeholder="kind"
            className="w-24 rounded border border-ink-muted/40 bg-surface-raised px-2 py-1 text-sm text-ink placeholder:text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent"
          />
          <input
            value={roleId}
            onChange={(e) => setRoleId(e.target.value)}
            placeholder={t("settings.memory.roleIdPlaceholder")}
            className="w-32 rounded border border-ink-muted/40 bg-surface-raised px-2 py-1 text-sm text-ink placeholder:text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent"
          />
          <label className="flex items-center gap-1 text-xs text-ink-muted">
            <input type="checkbox" checked={userProfile} onChange={(e) => setUserProfile(e.target.checked)} />
            {t("settings.memory.userProfile")}
          </label>
        </div>
        <button
          type="button"
          disabled={content.trim().length === 0 || rememberMut.isPending}
          onClick={() => rememberMut.mutate()}
          className="pixel-fill-accent px-3 py-1 text-sm text-surface disabled:opacity-50"
        >
          {t("settings.memory.remember")}
        </button>
      </div>

      <div className="rounded border border-ink-muted/30 p-3">
        <h4 className="mb-2 text-xs font-medium text-ink-muted">{t("settings.memory.recallTitle")}</h4>
        <div className="flex gap-2">
          <input
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") recallMut.mutate();
            }}
            placeholder={t("settings.memory.searchPlaceholder")}
            className="flex-1 rounded border border-ink-muted/40 bg-surface-raised px-2 py-1 text-sm text-ink placeholder:text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent"
          />
          <button
            type="button"
            disabled={recallMut.isPending}
            onClick={() => recallMut.mutate()}
            className="pixel-fill-accent px-3 py-1 text-sm text-surface disabled:opacity-50"
          >
            {t("settings.memory.recall")}
          </button>
        </div>
        {searchResults !== null && (
          <ul className="mt-2 space-y-1">
            {searchResults.length === 0 && (
              <li className="text-xs text-ink-muted">{t("settings.memory.noResults")}</li>
            )}
            {searchResults.map((m) => (
              <li key={m.id} className="rounded bg-surface-overlay/50 px-2 py-1 text-xs text-ink">
                <span className="font-medium">{m.kind}</span>: {m.content}
              </li>
            ))}
          </ul>
        )}
      </div>

      <div>
        <h4 className="mb-2 text-xs font-medium text-ink-muted">{t("settings.memory.listTitle")}</h4>
        <AsyncBoundary
          isLoading={listQuery.isLoading}
          error={listQuery.error}
          isEmpty={(listQuery.data?.length ?? 0) === 0}
          emptyLabel={t("settings.memory.empty")}
          onRetry={() => void listQuery.refetch()}
        >
          <ul className="space-y-1">
            {listQuery.data?.map((m) => (
              <li
                key={m.id}
                className="flex items-start justify-between gap-2 rounded border border-ink-muted/20 px-2 py-1.5 text-xs"
              >
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-1">
                    <span className="font-medium text-ink">{m.kind}</span>
                    {m.userProfile && (
                      <span className="rounded bg-ink-accent/20 px-1 text-[10px] text-ink-accent">profile</span>
                    )}
                    {m.roleId && (
                      <span className="rounded bg-surface-overlay px-1 text-[10px] text-ink-muted">{m.roleId}</span>
                    )}
                  </div>
                  <p className="mt-0.5 text-ink-muted">{m.content}</p>
                  {m.tags.length > 0 && (
                    <p className="mt-0.5 text-[10px] text-ink-muted/70">{m.tags.join(", ")}</p>
                  )}
                </div>
                {confirmingId === m.id ? (
                  <div className="flex shrink-0 gap-1">
                    <button
                      type="button"
                      onClick={() => deleteMut.mutate(m.id)}
                      className="text-state-danger hover:text-state-danger/80"
                    >
                      {t("settings.memory.confirmDelete")}
                    </button>
                    <button
                      type="button"
                      onClick={() => setConfirmingId(null)}
                      className="text-ink-muted hover:text-ink"
                    >
                      {t("settings.memory.cancel")}
                    </button>
                  </div>
                ) : (
                  <button
                    type="button"
                    onClick={() => setConfirmingId(m.id)}
                    className="shrink-0 text-ink-muted hover:text-state-danger"
                    aria-label={t("settings.memory.delete")}
                  >
                    ×
                  </button>
                )}
              </li>
            ))}
          </ul>
        </AsyncBoundary>
      </div>
    </section>
  );
}
