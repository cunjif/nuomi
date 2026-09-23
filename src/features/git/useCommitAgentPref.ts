import { create } from "zustand";

interface SelectedAgent {
  kind: string;
  id: string;
}

interface CommitAgentPrefState {
  selectedAgent: SelectedAgent | null;
  sensitiveAcknowledged: boolean;
  setSelectedAgent: (agent: SelectedAgent | null) => void;
  clearSelectedAgent: () => void;
  markSensitiveAcknowledged: () => void;
}

const STORAGE_KEY = "nuomi.ai_commit.pref";

function loadFromStorage(): Pick<CommitAgentPrefState, "selectedAgent" | "sensitiveAcknowledged"> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw == null) return { selectedAgent: null, sensitiveAcknowledged: false };
    const parsed = JSON.parse(raw) as Partial<{
      selectedAgent: SelectedAgent | null;
      sensitiveAcknowledged: boolean;
    }>;
    return {
      selectedAgent: parsed.selectedAgent ?? null,
      sensitiveAcknowledged: parsed.sensitiveAcknowledged ?? false,
    };
  } catch {
    return { selectedAgent: null, sensitiveAcknowledged: false };
  }
}

function saveToStorage(state: Pick<CommitAgentPrefState, "selectedAgent" | "sensitiveAcknowledged">): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch {
    // ignore quota / serialization errors
  }
}

const initial = loadFromStorage();

export const useCommitAgentPref = create<CommitAgentPrefState>((set, get) => ({
  ...initial,
  setSelectedAgent: (agent) => {
    set({ selectedAgent: agent });
    saveToStorage({ selectedAgent: agent, sensitiveAcknowledged: get().sensitiveAcknowledged });
  },
  clearSelectedAgent: () => {
    set({ selectedAgent: null });
    saveToStorage({ selectedAgent: null, sensitiveAcknowledged: get().sensitiveAcknowledged });
  },
  markSensitiveAcknowledged: () => {
    set({ sensitiveAcknowledged: true });
    saveToStorage({ selectedAgent: get().selectedAgent, sensitiveAcknowledged: true });
  },
}));
