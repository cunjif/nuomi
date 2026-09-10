/**
 * PluginsView tests: disk-scan list, folder-picker install, uninstall confirm.
 * IPC is doubled via injectIpcCommands; the OS picker is doubled via vi.mock.
 */
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { injectIpcCommands, resetIpcCommands } from "../../lib/ipc/client";
import { i18n } from "../../i18n";
import type { PluginInfoDto, PluginListResultDto } from "../../lib/ipc/bindings.gen";
import { PluginsView } from "./PluginsView";

const openDialog = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: (...a: unknown[]) => openDialog(...a) }));

const samplePlugin: PluginInfoDto = {
  id: "upper",
  name: "Upper",
  version: "1.0.0",
  apiVersion: 1,
  description: "Uppercases text",
  source: "user",
  dir: "/x/upper",
  uninstallable: true,
  tools: ["upper.upper"],
  hooks: ["post_tool_call"],
  events: ["session.start"],
  permissions: { fsRead: ["./data/**"], fsWrite: [], network: [], shell: false },
};

const sampleList: PluginListResultDto = {
  plugins: [samplePlugin],
  skipped: [],
  failed: [],
};

const emptyList: PluginListResultDto = { plugins: [], skipped: [], failed: [] };

beforeEach(async () => {
  await i18n.changeLanguage("en");
  resetIpcCommands();
  openDialog.mockReset();
});

describe("PluginsView", () => {
  it("lists installed plugins with contributions and permissions", async () => {
    injectIpcCommands({ pluginList: async () => ({ status: "ok", data: sampleList }) });
    renderWithProviders(<PluginsView />);

    expect(await screen.findByText("Upper")).toBeInTheDocument();
    expect(screen.getByText("upper.upper")).toBeInTheDocument();
    expect(screen.getByText("fs.read:./data/**")).toBeInTheDocument();
  });

  it("installs from a folder via the OS picker", async () => {
    const install = vi.fn(async () => ({ status: "ok" as const, data: samplePlugin }));
    injectIpcCommands({
      pluginList: async () => ({ status: "ok", data: emptyList }),
      pluginInstallFromPath: install,
    });
    openDialog.mockResolvedValue("/picked/plugin");

    renderWithProviders(<PluginsView />);
    fireEvent.click(await screen.findByRole("button", { name: /install from folder/i }));

    await waitFor(() => expect(install).toHaveBeenCalledWith("/picked/plugin"));
  });

  it("uninstalls a user plugin after confirmation", async () => {
    const uninstall = vi.fn(async () => ({ status: "ok" as const, data: null }));
    injectIpcCommands({
      pluginList: async () => ({ status: "ok", data: sampleList }),
      pluginUninstall: uninstall,
    });
    vi.stubGlobal("confirm", () => true);

    renderWithProviders(<PluginsView />);
    fireEvent.click(await screen.findByRole("button", { name: /^uninstall$/i }));

    await waitFor(() => expect(uninstall).toHaveBeenCalledWith("upper"));
  });

  it("does not uninstall without confirmation", async () => {
    const uninstall = vi.fn(async () => ({ status: "ok" as const, data: null }));
    injectIpcCommands({
      pluginList: async () => ({ status: "ok", data: sampleList }),
      pluginUninstall: uninstall,
    });
    vi.stubGlobal("confirm", () => false);

    renderWithProviders(<PluginsView />);
    fireEvent.click(await screen.findByRole("button", { name: /^uninstall$/i }));

    await new Promise((r) => setTimeout(r, 20));
    expect(uninstall).not.toHaveBeenCalled();
  });
});
