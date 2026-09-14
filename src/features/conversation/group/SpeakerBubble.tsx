import type { ReactNode } from "react";

export interface SpeakerBubbleProps {
  roleName: string;
  roleColor: string;
  content: string;
  isHandoff?: boolean;
}

/** Speaker message bubble with colored role indicator. */
export function SpeakerBubble({ roleName, roleColor, content, isHandoff }: SpeakerBubbleProps): ReactNode {
  return (
    <div className="flex gap-2 py-1">
      <div className="w-1 shrink-0 rounded-full" style={{ backgroundColor: roleColor }} />
      <div className="min-w-0 flex-1">
        <span
          className="inline-block rounded px-1.5 py-0.5 text-xs font-medium"
          style={{ backgroundColor: `${roleColor}20`, color: roleColor }}
        >
          {roleName}
          {isHandoff && " →"}
        </span>
        <p className="mt-0.5 text-sm text-ink">{content}</p>
      </div>
    </div>
  );
}
