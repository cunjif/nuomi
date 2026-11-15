/**
 * uiStore file-tree expansion bucket tests (P2-N): toggles are bucketed per
 * workspace + focused work directory, the whole bucket map persists as one
 * app_settings blob, and hydration restores it exactly once per app run.
 * IPC rides the test-double (side-effect import, P2-K lesson).
 */
import { waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
// Side-effect import: injects the IPC test-double (toggleTreeDir persists
// through appSettingSet) and resets the in-memory state before each test.
import "../../test/helpers";
import { stubLocalStorage } from "../../test/stubStorage";
import { tdState } from "../../lib/ipc/test-double-state";
import { TREE_EXPANDED_SETTINGS_KEY, treeBucketKey, useUiStore } from "./uiStore";

beforeEach(() => {
  stubLocalStorage();
  useUiStore.setState({ expandedByDir: {}, treeExpandedHydrated: false });
});

describe("treeBucketKey", () => {
  it("joins workspace and directory ids with ::", () => {
    expect(treeBucketKey("ws-1", "dir-9")).toBe("ws-1::dir-9");
  });
});

describe("toggleTreeDir", () => {
  it("adds then removes a path within one bucket", () => {
    const key = treeBucketKey("ws-1", "dir-9");
    useUiStore.getState().toggleTreeDir(key, "src");
    expect(useUiStore.getState().expandedByDir[key]).toEqual(["src"]);
    useUiStore.getState().toggleTreeDir(key, "src");
    expect(useUiStore.getState().expandedByDir[key]).toEqual([]);
  });

  it("keeps buckets independent", () => {
    const a = treeBucketKey("ws-1", "dir-a");
    const b = treeBucketKey("ws-2", "dir-a");
    useUiStore.getState().toggleTreeDir(a, "src");
    useUiStore.getState().toggleTreeDir(b, "docs");
    expect(useUiStore.getState().expandedByDir[a]).toEqual(["src"]);
    expect(useUiStore.getState().expandedByDir[b]).toEqual(["docs"]);
  });

  it("persists the whole blob to app_settings (best-effort)", async () => {
    const a = treeBucketKey("ws-1", "dir-a");
    useUiStore.getState().toggleTreeDir(a, "src");
    await waitFor(() => {
      const raw = tdState.appSettings.get(TREE_EXPANDED_SETTINGS_KEY);
      expect(raw).toBeDefined();
      expect(JSON.parse(raw!)).toEqual({ [a]: ["src"] });
    });
  });
});

describe("hydrateTreeExpanded", () => {
  it("restores buckets from the blob and drops non-string[] entries", async () => {
    const a = treeBucketKey("ws-1", "dir-a");
    tdState.appSettings.set(
      TREE_EXPANDED_SETTINGS_KEY,
      JSON.stringify({ [a]: ["src", "docs"], bad: "nope", mixed: ["src", 42] }),
    );
    await useUiStore.getState().hydrateTreeExpanded();
    expect(useUiStore.getState().expandedByDir[a]).toEqual(["src", "docs"]);
    expect(useUiStore.getState().expandedByDir["bad"]).toBeUndefined();
    expect(useUiStore.getState().expandedByDir["mixed"]).toBeUndefined();
    expect(useUiStore.getState().treeExpandedHydrated).toBe(true);
  });

  it("drops a corrupted JSON blob without throwing", async () => {
    tdState.appSettings.set(TREE_EXPANDED_SETTINGS_KEY, "{not json");
    await useUiStore.getState().hydrateTreeExpanded();
    expect(useUiStore.getState().expandedByDir).toEqual({});
    expect(useUiStore.getState().treeExpandedHydrated).toBe(true);
  });

  it("runs only once per app run — later memory state is kept", async () => {
    const a = treeBucketKey("ws-1", "dir-a");
    tdState.appSettings.set(TREE_EXPANDED_SETTINGS_KEY, JSON.stringify({ [a]: ["old"] }));
    await useUiStore.getState().hydrateTreeExpanded();
    expect(useUiStore.getState().expandedByDir[a]).toEqual(["old"]);
    // A later FileTree mount re-calls hydrate; it must not clobber the
    // newer in-memory state with the (stale) blob.
    useUiStore.setState({ expandedByDir: { [a]: ["fresh"] } });
    await useUiStore.getState().hydrateTreeExpanded();
    expect(useUiStore.getState().expandedByDir[a]).toEqual(["fresh"]);
  });
});
