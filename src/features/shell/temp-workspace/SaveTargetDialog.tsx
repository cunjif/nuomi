/**
 * Save-target confirmation dialog (workbench_enhance spec 5.2.1-3 /
 * design 2.2.2). Prompts for an explicit target path before writing a
 * temp-workspace file to disk — the original external file is never
 * silently overwritten. On a write failure the edit content is preserved
 * (spec DFX 4.2.4).
 */
import type { ReactNode } from "react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../../components/ui/Button";
import { Dialog } from "../../../components/ui/Dialog";
import { Field } from "../../../components/ui/Field";
import { ipc } from "../../../lib/ipc/client";
import { useUiStore } from "../../../lib/store/uiStore";
import { toast } from "../../../lib/store/toastStore";

interface SaveTargetDialogProps {
  open: boolean;
  absPath: string;
  content: string;
  onClose: () => void;
}

export function SaveTargetDialog({ open, absPath, content, onClose }: SaveTargetDialogProps): ReactNode {
  const { t } = useTranslation();
  const [target, setTarget] = useState(absPath);
  const [saving, setSaving] = useState(false);
  const clearTempFileDirty = useUiStore((s) => s.clearTempFileDirty);

  useEffect(() => {
    if (open) setTarget(absPath);
  }, [open, absPath]);

  const handleSave = async (): Promise<void> => {
    setSaving(true);
    try {
      await ipc.writeFile(target, content, null);
      clearTempFileDirty(absPath);
      toast.success(t("workbench.tempWorkspace.save"));
      onClose();
    } catch {
      toast.error(t("workbench.tempWorkspace.saveFailed"));
      // Content preserved — dialog stays open (spec DFX 4.2.4).
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} title={t("workbench.tempWorkspace.saveTitle")} onClose={onClose} className="max-w-lg">
      <div className="flex flex-col gap-3">
        <p className="text-xs text-ink-muted">{t("workbench.tempWorkspace.saveHint")}</p>
        <label className="flex flex-col gap-1 text-xs text-ink-muted">
          {t("workbench.tempWorkspace.targetPath")}
          <Field
            type="text"
            value={target}
            onChange={(e) => setTarget(e.target.value)}
            aria-label={t("workbench.tempWorkspace.targetPath")}
          />
        </label>
        <div className="flex justify-end gap-2">
          <Button type="button" variant="ghost" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button type="button" disabled={saving || target.trim() === ""} onClick={() => void handleSave()}>
            {t("workbench.tempWorkspace.save")}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}
