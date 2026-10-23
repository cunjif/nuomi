/**
 * Source navigation orchestration (chat_rich_bind spec 5.2.1-5/6/7, 5.2.3):
 * `probeSourcePath` classifies the target, then the three branches fan out —
 *
 *  workspace_file → (cross-workspace: `workspaceSwitch` first)
 *                   `workspaceFocusDirectory` → `uiStore.openFile` →
 *                   `setActiveArea("workbench")`
 *  external_file  → `uiStore.openTempFile` (memory-only temp tab) →
 *                   `setActiveArea("workbench")`
 *  web_url        → `openExternalUrl` (system browser; in-app rendering of
 *                   arbitrary pages is forbidden, spec 4.3-4)
 *  unavailable    → toast "source file missing" (spec 5.2.3-1/2)
 *
 * A failed branch never affects the other list entries (each navigation is
 * an isolated try/catch).
 */
import { i18n } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { openExternalUrl } from "../../lib/openUrl";
import { toast } from "../../lib/store/toastStore";
import { useUiStore } from "../../lib/store/uiStore";
import type { SourceEntryDto } from "../../lib/events/types";

export async function navigateSource(source: SourceEntryDto): Promise<void> {
  if (source.kind === "cli_agent" || source.target === "") return;
  try {
    const probe = await ipc.probeSourcePath(source.target);
    const store = useUiStore.getState();
    switch (probe.kind) {
      case "workspace_file": {
        if (probe.workspaceId === null || probe.directoryId === null || probe.relativePath === null) {
          toast.warn(i18n.t("chat.sources.sourceMissing"));
          return;
        }
        if (probe.available === false) {
          toast.warn(i18n.t("chat.sources.sourceMissing"));
          return;
        }
        if (store.activeWorkspaceId !== probe.workspaceId) {
          await ipc.workspaceSwitch(probe.workspaceId);
          store.setActiveWorkspaceId(probe.workspaceId);
        }
        await ipc.workspaceFocusDirectory(probe.workspaceId, probe.directoryId);
        useUiStore.getState().openFile(probe.relativePath, probe.workspaceId);
        useUiStore.getState().setActiveArea("workbench");
        return;
      }
      case "external_file": {
        if (probe.normalizedTarget === null || probe.available === false) {
          toast.warn(i18n.t("chat.sources.sourceMissing"));
          return;
        }
        const name = probe.normalizedTarget.split(/[\\/]/).at(-1) ?? probe.normalizedTarget;
        useUiStore.getState().openTempFile(probe.normalizedTarget, name);
        return;
      }
      case "web_url": {
        await openExternalUrl(source.target);
        return;
      }
      default: {
        toast.warn(i18n.t("chat.sources.sourceMissing"));
      }
    }
  } catch (error) {
    console.debug(
      `[sources] navigation failed (len=${source.target.length}): ${error instanceof Error ? error.message : String(error)}`,
    );
    toast.error(i18n.t("chat.sources.openFailed"));
  }
}
