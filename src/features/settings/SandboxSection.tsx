import type { ReactNode } from "react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { describeError } from "../../i18n";
import { validateSandboxUrl } from "../sandbox/validateSandboxUrl";

export function SandboxSection(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();

  const endpointQuery = useQuery({
    queryKey: ["sandbox", "endpoint"],
    queryFn: () => ipc.getSandboxEndpointInfo(),
  });

  const [url, setUrl] = useState("");
  useEffect(() => {
    if (endpointQuery.data != null) {
      setUrl(endpointQuery.data.url);
    }
  }, [endpointQuery.data]);

  const envOverridden = endpointQuery.data?.source === "envvar";

  const handleSave = async (): Promise<void> => {
    const validation = validateSandboxUrl(url);
    if (!validation.valid) {
      toast.error(t("sandbox.section.invalidUrl"));
      return;
    }
    try {
      await ipc.appSettingSet("sandbox_cube_url", url);
      toast.success(t("sandbox.section.saved"));
      void qc.invalidateQueries({ queryKey: ["sandbox"] });
    } catch {
      toast.error(t("sandbox.section.saveFailed"));
    }
  };

  if (endpointQuery.isLoading) {
    return <div className="p-4 text-sm text-ink-muted">{t("sandbox.section.loading")}</div>;
  }
  if (endpointQuery.isError) {
    return <div className="p-4 text-sm text-ink-muted">{describeError(endpointQuery.error)}</div>;
  }

  return (
    <div className="flex flex-col gap-3 p-4">
      <h3 className="text-sm font-semibold">{t("sandbox.section.title")}</h3>
      <p className="text-xs text-ink-muted">{t("sandbox.section.description")}</p>
      {envOverridden && (
        <p className="rounded-[12px_255px_15px_225px/225px_15px_255px_12px] border border-ink-muted/40 bg-surface-overlay px-3 py-2 text-xs text-ink-accent">
          {t("sandbox.section.envOverridden")}
        </p>
      )}
      <div className="flex gap-2">
        <input
          type="text"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          disabled={envOverridden}
          placeholder="http://127.0.0.1:8819"
          className="flex-1 rounded-[12px_255px_15px_225px/225px_15px_255px_12px] border border-ink-muted/40 bg-surface px-3 py-1.5 font-mono text-xs text-ink focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-60"
        />
        <button
          type="button"
          onClick={() => void handleSave()}
          disabled={envOverridden}
          className="rounded-[12px_255px_15px_225px/225px_15px_255px_12px] bg-ink-accent px-4 py-1.5 font-note-hand text-xs text-surface hover:opacity-90 disabled:opacity-60"
        >
          {t("sandbox.section.save")}
        </button>
      </div>
      <p className="text-xs text-ink-muted">
        {t(`sandbox.section.source.${endpointQuery.data?.source ?? "default"}`)}
      </p>
    </div>
  );
}
