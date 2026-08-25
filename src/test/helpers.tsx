/**
 * Shared test wiring: providers + IPC test-double injection + event bus
 * reset. Tests never touch the real Tauri runtime (testing.md rule).
 */
import type { ReactElement, ReactNode } from "react";
import { cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { injectIpcCommands, resetIpcCommands } from "../lib/ipc/client";
import { resetTestBus } from "../lib/events/transport";
import { tdReset, testDoubleCommands } from "../lib/ipc/test-double";
import { useToastStore } from "../lib/store/toastStore";
import "../i18n";

export function renderWithProviders(ui: ReactElement): ReturnType<typeof render> {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return render(<QueryClientProvider client={qc}>{ui as ReactNode}</QueryClientProvider>);
}

export function setupTestIpc(): void {
  injectIpcCommands(testDoubleCommands());
}

beforeEach(() => {
  setupTestIpc();
  tdReset();
  resetTestBus();
  useToastStore.setState({ toasts: [] });
});

afterEach(() => {
  cleanup();
});

export { resetIpcCommands };
