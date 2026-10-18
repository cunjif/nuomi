import type { ReactNode } from "react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "../../components/ui/Button";
import { Dialog } from "../../components/ui/Dialog";
import { Field, SelectField } from "../../components/ui/Field";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import type { SandboxLimitsDto, SandboxRuntimeDto } from "../../lib/ipc/bindings.gen";
import { toast } from "../../lib/store/toastStore";
import { SandboxCodeEditor } from "./SandboxCodeEditor";

/** Monaco language id per sandbox runtime id. */
export function languageForRuntime(runtimeId: string): string {
  switch (runtimeId) {
    case "python":
      return "python";
    case "node":
      return "javascript";
    case "go":
      return "go";
    default:
      return "plaintext";
  }
}

/** Shared limits editor: timeout (required), CPU / memory caps, network. */
export function LimitsFields({
  limits,
  onChange,
}: {
  limits: SandboxLimitsDto;
  onChange: (next: SandboxLimitsDto) => void;
}): ReactNode {
  const { t } = useTranslation();
  return (
    <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
      <label className="flex flex-col gap-1 text-xs text-ink-muted">
        {t("sandbox.limitsTimeout")}
        <Field
          type="number"
          min={1}
          value={limits.timeoutSecs}
          onChange={(e) => onChange({ ...limits, timeoutSecs: Math.max(1, Number(e.target.value) || 1) })}
          aria-label={t("sandbox.limitsTimeout")}
        />
      </label>
      <label className="flex flex-col gap-1 text-xs text-ink-muted">
        {t("sandbox.limitsCpu")}
        <Field
          type="number"
          min={1}
          max={100}
          placeholder="—"
          value={limits.cpuPercent ?? ""}
          onChange={(e) => {
            const raw = e.target.value;
            onChange({ ...limits, cpuPercent: raw === "" ? null : Math.max(1, Math.min(100, Number(raw) || 1)) });
          }}
          aria-label={t("sandbox.limitsCpu")}
        />
      </label>
      <label className="flex flex-col gap-1 text-xs text-ink-muted">
        {t("sandbox.limitsMemory")}
        <Field
          type="number"
          min={16}
          placeholder="—"
          value={limits.memoryMb ?? ""}
          onChange={(e) => {
            const raw = e.target.value;
            onChange({ ...limits, memoryMb: raw === "" ? null : Math.max(16, Number(raw) || 16) });
          }}
          aria-label={t("sandbox.limitsMemory")}
        />
      </label>
      <label className="flex items-center gap-2 self-end pb-1 text-xs text-ink-muted">
        <input
          type="checkbox"
          checked={limits.networkEnabled}
          onChange={(e) => onChange({ ...limits, networkEnabled: e.target.checked })}
          aria-label={t("sandbox.limitsNetwork")}
        />
        {t("sandbox.limitsNetwork")}
      </label>
    </div>
  );
}

export function defaultLimits(): SandboxLimitsDto {
  return { timeoutSecs: 120, cpuPercent: null, memoryMb: null, networkEnabled: false };
}

interface SandboxTaskFormProps {
  open: boolean;
  onClose: () => void;
  runtimes: SandboxRuntimeDto[];
}

/**
 * "Run code" dialog (ADR-0022 task 7.7): runtime picker, resource limits,
 * authorized-directory checkboxes (mounted work directories of the active
 * workspace — unchecked by default) and a Monaco code editor.
 */
export function SandboxTaskForm({ open, onClose, runtimes }: SandboxTaskFormProps): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [runtime, setRuntime] = useState(runtimes[0]?.id ?? "");
  const [source, setSource] = useState("");
  const [limits, setLimits] = useState<SandboxLimitsDto>(defaultLimits);
  const workspaceQuery = useQuery({
    queryKey: ["workspace", "active"],
    queryFn: () => ipc.getActiveWorkspace(),
  });
  const mountedDirs = useMemo(
    () => (workspaceQuery.data?.directories ?? []).filter((d) => d.mounted),
    [workspaceQuery.data],
  );
  const [selectedDirs, setSelectedDirs] = useState<string[]>([]);

  const submitMut = useMutation({
    mutationFn: () =>
      ipc.sandboxRunCode({
        runtime,
        source,
        authorizedDirs: selectedDirs,
        limits: { ...limits, timeoutSecs: limits.timeoutSecs },
      }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["sandbox"] });
      toast.success(t("sandbox.submitOk"));
      onClose();
    },
    onError: (e) => toast.error(describeError(e)),
  });

  const toggleDir = (absPath: string): void => {
    setSelectedDirs((prev) =>
      prev.includes(absPath) ? prev.filter((p) => p !== absPath) : [...prev, absPath],
    );
  };

  return (
    <Dialog
      open={open}
      title={t("sandbox.runCodeTitle")}
      onClose={onClose}
      className="max-w-2xl"
    >
      <form
        className="flex flex-col gap-3"
        onSubmit={(e) => {
          e.preventDefault();
          submitMut.mutate();
        }}
      >
        <label className="flex flex-col gap-1 text-xs text-ink-muted">
          {t("sandbox.runtime")}
          <SelectField
            value={runtime}
            onChange={(e) => setRuntime(e.target.value)}
            aria-label={t("sandbox.runtime")}
          >
            {runtimes.map((r) => (
              <option key={r.id} value={r.id}>
                {r.name} — {r.isolation}
              </option>
            ))}
          </SelectField>
        </label>

        <fieldset className="flex flex-col gap-1">
          <legend className="text-xs text-ink-muted">{t("sandbox.authorizedDirs")}</legend>
          {mountedDirs.length === 0 ? (
            <p className="text-xs text-ink-muted">{t("sandbox.noMountedDirs")}</p>
          ) : (
            <div className="flex flex-col gap-1">
              {mountedDirs.map((dir) => (
                <label key={dir.id} className="flex items-center gap-2 text-xs text-ink">
                  <input
                    type="checkbox"
                    checked={selectedDirs.includes(dir.absPath)}
                    onChange={() => toggleDir(dir.absPath)}
                  />
                  {dir.absPath}
                </label>
              ))}
            </div>
          )}
        </fieldset>

        <LimitsFields limits={limits} onChange={setLimits} />

        <div className="flex flex-col gap-1">
          <span className="text-xs text-ink-muted">{t("sandbox.code")}</span>
          <SandboxCodeEditor
            value={source}
            onChange={setSource}
            language={languageForRuntime(runtime)}
            ariaLabel={t("sandbox.code")}
          />
        </div>

        <div className="flex justify-end gap-2">
          <Button type="button" variant="ghost" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button type="submit" disabled={submitMut.isPending || source.trim() === ""}>
            {t("sandbox.run")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
