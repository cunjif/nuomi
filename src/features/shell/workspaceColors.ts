/**
 * Workspace color tags → Tailwind dot classes.
 * `color_tag` is free-form TEXT on the backend (hash-assigned at creation),
 * so an unknown tag must fall back to a neutral dot instead of rendering
 * nothing. These are decorative identity dots — accessibility is carried by
 * the workspace labels next to them, not the color.
 */
const WORKSPACE_COLOR_CLASSES: Record<string, string> = {
  "paper-yellow": "bg-amber-300",
  "ink-blue": "bg-blue-500",
  "moss-green": "bg-green-500",
  "brick-red": "bg-red-500",
  "violet": "bg-purple-500",
  "ochre-orange": "bg-orange-500",
  "slate-gray": "bg-slate-400",
  "pink-rose": "bg-pink-400",
};

export function workspaceColorClass(colorTag: string): string {
  return WORKSPACE_COLOR_CLASSES[colorTag] ?? "bg-ink-muted";
}
