/**
 * Agent display helpers: flavor labels, provider names, color tokens.
 * The backend resolves agent names; these are pure presentation utilities.
 * ADR 0013: ConversationDto.agent 移除，改用 participantAgents[0]。
 */
import type { AgentOptionDto, AgentRefDto } from "../ipc/client";

/** Human-readable flavor label for CLI agents. */
export function flavorLabel(flavor: string): string {
  switch (flavor) {
    case "claude_code":
      return "Claude Code";
    case "codex":
      return "Codex";
    default:
      return flavor;
  }
}

/** Short display name for an agent ref on a conversation. */
export function agentDisplayName(agent: AgentRefDto | null): string {
  if (!agent) return "Default";
  return agent.name || `${agent.kind}:${agent.id}`;
}

/** True when the agent is a CLI agent (vs a role). */
export function isCliAgent(agent: AgentRefDto | null): boolean {
  return agent?.kind === "cli";
}

/** Group agent options by kind for the picker popover. */
export function groupAgentOptions(options: AgentOptionDto[]): {
  cli: AgentOptionDto[];
  role: AgentOptionDto[];
} {
  return {
    cli: options.filter((o) => o.kind === "cli"),
    role: options.filter((o) => o.kind === "role"),
  };
}
