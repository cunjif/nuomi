import type { ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type { CapabilityDto, RoleDto } from "../../lib/ipc/bindings.gen";
import { ipc } from "../../lib/ipc/client";

export interface PresetRoleSelection {
  name: string;
  systemPromptOverride: string | null;
  requiredCapabilities: CapabilityDto[];
}

interface PresetRolePickerProps {
  onSelect: (role: PresetRoleSelection) => void;
  onClose: () => void;
}

/** Popover listing builtin (preset) and custom roles for inheritance. */
export function PresetRolePicker({ onSelect, onClose }: PresetRolePickerProps): ReactNode {
  const { t } = useTranslation();
  const query = useQuery({ queryKey: ["roles"], queryFn: ipc.listRoles });
  const roles = query.data ?? [];
  const builtinRoles = roles.filter((r) => r.builtin);
  const customRoles = roles.filter((r) => !r.builtin);

  const renderRow = (role: RoleDto): ReactNode => (
    <button
      key={role.id}
      type="button"
      onClick={() => {
        onSelect({
          name: role.name,
          systemPromptOverride: role.systemPromptOverride,
          requiredCapabilities: [...role.requiredCapabilities],
        });
        onClose();
      }}
      className="block w-full rounded px-2 py-1 text-left text-xs text-ink hover:bg-surface-overlay focus-visible:ring-1 focus-visible:ring-ink-accent"
    >
      <span className="font-medium">{role.name}</span>
      {role.systemPromptOverride !== null && (
        <span className="ml-2 text-[10px] text-ink-muted line-clamp-1">
          {role.systemPromptOverride}
        </span>
      )}
    </button>
  );

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
      role="dialog"
      aria-modal="true"
      aria-label={t("settings.roles.presetPickerTitle")}
      onClick={onClose}
    >
      <div
        className="max-h-80 w-full max-w-sm overflow-y-auto rounded border border-ink-muted/40 bg-surface-raised p-3"
        onClick={(e) => e.stopPropagation()}
      >
        <h4 className="mb-2 text-sm font-semibold text-ink">
          {t("settings.roles.presetPickerTitle")}
        </h4>
        {builtinRoles.length > 0 && (
          <div className="mb-2">
            <p className="mb-1 text-[10px] font-semibold uppercase tracking-wide text-ink-muted">
              {t("settings.roles.presetPickerBuiltin")}
            </p>
            <div className="flex flex-col gap-0.5">
              {builtinRoles.map(renderRow)}
            </div>
          </div>
        )}
        {customRoles.length > 0 && (
          <div>
            <p className="mb-1 text-[10px] font-semibold uppercase tracking-wide text-ink-muted">
              {t("settings.roles.presetPickerCustom")}
            </p>
            <div className="flex flex-col gap-0.5">
              {customRoles.map(renderRow)}
            </div>
          </div>
        )}
        {roles.length === 0 && (
          <p className="text-xs text-ink-muted">{t("settings.roles.presetPickerEmpty")}</p>
        )}
        <div className="mt-2 flex justify-end">
          <button
            type="button"
            onClick={onClose}
            className="rounded border border-ink-muted px-2 py-0.5 text-xs text-ink-muted hover:bg-surface-overlay focus-visible:ring-1 focus-visible:ring-ink-accent"
          >
            {t("common.cancel")}
          </button>
        </div>
      </div>
    </div>
  );
}
