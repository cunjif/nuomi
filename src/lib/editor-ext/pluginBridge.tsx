/**
 * Plugin bridge (ADR 0010): hydrates editor extensions delivered by kernel
 * plugins over IPC. This is the Cordis-mirror completion promised in
 * registry.ts — a plugin whose `plugin.toml` declares an `[editor]` section
 * becomes a first-class EditorExtension with the derived id
 * `plugin.<id>.editor`, managed in the same registry as the built-ins.
 *
 * Contribution mapping (all RPC goes through `plugin_editor_call`, which the
 * host only forwards for manifest-declared capabilities):
 * - hover   → Monaco HoverProvider per declared language (["*"] binds the
 *             provider lazily to each mounted editor's language)
 * - symbols → SymbolProvider with a synchronous cache; the RPC refresh runs
 *             in the background and bumps the registry version store so
 *             reactive consumers re-render (the outline pipeline is sync by
 *             design — workspace indexing stays fast and local)
 * - command → chat slash commands `/<pluginId>.<name>` executed via the
 *             manifest-declared tool mapping
 * - overlay → sandboxed iframe (allow-scripts only; URLs were validated at
 *             manifest load: https or loopback http)
 */
import type { ReactNode } from "react";
import type { PluginInfoDto, JsonValue } from "../ipc/bindings.gen";
import type { EditorExtension, EditorFileRef, DocumentSymbol, SymbolProvider } from "./types";
import {
  bumpEditorExtVersion,
  ensureEditorExtensionsActivated,
  getRegisteredEditorExtensions,
  initEditorExtEnabledStore,
  registerEditorExtension,
  unregisterEditorExtension,
} from "./registry";
import { ipc } from "../ipc/client";

/** Derived extension id for a kernel plugin's editor contributions. */
export function pluginEditorExtensionId(pluginId: string): string {
  return `plugin.${pluginId}.editor`;
}

/** Prefix reserved for plugin-delivered editor extensions. */
export const PLUGIN_EDITOR_PREFIX = "plugin.";

function languageMatches(languages: readonly string[], language: string): boolean {
  return languages.includes("*") || languages.includes(language);
}

/** All plugin-delivered extensions currently registered (hydration cleanup). */
function getPluginEditorExtensions(): readonly EditorExtension[] {
  return getRegisteredEditorExtensions().filter((e) => e.id.startsWith(PLUGIN_EDITOR_PREFIX));
}

function buildHoverProvider(pluginId: string) {
  return {
    provideHover: async (
      model: { uri: { scheme: string; path: string; toString(): string }; getLanguageId(): string },
      position: { lineNumber: number; column: number },
    ) => {
      try {
        const path = model.uri.scheme === "file" ? model.uri.path : model.uri.toString();
        const result = await ipc.pluginEditorCall(pluginId, "editor/hover", {
          path,
          language: model.getLanguageId(),
          line: position.lineNumber - 1,
          character: position.column - 1,
        });
        const contents = (result as { contents?: unknown } | null)?.contents;
        return typeof contents === "string" && contents.length > 0
          ? { contents: [{ value: contents }] }
          : null;
      } catch {
        // Kernel not ready / plugin gone / timeout — Monaco semantics: null.
        return null;
      }
    },
  };
}

/**
 * Synchronous symbol provider backed by a per-path cache. The first call
 * returns [] (or stale symbols) and kicks off the RPC; when it lands the
 * cache is filled and notify() re-renders reactive consumers.
 */
function buildSymbolsProvider(pluginId: string, languages: readonly string[]): SymbolProvider {
  const cache = new Map<string, { content: string; symbols: DocumentSymbol[]; fresh: boolean }>();
  return {
    id: `${pluginEditorExtensionId(pluginId)}.symbols`,
    languages,
    provideSymbols(doc: EditorFileRef): DocumentSymbol[] {
      const hit = cache.get(doc.path);
      if (hit !== undefined && hit.content === doc.content) return hit.symbols;
      const stale = hit?.fresh === true ? hit.symbols : [];
      void (async () => {
        try {
          const result = await ipc.pluginEditorCall(pluginId, "editor/symbols", {
            path: doc.path,
            language: doc.language,
            content: doc.content,
          });
          const symbols = (result as { symbols?: unknown } | null)?.symbols;
          if (!Array.isArray(symbols)) return;
          cache.set(doc.path, {
            content: doc.content,
            symbols: symbols as DocumentSymbol[],
            fresh: true,
          });
          // The outline pipeline is synchronous (MonacoTab useMemo on
          // extVersion): without this bump the symbols would only appear on
          // the next unrelated re-render.
          bumpEditorExtVersion();
        } catch {
          cache.delete(doc.path);
        }
      })();
      return stale;
    },
  };
}

function PluginOverlayFrame(props: { url: string; title: string; width: number; height: number }): ReactNode {
  return (
    <div
      className="overflow-hidden rounded border border-ink-muted/30 bg-surface-raised shadow-lg"
      style={{ width: props.width, height: props.height }}
    >
      <div className="border-b border-ink-muted/20 px-2 py-1 text-[11px] text-ink-muted">{props.title}</div>
      <iframe
        src={props.url}
        title={props.title}
        // No allow-same-origin (never share our origin) and no
        // allow-top-navigation; manifest validation already restricted src
        // to https or loopback http.
        sandbox="allow-scripts"
        className="h-[calc(100%-22px)] w-full border-0"
      />
    </div>
  );
}

function makePluginExtension(info: PluginInfoDto): EditorExtension | null {
  const editor = info.editor;
  if (editor === null || editor === undefined) return null;
  const languages = editor.languages;
  return {
    id: pluginEditorExtensionId(info.id),
    title: info.name,
    contribute(ctx) {
      // Hover: bind lazily per mounted editor language so ["*"] works
      // without enumerating Monaco language ids at contribute time.
      if (editor.hover) {
        const hoverProvider = buildHoverProvider(info.id);
        const boundLanguages = new Set<string>();
        ctx.registerEditorReady((monaco, ed) => {
          const language = ed.getModel()?.getLanguageId();
          if (language === undefined || !languageMatches(languages, language)) return;
          if (boundLanguages.has(language)) return;
          boundLanguages.add(language);
          monaco.languages.registerHoverProvider(language, hoverProvider);
        });
        ctx.reportCapability("editor.hover");
      }
      if (editor.symbols) {
        ctx.registerOutline(buildSymbolsProvider(info.id, languages));
        ctx.reportCapability("editor.symbols");
      }
      for (const command of editor.commands) {
        ctx.registerCommand({
          // Namespace by plugin id: /upper.ask.
          name: `${info.id}.${command.name}`,
          description: command.title.length > 0 ? command.title : command.tool,
          args: "optional",
          async run(args) {
            let params: Record<string, JsonValue> = {};
            const trimmed = args.trim();
            if (trimmed.length > 0) {
              try {
                const parsed: unknown = JSON.parse(trimmed);
                if (parsed !== null && typeof parsed === "object" && !Array.isArray(parsed)) {
                  params = parsed as Record<string, JsonValue>;
                } else {
                  params = { input: parsed as JsonValue };
                }
              } catch {
                params = { input: trimmed };
              }
            }
            await ipc.pluginEditorCall(info.id, "editor/command", { name: command.name, arguments: params });
          },
        });
        ctx.reportCapability(`editor.command.${command.name}`);
      }
      for (const overlay of editor.overlays) {
        ctx.registerOverlay({
          id: `${pluginEditorExtensionId(info.id)}.${overlay.id}`,
          Component: () => (
            <PluginOverlayFrame
              url={overlay.url}
              title={overlay.title.length > 0 ? overlay.title : info.name}
              width={overlay.width}
              height={overlay.height}
            />
          ),
        });
        ctx.reportCapability(`editor.overlay.${overlay.id}`);
      }
    },
  };
}

/**
 * Pulls the plugin list and (un)registers one derived extension per plugin.
 * The list is a pure disk scan, so this works before `kernel-ready`; editor
 * RPCs stay inert until the kernel is up. Idempotent (same id + same version
 * is a no-op) — called on Shell mount and after every install. Never throws.
 */
export async function hydratePluginEditorExtensions(): Promise<void> {
  await initEditorExtEnabledStore();
  let plugins: PluginInfoDto[];
  try {
    plugins = (await ipc.pluginList()).plugins;
  } catch {
    // Non-Tauri runtime (tests / web preview) — nothing to hydrate.
    return;
  }
  const wanted = new Set<string>();
  for (const plugin of plugins) {
    if (plugin.editor === null) continue;
    wanted.add(pluginEditorExtensionId(plugin.id));
    const existing = getRegisteredEditorExtensions().find(
      (e) => e.id === pluginEditorExtensionId(plugin.id),
    );
    const pluginStamp = `${plugin.id}@${plugin.version}`;
    if (existing?.metadata?.["pluginStamp"] === pluginStamp) {
      // Same id, same version — already contributed; leave it activated.
      continue;
    }
    if (existing !== undefined) {
      // Plugin version changed: drop the stale extension (and every
      // contribution it already made) before re-registering, otherwise the
      // contribute() of the new one would pile up on the old entries.
      unregisterEditorExtension(pluginEditorExtensionId(plugin.id));
    }
    const ext = makePluginExtension(plugin);
    if (ext === null) continue;
    registerEditorExtension({ ...ext, metadata: { pluginStamp } });
  }
  for (const ext of getPluginEditorExtensions()) {
    if (!wanted.has(ext.id)) unregisterEditorExtension(ext.id);
  }
  ensureEditorExtensionsActivated();
}

/** Removes the derived extension after an uninstall (immediate UI effect). */
export function teardownPluginEditorExtension(pluginId: string): void {
  unregisterEditorExtension(pluginEditorExtensionId(pluginId));
}
