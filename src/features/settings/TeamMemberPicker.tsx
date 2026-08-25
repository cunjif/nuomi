import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { RoleDto } from "../../lib/ipc/bindings.gen";

interface TeamMemberPickerProps {
  roles: RoleDto[];
  memberIds: string[];
  onToggle: (roleId: string) => void;
  onMove: (roleId: string, delta: -1 | 1) => void;
}

/** Two-pane role membership editor: checkbox pool + ordered member list. */
export function TeamMemberPicker({ roles, memberIds, onToggle, onMove }: TeamMemberPickerProps): ReactNode {
  const { t } = useTranslation();
  const roleName = new Map(roles.map((r) => [r.id, r.name]));

  return (
    <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
      <fieldset className="rounded border border-ink-muted/30 p-2">
        <legend className="px-1 text-xs text-ink-muted">{t("settings.teams.members")}</legend>
        <ul className="flex flex-col gap-1">
          {roles.map((role) => (
            <li key={role.id}>
              <label className="flex items-center gap-1.5 text-xs text-ink">
                <input
                  type="checkbox"
                  checked={memberIds.includes(role.id)}
                  onChange={() => onToggle(role.id)}
                  className="accent-[var(--nuomi-accent)]"
                />
                {role.name}
              </label>
            </li>
          ))}
          {roles.length === 0 && <li className="text-xs text-ink-muted">{t("settings.roles.empty")}</li>}
        </ul>
      </fieldset>
      <fieldset className="rounded border border-ink-muted/30 p-2">
        <legend className="px-1 text-xs text-ink-muted">{t("settings.teams.membersHint")}</legend>
        <ol className="flex flex-col gap-1">
          {memberIds.map((roleId, index) => {
            const label = roleName.get(roleId) ?? roleId;
            return (
              <li key={roleId} className="flex items-center gap-1 text-xs text-ink">
                <span className="min-w-4 text-ink-muted">{index + 1}.</span>
                <span className="flex-1 truncate">{label}</span>
                <button
                  type="button"
                  onClick={() => onMove(roleId, -1)}
                  disabled={index === 0}
                  aria-label={`${t("settings.teams.moveUp")} ${label}`}
                  className="rounded border border-ink-muted px-1.5 py-0.5 hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-40"
                >
                  ↑
                </button>
                <button
                  type="button"
                  onClick={() => onMove(roleId, 1)}
                  disabled={index === memberIds.length - 1}
                  aria-label={`${t("settings.teams.moveDown")} ${label}`}
                  className="rounded border border-ink-muted px-1.5 py-0.5 hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-40"
                >
                  ↓
                </button>
              </li>
            );
          })}
          {memberIds.length === 0 && (
            <li className="text-xs text-ink-muted">{t("settings.teams.noMembers")}</li>
          )}
        </ol>
      </fieldset>
    </div>
  );
}
