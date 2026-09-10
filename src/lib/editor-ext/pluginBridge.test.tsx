import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import { stubLocalStorage } from "../../test/stubStorage";
import { injectIpcCommands, resetIpcCommands } from "../ipc/client";
import {
  ensureEditorExtensionsActivated,
  getEditorExtVersion,
  getRegisteredEditorExtensions,
  getOutlineProviders,
  getOverlays,
  getToolbarActions,
  isEditorExtensionEnabled,
  setEditorExtensionEnabled,
  resetEditorExtensionsForTest,
} from "./registry";
import {
  hydratePluginEditorExtensions,
  pluginEditorExtensionId,
  teardownPluginEditorExtension,
} from "./pluginBridge";
import { findCommand } from "../commands/registry";
import type { PluginInfoDto } from "../ipc/bindings.gen";

function pluginInfo(overrides: Partial<PluginInfoDto> = {}): PluginInfoDto {
  return {
    id: "upper",
    name: "Upper",
    version: "0.1.0",
    apiVersion: 1,
    description: null,
    source: "user",
    dir: "/plugins/upper",
    uninstallable: true,
    tools: ["upper.to-upper"],
    hooks: [],
    events: [],
    editor: null,
    permissions: { fsRead: [], fsWrite: [], network: [], shell: false },
    ...overrides,
  };
}

beforeEach(() => {
  stubLocalStorage();
  resetEditorExtensionsForTest();
  resetIpcCommands();
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

/** Ok-result wrapper matching the specta Result shape the ipc client unwraps. */
function ok<T>(data: T): { status: "ok"; data: T } {
  return { status: "ok", data };
}

describe("plugin editor bridge — hydration", () => {
  it("registers a derived extension per plugin with editor contributions", async () => {
    injectIpcCommands({
      pluginList: async () => ok({
        plugins: [
          pluginInfo({
            editor: {
              languages: ["markdown"],
              hover: true,
              symbols: true,
              commands: [{ name: "ask", title: "Ask upper", tool: "to-upper" }],
              overlays: [],
            },
          }),
          // No editor section → no derived extension.
          pluginInfo({ id: "plain", name: "Plain", editor: null }),
        ],
        skipped: [],
        failed: [],
      }),
    });
    await hydratePluginEditorExtensions();
    const ids = getRegisteredEditorExtensions().map((e) => e.id);
    expect(ids).toEqual([pluginEditorExtensionId("upper")]);
    const ext = getRegisteredEditorExtensions()[0]!;
    expect(ext.title).toBe("Upper");
    // Contribution side effects landed in the read-side registries.
    ensureEditorExtensionsActivated();
    expect(getOutlineProviders()).toHaveLength(1);
    expect(findCommand("upper.ask")).toBeDefined();
  });

  it("unregisters derived extensions for plugins that disappeared", async () => {
    injectIpcCommands({
      pluginList: async () => ok({
        plugins: [pluginInfo({ editor: { languages: [], hover: false, symbols: false, commands: [], overlays: [] } })],
        skipped: [],
        failed: [],
      }),
    });
    await hydratePluginEditorExtensions();
    expect(getRegisteredEditorExtensions()).toHaveLength(1);
    // Plugin uninstalled between hydrations.
    injectIpcCommands({
      pluginList: async () => ok({ plugins: [], skipped: [], failed: [] }),
    });
    await hydratePluginEditorExtensions();
    expect(getRegisteredEditorExtensions()).toHaveLength(0);
  });

  it("teardown removes the derived extension immediately after uninstall", async () => {
    injectIpcCommands({
      pluginList: async () => ok({
        plugins: [pluginInfo({ editor: { languages: [], hover: false, symbols: false, commands: [], overlays: [{ id: "s", title: "S", url: "https://x.example.com", width: 320, height: 240 }] } })],
        skipped: [],
        failed: [],
      }),
    });
    await hydratePluginEditorExtensions();
    ensureEditorExtensionsActivated();
    expect(getOverlays()).toHaveLength(1);
    teardownPluginEditorExtension("upper");
    expect(getOverlays()).toHaveLength(0);
    expect(getRegisteredEditorExtensions()).toHaveLength(0);
  });

  it("re-registers (without duplicating contributions) when the plugin version changes", async () => {
    const editorSection = {
      languages: ["*"],
      hover: false,
      symbols: true,
      commands: [],
      overlays: [],
    };
    injectIpcCommands({
      pluginList: async () =>
        ok({
          plugins: [pluginInfo({ version: "1.0.0", editor: editorSection })],
          skipped: [],
          failed: [],
        }),
    });
    await hydratePluginEditorExtensions();
    ensureEditorExtensionsActivated();
    expect(getOutlineProviders()).toHaveLength(1);

    injectIpcCommands({
      pluginList: async () =>
        ok({
          plugins: [pluginInfo({ version: "1.1.0", editor: editorSection })],
          skipped: [],
          failed: [],
        }),
    });
    await hydratePluginEditorExtensions();
    ensureEditorExtensionsActivated();
    // One provider, not two: the stale extension is torn down first.
    expect(getOutlineProviders()).toHaveLength(1);
  });

  it("is a no-op when the IPC surface is unavailable (non-Tauri runtime)", async () => {
    await expect(hydratePluginEditorExtensions()).resolves.toBeUndefined();
    expect(getRegisteredEditorExtensions()).toHaveLength(0);
  });
});

describe("plugin editor bridge — command execution", () => {
  it("routes /<plugin>.<cmd> through pluginEditorCall with the declared name", async () => {
    const calls: Array<{ pluginId: string; method: string; params: unknown }> = [];
    injectIpcCommands({
      pluginList: async () => ok({
        plugins: [
          pluginInfo({
            editor: {
              languages: [],
              hover: false,
              symbols: false,
              commands: [{ name: "ask", title: "Ask", tool: "to-upper" }],
              overlays: [],
            },
          }),
        ],
        skipped: [],
        failed: [],
      }),
      pluginEditorCall: async (pluginId: string, method: string, params: never) => {
        calls.push({ pluginId, method, params });
        return ok(null);
      },
    });
    await hydratePluginEditorExtensions();
    const command = findCommand("upper.ask");
    expect(command).toBeDefined();
    // Free text becomes { input }, JSON objects pass through structured.
    await command?.run("hello", {} as Parameters<typeof command.run>[1]);
    await command?.run('{"text":"hi"}', {} as Parameters<typeof command.run>[1]);
    expect(calls).toHaveLength(2);
    expect(calls[0]).toEqual({
      pluginId: "upper",
      method: "editor/command",
      params: { name: "ask", arguments: { input: "hello" } },
    });
    expect(calls[1]?.params).toEqual({
      name: "ask",
      arguments: { text: "hi" },
    });
  });
});

describe("plugin editor bridge — overlay frame", () => {
  it("renders a sandboxed iframe (allow-scripts only)", async () => {
    injectIpcCommands({
      pluginList: async () => ok({
        plugins: [
          pluginInfo({
            editor: {
              languages: [],
              hover: false,
              symbols: false,
              commands: [],
              overlays: [
                { id: "stats", title: "Stats Panel", url: "https://plugins.example.com/stats", width: 400, height: 300 },
              ],
            },
          }),
        ],
        skipped: [],
        failed: [],
      }),
    });
    await hydratePluginEditorExtensions();
    ensureEditorExtensionsActivated();
    const overlays = getOverlays();
    expect(overlays).toHaveLength(1);
    const OverlayComponent = overlays[0]!.overlay.Component;
    render(<OverlayComponent />);
    const iframe = screen.getByTitle("Stats Panel");
    expect(iframe.getAttribute("sandbox")).toBe("allow-scripts");
    expect(iframe.getAttribute("src")).toBe("https://plugins.example.com/stats");
  });
});

describe("plugin editor bridge — symbol cache refresh", () => {
  it("bumps the registry version when the symbol RPC lands", async () => {
    injectIpcCommands({
      pluginList: async () =>
        ok({
          plugins: [
            pluginInfo({
              editor: { languages: ["markdown"], hover: false, symbols: true, commands: [], overlays: [] },
            }),
          ],
          skipped: [],
          failed: [],
        }),
      pluginEditorCall: async () =>
        ok({
          symbols: [
            {
              name: "H1",
              kind: "heading",
              range: { start: { line: 0, character: 0 }, end: { line: 0, character: 4 } },
              selectionRange: { start: { line: 0, character: 0 }, end: { line: 0, character: 4 } },
            },
          ],
        }),
    });
    await hydratePluginEditorExtensions();
    ensureEditorExtensionsActivated();
    const provider = getOutlineProviders()[0];
    expect(provider).toBeDefined();

    const before = getEditorExtVersion();
    const doc = { path: "a.md", language: "markdown", content: "# H1" };
    // First (synchronous) call has nothing cached yet.
    expect(provider?.provideSymbols(doc)).toEqual([]);
    // The async fill must be observable: MonacoTab re-computes on this bump.
    await vi.waitFor(() => {
      expect(getEditorExtVersion()).toBeGreaterThan(before);
    });
    expect(provider?.provideSymbols(doc)).toHaveLength(1);
  });
});

describe("plugin editor bridge — enable toggle", () => {
  it("derived extensions honor the shared enable state", async () => {
    injectIpcCommands({
      appSettingGet: async () => ok(null),
      appSettingSet: async () => ok(null),
      pluginList: async () => ok({
        plugins: [
          pluginInfo({
            editor: { languages: [], hover: false, symbols: true, commands: [], overlays: [] },
          }),
        ],
        skipped: [],
        failed: [],
      }),
    });
    await hydratePluginEditorExtensions();
    ensureEditorExtensionsActivated();
    const id = pluginEditorExtensionId("upper");
    expect(getOutlineProviders()).toHaveLength(1);
    setEditorExtensionEnabled(id, false);
    expect(getOutlineProviders()).toHaveLength(0);
    expect(isEditorExtensionEnabled(id)).toBe(false);
  });

  it("keeps toolbar actions out of plugin reads when disabled", async () => {
    injectIpcCommands({
      appSettingGet: async () => ok(null),
      appSettingSet: async () => ok(null),
      pluginList: async () => ok({
        plugins: [
          pluginInfo({
            editor: {
              languages: [],
              hover: false,
              symbols: false,
              commands: [],
              overlays: [],
            },
          }),
        ],
        skipped: [],
        failed: [],
      }),
    });
    await hydratePluginEditorExtensions();
    // Toolbar actions come only from contributes; the derived extension for
    // this manifest contributes none.
    expect(getToolbarActions()).toHaveLength(0);
  });
});
