/**
 * Typed IPC client: unwraps specta `Result` into data-or-thrown-error and
 * allows tests to swap the underlying command set (ipc-contract rule:
 * tests never touch the real Tauri runtime).
 */
import { commands as production, type IpcError, type Result } from "./bindings.gen";

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

/** Test-only: replace one or more commands with doubles. */
export function injectIpcCommands(overrides: Partial<Commands>): void {
  current = { ...production, ...overrides };
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
  createTask: (title: string, description: string) =>
    unwrap(current.createTask(title, description)),
  listTasks: (status: string | null) => unwrap(current.listTasks(status)),
  updateTaskStatus: (taskId: string, status: string) =>
    unwrap(current.updateTaskStatus(taskId, status)),
  getRun: (runId: string) => unwrap(current.getRun(runId)),
  listRunsByTask: (taskId: string) => unwrap(current.listRunsByTask(taskId)),
  listPendingApprovals: () => unwrap(current.listPendingApprovals()),
  resolveApproval: (approvalId: string, approved: boolean) =>
    unwrap(current.resolveApproval(approvalId, approved)),
  listDir: (path: string) => unwrap(current.listDir(path)),
  readFile: (path: string) => unwrap(current.readFile(path)),
  writeFile: (path: string, content: string) =>
    unwrap(current.writeFile(path, content)),
  gitStatus: () => unwrap(current.gitStatus()),
  gitLog: (limit: number) => unwrap(current.gitLog(limit)),
  gitStage: (paths: string[]) => unwrap(current.gitStage(paths)),
  gitCommit: (message: string) => unwrap(current.gitCommit(message)),
  gitPush: (remote: string, branch: string) => unwrap(current.gitPush(remote, branch)),
  gitWorktrees: () => unwrap(current.gitWorktrees()),
  getWorkspace: () => unwrap(current.getWorkspace()),
  setWorkspace: (path: string) => unwrap(current.setWorkspace(path)),
  createSchedule: (name: string, cronExpr: string, taskTitle: string, taskDescription: string) =>
    unwrap(current.createSchedule(name, cronExpr, taskTitle, taskDescription)),
  listSchedules: () => unwrap(current.listSchedules()),
  toggleSchedule: (scheduleId: string, enabled: boolean) =>
    unwrap(current.toggleSchedule(scheduleId, enabled)),
  deleteSchedule: (scheduleId: string) => unwrap(current.deleteSchedule(scheduleId)),
  upsertProvider: (provider: Parameters<Commands["upsertProvider"]>[0]) =>
    unwrap(current.upsertProvider(provider)),
  listProviders: () => unwrap(current.listProviders()),
  setSensitiveTools: (patterns: string[]) => unwrap(current.setSensitiveTools(patterns)),
  getSensitiveTools: () => unwrap(current.getSensitiveTools()),
  setOnlineAuthorized: (authorized: boolean) =>
    unwrap(current.setOnlineAuthorized(authorized)),
  getOnlineAuthorized: () => unwrap(current.getOnlineAuthorized()),
};

export type Ipc = typeof ipc;
