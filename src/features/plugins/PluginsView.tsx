import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import type { PluginInfoDto } from "../../lib/ipc/bindings.gen";
import { Icon } from "../../components/ui/Icon/Icon";
import {
  PLUGIN_EDITOR_PREFIX,
  ensureEditorExtensionsActivated,
  getRegisteredEditorExtensions,
  hydratePluginEditorExtensions,
  isEditorExtensionEnabled,
  pluginEditorExtensionId,
  setEditorExtensionEnabled,
  teardownPluginEditorExtension,
  useEditorExtVersion,
} from "../../lib/editor-ext";
import type { EditorExtension } from "../../lib/editor-ext";

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

/** Shared enable toggle: on/off chip with aria-pressed semantics. */
function ExtToggle({
  id,
  label,
  onToggle,
}: {
  id: string;
  label?: string;
  onToggle?: (next: boolean) => void;
}): ReactNode {
  const { t } = useTranslation();
  const enabled = isEditorExtensionEnabled(id);
  return (
    <button
      type="button"
      onClick={() => {
        setEditorExtensionEnabled(id, !enabled);
        if (!enabled) ensureEditorExtensionsActivated();
        onToggle?.(!enabled);
      }}
      aria-pressed={enabled}
      aria-label={label}
      className={`shrink-0 rounded-full border px-2.5 py-0.5 text-xs focus-visible:ring-2 focus-visible:ring-ink-accent ${
        enabled ? "border-state-ok/50 text-state-ok" : "border-ink-muted/40 text-ink-muted"
      }`}
    >
      {enabled ? t("editor.extEnabled") : t("editor.extDisabled")}
    </button>
  );
}

function KindBadge({ kind }: { kind: "builtin" | "plugin" }): ReactNode {
  const { t } = useTranslation();
  return (
    <span className="rounded-full bg-ink-accent/10 px-2 py-0.5 text-[10px] uppercase tracking-wide text-ink-accent">
      {t(kind === "builtin" ? "plugins.kindBuiltin" : "plugins.kindPlugin")}
    </span>
  );
}

/** One built-in editor extension row (single-source title + enable toggle). */
function BuiltinExtRow({ ext }: { ext: EditorExtension }): ReactNode {
  const { t } = useTranslation();
  const title = ext.title ?? (ext.titleI18nKey !== undefined ? t(ext.titleI18nKey) : ext.id);
  return (
    <li className="flex items-center gap-3 rounded-[12px_255px_15px_225px/225px_15px_255px_12px] border border-ink-muted/25 bg-surface-overlay px-4 py-3">
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="font-note-hand text-base text-ink-accent">{title}</span>
          <KindBadge kind="builtin" />
          <span className="font-mono text-[10px] text-ink-muted">{ext.id}</span>
        </div>
      </div>
      <ExtToggle id={ext.id} label={title} />
    </li>
  );
}

function PluginCard({ plugin }: { plugin: PluginInfoDto }): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const uninstallMut = useMutation({
    mutationFn: () => ipc.pluginUninstall(plugin.id),
    onSuccess: () => {
      // The derived editor extension disappears immediately; the plugin
      // process itself exits on the next boot (existing ADR 0009 semantic).
      teardownPluginEditorExtension(plugin.id);
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
  const editor = plugin.editor;
  const editorBadges = editor === null ? [] : [
    ...(editor.hover ? ["hover"] : []),
    ...(editor.symbols ? ["symbols"] : []),
    ...editor.commands.map((c) => `/${plugin.id}.${c.name}`),
    ...editor.overlays.map((o) => `${o.id} overlay`),
  ];

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
            <KindBadge kind="plugin" />
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

      {editor !== null && (
        <div className="mt-2 flex items-center gap-2 rounded border border-ink-muted/20 px-3 py-2">
          <span className="text-xs font-medium text-ink">{t("plugins.editorContrib")}</span>
          {editorBadges.length === 0 ? (
            <span className="text-xs italic text-ink-muted/70">{t("plugins.noEditorContrib")}</span>
          ) : (
            <span className="min-w-0 flex-1 truncate text-xs text-ink-muted" title={editorBadges.join(" · ")}>
              {editorBadges.join(" · ")}
            </span>
          )}
          {/* Controls only the derived editor extension — the plugin itself
              loads at boot regardless (nextBootNote explains the model). */}
          <ExtToggle id={pluginEditorExtensionId(plugin.id)} label={t("plugins.editorToggleHint")} />
        </div>
      )}

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
 * Extensions Center (ADR 0009 §4 addendum + ADR 0010): one surface for both
 * halves of the extension system. Built-in editor extensions and kernel
 * plugins share the same enable-toggle semantics (app_settings), while
 * install/uninstall of side-loaded plugins keeps the next-boot model.
 */
export function PluginsView(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  // Re-renders the built-in section when plugin hydration changes the registry.
  useEditorExtVersion();

  const listQuery = useQuery({
    queryKey: ["plugins"],
    queryFn: () => ipc.pluginList(),
  });

  const installMut = useMutation({
    mutationFn: (path: string) => ipc.pluginInstallFromPath(path),
    onSuccess: (info) => {
      // Re-scan and register the new plugin's derived editor extension right
      // away so the Extensions Center matches the disk. Its editor RPCs stay
      // unavailable until the next boot (the plugin process is only spawned
      // then, per the next-boot model) and degrade gracefully.
      void hydratePluginEditorExtensions();
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
  const builtinExts = getRegisteredEditorExtensions().filter(
    (e) => !e.id.startsWith(PLUGIN_EDITOR_PREFIX),
  );

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
        <section>
          <h2 className="text-xs font-semibold uppercase tracking-wide text-ink-muted/70">
            {t("plugins.builtinSection")}
          </h2>
          {builtinExts.length === 0 ? (
            <p className="mt-1 text-sm text-ink-muted">{t("common.empty")}</p>
          ) : (
            <ul className="mt-2 grid gap-2">
              {builtinExts.map((ext) => (
                <BuiltinExtRow key={ext.id} ext={ext} />
              ))}
            </ul>
          )}
        </section>

        <section className="mt-4">
          <h2 className="text-xs font-semibold uppercase tracking-wide text-ink-muted/70">
            {t("plugins.pluginSection")}
          </h2>
          <div className="mt-2">
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
          </div>
        </section>

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
