/**
 * In-memory state for the IPC test double (see test-double-commands.ts for
 * the command implementations). Types come from the generated bindings so
 * drift fails typecheck.
 */
import type {
  AgentProfileDto,
  ApprovalDto,
  EventDto,
  FileEntryDto,
  IntegrationDto,
  ProviderDto,
  RoleDto,
  ScheduleDto,
  SessionDto,
  TaskDto,
  TeamDto,
  WhiteBoardNoteDto,
} from "./bindings.gen";

interface DoubleState {
  sessions: SessionDto[];
  events: Map<string, EventDto[]>;
  tasks: TaskDto[];
  runs: Array<{ id: string; taskId: string; sessionId: string; status: string; heartbeatAt: number }>;
  approvals: ApprovalDto[];
  schedules: ScheduleDto[];
  providers: ProviderDto[];
  agentProfiles: AgentProfileDto[];
  roles: RoleDto[];
  teams: TeamDto[];
  integrations: IntegrationDto[];
  whiteboardNotes: WhiteBoardNoteDto[];
  sensitiveTools: string[] | null;
  onlineAuthorized: boolean;
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
  agentProfiles: [],
  roles: [],
  teams: [],
  integrations: [],
  whiteboardNotes: [],
  sensitiveTools: null,
  onlineAuthorized: false,
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

/** Wipe all double state between tests. */
export function tdReset(): void {
  tdState.sessions.length = 0;
  tdState.events.clear();
  tdState.tasks.length = 0;
  tdState.runs.length = 0;
  tdState.approvals.length = 0;
  tdState.schedules.length = 0;
  tdState.providers.length = 0;
  tdState.agentProfiles.length = 0;
  tdState.roles.length = 0;
  tdState.teams.length = 0;
  tdState.integrations.length = 0;
  tdState.whiteboardNotes.length = 0;
  tdState.sensitiveTools = null;
  tdState.onlineAuthorized = false;
  tdState.files.clear();
}

/** Seed workspace entries: value is directory marker or file content. */
export function tdSeedFiles(entries: Record<string, string | null>): void {
  for (const [path, content] of Object.entries(entries)) {
    tdState.files.set(path, { isDir: content === null, size: content?.length ?? 0, content });
  }
}
