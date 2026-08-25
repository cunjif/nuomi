import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";

/**
 * Git panel (SPEC D5/US5): read status/log, stage selected paths,
 * commit with message, push to remote/branch.
 */
export function GitView(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [message, setMessage] = useState("");
  const [remote, setRemote] = useState("origin");
  const [branch, setBranch] = useState("main");

  const statusQuery = useQuery({ queryKey: ["git", "status"], queryFn: ipc.gitStatus });
  const logQuery = useQuery({ queryKey: ["git", "log"], queryFn: () => ipc.gitLog(20) });
  const worktreesQuery = useQuery({ queryKey: ["git", "worktrees"], queryFn: ipc.gitWorktrees });

  const refresh = () => {
    void qc.invalidateQueries({ queryKey: ["git"] });
    setSelected(new Set());
  };
  const fail = (e: unknown) => toast.error(`${t("git.actionFailed")}: ${describeError(e)}`);

  const switchMut = useMutation({
    mutationFn: (path: string) => ipc.setWorkspace(path),
    onSuccess: () => {
      toast.success(t("git.switched"));
      void qc.invalidateQueries({ queryKey: ["git"] });
    },
    onError: fail,
  });

  const stageMut = useMutation({
    mutationFn: (paths: string[]) => ipc.gitStage(paths),
    onSuccess: refresh,
    onError: fail,
  });
  const commitMut = useMutation({
    mutationFn: () => ipc.gitCommit(message),
    onSuccess: () => {
      setMessage("");
      refresh();
    },
    onError: fail,
  });
  const pushMut = useMutation({
    mutationFn: () => ipc.gitPush(remote, branch),
    onSuccess: (out) => toast.success(out || t("git.pushDone")),
    onError: fail,
  });

  const toggle = (path: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  return (
    <section aria-label={t("git.heading")} className="flex h-full flex-col gap-3 overflow-y-auto p-3">
      <h2 className="text-lg font-semibold">{t("git.heading")}</h2>

      <fieldset className="rounded border border-surface-overlay p-2">
        <legend className="px-1 text-ink-muted">{t("git.changes")}</legend>
        {statusQuery.isLoading && <p role="status">{t("common.loading")}</p>}
        {statusQuery.isError && <p role="alert">{t("git.notRepo")}</p>}
        {statusQuery.data?.length === 0 && <p>{t("git.clean")}</p>}
        <ul>
          {statusQuery.data?.map((e) => (
            <li key={e.path} className="flex items-center gap-2 py-0.5">
              <input
                type="checkbox"
                id={`stage-${e.path}`}
                checked={selected.has(e.path)}
                onChange={() => toggle(e.path)}
              />
              <label htmlFor={`stage-${e.path}`} className="font-mono text-sm">
                <span className="text-ink-muted">{e.indexStatus || e.worktreeStatus} </span>
                {e.path}
              </label>
            </li>
          ))}
        </ul>
        <button
          type="button"
          className="mt-2 rounded bg-surface-overlay px-2 py-1 text-sm disabled:opacity-50"
          disabled={selected.size === 0 || stageMut.isPending}
          onClick={() => stageMut.mutate([...selected])}
        >
          {t("git.stage")}
        </button>
      </fieldset>

      <fieldset className="rounded border border-surface-overlay p-2">
        <legend className="px-1 text-ink-muted">{t("git.commitSection")}</legend>
        <textarea
          aria-label={t("git.message")}
          className="w-full rounded bg-surface-raised p-1 text-sm"
          rows={2}
          value={message}
          onChange={(ev) => setMessage(ev.target.value)}
        />
        <button
          type="button"
          className="mt-1 rounded bg-surface-overlay px-2 py-1 text-sm disabled:opacity-50"
          disabled={message.trim().length === 0 || commitMut.isPending}
          onClick={() => commitMut.mutate()}
        >
          {t("git.commit")}
        </button>
      </fieldset>

      <fieldset className="rounded border border-surface-overlay p-2">
        <legend className="px-1 text-ink-muted">{t("git.pushSection")}</legend>
        <div className="flex items-center gap-2">
          <input
            aria-label={t("git.remote")}
            className="w-24 rounded bg-surface-raised p-1 text-sm"
            value={remote}
            onChange={(ev) => setRemote(ev.target.value)}
          />
          <input
            aria-label={t("git.branch")}
            className="w-32 rounded bg-surface-raised p-1 text-sm"
            value={branch}
            onChange={(ev) => setBranch(ev.target.value)}
          />
          <button
            type="button"
            className="rounded bg-surface-overlay px-2 py-1 text-sm disabled:opacity-50"
            disabled={pushMut.isPending}
            onClick={() => pushMut.mutate()}
          >
            {t("git.push")}
          </button>
        </div>
      </fieldset>

      <fieldset className="rounded border border-surface-overlay p-2">
        <legend className="px-1 text-ink-muted">{t("git.worktrees")}</legend>
        <ul>
          {worktreesQuery.data?.map((w) => (
            <li key={w.path} className="flex items-center justify-between gap-2 py-0.5">
              <span className="font-mono text-xs">
                {w.path}
                {w.branch != null && <span className="text-ink-muted"> ({w.branch})</span>}
                {w.isCurrent && <span className="text-state-ok"> ●</span>}
              </span>
              {!w.isCurrent && (
                <button
                  type="button"
                  className="rounded bg-surface-overlay px-2 py-0.5 text-xs"
                  onClick={() => switchMut.mutate(w.path)}
                >
                  {t("git.switchTo")}
                </button>
              )}
            </li>
          ))}
        </ul>
      </fieldset>

      <fieldset className="rounded border border-surface-overlay p-2">
        <legend className="px-1 text-ink-muted">{t("git.log")}</legend>
        {logQuery.isError && <p role="alert">{t("git.notRepo")}</p>}
        <ul className="font-mono text-xs">
          {logQuery.data?.map((c) => (
            <li key={c.hash} className="py-0.5">
              <span className="text-ink-muted">{c.hash.slice(0, 7)} </span>
              {c.subject}
              <span className="text-ink-muted"> — {c.author}</span>
            </li>
          ))}
        </ul>
      </fieldset>
    </section>
  );
}
