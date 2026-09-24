import { useState } from "react";

interface CloseConfirmDialogProps {
  workspaceId: string;
  reason: string;
  dirtyFiles: string[];
  runningTasks: string[];
  onSaveAndClose: () => void;
  onDiscardAndClose: () => void;
  onCancel: () => void;
}

export function CloseConfirmDialog({
  reason,
  dirtyFiles,
  runningTasks,
  onSaveAndClose,
  onDiscardAndClose,
  onCancel,
}: CloseConfirmDialogProps) {
  const [open, setOpen] = useState(true);
  if (!open) return null;

  const handleCancel = () => {
    setOpen(false);
    onCancel();
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
      <div className="w-full max-w-md rounded-lg bg-paper-bg border border-paper-line/40 shadow-lg p-6">
        <h2 className="text-lg font-medium mb-2">关闭工作区确认</h2>
        <p className="text-sm text-paper-muted mb-4">{reason}</p>

        {dirtyFiles.length > 0 && (
          <div className="mb-4">
            <h3 className="text-sm font-medium mb-1">未保存的文件</h3>
            <ul className="text-sm text-paper-muted max-h-32 overflow-auto">
              {dirtyFiles.map((f) => (
                <li key={f} className="truncate">{f}</li>
              ))}
            </ul>
          </div>
        )}

        {runningTasks.length > 0 && (
          <div className="mb-4">
            <h3 className="text-sm font-medium mb-1">运行中的任务</h3>
            <ul className="text-sm text-paper-muted max-h-32 overflow-auto">
              {runningTasks.map((t) => (
                <li key={t} className="truncate">{t}</li>
              ))}
            </ul>
          </div>
        )}

        <div className="flex justify-end gap-2 mt-6">
          <button
            onClick={handleCancel}
            className="px-3 py-1.5 rounded-md text-sm hover:bg-paper-bg/60"
          >
            取消
          </button>
          <button
            onClick={() => { setOpen(false); onDiscardAndClose(); }}
            className="px-3 py-1.5 rounded-md text-sm bg-paper-warn/20 text-paper-warn hover:opacity-80"
          >
            放弃更改并关闭
          </button>
          <button
            onClick={() => { setOpen(false); onSaveAndClose(); }}
            className="px-3 py-1.5 rounded-md text-sm bg-paper-accent text-paper-bg hover:opacity-90"
          >
            保存并关闭
          </button>
        </div>
      </div>
    </div>
  );
}
