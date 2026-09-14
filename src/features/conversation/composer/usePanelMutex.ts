import { useCallback, useState } from "react";

export type PanelId = "function" | "command" | "mention" | null;

export interface UsePanelMutexResult {
  /** Currently open panel, or null if none. */
  openPanel: PanelId;
  /** Open a panel, closing any other that's currently open. */
  open: (panel: Exclude<PanelId, null>) => void;
  /** Close the currently open panel. */
  close: () => void;
  /** Toggle a panel: close if it's the current one, open otherwise. */
  toggle: (panel: Exclude<PanelId, null>) => void;
  /** Check if a specific panel is open. */
  isOpen: (panel: Exclude<PanelId, null>) => boolean;
}

/**
 * Panel mutual exclusion: ensures only one popup panel (function menu,
 * slash command, @ mention) is open at a time. Opening a new panel
 * closes the previous one.
 */
export function usePanelMutex(): UsePanelMutexResult {
  const [openPanel, setOpenPanel] = useState<PanelId>(null);

  const open = useCallback((panel: Exclude<PanelId, null>): void => {
    setOpenPanel(panel);
  }, []);

  const close = useCallback((): void => {
    setOpenPanel(null);
  }, []);

  const toggle = useCallback((panel: Exclude<PanelId, null>): void => {
    setOpenPanel((current) => (current === panel ? null : panel));
  }, []);

  const isOpen = useCallback((panel: Exclude<PanelId, null>): boolean => {
    return openPanel === panel;
  }, [openPanel]);

  return { openPanel, open, close, toggle, isOpen };
}
