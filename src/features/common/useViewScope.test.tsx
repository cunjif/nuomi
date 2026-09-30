import { renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { injectIpcCommands } from "../../lib/ipc/client";
import { testDoubleCommands } from "../../lib/ipc/test-double";
import { useViewScope } from "./useViewScope";

function wrapper({ children }: { children: ReactNode }) {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return <QueryClientProvider client={qc}>{children}</QueryClientProvider>;
}

describe("useViewScope", () => {
  it("defaults to 'all' when the IPC returns 'all'", async () => {
    const base = testDoubleCommands();
    injectIpcCommands(base);

    const { result } = renderHook(() => useViewScope("board"), { wrapper });

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    expect(result.current.scope).toBe("all");
  });

  it("setScope calls the IPC and updates the scope", async () => {
    const base = testDoubleCommands();
    const setViewScope = vi.fn(base.setViewScope);
    injectIpcCommands({ ...base, setViewScope });

    const { result } = renderHook(() => useViewScope("scheduler"), { wrapper });

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await result.current.setScope("focused");
    expect(setViewScope).toHaveBeenCalledWith("scheduler", "focused");
  });
});
