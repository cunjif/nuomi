import type { CapabilityDto } from "../../lib/ipc/bindings.gen";

export interface HyperParams {
  temperature: number;
  topP: number;
  maxTokens: number;
}

/**
 * Static recommendation rules mapping capability tags to hyperparams.
 * Zero token consumption — pure heuristic based on task type.
 * Priority: reasoning > image > voice > video.
 */
export function recommendHyperparams(capabilities: CapabilityDto[]): HyperParams {
  if (capabilities.includes("reasoning")) {
    return { temperature: 0.7, topP: 0.95, maxTokens: 8192 };
  }
  if (capabilities.includes("image")) {
    return { temperature: 0.8, topP: 1.0, maxTokens: 4096 };
  }
  if (capabilities.includes("voice")) {
    return { temperature: 0.5, topP: 0.9, maxTokens: 2048 };
  }
  if (capabilities.includes("video")) {
    return { temperature: 0.5, topP: 0.9, maxTokens: 2048 };
  }
  return { temperature: 0.7, topP: 0.95, maxTokens: 8192 };
}
