/**
 * First-party slash commands. All of them route to existing IPC calls or
 * store actions — the frontend never implements backend logic here. The
 * module registers the commands on import (side effect, single ESM instance).
 *
 * Command modules are split by category (typescript-react rule: >200 lines
 * → split). This file aggregates and registers them all.
 *
 * Future: Rust plugins will deliver extra commands the same way — hydrate the
 * registry at startup via registerCommand() per backend-provided entry.
 */
import { getCommands, registerCommand, commandDescription, type SlashCommand } from "./registry";
import { sessionCommands } from "./sessionCommands";
import { agentCommands } from "./agentCommands";
import { contextCommands } from "./contextCommands";
import { taskCommands } from "./taskCommands";

const BUILTIN_COMMANDS: SlashCommand[] = [
  ...sessionCommands,
  ...agentCommands,
  ...contextCommands,
  ...taskCommands,
  {
    name: "help",
    category: "view",
    descriptionI18nKey: "commands.help.description",
    args: "none",
    keywords: ["?", "帮助"],
    run(_args, ctx) {
      const lines = getCommands().map((cmd) => {
        const usage = cmd.usage ? ` ${ctx.t(cmd.usage)}` : "";
        return `/${cmd.name}${usage} — ${commandDescription(cmd, ctx.t)}`;
      });
      ctx.toast.success(`${ctx.t("commands.helpTitle")} ${lines.join(" | ")}`);
    },
  },
];

for (const command of BUILTIN_COMMANDS) registerCommand(command);
