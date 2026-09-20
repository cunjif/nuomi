import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { NewConversationDialog } from "./NewConversationDialog";
import type { ConversationKind } from "../../../lib/conversation/kinds";

const MENU_ITEMS: Array<{ kind: ConversationKind; labelKey: string }> = [
  { kind: "chat", labelKey: "conversation.newChat" },
  { kind: "group", labelKey: "conversation.newGroup" },
  { kind: "background", labelKey: "conversation.newBackground" },
  { kind: "scheduled", labelKey: "conversation.newScheduled" },
];

/**
 * Split button for creating new conversations. Left click opens the
 * NewConversationDialog wizard for a default chat; the dropdown arrow opens
 * a kind menu.
 */
export function NewConversationMenu(): ReactNode {
  const { t } = useTranslation();
  const [menuOpen, setMenuOpen] = useState(false);
  const [dialogKind, setDialogKind] = useState<ConversationKind | null>(null);

  return (
    <div className="relative flex">
      <button
        type="button"
        onClick={() => setDialogKind("chat")}
        className="pixel-fill-accent rounded-l px-2 py-0.5 text-xs text-surface focus-visible:ring-2 focus-visible:ring-ink-accent"
      >
        {t("sessions.newSession")}
      </button>
      <button
        type="button"
        onClick={() => setMenuOpen((v) => !v)}
        aria-label={t("conversation.newMenuLabel")}
        className="pixel-fill-accent rounded-r border-l border-surface/30 px-1 py-0.5 text-xs text-surface focus-visible:ring-2 focus-visible:ring-ink-accent"
      >
        ▾
      </button>
      {menuOpen && (
        <ul className="absolute right-0 top-full z-10 mt-0.5 rounded border border-ink-muted/40 bg-surface-raised shadow-lg">
          {MENU_ITEMS.map((item) => (
            <li key={item.kind}>
              <button
                type="button"
                onClick={() => {
                  setDialogKind(item.kind);
                  setMenuOpen(false);
                }}
                className="block w-full px-2 py-1 text-left text-xs text-ink hover:bg-surface-overlay"
              >
                {t(item.labelKey)}
              </button>
            </li>
          ))}
        </ul>
      )}
      {dialogKind && (
        <NewConversationDialog
          kind={dialogKind}
          onClose={() => setDialogKind(null)}
        />
      )}
    </div>
  );
}
