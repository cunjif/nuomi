/**
 * Legacy split view (task 8.3). Unreachable under the ADR-0021 single-open
 * model: task 6.6 removed the multi-open store state (`splitWorkspaceIds`)
 * and every write path that could produce layoutMode "split", so there is no
 * pane pair to render anymore. The Shell render branch and this component
 * are removed in task 6.8.
 */
export function SplitView(): React.ReactNode {
  return null;
}
