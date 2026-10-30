import { renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { ReactNode } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { injectIpcCommands } from "../../../lib/ipc/client";
import { testDoubleCommands } from "../../../lib/ipc/test-double";
import { tdReset, tdSeedFiles } from "../../../lib/ipc/test-double";
import { useUiStore } from "../../../lib/store/uiStore";
import { useActiveFileRunSource } from "./useActiveFileRunSource";

function wrapper({ children }: { children: ReactNode }): ReactNode {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  return <QueryClientProvider client={qc}>{children}</QueryClientProvider>;
}

function setActiveFile(path: string): void {
  useUiStore.setState({
    activeWorkspaceId: "ws-test",
    editorByWorkspace: {
      "ws-test": {
        openFiles: [path],
        activeFile: path,
        selectedPaths: [],
        lastSelectedPath: null,
        clipboardPaths: [],
        clipboardMode: null,
        dirtyPaths: {},
        crossRefs: {},
      },
    },
  });
}

describe("useActiveFileRunSource", () => {
  it("returns matched runtime + file content for an active python file", async () => {
    injectIpcCommands(testDoubleCommands());
    tdReset();
    tdSeedFiles({ "main.py": "print('hi')" });
    setActiveFile("main.py");

    const { result } = renderHook(() => useActiveFileRunSource(), { wrapper });
    await waitFor(() => expect(result.current.source).toBe("print('hi')"));
    expect(result.current.runtime).toBe("python");
    expect(result.current.language).toBe("python");
  });

  it("returns runtime null and empty source when there is no active file", async () => {
    injectIpcCommands(testDoubleCommands());
    tdReset();
    useUiStore.setState({ activeWorkspaceId: null, editorByWorkspace: {} });

    const { result } = renderHook(() => useActiveFileRunSource(), { wrapper });
    await waitFor(() => expect(result.current.runtime).toBeNull());
    expect(result.current.source).toBe("");
    expect(result.current.language).toBe("plaintext");
  });

  it("returns runtime null when the language has no matching runtime", async () => {
    injectIpcCommands(testDoubleCommands());
    tdReset();
    tdSeedFiles({ "README.md": "# title" });
    setActiveFile("README.md");

    const { result } = renderHook(() => useActiveFileRunSource(), { wrapper });
    await waitFor(() => expect(result.current.source).toBe("# title"));
    expect(result.current.runtime).toBeNull();
    expect(result.current.language).toBe("markdown");
  });
});
