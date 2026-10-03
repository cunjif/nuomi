/**
 * Typed IPC client: unwraps specta `Result` into data-or-thrown-error and
 * allows tests to swap the underlying command set (ipc-contract rule:
 * tests never touch the real Tauri runtime).
 */
import {
  commands as production,
  type AgentDetailDto,
  type AgentOptionDto,
  type AgentRefDto,
  type AgentRefInput,
  type AsrModelDto,
  type AttachmentDto,
  type CloseWorkspaceResult,
  type ContextInjectionDto,
  type ContextInjectionInput,
  type ConversationDto,
  type ConversationInput,
  type ConversationUpdateInput,
  type CrossSearchOutcomeDto,
  type DiffResultDto,
  type EventDto,
  type FileReferenceDto,
  type FocusWorkspaceResult,
  type InjectableRuleDto,
  type InjectableSessionDto,
  type IpcError,
  type IsolationViolationDto,
  type JsonValue,
  type LayoutSnapshotDto,
  type OpenSetDto,
  type OpenWorkspaceResult,
  type OrphanSessionDto,
  type RecentWorkspaceDto,
  type RemoveWorkspaceResult,
  type Result,
  type RunDto,
  type ScheduleDto,
  type ScheduleInput,
  type TodoItemDto,
  type WorkspaceEntryDto,
} from "./bindings.gen";

/** Structured IPC failure — `code` is stable and maps to i18n keys. */
export class IpcCommandError extends Error {
  readonly code: string;
  readonly details?: unknown;
  constructor(code: string, message: string, details?: unknown) {
    super(message);
    this.name = "IpcCommandError";
    this.code = code;
    this.details = details;
  }
}

type Commands = typeof production;

function unwrap<T>(pending: Promise<Result<T, IpcError>>): Promise<T> {
  return pending.then((r) => {
    if (r.status === "ok") return r.data;
    const e = r.error.generic;
    throw new IpcCommandError(e.code, e.message, e.details);
  });
}

let current: Commands = production;

/** Test-only: replace one or more commands with doubles. Merges over the
 * current set so stacked injections keep earlier doubles intact. */
export function injectIpcCommands(overrides: Partial<Commands>): void {
  current = { ...current, ...overrides };
}

/** Test-only: restore the production command set. */
export function resetIpcCommands(): void {
  current = production;
}

export const ipc = {
  createSession: () => unwrap(current.createSession()),
  listSessions: () => unwrap(current.listSessions()),
  resumeSession: (sessionId: string) => unwrap(current.resumeSession(sessionId)),
  listEvents: (sessionId: string, afterSeq: number) =>
    unwrap(current.listEvents(sessionId, afterSeq)),
  submitTask: (sessionId: string, input: string) =>
    unwrap(current.submitTask(sessionId, input)),
  enqueueMessage: (sessionId: string, input: string) =>
    unwrap(current.enqueueMessage(sessionId, input)),
  listMessageQueue: (sessionId: string) =>
    unwrap(current.listMessageQueue(sessionId)),
  cancelMessageQueueItem: (id: string) =>
    unwrap(current.cancelMessageQueueItem(id)),
  clearMessageQueue: (sessionId: string) =>
    unwrap(current.clearMessageQueue(sessionId)),
  createTask: (title: string, description: string) =>
    unwrap(current.createTask(title, description)),
  listTasks: (status: string | null, workspaceId: string | null) => unwrap(current.listTasks(status, workspaceId)),
  updateTaskStatus: (taskId: string, status: string) =>
    unwrap(current.updateTaskStatus(taskId, status)),
  deleteTask: (taskId: string) => unwrap(current.deleteTask(taskId)),
  getRun: (runId: string) => unwrap(current.getRun(runId)),
  listRunsByTask: (taskId: string) => unwrap(current.listRunsByTask(taskId)),
  listPendingApprovals: (workspaceId: string | null) => unwrap(current.listPendingApprovals(workspaceId)),
  resolveApproval: (approvalId: string, approved: boolean) =>
    unwrap(current.resolveApproval(approvalId, approved)),
  listDir: (path: string) => unwrap(current.listDir(path)),
  readFile: (path: string) => unwrap(current.readFile(path)),
  writeFile: (path: string, content: string) =>
    unwrap(current.writeFile(path, content)),
  createFile: (path: string, content: string) =>
    unwrap(current.createFile(path, content)),
  createDir: (path: string) => unwrap(current.createDir(path)),
  deletePath: (path: string) => unwrap(current.delete(path)),
  renamePath: (from: string, to: string) => unwrap(current.rename(from, to)),
  copyPath: (from: string, to: string) => unwrap(current.copy(from, to)),
  gitStatus: () => unwrap(current.gitStatus()),
  gitLog: (limit: number) => unwrap(current.gitLog(limit)),
  gitStage: (paths: string[]) => unwrap(current.gitStage(paths)),
  gitCommit: (message: string) => unwrap(current.gitCommit(message)),
  gitPush: (remote: string, branch: string) => unwrap(current.gitPush(remote, branch)),
  gitWorktrees: () => unwrap(current.gitWorktrees()),
  gitDiff: (path: string, staged: boolean) => unwrap(current.gitDiff(path, staged)),
  gitStagedDiff: () => unwrap(current.gitStagedDiff()),
  listCommitAgents: () => unwrap(current.listCommitAgents()),
  aiCommitGenerate: (roleAgent?: { kind: string; id: string }) =>
    unwrap(current.aiCommitGenerate(roleAgent ?? null)),
  getWorkspace: () => unwrap(current.getWorkspace()),
  setWorkspace: (path: string) => unwrap(current.setWorkspace(path)),
  createSchedule: (name: string, cronExpr: string, taskTitle: string, taskDescription: string) =>
    unwrap(current.createSchedule(name, cronExpr, taskTitle, taskDescription)),
  listSchedules: (workspaceId: string | null) => unwrap(current.listSchedules(workspaceId)),
  toggleSchedule: (scheduleId: string, enabled: boolean) =>
    unwrap(current.toggleSchedule(scheduleId, enabled)),
  deleteSchedule: (scheduleId: string) => unwrap(current.deleteSchedule(scheduleId)),
  upsertProvider: (provider: Parameters<Commands["upsertProvider"]>[0]) =>
    unwrap(current.upsertProvider(provider)),
  listProviders: () => unwrap(current.listProviders()),
  deleteProvider: (providerId: string, force: boolean = false) =>
    unwrap(current.deleteProvider(providerId, force)),
  testProviderConnection: (input: Parameters<Commands["testProviderConnection"]>[0]) =>
    unwrap(current.testProviderConnection(input)),
  listProviderModels: (input: Parameters<Commands["listProviderModels"]>[0]) =>
    unwrap(current.listProviderModels(input)),
  setSensitiveTools: (patterns: string[]) => unwrap(current.setSensitiveTools(patterns)),
  getSensitiveTools: () => unwrap(current.getSensitiveTools()),
  setOnlineAuthorized: (authorized: boolean) =>
    unwrap(current.setOnlineAuthorized(authorized)),
  getOnlineAuthorized: () => unwrap(current.getOnlineAuthorized()),
  listAgentProfiles: () => unwrap(current.listAgentProfiles()),
  upsertAgentProfile: (profile: Parameters<Commands["upsertAgentProfile"]>[0]) =>
    unwrap(current.upsertAgentProfile(profile)),
  deleteAgentProfile: (profileId: string) =>
    unwrap(current.deleteAgentProfile(profileId)),
  checkCliAgent: (profileId: string) => unwrap(current.checkCliAgent(profileId)),
  listRoles: () => unwrap(current.listRoles()),
  upsertRole: (role: Parameters<Commands["upsertRole"]>[0]) => unwrap(current.upsertRole(role)),
  deleteRole: (roleId: string, force: boolean = false) =>
    unwrap(current.deleteRole(roleId, force)),
  seedBuiltinRoles: () => unwrap(current.seedBuiltinRoles()),
  generateRole: (
    description: string,
    binding: Parameters<Commands["generateRole"]>[1],
  ) => unwrap(current.generateRole(description, binding)),
  getRoleDirectorBinding: () => unwrap(current.getRoleDirectorBinding()),
  setRoleDirectorBinding: (
    binding: Parameters<Commands["setRoleDirectorBinding"]>[0],
  ) => unwrap(current.setRoleDirectorBinding(binding)),
  getRoutingRules: () => unwrap(current.getRoutingRules()),
  setRoutingRules: (rules: Parameters<Commands["setRoutingRules"]>[0]) =>
    unwrap(current.setRoutingRules(rules)),
  routeCapability: (request: Parameters<Commands["routeCapability"]>[0]) =>
    unwrap(current.routeCapability(request)),
  listTeams: () => unwrap(current.listTeams()),
  upsertTeam: (team: Parameters<Commands["upsertTeam"]>[0]) => unwrap(current.upsertTeam(team)),
  deleteTeam: (teamId: string) => unwrap(current.deleteTeam(teamId)),
  listWhiteboardNotes: (sessionId: string) => unwrap(current.listWhiteboardNotes(sessionId)),
  formTeam: (task: string, sessionId: string | null) => unwrap(current.formTeam(task, sessionId)),
  previewTeam: (task: string) => unwrap(current.previewTeam(task)),
  runTeamOnTask: (taskId: string, teamId: string) => unwrap(current.runTeamOnTask(taskId, teamId)),
  runTeamSession: (sessionId: string, teamId: string, task: string) =>
    unwrap(current.runTeamSession(sessionId, teamId, task)),
  listIntegrations: () => unwrap(current.listIntegrations()),
  upsertIntegration: (input: Parameters<Commands["upsertIntegration"]>[0]) =>
    unwrap(current.upsertIntegration(input)),
  deleteIntegration: (integrationId: string) =>
    unwrap(current.deleteIntegration(integrationId)),
  testIntegration: (integrationId: string) =>
    unwrap(current.testIntegration(integrationId)),
  pluginList: () => unwrap(current.pluginList()),
  pluginInstallFromPath: (path: string) => unwrap(current.pluginInstallFromPath(path)),
  pluginUninstall: (pluginId: string) => unwrap(current.pluginUninstall(pluginId)),
  pluginOpenDir: () => unwrap(current.pluginOpenDir()),
  pluginEditorCall: (pluginId: string, method: string, params: JsonValue) =>
    unwrap(current.pluginEditorCall(pluginId, method, params)),
  appSettingGet: (key: string) => unwrap(current.appSettingGet(key)),
  appSettingSet: (key: string, value: string) => unwrap(current.appSettingSet(key, value)),
  getViewScope: (surface: string) => unwrap(current.getViewScope(surface)),
  setViewScope: (surface: string, scope: string) => unwrap(current.setViewScope(surface, scope)),
  createConversation: (input: ConversationInput) => unwrap(current.createConversation(input)),
  listConversations: (kind: string | null, workspaceId: string | null) =>
    unwrap(current.listConversations(kind, workspaceId)),
  getConversation: (sessionId: string) => unwrap(current.getConversation(sessionId)),
  setConversationAgent: (sessionId: string, agent: AgentRefInput | null) =>
    unwrap(current.setConversationAgent(sessionId, agent)),
  listAgentOptions: () => unwrap(current.listAgentOptions()),
  updateConversation: (sessionId: string, input: ConversationUpdateInput) =>
    unwrap(current.updateConversation(sessionId, input)),
  addConversationAgent: (sessionId: string, agent: AgentRefInput) =>
    unwrap(current.addConversationAgent(sessionId, agent)),
  removeConversationAgent: (sessionId: string, agent: AgentRefInput) =>
    unwrap(current.removeConversationAgent(sessionId, agent)),
  deleteConversation: (sessionId: string) =>
    unwrap(current.deleteConversation(sessionId)),
  clearConversations: (workspaceId: string | null) =>
    unwrap(current.clearConversations(workspaceId)),
  getAgentDetail: (agentKind: string, agentId: string) =>
    unwrap(current.getAgentDetail(agentKind, agentId)),
  submitMessage: (
    sessionId: string,
    text: string,
    attachmentIds: string[],
    routeTargetAgentIds?: string[] | null,
    contextInjectionIds?: string[] | null,
  ) =>
    unwrap(
      current.submitMessage(
        sessionId,
        text,
        attachmentIds,
        routeTargetAgentIds ?? null,
        contextInjectionIds ?? null,
      ),
    ),
  stopConversation: (sessionId: string) => unwrap(current.stopConversation(sessionId)),
  listActiveRuns: () => unwrap(current.listActiveRuns()),
  cancelRun: (runId: string) => unwrap(current.cancelRun(runId)),
  saveAttachment: (sessionId: string, name: string, mime: string, dataBase64: string) =>
    unwrap(current.saveAttachment(sessionId, name, mime, dataBase64)),
  listAttachments: (sessionId: string) => unwrap(current.listAttachments(sessionId)),
  deleteAttachment: (attachmentId: string) => unwrap(current.deleteAttachment(attachmentId)),
  upsertSchedule: (input: ScheduleInput) => unwrap(current.upsertSchedule(input)),
  updateSchedule: (scheduleId: string, input: ScheduleInput) =>
    unwrap(current.updateSchedule(scheduleId, input)),
  injectContext: (sessionId: string, input: ContextInjectionInput) =>
    unwrap(current.injectContext(sessionId, input)),
  listInjectableSessions: () => unwrap(current.listInjectableSessions()),
  listInjectableRules: () => unwrap(current.listInjectableRules()),
  transcribeAudio: (audioBase64: string, modelSource?: string | null) =>
    unwrap(current.transcribeAudio(audioBase64, modelSource ?? null)),
  listAsrModels: () => unwrap(current.listAsrModels()),
  listWorkspaces: () => unwrap(current.listWorkspaces()),
  addWorkspace: (path: string) => unwrap(current.addWorkspace(path)),
  removeWorkspace: (id: string) => unwrap(current.removeWorkspace(id)),
  activateWorkspace: (id: string) => unwrap(current.activateWorkspace(id)),
  getActiveWorkspace: () => unwrap(current.getActiveWorkspace()),
  listOrphanSessions: () => unwrap(current.listOrphanSessions()),
  reclaimOrphanSessions: (workspaceId: string) =>
    unwrap(current.reclaimOrphanSessions(workspaceId)),
  openWorkspace: (id: string) => unwrap(current.openWorkspace(id)),
  closeWorkspace: (id: string, force: boolean) => unwrap(current.closeWorkspace(id, force)),
  focusWorkspace: (id: string) => unwrap(current.focusWorkspace(id)),
  closeAllWorkspaces: (excludePinned: boolean) =>
    unwrap(current.closeAllWorkspaces(excludePinned)),
  getOpenSet: () => unwrap(current.getOpenSet()),
  pinWorkspace: (id: string) => unwrap(current.pinWorkspace(id)),
  unpinWorkspace: (id: string) => unwrap(current.unpinWorkspace(id)),
  getLayoutSnapshot: () => unwrap(current.getLayoutSnapshot()),
  setLayoutSnapshot: (mode: string, splitWorkspaceIds: [string, string] | null) =>
    unwrap(current.setLayoutSnapshot(mode, splitWorkspaceIds)),
  getRecentWorkspaces: (limit: number) => unwrap(current.getRecentWorkspaces(limit)),
  crossWorkspaceSearch: (query: string, matchContent: boolean) =>
    unwrap(current.crossWorkspaceSearch(query, matchContent)),
  crossWorkspaceReference: (sourceWorkspaceId: string, filePath: string) =>
    unwrap(current.crossWorkspaceReference(sourceWorkspaceId, filePath)),
  crossWorkspaceCompare: (
    workspaceA: string,
    fileA: string,
    workspaceB: string,
    fileB: string,
  ) => unwrap(current.crossWorkspaceCompare(workspaceA, fileA, workspaceB, fileB)),
  detectIsolationViolations: () => unwrap(current.detectIsolationViolations()),
};

export type Ipc = typeof ipc;
export type {
  AgentDetailDto,
  AgentOptionDto,
  AgentRefDto,
  AgentRefInput,
  AsrModelDto,
  AttachmentDto,
  CloseWorkspaceResult,
  ContextInjectionDto,
  ContextInjectionInput,
  ConversationDto,
  ConversationInput,
  ConversationUpdateInput,
  CrossSearchOutcomeDto,
  DiffResultDto,
  EventDto,
  FileReferenceDto,
  FocusWorkspaceResult,
  InjectableRuleDto,
  InjectableSessionDto,
  IsolationViolationDto,
  LayoutSnapshotDto,
  OpenSetDto,
  OpenWorkspaceResult,
  OrphanSessionDto,
  RecentWorkspaceDto,
  RunDto,
  ScheduleDto,
  ScheduleInput,
  TodoItemDto,
  WorkspaceEntryDto,
  RemoveWorkspaceResult,
};
