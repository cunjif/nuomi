/**
 * Task-related slash commands: /stop, /background, /schedule, /group.
 */
import type { SlashCommand } from "./registry";

export const taskCommands: SlashCommand[] = [
  {
    name: "stop",
    category: "task",
    descriptionI18nKey: "commands.stop.description",
    args: "none",
    keywords: ["cancel", "终止", "停止"],
    async run(_args, ctx) {
      if (!ctx.sessionId) {
        throw new Error(ctx.t("commands.stopNoSession"));
      }
      await ctx.ipc.stopConversation(ctx.sessionId);
      void ctx.queryClient.invalidateQueries({ queryKey: ["activeRuns"] });
      ctx.toast.success(ctx.t("commands.stopped"));
    },
  },
  {
    name: "background",
    category: "task",
    descriptionI18nKey: "commands.background.description",
    usage: "commands.background.usage",
    args: "text",
    keywords: ["bg", "后台", "任务"],
    async run(args, ctx) {
      const title = args.trim();
      if (title.length === 0) throw new Error(ctx.t("commands.backgroundRequired"));
      const session = await ctx.ipc.createConversation({
        kind: "background",
        title,
        agent: null,
        teamId: null,
        workspaceId: ctx.focusedWorkspaceId ?? "__migrated__",
      });
      void ctx.queryClient.invalidateQueries({ queryKey: ["conversations"] });
      ctx.selectSession(session.id);
      ctx.toast.success(ctx.t("commands.backgroundCreated", { title }));
    },
  },
  {
    name: "group",
    category: "session",
    descriptionI18nKey: "commands.group.description",
    args: "none",
    keywords: ["team", "群聊", "团队"],
    async run(_args, ctx) {
      const session = await ctx.ipc.createConversation({
        kind: "group",
        title: null,
        agent: null,
        teamId: null,
        workspaceId: ctx.focusedWorkspaceId ?? "__migrated__",
      });
      void ctx.queryClient.invalidateQueries({ queryKey: ["conversations"] });
      ctx.selectSession(session.id);
    },
  },
  {
    name: "schedule",
    category: "task",
    descriptionI18nKey: "commands.schedule.description",
    usage: "commands.schedule.usage",
    args: "text",
    keywords: ["cron", "定时", "调度"],
    async run(_args, ctx) {
      ctx.navigate("scheduler");
    },
  },
];
