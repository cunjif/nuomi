import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";

interface NewTaskFormProps {
  onDone: () => void;
}

/** Create-task form; failure toasts and keeps the draft in the fields. */
export function NewTaskForm({ onDone }: NewTaskFormProps): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");

  const createMut = useMutation({
    mutationFn: () => ipc.createTask(title.trim(), description.trim()),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["tasks"] });
      setTitle("");
      setDescription("");
      onDone();
    },
    onError: (e) => toast.error(`${t("board.createFailed")}: ${describeError(e)}`),
  });

  return (
    <form
      aria-label={t("board.newTask")}
      className="mb-2 flex flex-wrap items-end gap-2 rounded border border-ink-muted/40 bg-surface-raised p-2"
      onSubmit={(e) => {
        e.preventDefault();
        if (title.trim().length > 0) createMut.mutate();
      }}
    >
      <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
        {t("board.taskTitle")}
        <input
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          required
          className="w-48 rounded border border-ink-muted/40 bg-surface px-2 py-1 text-sm text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
        />
      </label>
      <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
        {t("board.taskDescription")}
        <input
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          className="w-64 rounded border border-ink-muted/40 bg-surface px-2 py-1 text-sm text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
        />
      </label>
      <button
        type="submit"
        disabled={createMut.isPending || title.trim().length === 0}
        className="rounded bg-ink-accent px-3 py-1.5 text-sm text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
      >
        {t("common.create")}
      </button>
      <button
        type="button"
        onClick={onDone}
        className="rounded border border-ink-muted px-3 py-1.5 text-sm text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
      >
        {t("common.cancel")}
      </button>
    </form>
  );
}
