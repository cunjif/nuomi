import type { TerminalInstance } from "../../lib/store/uiStore";

/**
 * Next default tab title for a newly spawned terminal. Starts from the bucket
 * size + 1 and skips names already taken (renaming or closing tabs can
 * otherwise produce two tabs both called "Terminal 2").
 */
export function nextTerminalTitle(terminals: TerminalInstance[]): string {
  const taken = new Set(terminals.map((t) => t.title));
  let n = terminals.length + 1;
  while (taken.has(`Terminal ${n}`)) n += 1;
  return `Terminal ${n}`;
}