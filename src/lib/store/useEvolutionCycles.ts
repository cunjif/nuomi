import { create } from "zustand";

import type { CycleDetailDto, EvolutionCycleDto } from "../ipc/bindings.gen";

interface EvolutionCyclesState {
  cycles: EvolutionCycleDto[];
  detailCache: Record<string, CycleDetailDto>;
  setCycles: (cycles: EvolutionCycleDto[]) => void;
  setDetail: (cycleId: string, detail: CycleDetailDto) => void;
  updateCycle: (cycle: EvolutionCycleDto) => void;
  removeCycle: (cycleId: string) => void;
}

export const useEvolutionCycles = create<EvolutionCyclesState>((set) => ({
  cycles: [],
  detailCache: {},
  setCycles: (cycles) => set({ cycles }),
  setDetail: (cycleId, detail) =>
    set((s) => ({
      detailCache: { ...s.detailCache, [cycleId]: detail },
    })),
  updateCycle: (cycle) =>
    set((s) => {
      const exists = s.cycles.some((c) => c.id === cycle.id);
      return {
        cycles: exists
          ? s.cycles.map((c) => (c.id === cycle.id ? cycle : c))
          : [cycle, ...s.cycles],
      };
    }),
  removeCycle: (cycleId) =>
    set((s) => {
      const { [cycleId]: _, ...rest } = s.detailCache;
      return {
        cycles: s.cycles.filter((c) => c.id !== cycleId),
        detailCache: rest,
      };
    }),
}));
