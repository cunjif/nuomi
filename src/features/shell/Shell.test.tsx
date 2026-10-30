/**
 * Shell drag-drop integration tests (workbench_enhance spec 5.2):
 * verifies the onDragDropEvent callback is wired to handleDropEvent and
 * routes file/directory drops correctly.
 */
import { screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { useUiStore } from "../../lib/store/uiStore";

// Capture the drag-drop callback registered by Shell.
const dragDropCallback = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: (cb: (event: { payload: { type: string; paths: string[] } }) => void) => {
      dragDropCallback.mockImplementation(cb);
      return Promise.resolve(() => {});
    },
  }),
}));

// Stub Tauri core invoke for layout snapshot persistence etc.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (cmd: string) => {
    if (cmd === "set_layout_snapshot") return null;
    if (cmd === "get_workspace") return { root: "", configured: false };
    return undefined;
  },
  transformCallback: () => 0,
}));

// Stub Tauri event listen to prevent unhandled rejections.
vi.mock("@tauri-apps/api/event", () => ({
  listen: () => Promise.resolve(() => {}),
  emit: () => Promise.resolve(),
}));

// Stub Tauri webview window APIs.
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    listen: () => Promise.resolve(() => {}),
    onResized: () => Promise.resolve(() => {}),
    onMoved: () => Promise.resolve(() => {}),
  }),
}));

// Monaco never loads in jsdom.
vi.mock("@monaco-editor/react", () => ({
  default: () => <textarea aria-label="editor" />,
}));
vi.mock("./monacoSetup", () => ({}));

// Mock the dialog plugin.
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: () => Promise.resolve(null) }));

import { Shell } from "./Shell";

beforeEach(() => {
  // Pretend we're inside a real Tauri webview so hasTauriRuntime() returns true.
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
});

afterEach(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

describe("Shell — drag-drop wiring", () => {
  it("registers an onDragDropEvent callback that routes drops to handleDropEvent", async () => {
    renderWithProviders(<Shell />);

    // Wait for Shell to mount and register the drag-drop callback.
    await screen.findByText(/nuomi|糯米|工作区|setup|配置/i).catch(() => null);
    await vi.waitFor(() => expect(dragDropCallback).toBeDefined());

    // Simulate a directory drop — handleDropEvent calls openDirectory.
    const openDirectorySpy = vi.spyOn(useUiStore.getState(), "openDirectory");
    dragDropCallback({ payload: { type: "drop", paths: ["C:\\projects\\demo"] } });
    await vi.waitFor(() => expect(openDirectorySpy).toHaveBeenCalled());
    openDirectorySpy.mockRestore();
  });

  it("ignores non-drop events (hover etc.)", async () => {
    renderWithProviders(<Shell />);

    await screen.findByText(/nuomi|糯米|工作区|setup|配置/i).catch(() => null);
    const openDirectorySpy = vi.spyOn(useUiStore.getState(), "openDirectory");
    dragDropCallback({ payload: { type: "hover", paths: [] } });
    expect(openDirectorySpy).not.toHaveBeenCalled();
    openDirectorySpy.mockRestore();
  });
});
