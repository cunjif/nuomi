/**
 * In-memory state for the IPC test double (see test-double-commands.ts for
 * the command implementations). Types come from the generated bindings so
 * drift fails typecheck.
 */
import type {
  AgentProfileDto,
  ApprovalDto,
  ConversationDto,
  EventDto,
  FileEntryDto,
  IntegrationDto,
  ProviderDto,
  RecentWorkspaceDto,
  RoleDto,
  RunDto,
  ScheduleDto,
  TaskDto,
  TeamDto,
  WhiteBoardNoteDto,
  EvolutionSettingsDto,
} from "./bindings.gen";

interface DoubleState {
  /**
   * Conversations, not bare sessions: a session *is* a conversation since
   * migration 0011, and `ConversationDto` is the superset both
   * `listSessions` and `listConversations` serve.
   */
  sessions: ConversationDto[];
  events: Map<string, EventDto[]>;
  tasks: TaskDto[];
  runs: RunDto[];
  approvals: ApprovalDto[];
  /** Pending team-plan proposals (P2-F 组 2) served by listProposals. */
  proposals: import("./bindings.gen").ProposalDto[];
  schedules: ScheduleDto[];
  providers: ProviderDto[];
  /** Catalog returned by listProviderModels (tests override per case). */
  providerModels: string[];
  agentProfiles: AgentProfileDto[];
  roles: RoleDto[];
  teams: TeamDto[];
  integrations: IntegrationDto[];
  whiteboardNotes: WhiteBoardNoteDto[];
  sensitiveTools: string[] | null;
  onlineAuthorized: boolean;
  evolutionSettings: EvolutionSettingsDto | null;
  /** Persisted capability-routing rules (get/setRoutingRules round-trip). */
  routingRules: { preferLocal: boolean; capabilityOverrides: Record<string, string> };
  /** Persisted role director self-binding (get/setRoleDirectorBinding round-trip). */
  roleDirectorBinding: import("./bindings.gen").RoleDirectorBindingDto | null;
  /** App KV settings backing store (get/setAppSetting round-trip). */
  appSettings: Map<string, string>;
  /** Active workspace root returned by get/setWorkspace (files stay flat paths). */
  workspaceRoot: string;
  /** Whether the workspace has been configured (drives first-launch gating). */
  workspaceConfigured: boolean;
  /** Registered workspaces for the multi-workspace test double. */
  workspaces: import("./bindings.gen").WorkspaceEntryDto[];
  /**
   * Explicit recent-workspace seeds for `getRecentWorkspaces`. `null` means
   * "derive from `workspaces`" (label from directory display names, mirroring
   * the Rust derivation); an array — including an empty one — is served as-is.
   */
  recentWorkspaces: RecentWorkspaceDto[] | null;
  /** Session-state active (focused) work directory of the open workspace. */
  activeDirectoryId: string | null;
  /** path → entry; dirs have content === null */
  files: Map<string, { isDir: boolean; size: number; content: string | null }>;
  /**
   * Ids of steward sessions (mirrors the `steward_sessions` table): their
   * authoritative marker. `listConversations`/`listSessions` exclude them,
   * matching the Rust repo layer's NOT EXISTS anti-join.
   */
  stewardSessionIds: Set<string>;
  /** Sandbox tasks (ADR-0022): newest first; tests mutate rows directly. */
  sandboxTasks: import("./bindings.gen").SandboxTaskDto[];
  /** Runtimes declared by the sandbox plugin double (tests override). */
  sandboxRuntimes: import("./bindings.gen").SandboxRuntimeDto[];
}

export const tdState: DoubleState = {
  sessions: [],
  events: new Map(),
  tasks: [],
  runs: [],
  approvals: [],
  proposals: [],
  schedules: [],
  providers: [],
  providerModels: ["gpt-4o", "gpt-4o-mini"],
  agentProfiles: [],
  roles: [],
  teams: [],
  integrations: [],
  whiteboardNotes: [],
  sensitiveTools: null,
  onlineAuthorized: false,
  evolutionSettings: null,
  routingRules: { preferLocal: false, capabilityOverrides: {} },
  roleDirectorBinding: null,
  appSettings: new Map(),
  workspaceRoot: "C:\\workspace",
  workspaceConfigured: true,
  workspaces: [],
  recentWorkspaces: null,
  activeDirectoryId: null,
  files: new Map(),
  stewardSessionIds: new Set(),
  sandboxTasks: [],
  sandboxRuntimes: [
    { id: "python", name: "Python 3", isolation: "test double (no isolation)" },
    { id: "node", name: "Node.js", isolation: "test double (no isolation)" },
  ],
};

let idCounter = 0;
export const nextId = (prefix: string): string => `${prefix}-${++idCounter}`;

/** Immediate-children listing over the seeded flat path map. */
export function listDir(path: string): FileEntryDto[] {
  const prefix = path === "" || path.endsWith("/") ? path : `${path}/`;
  const seen = new Set<string>();
  const out: FileEntryDto[] = [];
  for (const [p, entry] of tdState.files) {
    if (!p.startsWith(prefix) || p === prefix.replace(/\/$/, "")) continue;
    const rest = p.slice(prefix.length);
    const name = rest.split("/")[0] ?? "";
    if (!name || seen.has(name)) continue;
    seen.add(name);
    const isDir = rest.includes("/") || entry.isDir;
    out.push({ name, isDir, size: isDir ? 0 : entry.size });
  }
  return out.sort((a, b) => a.name.localeCompare(b.name));
}

/**
 * Seeds a conversation row. Omitted fields fall back to a plain chat session
 * so tests only spell out what they actually assert on.
 */
export function seedConversation(row: Partial<ConversationDto> & { id: string }): void {
  tdState.sessions.push({
    title: row.id,
    kind: "chat",
    teamId: null,
    taskId: null,
    scheduleId: null,
    createdAt: 1,
    updatedAt: 1,
    goal: null,
    mainAgentId: null,
    routeMode: null,
    whiteboardRouteMode: null,
    participantAgents: [],
    todoList: [],
    workspaceId: "__migrated__",
    ...row,
  });
}

/**
 * Seeds a steward conversation: a sessions row (kind 'background', mirroring
 * migration 0027) plus the authoritative `steward_sessions` marker. Steward
 * sessions are excluded from conversation listings, so components rendering
 * the IM session list can assert they never leak in.
 */
export function seedStewardSession(row: Partial<ConversationDto> & { id: string }): void {
  seedConversation({ kind: "background", title: row.id, ...row });
  tdState.stewardSessionIds.add(row.id);
}

/**
 * Seeds a Provider row. The conversation surface is gated on "at least one
 * Provider exists", so any test that renders it must seed one.
 */
export function seedProvider(row: Partial<ProviderDto> & { id: string }): void {
  tdState.providers.push({
    name: row.id,
    protocol: "open_ai_compatible",
    baseUrl: "https://example.invalid/v1",
    hasKey: true,
    capabilities: [],
    isMaster: true,
    settings: {},
    ...row,
  });
}

export function seedRole(row: Partial<RoleDto> & { id: string }): void {
  tdState.roles.push({
    name: row.id,
    providerId: null,
    providerIds: [],
    systemPromptOverride: null,
    toolAllowlist: [],
    requiredCapabilities: [],
    temperature: null,
    maxTokens: null,
    params: {},
    builtin: false,
    generated: false,
    ephemeral: false,
    source: null,
    createdAt: 0,
    updatedAt: 0,
    ...row,
  });
}

/** Wipe all double state between tests. */
export function tdReset(): void {
  tdState.sessions.length = 0;
  tdState.events.clear();
  tdState.tasks.length = 0;
  tdState.runs.length = 0;
  tdState.approvals.length = 0;
  tdState.proposals.length = 0;
  tdState.schedules.length = 0;
  tdState.providers.length = 0;
  tdState.providerModels = ["gpt-4o", "gpt-4o-mini"];
  tdState.agentProfiles.length = 0;
  tdState.roles.length = 0;
  tdState.teams.length = 0;
  tdState.integrations.length = 0;
  tdState.whiteboardNotes.length = 0;
  tdState.sensitiveTools = null;
  tdState.onlineAuthorized = false;
  tdState.evolutionSettings = null;
  tdState.routingRules = { preferLocal: false, capabilityOverrides: {} };
  tdState.roleDirectorBinding = null;
  tdState.appSettings.clear();
  tdState.workspaceRoot = "C:\\workspace";
  tdState.workspaceConfigured = true;
  tdState.workspaces.length = 0;
  tdState.recentWorkspaces = null;
  tdState.activeDirectoryId = null;
  tdState.files.clear();
  tdState.stewardSessionIds.clear();
  tdState.sandboxTasks.length = 0;
}

/** Seed workspace entries: value is directory marker or file content. */
export function tdSeedFiles(entries: Record<string, string | null>): void {
  for (const [path, content] of Object.entries(entries)) {
    tdState.files.set(path, { isDir: content === null, size: content?.length ?? 0, content });
  }
}
