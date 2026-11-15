/**
 * FileTree expansion persistence tests (P2-N): expansion lives in uiStore
 * buckets keyed by workspace + focused work directory, is persisted to
 * app_settings, is restored on remount (hydrate path), swaps buckets when
 * the focused directory changes, and is shared across both FileTree hosts
 * (workbench sidebar + terminal area).
 */
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { stubLocalStorage } from "../../test/stubStorage";
import { tdState } from "../../lib/ipc/test-double-state";
import type { WorkDirectoryDto, WorkspaceEntryDto } from "../../lib/ipc/bindings.gen";
import { TREE_EXPANDED_SETTINGS_KEY, treeBucketKey, useUiStore } from "../../lib/store/uiStore";
import { FileTree } from "./FileTree";

const folderPick = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: (...a: unknown[]) => folderPick(...a) }));

function makeDir(
  overrides: Partial<WorkDirectoryDto> & { id: string; displayName: string },
): WorkDirectoryDto {
  return {
    absPath: `C:\\dirs\\${overrides.displayName}`,
    mounted: true,
    addedAt: 1,
    available: true,
    isActive: false,
    ...overrides,
  };
}

function seedOpenWs(dirs: WorkDirectoryDto[]): WorkspaceEntryDto {
  const label = dirs.map((d) => d.displayName).join("-");
  const ws: WorkspaceEntryDto = {
    id: "ws-1",
    rootPath: "C:\\legacy-root",
    colorTag: "ink-blue",
    createdAt: 1,
    isActive: true,
    isFocused: true,
    isOpen: true,
    directoryPresent: dirs.some((d) => d.available),
    label,
    labelFull: label,
    directories: dirs,
  };
  tdState.workspaces.push(ws);
  return ws;
}

/** Flat path map the test-double's listDir walks ("" root = focused dir). */
function seedFiles(): void {
  tdState.files.set("src/main.rs", { isDir: false, size: 3, content: "x" });
  tdState.files.set("src/lib/mod.rs", { isDir: false, size: 3, content: "x" });
  tdState.files.set("README.md", { isDir: false, size: 3, content: "x" });
}

beforeEach(() => {
  folderPick.mockReset();
  stubLocalStorage();
  seedOpenWs([
    makeDir({ id: "dir-alpha", displayName: "alpha" }),
    makeDir({ id: "dir-beta", displayName: "beta" }),
  ]);
  seedFiles();
  useUiStore.setState({
    activeWorkspaceId: "ws-1",
    editorByWorkspace: {},
    expandedByDir: {},
    treeExpandedHydrated: false,
  });
});

describe("FileTree — expansion persistence (P2-N)", () => {
  it("persists expansion into the store bucket and app_settings", async () => {
    renderWithProviders(<FileTree workspaceId="ws-1" />);
    const src = await screen.findByRole("button", { name: "src" });
    expect(src).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(src);
    await waitFor(() => expect(src).toHaveAttribute("aria-expanded", "true"));
    const key = treeBucketKey("ws-1", "dir-alpha");
    expect(useUiStore.getState().expandedByDir[key]).toEqual(["src"]);
    const raw = tdState.appSettings.get(TREE_EXPANDED_SETTINGS_KEY);
    expect(raw).toBeDefined();
    expect(JSON.parse(raw!)).toMatchObject({ [key]: ["src"] });
  });

  it("restores expansion after remount (simulated restart hydrate)", async () => {
    const first = renderWithProviders(<FileTree workspaceId="ws-1" />);
    const src = await screen.findByRole("button", { name: "src" });
    fireEvent.click(src);
    await waitFor(() => expect(src).toHaveAttribute("aria-expanded", "true"));
    first.unmount();
    // Simulate an app restart: fresh in-memory buckets, blob survives in
    // app_settings (the test-double map is NOT reset here).
    useUiStore.setState({ expandedByDir: {}, treeExpandedHydrated: false });
    renderWithProviders(<FileTree workspaceId="ws-1" />);
    const restored = await screen.findByRole("button", { name: "src" });
    await waitFor(() => expect(restored).toHaveAttribute("aria-expanded", "true"));
  });

  it("expansion is bucketed per focused directory and each bucket restores", async () => {
    const first = renderWithProviders(<FileTree workspaceId="ws-1" />);
    const src = await screen.findByRole("button", { name: "src" });
    fireEvent.click(src);
    await waitFor(() => expect(src).toHaveAttribute("aria-expanded", "true"));

    // Switch the focused directory to dir-beta and remount: the same
    // relative path lands in a different (empty) bucket → collapsed.
    first.unmount();
    tdState.activeDirectoryId = "dir-beta";
    useUiStore.setState({ expandedByDir: {}, treeExpandedHydrated: false });
    const second = renderWithProviders(<FileTree workspaceId="ws-1" />);
    const srcBeta = await screen.findByRole("button", { name: "src" });
    await waitFor(() => expect(srcBeta).toHaveAttribute("aria-expanded", "false"));

    // Expand in the beta bucket, then switch back to alpha: both buckets
    // restore independently.
    fireEvent.click(srcBeta);
    await waitFor(() => expect(srcBeta).toHaveAttribute("aria-expanded", "true"));
    second.unmount();
    tdState.activeDirectoryId = "dir-alpha";
    useUiStore.setState({ expandedByDir: {}, treeExpandedHydrated: false });
    renderWithProviders(<FileTree workspaceId="ws-1" />);
    const srcAlpha = await screen.findByRole("button", { name: "src" });
    await waitFor(() => expect(srcAlpha).toHaveAttribute("aria-expanded", "true"));
  });

  it("shares expansion across both FileTree hosts", async () => {
    renderWithProviders(
      <>
        <FileTree workspaceId="ws-1" />
        <FileTree workspaceId="ws-1" />
      </>,
    );
    const srcs = await screen.findAllByRole("button", { name: "src" });
    expect(srcs.length).toBe(2);
    fireEvent.click(srcs[0]!);
    await waitFor(() => {
      for (const btn of screen.getAllByRole("button", { name: "src" })) {
        expect(btn).toHaveAttribute("aria-expanded", "true");
      }
    });
  });

  it("keeps the Products node in its own workspace-scoped bucket", async () => {
    renderWithProviders(<FileTree workspaceId="ws-1" />);
    const products = await screen.findByRole("button", { name: "Products" });
    expect(products).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(products);
    await waitFor(() => expect(products).toHaveAttribute("aria-expanded", "true"));
    expect(useUiStore.getState().expandedByDir[treeBucketKey("ws-1", "products")]).toEqual([
      "::products",
    ]);
  });
});
