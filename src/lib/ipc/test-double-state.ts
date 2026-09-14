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
  RoleDto,
  RunDto,
  ScheduleDto,
  TaskDto,
  TeamDto,
  WhiteBoardNoteDto,
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
  /** Persisted capability-routing rules (get/setRoutingRules round-trip). */
  routingRules: { preferLocal: boolean; capabilityOverrides: Record<string, string> };
  /** App KV settings backing store (get/setAppSetting round-trip). */
  appSettings: Map<string, string>;
  /** Active workspace root returned by get/setWorkspace (files stay flat paths). */
  workspaceRoot: string;
  /** Whether the workspace has been configured (drives first-launch gating). */
  workspaceConfigured: boolean;
  /** Registered workspaces for the multi-workspace test double. */
  workspaces: import("./bindings.gen").WorkspaceEntryDto[];
  /** path → entry; dirs have content === null */
  files: Map<string, { isDir: boolean; size: number; content: string | null }>;
}

export const tdState: DoubleState = {
  sessions: [],
  events: new Map(),
  tasks: [],
  runs: [],
  approvals: [],
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
  routingRules: { preferLocal: false, capabilityOverrides: {} },
  appSettings: new Map(),
  workspaceRoot: "C:\\workspace",
  workspaceConfigured: true,
  workspaces: [],
  files: new Map(),
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
    agent: null,
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
    ...row,
  });
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

/** Wipe all double state between tests. */
export function tdReset(): void {
  tdState.sessions.length = 0;
  tdState.events.clear();
  tdState.tasks.length = 0;
  tdState.runs.length = 0;
  tdState.approvals.length = 0;
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
  tdState.routingRules = { preferLocal: false, capabilityOverrides: {} };
  tdState.appSettings.clear();
  tdState.workspaceRoot = "C:\\workspace";
  tdState.workspaceConfigured = true;
  tdState.workspaces.length = 0;
  tdState.files.clear();
}

/** Seed workspace entries: value is directory marker or file content. */
export function tdSeedFiles(entries: Record<string, string | null>): void {
  for (const [path, content] of Object.entries(entries)) {
    tdState.files.set(path, { isDir: content === null, size: content?.length ?? 0, content });
  }
}
