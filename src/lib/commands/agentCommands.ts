/**
 * Agent-related slash commands: /agent.
 */
import type { CommandSuggestion } from "./commandTypes";
import type { SlashCommand } from "./registry";
import { isRoleReady } from "../conversation/roleReady";

export const agentCommands: SlashCommand[] = [
  {
    name: "agent",
    category: "agent",
    descriptionI18nKey: "commands.agent.description",
    usage: "commands.agent.usage",
    args: "agent",
    keywords: ["switch", "切换", "智能体"],
    async suggest(ctx) {
      const roles = await ctx.ipc.listRoles();
      return roles.filter(isRoleReady).map<CommandSuggestion>((r) => ({
        label: r.name,
        value: `role:${r.id}`,
        description: r.builtin ? "builtin" : undefined,
      }));
    },
    async run(args, ctx) {
      if (!ctx.sessionId) {
        throw new Error(ctx.t("commands.agentNoSession"));
      }
      const trimmed = args.trim();
      if (trimmed.length === 0) {
        return;
      }
      const roles = await ctx.ipc.listRoles();
      const readyRoles = roles.filter(isRoleReady);
      const match = readyRoles.find((r) => `role:${r.id}` === trimmed || r.name === trimmed);
      if (!match) {
        throw new Error(ctx.t("commands.agentNotFound", { name: trimmed }));
      }
      await ctx.ipc.setConversationAgent(ctx.sessionId, { kind: "role", id: match.id });
      void ctx.queryClient.invalidateQueries({ queryKey: ["conversation", ctx.sessionId] });
      ctx.toast.success(ctx.t("commands.agentSwitched", { name: match.name }));
    },
  },
];
