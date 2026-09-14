/**
 * Context-related slash commands: /attach, /workspace, /theme.
 */
import type { SlashCommand } from "./registry";
import { guessMime, isAllowedMime, toBase64 } from "../conversation/attachmentModel";

export const contextCommands: SlashCommand[] = [
  {
    name: "attach",
    category: "context",
    descriptionI18nKey: "commands.attach.description",
    usage: "commands.attach.usage",
    args: "path",
    keywords: ["file", "附件", "文件"],
    async run(args, ctx) {
      const path = args.trim();
      if (path.length === 0) throw new Error(ctx.t("commands.attachRequired"));
      if (!ctx.sessionId) throw new Error(ctx.t("commands.attachNoSession"));
      const content = await ctx.ipc.readFile(path);
      const name = path.split(/[/\\]/).pop() ?? path;
      const mime = guessMime(name);
      if (!isAllowedMime(mime)) {
        throw new Error(ctx.t("composer.attachmentInvalidMime"));
      }
      await ctx.ipc.saveAttachment(ctx.sessionId, name, mime, toBase64(content));
      void ctx.queryClient.invalidateQueries({ queryKey: ["attachments", ctx.sessionId] });
      ctx.toast.success(ctx.t("commands.attachAdded", { path }));
    },
  },
  {
    name: "workspace",
    category: "view",
    descriptionI18nKey: "commands.workspace.description",
    usage: "commands.workspace.usage",
    args: "text",
    keywords: ["dir", "目录", "工作区", "list", "add", "remove", "switch"],
    async run(args, ctx) {
      const trimmed = args.trim();
      const parts = trimmed.split(/\s+/);
      const sub = parts[0]?.toLowerCase();
      const invalidate = (): void => {
        void ctx.queryClient.invalidateQueries({ queryKey: ["workspace"] });
        void ctx.queryClient.invalidateQueries({ queryKey: ["workspaces"] });
        void ctx.queryClient.invalidateQueries({ queryKey: ["dir"] });
        void ctx.queryClient.invalidateQueries({ queryKey: ["file"] });
        void ctx.queryClient.invalidateQueries({ queryKey: ["sessions"] });
      };

      if (sub === "list") {
        const list = await ctx.ipc.listWorkspaces();
        const lines = list.map((w) => `${w.isActive ? "●" : "○"} ${w.rootPath}${w.directoryPresent ? "" : " ⚠"}`);
        ctx.toast.success(lines.length > 0 ? lines.join("\n") : ctx.t("workspace.empty"));
        return;
      }

      if (sub === "add") {
        const path = parts.slice(1).join(" ").trim();
        if (path.length === 0) throw new Error("/workspace add <path>");
        await ctx.ipc.addWorkspace(path);
        invalidate();
        ctx.toast.success(ctx.t("workspace.added"));
        return;
      }

      if (sub === "remove") {
        const id = parts[1]?.trim();
        if (!id) throw new Error("/workspace remove <id>");
        await ctx.ipc.removeWorkspace(id);
        invalidate();
        ctx.toast.success(ctx.t("workspace.removed"));
        return;
      }

      if (sub === "switch") {
        const id = parts[1]?.trim();
        if (!id) throw new Error("/workspace switch <id>");
        await ctx.ipc.activateWorkspace(id);
        invalidate();
        ctx.toast.success(ctx.t("workspace.switched"));
        return;
      }

      // Legacy: /workspace <path> → add + activate.
      if (trimmed.length === 0) throw new Error(ctx.t("commands.workspaceRequired"));
      await ctx.ipc.setWorkspace(trimmed);
      invalidate();
      ctx.toast.success(ctx.t("commands.workspaceDone", { path: trimmed }));
    },
  },
  {
    name: "theme",
    category: "view",
    descriptionI18nKey: "commands.theme.description",
    args: "none",
    keywords: ["dark", "light", "主题"],
    run(_args, ctx) {
      ctx.toggleTheme();
      ctx.toast.success(ctx.t("commands.themeToggled"));
    },
  },
];
