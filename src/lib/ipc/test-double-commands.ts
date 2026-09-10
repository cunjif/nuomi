/**
 * In-memory command implementations for the IPC test double
 * (ipc-contract rule: tests never touch the real Tauri runtime). Types are
 * imported from the generated bindings so drift fails typecheck.
 */
import type {
  AgentProfileInput,
  IpcError,
  IntegrationDto,
  IntegrationInput,
  JsonValue,
  PluginInfoDto,
  PluginListResultDto,
  ProviderInput,
  Result,
  RoleDto,
  RoleInput,
  ScheduleDto,
  SessionDto,
  TaskDto,
  TeamDto,
  TeamInput,
  TeamPlanDto,
} from "./bindings.gen";
import type { commands as Commands } from "./bindings.gen";
import { listDir, nextId, tdState } from "./test-double-state";

type CommandSet = typeof Commands;

/** Preset role names mirrored from the backend catalog (deterministic double). */
const PRESET_ROLE_NAMES = [
  "Coder",
  "Code Reviewer",
  "Planner",
  "Docs Writer",
  "Test Engineer",
  "Data Analyst",
  "Translator",
  "Ops Rescuer",
  "Researcher",
  "Creative Writer",
  "Role Director",
] as const;

const ok = <T>(data: T): Result<T, never> => ({ status: "ok", data });
const err = (code: string, message: string): Result<never, { generic: { code: string; message: string } }> => ({
  status: "error",
  error: { generic: { code, message } },
});
const errWithDetails = (code: string, message: string, details: JsonValue): Result<never, IpcError> => ({
  status: "error",
  error: { generic: { code, message, details } },
});

/** Preset member roles every auto-formed team binds (deterministic). */
const AUTO_FORM_ROLE_IDS = ["role-auto-planner", "role-auto-worker"] as const;

/** Fixed plan copy for the preview double (打磨③b: deterministic dry-run). */
const PREVIEW_RATIONALE = "规划器选择一个 CLI 规划成员与既有 worker Role 组成群聊团队";

/** Deterministic slug: keep letters/digits (CJK included), collapse the rest to `-`. */
const slug = (text: string): string =>
  text
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, "-")
    .replace(/^-+|-+$/g, "");

/** Webhook URLs are write-only: DTOs carry protocol+host+last-4 chars only (SPEC D5). */
function maskWebhookUrl(raw: string): string {
  const tail = raw.slice(-4);
  try {
    return `${new URL(raw).origin}/***${tail}`;
  } catch {
    return `***${tail}`;
  }
}

/** Only absolute http(s) URLs are valid webhook endpoints. */
function isValidWebhookUrl(raw: string): boolean {
  try {
    const url = new URL(raw);
    return url.protocol === "http:" || url.protocol === "https:";
  } catch {
    return false;
  }
}

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
    async deleteTask(taskId) {
      const idx = tdState.tasks.findIndex((t) => t.id === taskId);
      if (idx < 0) return err("task.not_found", `task#${taskId} not found`);
      const removed = tdState.tasks[idx];
      if (removed && removed.status === "running") {
        return err("task.invalid_status", `task#${taskId} is running; cancel it before deleting`);
      }
      tdState.tasks.splice(idx, 1);
      // Mirror the backend cascade: approvals → runs → task.
      const runIds = new Set(tdState.runs.filter((r) => r.taskId === taskId).map((r) => r.id));
      tdState.runs = tdState.runs.filter((r) => r.taskId !== taskId);
      tdState.approvals = tdState.approvals.filter((a) => !runIds.has(a.runId));
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
      return ok({ root: tdState.workspaceRoot, configured: tdState.workspaceConfigured });
    },
    async setWorkspace(path: string) {
      tdState.workspaceRoot = path;
      tdState.workspaceConfigured = true;
      return ok({ root: tdState.workspaceRoot, configured: true });
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
      const settings = {
        models: (provider.settings.models ?? []).map((m) => ({
          id: m.id,
          capabilities: [...m.capabilities],
        })),
        defaultModel: provider.settings.defaultModel ?? null,
        temperature: provider.settings.temperature ?? null,
        topP: provider.settings.topP ?? null,
        maxTokens: provider.settings.maxTokens ?? null,
        timeoutSecs: provider.settings.timeoutSecs ?? null,
        retry: provider.settings.retry ?? null,
        maxConcurrency: provider.settings.maxConcurrency ?? null,
        priority: provider.settings.priority ?? null,
        roles: [...(provider.settings.roles ?? [])],
        enabled: provider.settings.enabled ?? true,
      };
      const existing = provider.id !== null
        ? tdState.providers.find((p) => p.id === provider.id)
        : tdState.providers.find((p) => p.name === provider.name);
      if (existing) {
        existing.name = provider.name;
        existing.protocol = provider.protocol;
        existing.baseUrl = provider.baseUrl;
        existing.capabilities = [...provider.capabilities];
        existing.isMaster = provider.isMaster;
        if (provider.apiKey !== null) existing.hasKey = true;
        existing.settings = settings;
      } else {
        tdState.providers.push({
          id: provider.id ?? nextId("prov"),
          name: provider.name,
          protocol: provider.protocol,
          baseUrl: provider.baseUrl,
          hasKey: provider.apiKey !== null,
          capabilities: [...provider.capabilities],
          isMaster: provider.isMaster,
          settings,
        });
      }
      return ok(null);
    },
    async listProviders() {
      return ok([...tdState.providers]);
    },
    async deleteProvider(providerId) {
      const idx = tdState.providers.findIndex((p) => p.id === providerId);
      if (idx < 0) return err("provider.not_found", `provider#${providerId} not found`);
      tdState.providers.splice(idx, 1);
      return ok(null);
    },
    async testProviderConnection(input) {
      if (!isValidWebhookUrl(input.baseUrl)) {
        return ok({ ok: false, latencyMs: 0, error: "invalid base url" });
      }
      return ok({ ok: true, latencyMs: 42, error: null });
    },
    async listProviderModels() {
      // Test double: a fixed catalog; tests override it to drive the picker.
      return ok({ models: [...tdState.providerModels], error: null });
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
    async listAgentProfiles() {
      return ok([...tdState.agentProfiles]);
    },
    async upsertAgentProfile(profile: AgentProfileInput) {
      const now = Date.now();
      // `name` is the idempotency key: an existing profile is updated in place.
      const existing = tdState.agentProfiles.find((p) => p.name === profile.name);
      if (existing) {
        existing.flavor = profile.flavor;
        existing.command = profile.command;
        existing.args = [...profile.args];
        existing.env = { ...profile.env };
        existing.workingDir = profile.workingDir;
        existing.enabled = profile.enabled;
        existing.updatedAt = now;
        return ok({ ...existing });
      }
      const created = {
        id: nextId("agent"),
        name: profile.name,
        adapter: "cli",
        flavor: profile.flavor,
        command: profile.command,
        args: [...profile.args],
        env: { ...profile.env },
        workingDir: profile.workingDir,
        enabled: profile.enabled,
        createdAt: now,
        updatedAt: now,
      };
      tdState.agentProfiles.push(created);
      return ok({ ...created });
    },
    async deleteAgentProfile(profileId) {
      const idx = tdState.agentProfiles.findIndex((p) => p.id === profileId);
      if (idx < 0) return err("store.not_found", `agentProfile#${profileId} not found`);
      tdState.agentProfiles.splice(idx, 1);
      return ok(null);
    },
    async checkCliAgent(_profileId) {
      return ok({ ok: true, versionLine: "fake-cli 1.0.0", error: null });
    },
    async listRoles() {
      return ok([...tdState.roles]);
    },
    async upsertRole(role: RoleInput) {
      if (role.name.trim().length === 0) {
        return err("role.invalid", "role name must not be empty");
      }
      const now = Date.now();
      // `name` is the idempotency key: an existing role is updated in place.
      const existing = tdState.roles.find((r) => r.name === role.name);
      if (existing) {
        existing.providerId = role.providerId;
        existing.providerIds = [...(role.providerIds ?? [])];
        existing.systemPromptOverride = role.systemPromptOverride;
        existing.toolAllowlist = [...role.toolAllowlist];
        existing.requiredCapabilities = [...(role.requiredCapabilities ?? [])];
        existing.temperature = role.temperature;
        existing.maxTokens = role.maxTokens;
        existing.params = role.params;
        existing.updatedAt = now;
        return ok({ ...existing });
      }
      const created: RoleDto = {
        id: nextId("role"),
        name: role.name,
        providerId: role.providerId,
        providerIds: [...(role.providerIds ?? [])],
        systemPromptOverride: role.systemPromptOverride,
        toolAllowlist: [...role.toolAllowlist],
        requiredCapabilities: [...(role.requiredCapabilities ?? [])],
        temperature: role.temperature,
        maxTokens: role.maxTokens,
        params: role.params,
        builtin: false,
        generated: false,
        ephemeral: false,
        source: null,
        createdAt: now,
        updatedAt: now,
      };
      tdState.roles.push(created);
      return ok({ ...created });
    },
    async deleteRole(roleId) {
      const idx = tdState.roles.findIndex((r) => r.id === roleId);
      if (idx < 0) return err("role.not_found", `role#${roleId} not found`);
      const removed = tdState.roles[idx];
      if (removed?.builtin) {
        return err("role.builtin_protected", `role#${roleId} is built-in`);
      }
      tdState.roles.splice(idx, 1);
      return ok(null);
    },
    async seedBuiltinRoles() {
      // Deterministic double: the full preset catalog lands once.
      const names = PRESET_ROLE_NAMES.filter(
        (n) => !tdState.roles.some((r) => r.name === n),
      );
      const now = Date.now();
      for (const name of names) {
        tdState.roles.push({
          id: nextId("role"),
          name,
          providerId: null,
          providerIds: [],
          systemPromptOverride: `Preset prompt for ${name}.`,
          toolAllowlist: [],
          requiredCapabilities: ["reasoning"],
          temperature: null,
          maxTokens: null,
          params: { preset: true, description: `${name} preset role` },
          builtin: true,
          generated: false,
          ephemeral: false,
          source: null,
          createdAt: now,
          updatedAt: now,
        });
      }
      return ok({ inserted: names.length, updated: 0, skipped: PRESET_ROLE_NAMES.length - names.length });
    },
    async generateRole(description) {
      const trimmed = description.trim();
      if (trimmed.length === 0) {
        return err("role.director_invalid", "description must not be empty");
      }
      const now = Date.now();
      const created: RoleDto = {
        id: nextId("role"),
        name: `directed-${tdState.roles.length + 1}`,
        providerId: null,
        providerIds: [],
        systemPromptOverride: `Generated for: ${trimmed}`,
        toolAllowlist: [],
        requiredCapabilities: ["reasoning"],
        temperature: null,
        maxTokens: null,
        params: { description: trimmed },
        builtin: false,
        generated: true,
        ephemeral: false,
        source: { description: trimmed, model: "openai_compatible", generatedAt: now },
        createdAt: now,
        updatedAt: now,
      };
      tdState.roles.push(created);
      return ok({ ...created });
    },
    async getRoutingRules() {
      return ok({ ...tdState.routingRules });
    },
    async setRoutingRules(rules) {
      tdState.routingRules = {
        preferLocal: rules.preferLocal ?? false,
        capabilityOverrides: { ...(rules.capabilityOverrides ?? {}) },
      };
      return ok(null);
    },
    async routeCapability(request) {
      const required = request.requiredCapabilities;
      const covering = tdState.providers.filter((p) =>
        required.every((cap) =>
          p.settings.models?.some((m) => m.capabilities.includes(cap)),
        ),
      );
      if (covering.length === 0 && tdState.roles.length === 0) {
        return err("route.no_capability", `no provider covers: ${required.join(", ")}`);
      }
      if (covering.length === 0) {
        return err("route.no_capability", `no provider covers: ${required.join(", ")}`);
      }
      const now = Date.now();
      const role: RoleDto = {
        id: nextId("temp-role"),
        name: `temp-${required.join("-")}-ab12cd34`,
        providerId: covering[0]?.id ?? null,
        providerIds: covering.map((p) => p.id),
        systemPromptOverride: null,
        toolAllowlist: [],
        requiredCapabilities: [...required],
        temperature: null,
        maxTokens: null,
        params: {},
        builtin: false,
        generated: false,
        ephemeral: true,
        source: null,
        createdAt: now,
        updatedAt: now,
      };
      return ok({ role, createdTemp: true });
    },
    async listTeams() {
      return ok([...tdState.teams]);
    },
    async upsertTeam(team: TeamInput) {
      if (team.name.trim().length === 0) {
        return err("team.invalid", "team name must not be empty");
      }
      if (team.memberRoleIds.length === 0) {
        return err("team.member_missing", "team needs at least one member role");
      }
      const missing = team.memberRoleIds.filter((id) => !tdState.roles.some((r) => r.id === id));
      if (missing.length > 0) {
        return errWithDetails(
          "team.member_missing",
          `unknown member roles: ${missing.join(", ")}`,
          { missing },
        );
      }
      const now = Date.now();
      // `name` is the idempotency key: an existing team is updated in place.
      const existing = tdState.teams.find((t) => t.name === team.name);
      if (existing) {
        existing.topology = team.topology;
        existing.memberRoleIds = [...team.memberRoleIds];
        existing.config = team.config;
        existing.updatedAt = now;
        return ok({ ...existing });
      }
      const created: TeamDto = {
        id: nextId("team"),
        name: team.name,
        topology: team.topology,
        memberRoleIds: [...team.memberRoleIds],
        config: team.config,
        createdAt: now,
        updatedAt: now,
      };
      tdState.teams.push(created);
      return ok({ ...created });
    },
    async deleteTeam(teamId) {
      const idx = tdState.teams.findIndex((t) => t.id === teamId);
      if (idx < 0) return err("team.not_found", `team#${teamId} not found`);
      tdState.teams.splice(idx, 1);
      return ok(null);
    },
    async listWhiteboardNotes(sessionId) {
      const notes = tdState.whiteboardNotes
        .filter((n) => n.sessionId === sessionId)
        .sort((a, b) => a.seq - b.seq);
      return ok(notes);
    },
    async formTeam(task, _sessionId) {
      const trimmed = task.trim();
      if (trimmed.length === 0) {
        return err("task.invalid", "task text must not be empty");
      }
      const now = Date.now();
      // Mirror the real former: the two preset member roles exist before the team row.
      for (const roleId of AUTO_FORM_ROLE_IDS) {
        if (!tdState.roles.some((r) => r.id === roleId)) {
          tdState.roles.push({
            id: roleId,
            name: `auto-${roleId === AUTO_FORM_ROLE_IDS[0] ? "planner" : "worker"}`,
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
            createdAt: now,
            updatedAt: now,
          });
        }
      }
      const n = tdState.teams.filter((t) => t.id.startsWith("auto-team-")).length + 1;
      const created: TeamDto = {
        id: `auto-team-${n}`,
        name: `auto-${slug(trimmed).slice(0, 12)}-${n}`,
        topology: "group_chat",
        memberRoleIds: [...AUTO_FORM_ROLE_IDS],
        config: { max_rounds: 6 },
        createdAt: now,
        updatedAt: now,
      };
      tdState.teams.push(created);
      return ok({ ...created });
    },
    async previewTeam(task) {
      const trimmed = task.trim();
      if (trimmed.length === 0) {
        return err("task.invalid", "task text must not be empty");
      }
      // Deterministic dry-run: same text always yields the same plan, and the
      // members mirror what form_team will actually build (打磨③b).
      const plan: TeamPlanDto = {
        topology: "group_chat",
        members: [
          { kind: "cli_profile", refId: "agent-auto-planner", name: "auto-planner", willCreateRole: true },
          { kind: "role", refId: AUTO_FORM_ROLE_IDS[1] ?? "role-auto-worker", name: "auto-worker", willCreateRole: false },
        ],
        maxRounds: 6,
        required: ["planner"],
        rationale: PREVIEW_RATIONALE,
      };
      return ok(plan);
    },
    async runTeamOnTask(taskId, teamId) {
      const task = tdState.tasks.find((t) => t.id === taskId);
      if (task === undefined) return err("task.not_found", `task#${taskId} not found`);
      if (!tdState.teams.some((t) => t.id === teamId)) {
        return err("team.not_found", `team#${teamId} not found`);
      }
      const run = {
        id: nextId("run"),
        taskId,
        sessionId: task.sessionId ?? nextId("s"),
        status: "succeeded",
        heartbeatAt: Date.now(),
      };
      tdState.runs.push(run);
      return ok(run);
    },
    async runTeamSession(_sessionId, _teamId, _task) {
      return ok({ finalOutput: "team ok", converged: true, rounds: 1 });
    },
    async listIntegrations() {
      return ok([...tdState.integrations]);
    },
    async upsertIntegration(input: IntegrationInput) {
      if (!isValidWebhookUrl(input.webhookUrl)) {
        return err("integration.invalid", `webhook url is not a valid http(s) url: ${input.name}`);
      }
      const now = Date.now();
      // `name` is the idempotency key: an existing integration is updated in place.
      const existing = tdState.integrations.find((i) => i.name === input.name);
      if (existing) {
        existing.kind = input.kind;
        existing.webhookUrlMasked = maskWebhookUrl(input.webhookUrl);
        existing.events = [...input.events];
        existing.enabled = input.enabled;
        existing.updatedAt = now;
        return ok({ ...existing });
      }
      const created: IntegrationDto = {
        id: nextId("integ"),
        name: input.name,
        kind: input.kind,
        webhookUrlMasked: maskWebhookUrl(input.webhookUrl),
        events: [...input.events],
        enabled: input.enabled,
        createdAt: now,
        updatedAt: now,
      };
      tdState.integrations.push(created);
      return ok({ ...created });
    },
    async deleteIntegration(integrationId) {
      const idx = tdState.integrations.findIndex((i) => i.id === integrationId);
      if (idx < 0) return err("integration.not_found", `integration#${integrationId} not found`);
      tdState.integrations.splice(idx, 1);
      return ok(null);
    },
    async testIntegration(integrationId) {
      const integration = tdState.integrations.find((i) => i.id === integrationId);
      if (!integration) return err("integration.not_found", `integration#${integrationId} not found`);
      return ok({ ok: true, error: null });
    },

    async pluginList() {
      return ok<PluginListResultDto>({ plugins: [], skipped: [], failed: [] });
    },
    async pluginInstallFromPath(_path: string) {
      return ok<PluginInfoDto>({
        id: "demo",
        name: "Demo",
        version: "1.0.0",
        apiVersion: 1,
        description: null,
        source: "user",
        dir: "/demo",
        uninstallable: true,
        tools: [],
        hooks: [],
        events: [],
        editor: null,
        permissions: { fsRead: [], fsWrite: [], network: [], shell: false },
      });
    },
    async pluginUninstall(_pluginId: string) {
      return ok(null);
    },
    async pluginOpenDir() {
      return ok(null);
    },
    async pluginEditorCall(_pluginId: string, _method: string, _params: JsonValue) {
      return err("plugin_not_loaded", "test double: no live plugin process");
    },
    async appSettingGet(key: string) {
      return ok<string | null>(tdState.appSettings.get(key) ?? null);
    },
    async appSettingSet(key: string, value: string) {
      tdState.appSettings.set(key, value);
      return ok(null);
    },

    async journalRollback(_seq) {
      return ok(null);
    },
  };
  return cmds;
}
