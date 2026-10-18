import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Button } from "../../components/ui/Button";
import { Dialog } from "../../components/ui/Dialog";
import { SelectField } from "../../components/ui/Field";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import type { SandboxRuntimeDto } from "../../lib/ipc/bindings.gen";
import { toast } from "../../lib/store/toastStore";
import { SandboxCodeEditor } from "./SandboxCodeEditor";
import { defaultLimits, languageForRuntime, LimitsFields } from "./SandboxTaskForm";

interface SandboxBuildFormProps {
  open: boolean;
  onClose: () => void;
  runtimes: SandboxRuntimeDto[];
}

/**
 * "Build app" dialog (ADR-0022 task 7.7): the build target resolves to the
 * active workspace's mounted directories automatically (kernel-side
 * convergence — no directory picker); signing keys stay in the host
 * keyring and never enter the sandbox.
 */
export function SandboxBuildForm({ open, onClose, runtimes }: SandboxBuildFormProps): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [runtime, setRuntime] = useState(runtimes[0]?.id ?? "");
  const [source, setSource] = useState("");
  const [limits, setLimits] = useState(defaultLimits);

  const submitMut = useMutation({
    mutationFn: () => ipc.sandboxBuildApp({ runtime, source, limits }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["sandbox"] });
      toast.success(t("sandbox.submitOk"));
      onClose();
    },
    onError: (e) => toast.error(describeError(e)),
  });

  return (
    <Dialog open={open} title={t("sandbox.buildTitle")} onClose={onClose} className="max-w-2xl">
      <form
        className="flex flex-col gap-3"
        onSubmit={(e) => {
          e.preventDefault();
          submitMut.mutate();
        }}
      >
        <p className="text-xs text-ink-muted">{t("sandbox.buildHint")}</p>
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

        <LimitsFields limits={limits} onChange={setLimits} />

        <div className="flex flex-col gap-1">
          <span className="text-xs text-ink-muted">{t("sandbox.buildScript")}</span>
          <SandboxCodeEditor
            value={source}
            onChange={setSource}
            language={languageForRuntime(runtime)}
            ariaLabel={t("sandbox.buildScript")}
          />
        </div>

        <div className="flex justify-end gap-2">
          <Button type="button" variant="ghost" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button type="submit" disabled={submitMut.isPending || source.trim() === ""}>
            {t("sandbox.build")}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}
