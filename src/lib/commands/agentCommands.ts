/**
 * Agent-related slash commands: /agent.
 */
import type { CommandSuggestion } from "./commandTypes";
import type { SlashCommand } from "./registry";

export const agentCommands: SlashCommand[] = [
  {
    name: "agent",
    category: "agent",
    descriptionI18nKey: "commands.agent.description",
    usage: "commands.agent.usage",
    args: "agent",
    keywords: ["switch", "切换", "智能体"],
    async suggest(ctx) {
      const options = await ctx.ipc.listAgentOptions();
      return options.map<CommandSuggestion>((o) => ({
        label: o.name,
        value: `${o.kind}:${o.id}`,
        description: o.builtin ? "builtin" : undefined,
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
      const options = await ctx.ipc.listAgentOptions();
      const match = options.find((o) => `${o.kind}:${o.id}` === trimmed || o.name === trimmed);
      if (!match) {
        throw new Error(ctx.t("commands.agentNotFound", { name: trimmed }));
      }
      await ctx.ipc.setConversationAgent(ctx.sessionId, { kind: match.kind, id: match.id });
      void ctx.queryClient.invalidateQueries({ queryKey: ["conversation", ctx.sessionId] });
      ctx.toast.success(ctx.t("commands.agentSwitched", { name: match.name }));
    },
  },
];
