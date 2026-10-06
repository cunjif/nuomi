/**
 * T9-11 端到端闭环测试：管家会话 → 进化受理 → 周期进度 → 验收门 → 批准合入 → 回滚可用
 *
 * 用有状态 IPC test-double 模拟后端行为，验证 spec §5.5.2 交互流程完整可走通。
 * IPC 级闭环验证完整流程；UI 渲染验证 GateInbox 可渲染（组件交互在 T9-10 已覆盖）。
 */
import { screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type {
  CycleDetailDto,
  EvolutionArtifactDto,
  EvolutionCycleDto,
  GateDecisionDto,
  ResolveOutcomeDto,
  StewardReplyDto,
  StewardSessionDto,
  Result,
  ConfigChangeResultDto,
} from "../../lib/ipc/bindings.gen";
import { injectIpcCommands, ipc } from "../../lib/ipc/client";
import { testDoubleCommands } from "../../lib/ipc/test-double";
import { renderWithProviders } from "../../test/helpers";
import { GateInbox } from "./GateInbox";

type Ok<T> = { status: "ok"; data: T };

function ok<T>(data: T): Ok<T> {
  return { status: "ok", data };
}

interface FlowState {
  session: StewardSessionDto | null;
  reply: StewardReplyDto | null;
  cycle: EvolutionCycleDto | null;
  gatePending: EvolutionArtifactDto[];
  resolveOutcome: ResolveOutcomeDto | null;
  rolledBack: boolean;
  rollbackSnapshotId: string | null;
}

function createFlowState(): FlowState {
  return {
    session: null,
    reply: null,
    cycle: null,
    gatePending: [],
    resolveOutcome: null,
    rolledBack: false,
    rollbackSnapshotId: null,
  };
}

const CYCLE_ID = "cycle-e2e";
const ARTIFACT_ID = "artifact-e2e";
const PROPOSAL_ID = "proposal-e2e";
const SNAPSHOT_ID = "snapshot-e2e";

function flowCommands(fs: FlowState) {
  const base = testDoubleCommands();
  return {
    ...base,
    async stewardCreateSession(title: string) {
      const s: StewardSessionDto = {
        id: "ss-e2e",
        stewardId: "steward",
        title,
        goal: null,
        createdAt: Date.now(),
        updatedAt: Date.now(),
      };
      fs.session = s;
      return ok(s) as unknown as Result<StewardSessionDto, never>;
    },
    async stewardListSessions() {
      return ok(fs.session ? [fs.session] : []) as unknown as Result<
        StewardSessionDto[],
        never
      >;
    },
    async stewardSendMessage(_sessionId: string, text: string) {
      if (text.includes("进化") || text.includes("evolve")) {
        fs.cycle = {
          id: CYCLE_ID,
          triggerSource: "user",
          triggerContext: text,
          phase: "cleanse",
          status: "running",
          createdAt: Date.now(),
        };
        fs.gatePending = [
          {
            id: ARTIFACT_ID,
            taskId: "task-e2e",
            producedByRole: "steward_researcher",
            artifactType: "research_report",
            content: { summary: "调研报告" },
            status: "pending_review",
            diffPreview: "--- before\n+++ after\n+新增进化方案",
            createdAt: Date.now(),
          },
        ];
        const reply: StewardReplyDto = {
          type: "evolution_accepted",
          cycle_id: CYCLE_ID,
          progress_subscribe_handle: "handle-e2e",
        };
        fs.reply = reply;
        return ok(reply) as unknown as Result<StewardReplyDto, never>;
      }
      const reply: StewardReplyDto = { type: "text", content: "已收到" };
      fs.reply = reply;
      return ok(reply) as unknown as Result<StewardReplyDto, never>;
    },
    async stewardListCycles(_status: string | null) {
      return ok(fs.cycle ? [fs.cycle] : []) as unknown as Result<
        EvolutionCycleDto[],
        never
      >;
    },
    async stewardGetCycle(_cycleId: string) {
      const detail: CycleDetailDto = {
        cycle: fs.cycle ?? {
          id: CYCLE_ID,
          triggerSource: "user",
          triggerContext: "",
          phase: "cleanse",
          status: "running",
          createdAt: 0,
        },
        tasks: [],
        artifacts: fs.gatePending,
      };
      return ok(detail) as unknown as Result<CycleDetailDto, never>;
    },
    async stewardListGatePending() {
      return ok(fs.gatePending) as unknown as Result<
        EvolutionArtifactDto[],
        never
      >;
    },
    async stewardResolveGate(_artifactId: string, decision: GateDecisionDto) {
      if (decision.kind === "approve") {
        fs.resolveOutcome = { kind: "merged", rollbackHandle: `config:${SNAPSHOT_ID}` };
        fs.gatePending = [];
        return ok(fs.resolveOutcome) as unknown as Result<ResolveOutcomeDto, never>;
      }
      if (decision.kind === "reject") {
        fs.resolveOutcome = { kind: "rejected" };
        fs.gatePending = [];
        return ok(fs.resolveOutcome) as unknown as Result<ResolveOutcomeDto, never>;
      }
      fs.resolveOutcome = { kind: "needs_revision", newTaskId: "task-revise" };
      return ok(fs.resolveOutcome) as unknown as Result<ResolveOutcomeDto, never>;
    },
    async stewardConfirmConfigChange(_proposalId: string) {
      fs.rollbackSnapshotId = SNAPSHOT_ID;
      const result: ConfigChangeResultDto = {
        proposalId: PROPOSAL_ID,
        snapshotId: SNAPSHOT_ID,
        targetType: "role",
        targetId: "td",
      };
      return ok(result) as unknown as Result<ConfigChangeResultDto, never>;
    },
    async stewardRollbackConfigChange(_snapshotId: string) {
      fs.rolledBack = true;
      return ok(null) as unknown as Result<null, never>;
    },
  };
}

describe("Steward E2E 闭环", () => {
  it("完整路径：创建会话→发送进化诉求→受理→周期可见→产物出现→批准合入→回滚", async () => {
    const fs = createFlowState();
    injectIpcCommands(flowCommands(fs));

    // Step 1: 创建管家会话
    const session = await ipc.stewardCreateSession("进化测试会话");
    expect(session.id).toBe("ss-e2e");
    expect(fs.session).not.toBeNull();

    // Step 2: 发送进化诉求
    const reply = await ipc.stewardSendMessage(session.id, "请进化提示词管理能力");
    expect(reply.type).toBe("evolution_accepted");
    if (reply.type === "evolution_accepted") {
      expect(reply.cycle_id).toBe(CYCLE_ID);
    }

    // Step 3: 进化周期已受理（spec §5.5.2 步骤 1-2）
    expect(fs.cycle).not.toBeNull();
    expect(fs.cycle!.status).toBe("running");
    expect(fs.cycle!.phase).toBe("cleanse");

    // Step 4: 周期列表可见（spec §5.5.2 步骤 3）
    const cycles = await ipc.stewardListCycles(null);
    expect(cycles).toHaveLength(1);
    expect(cycles[0]!.id).toBe(CYCLE_ID);

    const cycleDetail = await ipc.stewardGetCycle(CYCLE_ID);
    expect(cycleDetail.cycle.id).toBe(CYCLE_ID);
    expect(cycleDetail.artifacts).toHaveLength(1);

    // Step 5: 验收门收件箱出现待审批产物（spec §5.5.2 步骤 4）
    const pending = await ipc.stewardListGatePending();
    expect(pending).toHaveLength(1);
    expect(pending[0]!.id).toBe(ARTIFACT_ID);
    expect(pending[0]!.status).toBe("pending_review");
    expect(pending[0]!.diffPreview).toContain("新增进化方案");

    // Step 6: 用户批准产物（spec §5.5.2 步骤 5）
    const outcome = await ipc.stewardResolveGate(ARTIFACT_ID, { kind: "approve" });
    expect(outcome.kind).toBe("merged");
    expect(outcome.rollbackHandle).toBe(`config:${SNAPSHOT_ID}`);

    // Step 7: 合入后产物离开收件箱
    const pendingAfter = await ipc.stewardListGatePending();
    expect(pendingAfter).toHaveLength(0);

    // Step 8: 回滚入口可用（spec §5.5.2 步骤 6）
    expect(outcome.rollbackHandle).toBeTruthy();
    await ipc.stewardRollbackConfigChange(SNAPSHOT_ID);
    expect(fs.rolledBack).toBe(true);
  });

  it("驳回路径：发送进化→产物出现→驳回→产物离开收件箱", async () => {
    const fs = createFlowState();
    injectIpcCommands(flowCommands(fs));

    await ipc.stewardCreateSession("驳回测试");
    await ipc.stewardSendMessage("ss-e2e", "进化数据清洗规则");

    const pending = await ipc.stewardListGatePending();
    expect(pending).toHaveLength(1);

    const outcome = await ipc.stewardResolveGate(ARTIFACT_ID, { kind: "reject" });
    expect(outcome.kind).toBe("rejected");

    const pendingAfter = await ipc.stewardListGatePending();
    expect(pendingAfter).toHaveLength(0);
  });

  it("要求修改路径：发送进化→产物出现→要求修改→退回研发团队", async () => {
    const fs = createFlowState();
    injectIpcCommands(flowCommands(fs));

    await ipc.stewardCreateSession("修改测试");
    await ipc.stewardSendMessage("ss-e2e", "进化编排策略");

    const pending = await ipc.stewardListGatePending();
    expect(pending).toHaveLength(1);

    const outcome = await ipc.stewardResolveGate(ARTIFACT_ID, {
      kind: "request_changes",
      feedback: "请补充性能基准",
    });
    expect(outcome.kind).toBe("needs_revision");
    expect(outcome.newTaskId).toBe("task-revise");
  });

  it("配置变更闭环：确认→生成快照→回滚恢复", async () => {
    const fs = createFlowState();
    injectIpcCommands(flowCommands(fs));

    const result = await ipc.stewardConfirmConfigChange(PROPOSAL_ID);
    expect(result.snapshotId).toBe(SNAPSHOT_ID);
    expect(fs.rollbackSnapshotId).toBe(SNAPSHOT_ID);

    await ipc.stewardRollbackConfigChange(SNAPSHOT_ID);
    expect(fs.rolledBack).toBe(true);
  });

  it("GateInbox 渲染验证", async () => {
    const base = testDoubleCommands();
    injectIpcCommands(base);
    renderWithProviders(<GateInbox />);
    expect(await screen.findByText("验收门收件箱")).toBeInTheDocument();
  });
});
