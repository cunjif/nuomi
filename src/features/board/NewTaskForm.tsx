import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { describeError } from "../../i18n";
import { Button } from "../../components/ui/Button";
import { Field } from "../../components/ui/Field";
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
        <Field
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          required
          className="w-48 bg-surface text-sm"
        />
      </label>
      <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
        {t("board.taskDescription")}
        <Field
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          className="w-64 bg-surface text-sm"
        />
      </label>
      <button
        type="submit"
        disabled={createMut.isPending || title.trim().length === 0}
        className="pixel-fill-accent rounded-[12px_255px_15px_225px/225px_15px_255px_12px] px-3 py-1.5 font-note-hand text-sm text-surface shadow-sketch-sm focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
      >
        {t("common.create")}
      </button>
      <Button variant="outline" onClick={onDone}>
        {t("common.cancel")}
      </Button>
    </form>
  );
}
