import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { Button } from "../../components/ui/Button";
import { Card } from "../../components/ui/Card";
import { Badge, type BadgeTone } from "../../components/ui/Badge";
import { Field, TextareaField } from "../../components/ui/Field";
import { Icon } from "../../components/ui/Icon/Icon";
import { EmptyState } from "../../components/ui/EmptyState";
import { DiffView } from "./DiffView";
import { AiCommitBar } from "./AiCommitBar";

function statusTone(status: string): BadgeTone {
  switch (status) {
    case "M":
      return "warn";
    case "A":
      return "ok";
    case "D":
      return "danger";
    case "R":
    case "C":
      return "accent";
    case "?":
      return "neutral";
    case "U":
      return "danger";
    default:
      return "neutral";
  }
}

function displayStatus(indexStatus: string, worktreeStatus: string): string {
  const s = indexStatus !== " " && indexStatus !== "" ? indexStatus : worktreeStatus;
  return s === " " || s === "" ? "?" : s;
}

function isStaged(indexStatus: string): boolean {
  return indexStatus !== " " && indexStatus !== "" && indexStatus !== "?";
}

function Breadcrumbs({ path }: { path: string }): ReactNode {
  const parts = path.split(/[/\\]/);
  return (
    <div className="flex items-center gap-0.5 overflow-x-auto whitespace-nowrap text-xs text-ink-muted">
      {parts.map((part, i) => (
        <span key={i} className="flex items-center gap-0.5">
          {i > 0 && <Icon name="arrow-right" size={10} />}
          <span className={i === parts.length - 1 ? "font-mono text-ink" : ""}>{part}</span>
        </span>
      ))}
    </div>
  );
}

export function GitView(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [activeFile, setActiveFile] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  const [remote, setRemote] = useState("origin");
  const [branch, setBranch] = useState("main");

  const statusQuery = useQuery({ queryKey: ["git", "status"], queryFn: () => ipc.gitStatus() });
  const logQuery = useQuery({ queryKey: ["git", "log"], queryFn: () => ipc.gitLog(20) });
  const worktreesQuery = useQuery({ queryKey: ["git", "worktrees"], queryFn: () => ipc.gitWorktrees() });

  const activeEntry = statusQuery.data?.find((e) => e.path === activeFile) ?? null;
  const activeStaged = activeEntry ? isStaged(activeEntry.indexStatus) : false;
  const activePath = activeFile ?? "";

  const diffQuery = useQuery({
    queryKey: ["git", "diff", activePath, activeStaged],
    queryFn: () => ipc.gitDiff(activePath, activeStaged),
    enabled: activeFile !== null,
  });

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
    <section aria-label={t("git.heading")} className="flex h-full gap-2 p-2">
      <div className="flex w-80 shrink-0 flex-col gap-2 overflow-y-auto">
        <Card flush className="p-2">
          <TextareaField
            aria-label={t("git.message")}
            rows={2}
            value={message}
            onChange={(ev) => setMessage(ev.target.value)}
            onKeyDown={(e) => {
              if (e.ctrlKey && e.key === "Enter" && message.trim() && !commitMut.isPending) {
                commitMut.mutate();
              }
            }}
            placeholder={t("git.commitHint")}
          />
          <Button
            variant="solid"
            size="sm"
            className="mt-1.5 w-full"
            disabled={message.trim().length === 0 || commitMut.isPending}
            onClick={() => commitMut.mutate()}
          >
            <Icon name="commit" size={14} />
            {t("git.commit")}
          </Button>
          <AiCommitBar currentMessage={message} onMessageChange={setMessage} />
        </Card>

        <Card flush className="p-2">
          <p className="mb-1 font-note-hand text-sm text-ink-muted">{t("git.changes")}</p>
          {statusQuery.isLoading && <p role="status">{t("common.loading")}</p>}
          {statusQuery.isError && <p role="alert">{t("git.notRepo")}</p>}
          {statusQuery.data?.length === 0 && (
            <p className="text-xs text-ink-muted">{t("git.clean")}</p>
          )}
          <ul className="flex flex-col gap-0.5">
            {statusQuery.data?.map((e) => {
              const status = displayStatus(e.indexStatus, e.worktreeStatus);
              const isActive = e.path === activeFile;
              return (
                <li key={e.path}>
                  <div
                    className={`flex items-center gap-1.5 rounded px-1 py-0.5 ${
                      isActive ? "bg-surface-overlay" : "hover:bg-surface-overlay/50"
                    }`}
                  >
                    <input
                      type="checkbox"
                      id={`stage-${e.path}`}
                      checked={selected.has(e.path)}
                      onChange={() => toggle(e.path)}
                      className="shrink-0"
                    />
                    <label
                      htmlFor={`stage-${e.path}`}
                      className="flex min-w-0 flex-1 cursor-pointer items-center gap-1.5"
                      onClick={() => setActiveFile(e.path)}
                    >
                      <Badge tone={statusTone(status)} className="shrink-0">
                        {status}
                      </Badge>
                      <span className="truncate font-mono text-xs text-ink">{e.path}</span>
                    </label>
                  </div>
                </li>
              );
            })}
          </ul>
          <Button
            variant="outline"
            size="sm"
            className="mt-1.5 w-full"
            disabled={selected.size === 0 || stageMut.isPending}
            onClick={() => stageMut.mutate([...selected])}
          >
            {t("git.stage")}
          </Button>
        </Card>

        <Card flush className="p-2">
          <p className="mb-1 font-note-hand text-sm text-ink-muted">{t("git.pushSection")}</p>
          <div className="flex items-center gap-1.5">
            <div className="w-20">
              <Field
                aria-label={t("git.remote")}
                className="text-xs"
                value={remote}
                onChange={(ev) => setRemote(ev.target.value)}
              />
            </div>
            <div className="w-24">
              <Field
                aria-label={t("git.branch")}
                className="text-xs"
                value={branch}
                onChange={(ev) => setBranch(ev.target.value)}
              />
            </div>
            <Button
              variant="outline"
              size="sm"
              disabled={pushMut.isPending}
              onClick={() => pushMut.mutate()}
            >
              <Icon name="pull" size={14} />
              {t("git.push")}
            </Button>
          </div>
        </Card>

        {worktreesQuery.data != null && worktreesQuery.data.length > 0 && (
          <Card flush className="p-2">
            <p className="mb-1 font-note-hand text-sm text-ink-muted">{t("git.worktrees")}</p>
            <ul className="flex flex-col gap-0.5">
              {worktreesQuery.data.map((w) => (
                <li key={w.path} className="flex items-center justify-between gap-2">
                  <span className="font-mono text-xs">
                    {w.path}
                    {w.branch != null && <span className="text-ink-muted"> ({w.branch})</span>}
                    {w.isCurrent && <span className="text-state-ok"> ●</span>}
                  </span>
                  {!w.isCurrent && (
                    <Button variant="ghost" size="sm" onClick={() => switchMut.mutate(w.path)}>
                      {t("git.switchTo")}
                    </Button>
                  )}
                </li>
              ))}
            </ul>
          </Card>
        )}

        <Card flush className="p-2">
          <p className="mb-1 font-note-hand text-sm text-ink-muted">{t("git.log")}</p>
          {logQuery.isError && <p role="alert">{t("git.notRepo")}</p>}
          <ul className="flex flex-col gap-0.5 font-mono text-xs">
            {logQuery.data?.map((c) => (
              <li key={c.hash} className="py-0.5">
                <span className="text-ink-muted">{c.hash.slice(0, 7)} </span>
                {c.subject}
                <span className="text-ink-muted"> — {c.author}</span>
              </li>
            ))}
          </ul>
        </Card>
      </div>

      <div className="flex min-w-0 flex-1 flex-col overflow-hidden">
        {activeFile !== null ? (
          <>
            <div className="border-b border-dashed border-ink-muted/30 bg-surface-raised px-3 py-1.5">
              <Breadcrumbs path={activeFile} />
            </div>
            <div className="flex-1 overflow-auto bg-surface-raised">
              {diffQuery.isLoading && (
                <p role="status" className="p-3 text-sm text-ink-muted">
                  {t("common.loading")}
                </p>
              )}
              {diffQuery.isError && (
                <p role="alert" className="p-3 text-sm text-danger">
                  {t("git.notRepo")}
                </p>
              )}
              {diffQuery.data != null && <DiffView diffText={diffQuery.data} className="py-1" />}
            </div>
          </>
        ) : (
          <EmptyState
            icon="diff"
            title={t("git.selectFile")}
            hint={t("git.selectFileHint")}
            className="m-2"
          />
        )}
      </div>
    </section>
  );
}
