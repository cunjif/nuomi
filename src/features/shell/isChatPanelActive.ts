import type { View, ActiveArea } from "../../lib/store/uiStore";

/**
 * Returns true only when the main content area is showing the chat panel —
 * i.e. the active area is the chat surface (not the workbench) AND the
 * current view is "chat". Used to scope the ChatTabBar so it never leaks
 * into settings / git / trace / board / approvals / scheduler / plugins
 * views or the workbench area.
 */
export function isChatPanelActive(view: View, activeArea: ActiveArea): boolean {
  return view === "chat" && activeArea === "chat";
}
