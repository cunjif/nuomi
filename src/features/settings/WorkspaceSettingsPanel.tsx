import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";

interface WorkspaceSettingsState {
  restoreNonPinnedOnStartup: boolean;
  maxOpenWorkspaces: number;
  recentListCapacity: number;
}

const DEFAULTS: WorkspaceSettingsState = {
  restoreNonPinnedOnStartup: true,
  maxOpenWorkspaces: 8,
  recentListCapacity: 20,
};

const KEYS = {
  restoreNonPinnedOnStartup: "restore_non_pinned_on_startup",
  maxOpenWorkspaces: "max_open_workspaces",
  recentListCapacity: "recent_list_capacity",
} as const;

/** Multi-workspace configuration panel: open-set limits, startup restore, recent list capacity. */
export function WorkspaceSettingsPanel(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [state, setState] = useState<WorkspaceSettingsState>(DEFAULTS);

  useQuery({
    queryKey: ["workspace-settings"],
    queryFn: async () => {
      const [restore, maxOpen, recentCap] = await Promise.all([
        ipc.appSettingGet(KEYS.restoreNonPinnedOnStartup),
        ipc.appSettingGet(KEYS.maxOpenWorkspaces),
        ipc.appSettingGet(KEYS.recentListCapacity),
      ]);
      const next: WorkspaceSettingsState = {
        restoreNonPinnedOnStartup: restore !== "false",
        maxOpenWorkspaces: maxOpen ? parseInt(maxOpen, 10) || DEFAULTS.maxOpenWorkspaces : DEFAULTS.maxOpenWorkspaces,
        recentListCapacity: recentCap ? parseInt(recentCap, 10) || DEFAULTS.recentListCapacity : DEFAULTS.recentListCapacity,
      };
      setState(next);
      return next;
    },
    staleTime: 0,
  });

  const saveMut = useMutation({
    mutationFn: async (s: WorkspaceSettingsState) => {
      await Promise.all([
        ipc.appSettingSet(KEYS.restoreNonPinnedOnStartup, String(s.restoreNonPinnedOnStartup)),
        ipc.appSettingSet(KEYS.maxOpenWorkspaces, String(s.maxOpenWorkspaces)),
        ipc.appSettingSet(KEYS.recentListCapacity, String(s.recentListCapacity)),
      ]);
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["workspace-settings"] });
      toast.success(t("settings.workspace.saved", "已保存"));
    },
    onError: (e) =>
      toast.error(`${t("settings.workspace.saveFailed", "保存失败")}: ${describeError(e)}`),
  });

  const update = <K extends keyof WorkspaceSettingsState>(
    key: K,
    value: WorkspaceSettingsState[K],
  ) => setState((prev) => ({ ...prev, [key]: value }));

  return (
    <section className="mx-auto max-w-2xl space-y-6 p-4">
      <h2 className="text-title-hand text-base font-semibold">
        {t("settings.workspace.heading", "多工作区")}
      </h2>

      <label className="flex items-center gap-3">
        <input
          type="checkbox"
          checked={state.restoreNonPinnedOnStartup}
          onChange={(e) => update("restoreNonPinnedOnStartup", e.target.checked)}
          className="h-4 w-4 rounded border-ink-muted/40"
        />
        <span className="text-sm">
          {t("settings.workspace.restoreNonPinned", "启动时恢复非固定工作区")}
        </span>
      </label>

      <div className="space-y-1">
        <label className="text-sm font-medium">
          {t("settings.workspace.maxOpen", "最大同时开启工作区数")}
        </label>
        <input
          type="number"
          min={1}
          max={32}
          value={state.maxOpenWorkspaces}
          onChange={(e) => update("maxOpenWorkspaces", Math.max(1, Math.min(32, parseInt(e.target.value, 10) || DEFAULTS.maxOpenWorkspaces)))}
          className="w-24 rounded border border-ink-muted/40 bg-surface px-2 py-1 text-sm"
        />
      </div>

      <div className="space-y-1">
        <label className="text-sm font-medium">
          {t("settings.workspace.recentCapacity", "最近使用列表容量")}
        </label>
        <input
          type="number"
          min={1}
          max={100}
          value={state.recentListCapacity}
          onChange={(e) => update("recentListCapacity", Math.max(1, Math.min(100, parseInt(e.target.value, 10) || DEFAULTS.recentListCapacity)))}
          className="w-24 rounded border border-ink-muted/40 bg-surface px-2 py-1 text-sm"
        />
      </div>

      <button
        type="button"
        onClick={() => saveMut.mutate(state)}
        disabled={saveMut.isPending}
        className="sketch-btn px-4 py-1.5 text-sm text-ink-accent disabled:opacity-50"
      >
        {saveMut.isPending
          ? t("settings.workspace.saving", "保存中…")
          : t("settings.workspace.save", "保存")}
      </button>
    </section>
  );
}
