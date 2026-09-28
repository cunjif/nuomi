/**
 * Session-related slash commands: /new, /sessions, /clear.
 */
import type { SlashCommand } from "./registry";

export const sessionCommands: SlashCommand[] = [
  {
    name: "new",
    category: "session",
    descriptionI18nKey: "commands.new.description",
    args: "text",
    keywords: ["create", "新建", "会话"],
    async run(args, ctx) {
      const kind = args.trim() || "chat";
      const validKinds = ["chat", "group", "background", "scheduled"];
      if (!validKinds.includes(kind)) {
        throw new Error(ctx.t("commands.newInvalidKind", { kind }));
      }
      const session = await ctx.ipc.createConversation({ kind, title: null, agent: null, teamId: null, workspaceId: ctx.focusedWorkspaceId ?? "__migrated__" });
      void ctx.queryClient.invalidateQueries({ queryKey: ["conversations"] });
      ctx.selectSession(session.id);
      ctx.toast.success(ctx.t("commands.sessionCreated"));
    },
  },
  {
    name: "sessions",
    category: "view",
    descriptionI18nKey: "commands.sessions.description",
    args: "none",
    keywords: ["list", "列表", "会话"],
    run(_args, ctx) {
      ctx.navigate("chat");
    },
  },
  {
    name: "clear",
    category: "session",
    descriptionI18nKey: "commands.clear.description",
    args: "none",
    keywords: ["empty", "清空"],
    run() {},
  },
];
