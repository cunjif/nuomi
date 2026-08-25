/**
 * In-memory command implementations for the IPC test double
 * (ipc-contract rule: tests never touch the real Tauri runtime). Types are
 * imported from the generated bindings so drift fails typecheck.
 */
import type {
  ProviderInput,
  Result,
  ScheduleDto,
  SessionDto,
  TaskDto,
} from "./bindings.gen";
import type { commands as Commands } from "./bindings.gen";
import { listDir, nextId, tdState } from "./test-double-state";

type CommandSet = typeof Commands;

const ok = <T>(data: T): Result<T, never> => ({ status: "ok", data });
const err = (code: string, message: string): Result<never, { generic: { code: string; message: string } }> => ({
  status: "error",
  error: { generic: { code, message } },
});

export function testDoubleCommands(): CommandSet {
  const cmds: CommandSet = {
    async createSession() {
      const now = Date.now();
      const s: SessionDto = { id: nextId("s"), title: `Session ${tdState.sessions.length + 1}`, createdAt: now, updatedAt: now };
      tdState.sessions.unshift(s);
      return ok(s);
    },
    async listSessions() {
      return ok([...tdState.sessions]);
    },
    async resumeSession(sessionId) {
      return tdState.sessions.some((s) => s.id === sessionId)
        ? ok(null)
        : err("store.not_found", `session#${sessionId} not found`);
    },
    async listEvents(sessionId, afterSeq) {
      const all = tdState.events.get(sessionId) ?? [];
      return ok(all.filter((e) => e.seq > afterSeq));
    },
    async submitTask(sessionId, input) {
      const events = tdState.events.get(sessionId) ?? [];
      const seq = (events.at(-1)?.seq ?? 0) + 1;
      events.push({ seq, kind: "message", payload: { role: "user", content: input }, createdAt: Date.now() });
      tdState.events.set(sessionId, events);
      return ok({ finalText: "", steps: 0, truncated: false, sessionId });
    },
    async createTask(title, description) {
      const t: TaskDto = {
        id: nextId("t"),
        sessionId: null,
        title,
        description,
        status: "backlog",
        createdAt: Date.now(),
        updatedAt: Date.now(),
      };
      tdState.tasks.unshift(t);
      return ok(t);
    },
    async listTasks(status) {
      return ok(tdState.tasks.filter((t) => status === null || t.status === status));
    },
    async updateTaskStatus(taskId, status) {
      const t = tdState.tasks.find((x) => x.id === taskId);
      if (!t) return err("store.not_found", `task#${taskId} not found`);
      t.status = status;
      t.updatedAt = Date.now();
      return ok(null);
    },
    async getRun(runId) {
      const r = tdState.runs.find((x) => x.id === runId);
      return r ? ok(r) : err("store.not_found", `run#${runId} not found`);
    },
    async listRunsByTask(taskId) {
      return ok(tdState.runs.filter((r) => r.taskId === taskId));
    },
    async listPendingApprovals() {
      return ok([...tdState.approvals]);
    },
    async resolveApproval(approvalId, approved) {
      const idx = tdState.approvals.findIndex((a) => a.id === approvalId);
      if (idx < 0) return err("store.not_found", `approval#${approvalId} not found`);
      if (!approved) tdState.approvals.splice(idx, 1);
      else tdState.approvals.splice(idx, 1); // approved items leave the inbox too
      return ok(null);
    },
    async listDir(path) {
      return ok(listDir(path));
    },
    async readFile(path) {
      const f = tdState.files.get(path);
      return f && !f.isDir ? ok(f.content ?? "") : err("workspace.invalid_path", `not a file: ${path}`);
    },
    async writeFile(path, content) {
      const f = tdState.files.get(path);
      if (!f || f.isDir) return err("workspace.invalid_path", `not a file: ${path}`);
      f.content = content;
      f.size = content.length;
      return ok(null);
    },
    async gitStatus() {
      return ok([]);
    },
    async gitLog(_limit) {
      return ok([]);
    },
    async gitStage(_paths) {
      return ok(null);
    },
    async gitCommit(_message) {
      return ok("deadbeef");
    },
    async gitPush(_remote, _branch) {
      return ok("pushed");
    },
    async gitWorktrees() {
      return ok([]);
    },
    async getWorkspace() {
      return ok("C:\\workspace");
    },
    async setWorkspace(_path: string) {
      return ok("C:\\workspace");
    },
    async createSchedule(name, cronExpr, taskTitle, _taskDescription) {
      const s: ScheduleDto = {
        id: nextId("sched"),
        name,
        cronExpr,
        taskTitle,
        enabled: true,
        nextTriggerAt: null,
      };
      tdState.schedules.unshift(s);
      return ok(s);
    },
    async listSchedules() {
      return ok([...tdState.schedules]);
    },
    async toggleSchedule(scheduleId, enabled) {
      const s = tdState.schedules.find((x) => x.id === scheduleId);
      if (!s) return err("store.not_found", `schedule#${scheduleId} not found`);
      s.enabled = enabled;
      return ok(null);
    },
    async deleteSchedule(scheduleId) {
      const idx = tdState.schedules.findIndex((x) => x.id === scheduleId);
      if (idx < 0) return err("store.not_found", `schedule#${scheduleId} not found`);
      tdState.schedules.splice(idx, 1);
      return ok(null);
    },
    async upsertProvider(provider: ProviderInput) {
      const existing = tdState.providers.find((p) => p.name === provider.name);
      if (existing) {
        existing.protocol = provider.protocol;
        existing.baseUrl = provider.baseUrl;
        existing.capabilities = [...provider.capabilities];
        existing.isMaster = provider.isMaster;
        if (provider.apiKey !== null) existing.hasKey = true;
      } else {
        tdState.providers.push({
          id: nextId("prov"),
          name: provider.name,
          protocol: provider.protocol,
          baseUrl: provider.baseUrl,
          hasKey: provider.apiKey !== null,
          capabilities: [...provider.capabilities],
          isMaster: provider.isMaster,
        });
      }
      return ok(null);
    },
    async listProviders() {
      return ok([...tdState.providers]);
    },
    async setSensitiveTools(patterns) {
      tdState.sensitiveTools = [...patterns];
      return ok(null);
    },
    async getSensitiveTools() {
      return ok(tdState.sensitiveTools === null ? null : [...tdState.sensitiveTools]);
    },
    async setOnlineAuthorized(authorized) {
      tdState.onlineAuthorized = authorized;
      return ok(null);
    },
    async getOnlineAuthorized() {
      return ok(tdState.onlineAuthorized);
    },
  };
  return cmds;
}
