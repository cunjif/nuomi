import { create } from "zustand";

import type { EvolutionArtifactDto } from "../ipc/bindings.gen";

interface GatePendingState {
  pending: EvolutionArtifactDto[];
  setPending: (artifacts: EvolutionArtifactDto[]) => void;
  removeArtifact: (id: string) => void;
  updateArtifact: (artifact: EvolutionArtifactDto) => void;
}

export const useGatePending = create<GatePendingState>((set) => ({
  pending: [],
  setPending: (pending) => set({ pending }),
  removeArtifact: (id) =>
    set((s) => ({ pending: s.pending.filter((a) => a.id !== id) })),
  updateArtifact: (artifact) =>
    set((s) => ({
      pending: s.pending.map((a) => (a.id === artifact.id ? artifact : a)),
    })),
}));
