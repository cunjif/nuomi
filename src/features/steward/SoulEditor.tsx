import { useState, useEffect } from "react";
import type { ReactNode } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import type { SoulConfigDto } from "../../lib/ipc/bindings.gen";

const MAX_SOUL_LENGTH = 16000;

export interface SoulEditorProps {
  sessionId: string;
  soul: SoulConfigDto | null;
}

export function SoulEditor({ sessionId, soul }: SoulEditorProps): ReactNode {
  const qc = useQueryClient();
  const savedValue = soul?.content ?? "";
  const [draft, setDraft] = useState(savedValue);
  const [showDiscardPrompt, setShowDiscardPrompt] = useState(false);

  useEffect(() => {
    setDraft(savedValue);
  }, [savedValue]);

  const isDirty = draft !== savedValue;

  const saveMut = useMutation({
    mutationFn: (content: string) => ipc.stewardUpdateSoul(sessionId, content),
    onSuccess: () => {
      toast.success("SOUL.md 已保存");
      void qc.invalidateQueries({ queryKey: ["steward"] });
    },
    onError: (e) => toast.error(`保存失败: ${e instanceof Error ? e.message : String(e)}`),
  });

  const handleSave = (): void => {
    if (draft.length > MAX_SOUL_LENGTH) {
      toast.error(`内容过长（${draft.length} > ${MAX_SOUL_LENGTH}）`);
      return;
    }
    saveMut.mutate(draft);
  };

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium text-ink">SOUL.md</h3>
        <button
          type="button"
          className="pixel-fill-accent rounded px-2 py-0.5 text-xs text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
          onClick={handleSave}
          disabled={saveMut.isPending || !isDirty}
        >
          {saveMut.isPending ? "保存中..." : "保存"}
        </button>
      </div>
      <textarea
        className="sketch-input w-full bg-surface p-2 font-mono text-xs text-ink outline-none disabled:opacity-50"
        rows={8}
        placeholder="输入管家人格提示词（留空使用默认提示词）..."
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        disabled={saveMut.isPending}
      />
      <div className="flex justify-between text-xs text-ink-muted">
        <span>{draft.length} / {MAX_SOUL_LENGTH}</span>
        {isDirty && <span className="text-state-warn">有未保存的修改</span>}
      </div>
      {showDiscardPrompt && (
        <div className="rounded bg-surface-overlay p-2 text-xs">
          <p className="mb-1 text-ink">有未保存的修改，是否放弃？</p>
          <div className="flex gap-2">
            <button
              type="button"
              className="rounded px-2 py-0.5 text-ink hover:bg-surface"
              onClick={() => {
                setDraft(savedValue);
                setShowDiscardPrompt(false);
              }}
            >
              放弃修改
            </button>
            <button
              type="button"
              className="rounded px-2 py-0.5 text-ink-muted hover:bg-surface"
              onClick={() => setShowDiscardPrompt(false)}
            >
              保留草稿
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
