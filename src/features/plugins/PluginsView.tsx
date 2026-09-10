import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import type { PluginInfoDto } from "../../lib/ipc/bindings.gen";
import { Icon } from "../../components/ui/Icon/Icon";

/** Folder picker — reuses the existing `@tauri-apps/plugin-dialog` precedent. */
async function pickFolder(titleKey: string): Promise<string | null> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({ directory: true, multiple: false, title: titleKey });
  return typeof picked === "string" ? picked : null;
}

async function pickZip(titleKey: string): Promise<string | null> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({
    multiple: false,
    filters: [{ name: "Plugin archive", extensions: ["zip"] }],
    title: titleKey,
  });
  return typeof picked === "string" ? picked : null;
}

function PluginCard({ plugin }: { plugin: PluginInfoDto }): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const uninstallMut = useMutation({
    mutationFn: () => ipc.pluginUninstall(plugin.id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["plugins"] });
      toast.success(t("plugins.uninstalled", { name: plugin.name }));
    },
    onError: (e) => toast.error(t("plugins.uninstallFailed", { message: describeError(e) })),
  });

  const contributions = [
    ...plugin.tools.map((name) => ({ label: name, kind: t("plugins.toolsTitle") })),
    ...plugin.hooks.map((point) => ({ label: point, kind: t("plugins.hooksTitle") })),
    ...plugin.events.map((topic) => ({ label: topic, kind: t("plugins.eventsTitle") })),
  ];
  const perm = plugin.permissions;
  const hasPerms =
    perm.fsRead.length > 0 ||
    perm.fsWrite.length > 0 ||
    perm.network.length > 0 ||
    perm.shell;

  return (
    <li className="rounded-[12px_255px_15px_225px/225px_15px_255px_12px] border border-ink-muted/25 bg-surface-overlay px-4 py-3">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <span className="font-note-hand text-base text-ink-accent">{plugin.name}</span>
            <span className="text-xs text-ink-muted">v{plugin.version}</span>
            <span className="rounded-full bg-ink-muted/15 px-2 py-0.5 text-[10px] uppercase tracking-wide text-ink-muted">
              {t(`plugins.source.${plugin.source}`)}
            </span>
          </div>
          {plugin.description && (
            <p className="mt-1 line-clamp-2 text-sm text-ink-muted">{plugin.description}</p>
          )}
        </div>
        {plugin.uninstallable && (
          <button
            type="button"
            onClick={() => {
              if (confirm(t("plugins.confirmUninstall", { name: plugin.name }))) {
                uninstallMut.mutate();
              }
            }}
            className="shrink-0 rounded-[12px_255px_15px_225px/225px_15px_255px_12px] border border-ink-muted/30 px-3 py-1 text-sm text-ink-muted hover:border-red-500/50 hover:text-red-500 focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            {uninstallMut.isPending ? t("plugins.uninstalling") : t("plugins.uninstall")}
          </button>
        )}
      </div>

      <div className="mt-2 flex flex-wrap gap-1.5">
        {contributions.length === 0 ? (
          <span className="text-xs italic text-ink-muted/70">{t("plugins.noContributions")}</span>
        ) : (
          contributions.map((c) => (
            <span
              key={`${c.kind}:${c.label}`}
              className="rounded-full bg-ink-accent/10 px-2 py-0.5 text-[11px] text-ink-accent"
              title={c.kind}
            >
              {c.label}
            </span>
          ))
        )}
      </div>

      {hasPerms && (
        <div className="mt-2 text-[11px] text-ink-muted/80">
          <span className="font-medium">{t("plugins.permissionsTitle")}: </span>
          <span>
            {[
              ...perm.fsRead.map((p) => `fs.read:${p}`),
              ...perm.fsWrite.map((p) => `fs.write:${p}`),
              ...perm.network.map((n) => `net:${n}`),
              ...(perm.shell ? ["shell"] : []),
            ].join(" · ") || t("common.empty")}
          </span>
        </div>
      )}
    </li>
  );
}

/**
 * Plugin panel (ADR 0009 §4 addendum): the no-terminal install surface.
 * Lists every side-load location, installs from a folder or .zip via the OS
 * file picker, and uninstalls user-managed plugins. Install/uninstall take
 * effect on the next app boot (the running kernel keeps its process set).
 */
export function PluginsView(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();

  const listQuery = useQuery({
    queryKey: ["plugins"],
    queryFn: () => ipc.pluginList(),
  });

  const installMut = useMutation({
    mutationFn: (path: string) => ipc.pluginInstallFromPath(path),
    onSuccess: (info) => {
      void qc.invalidateQueries({ queryKey: ["plugins"] });
      toast.success(t("plugins.installed", { name: info.name }));
    },
    onError: (e) => toast.error(t("plugins.installFailed", { message: describeError(e) })),
  });

  const openDirMut = useMutation({
    mutationFn: () => ipc.pluginOpenDir(),
    onError: (e) => toast.error(t("errors.generic", { message: describeError(e) })),
  });

  const onPickFolder = async () => {
    const path = await pickFolder(t("plugins.pickFolderTitle"));
    if (path) installMut.mutate(path);
  };
  const onPickZip = async () => {
    const path = await pickZip(t("plugins.pickZipTitle"));
    if (path) installMut.mutate(path);
  };

  const data = listQuery.data;
  const plugins = data?.plugins ?? [];
  const skipped = data?.skipped ?? [];
  const failed = data?.failed ?? [];

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="shrink-0 border-b border-ink-muted/30 px-4 py-3">
        <h1 className="font-note-hand text-xl text-ink-accent">{t("plugins.title")}</h1>
        <p className="mt-0.5 text-sm text-ink-muted">{t("plugins.subtitle")}</p>
        <div className="mt-3 flex flex-wrap gap-2">
          <button
            type="button"
            onClick={onPickFolder}
            disabled={installMut.isPending}
            className="rounded-[12px_255px_15px_225px/225px_15px_255px_12px] border border-ink-muted/30 px-3 py-1.5 text-sm text-ink hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
          >
            <Icon name="plus" className="mr-1 inline h-4 w-4 align-[-2px]" />
            {installMut.isPending ? t("plugins.installing") : t("plugins.installFromFolder")}
          </button>
          <button
            type="button"
            onClick={onPickZip}
            disabled={installMut.isPending}
            className="rounded-[12px_255px_15px_225px/225px_15px_255px_12px] border border-ink-muted/30 px-3 py-1.5 text-sm text-ink hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
          >
            <Icon name="plus" className="mr-1 inline h-4 w-4 align-[-2px]" />
            {t("plugins.installFromZip")}
          </button>
          <button
            type="button"
            onClick={() => openDirMut.mutate()}
            className="rounded-[12px_255px_15px_225px/225px_15px_255px_12px] border border-ink-muted/30 px-3 py-1.5 text-sm text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            {t("plugins.openDir")}
          </button>
        </div>
        <p className="mt-2 text-xs text-ink-muted/70">{t("plugins.nextBootNote")}</p>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
        {listQuery.isLoading ? (
          <p className="text-sm text-ink-muted">…</p>
        ) : plugins.length === 0 ? (
          <p className="text-sm text-ink-muted">{t("plugins.noneInstalled")}</p>
        ) : (
          <ul className="grid gap-2">
            {plugins.map((p) => (
              <PluginCard key={`${p.source}:${p.id}`} plugin={p} />
            ))}
          </ul>
        )}

        {skipped.length > 0 && (
          <section className="mt-4">
            <h2 className="text-xs font-semibold uppercase tracking-wide text-ink-muted/70">
              {t("plugins.skippedTitle")}
            </h2>
            <ul className="mt-1 space-y-0.5 text-xs text-ink-muted/80">
              {skipped.map((s) => (
                <li key={s}>{s}</li>
              ))}
            </ul>
          </section>
        )}

        {failed.length > 0 && (
          <section className="mt-4">
            <h2 className="text-xs font-semibold uppercase tracking-wide text-red-500/80">
              {t("plugins.failedTitle")}
            </h2>
            <ul className="mt-1 space-y-0.5 text-xs text-red-500/80">
              {failed.map((f) => (
                <li key={f}>{f}</li>
              ))}
            </ul>
          </section>
        )}
      </div>
    </div>
  );
}
