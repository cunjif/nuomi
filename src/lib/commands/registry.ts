/**
 * Slash-command registry for the chat composer ("命令即快捷输入"): a `/`
 * prefix in the input box routes to a registered action instead of being sent
 * as a normal message (design distilled from pi's steering loop and
 * prime-agent's agent_message: commands are quick input shortcuts, not chat).
 *
 * Extension point: this registry is an open array grown via registerCommand().
 * Future Rust-plugin–provided commands can be hydrated at startup by calling
 * registerCommand() for each entry fetched over IPC — nothing else changes.
 */
import type { QueryClient } from "@tanstack/react-query";
import type { TFunction } from "i18next";
import type { Ipc } from "../ipc/client";
import type { toast } from "../store/toastStore";
import type { View } from "../store/uiStore";
import type { ArgKind, CommandCategory, CommandSuggestion } from "./commandTypes";

/** Services a command may use. Built by ChatView from existing state/props. */
export interface CommandContext {
  /** Currently selected session id (null when none). */
  sessionId: string | null;
  ipc: Ipc;
  queryClient: QueryClient;
  /** Switch the top-level shell view (the session list lives in the chat view's left rail). */
  navigate: (view: View) => void;
  selectSession: (sessionId: string) => void;
  toggleTheme: () => void;
  toast: typeof toast;
  t: TFunction;
}

export interface SlashCommand {
  /** Name without the leading "/", e.g. "workspace". */
  name: string;
  /** Grouping for `/help` and completion ordering. */
  category: CommandCategory;
  /**
   * i18n key of the human-readable description, e.g. "commands.workspace.description".
   * Optional when a direct `description` is supplied (plugin-provided
   * commands have no app i18n keys).
   */
  descriptionI18nKey?: string;
  /** Direct description (plugin-provided commands); wins over the i18n key. */
  description?: string;
  /**
   * i18n key of the argument hint shown in the completion panel, rendered as
   * e.g. "/workspace <path>" (i18n'd so zh/en can adapt the placeholder).
   */
  usage?: string;
  args: ArgKind;
  /** Fuzzy search keywords (zh/en) beyond the command name. */
  keywords?: string[];
  /** Dynamic parameter candidates; empty array degrades to plain text input. */
  suggest?: (ctx: CommandContext) => Promise<CommandSuggestion[]> | CommandSuggestion[];
  /**
   * Execute the command. Throwing keeps the composer draft and surfaces a
   * toast; resolving clears the draft (this is how /clear works — it is a
   * no-op run and the composer does the clearing).
   */
  run(args: string, ctx: CommandContext): Promise<void> | void;
  /** Origin: built-in or plugin-provided. */
  source?: "builtin" | "plugin";
}

export interface ParsedInput {
  /** Raw first token after "/", e.g. "nope" for "/nope x". */
  name: string;
  /** Remaining text with leading/trailing whitespace collapsed. */
  args: string;
  /** Matched registry entry; undefined for unknown commands. */
  command?: SlashCommand;
}

const registry: SlashCommand[] = [];

/** Register a command; a later registration with the same name wins. */
export function registerCommand(command: SlashCommand): void {
  const existing = registry.findIndex((c) => c.name === command.name);
  if (existing >= 0) registry.splice(existing, 1);
  registry.push(command);
}

export function getCommands(): readonly SlashCommand[] {
  return registry;
}

export function findCommand(name: string): SlashCommand | undefined {
  return registry.find((c) => c.name === name);
}

/** Human description: direct `description` first (plugin commands), i18n key fallback. */
export function commandDescription(command: SlashCommand, t: (key: string) => string): string {
  if (command.description !== undefined && command.description.length > 0) return command.description;
  return t(command.descriptionI18nKey ?? `commands.${command.name}.description`);
}

/** Closest matches for an unknown command: prefix match first, then contains. */
export function suggestCommands(name: string): SlashCommand[] {
  const lower = name.toLowerCase();
  return registry.filter(
    (c) => c.name.startsWith(lower) || c.name.includes(lower),
  );
}

/**
 * Parse composer text into a command invocation. Returns null when the input
 * is not a slash command (including a bare "/" or "/ " — those are ordinary
 * messages). Tolerates extra whitespace: "/cmd   arg1  arg2".
 */
export function parseInput(text: string): ParsedInput | null {
  const trimmed = text.trim();
  if (!trimmed.startsWith("/")) return null;
  const [name = "", ...rest] = trimmed.slice(1).split(/\s+/);
  if (name.length === 0) return null;
  const command = findCommand(name);
  return { name, args: rest.join(" ").trim(), command };
}
