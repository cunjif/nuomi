import { create } from "zustand";

import type { StewardSessionDto } from "../ipc/bindings.gen";

interface StewardSessionsState {
  sessions: StewardSessionDto[];
  currentSessionId: string | null;
  setSessions: (sessions: StewardSessionDto[]) => void;
  setCurrentSessionId: (id: string | null) => void;
  addSession: (session: StewardSessionDto) => void;
  removeSession: (id: string) => void;
}

export const useStewardSessions = create<StewardSessionsState>((set) => ({
  sessions: [],
  currentSessionId: null,
  setSessions: (sessions) => set({ sessions }),
  setCurrentSessionId: (id) => set({ currentSessionId: id }),
  addSession: (session) =>
    set((s) => ({
      sessions: [session, ...s.sessions],
      currentSessionId: session.id,
    })),
  removeSession: (id) =>
    set((s) => ({
      sessions: s.sessions.filter((sess) => sess.id !== id),
      currentSessionId: s.currentSessionId === id ? null : s.currentSessionId,
    })),
}));
