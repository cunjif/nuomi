/**
 * First-party slash commands. All of them route to existing IPC calls or
 * store actions — the frontend never implements backend logic here. The
 * module registers the commands on import (side effect, single ESM instance).
 *
 * Future: Rust plugins will deliver extra commands the same way — hydrate the
 * registry at startup via registerCommand() per backend-provided entry.
 */
import { getCommands, registerCommand, type SlashCommand } from "./registry";

const BUILTIN_COMMANDS: SlashCommand[] = [
  {
    name: "workspace",
    descriptionI18nKey: "commands.workspace.description",
    usage: "commands.workspace.usage",
    args: "required",
    async run(args, ctx) {
      const path = args.trim();
      if (path.length === 0) throw new Error(ctx.t("commands.workspaceRequired"));
      await ctx.ipc.setWorkspace(path);
      // Refresh workspace root, directory tree and open-file caches.
      void ctx.queryClient.invalidateQueries({ queryKey: ["workspace"] });
      void ctx.queryClient.invalidateQueries({ queryKey: ["dir"] });
      void ctx.queryClient.invalidateQueries({ queryKey: ["file"] });
      ctx.toast.success(ctx.t("commands.workspaceDone", { path }));
    },
  },
  {
    name: "new",
    descriptionI18nKey: "commands.new.description",
    args: "none",
    async run(_args, ctx) {
      const session = await ctx.ipc.createSession();
      void ctx.queryClient.invalidateQueries({ queryKey: ["sessions"] });
      ctx.selectSession(session.id);
      ctx.toast.success(ctx.t("commands.sessionCreated"));
    },
  },
  {
    name: "sessions",
    descriptionI18nKey: "commands.sessions.description",
    args: "none",
    run(_args, ctx) {
      // The session list panel lives in the chat view's left rail.
      ctx.navigate("chat");
    },
  },
  {
    // Esc semantics: nothing to do here — the composer clears its draft when
    // a command resolves, so an empty run() is the whole implementation.
    name: "clear",
    descriptionI18nKey: "commands.clear.description",
    args: "none",
    run() {},
  },
  {
    name: "help",
    descriptionI18nKey: "commands.help.description",
    args: "none",
    run(_args, ctx) {
      const lines = getCommands().map((cmd) => {
        const usage = cmd.usage ? ` ${ctx.t(cmd.usage)}` : "";
        return `/${cmd.name}${usage} — ${ctx.t(cmd.descriptionI18nKey)}`;
      });
      // The toast surface collapses newlines, so join on a readable separator.
      ctx.toast.success(`${ctx.t("commands.helpTitle")} ${lines.join(" | ")}`);
    },
  },
  {
    name: "theme",
    descriptionI18nKey: "commands.theme.description",
    args: "none",
    run(_args, ctx) {
      ctx.toggleTheme();
      ctx.toast.success(ctx.t("commands.themeToggled"));
    },
  },
];

for (const command of BUILTIN_COMMANDS) registerCommand(command);
