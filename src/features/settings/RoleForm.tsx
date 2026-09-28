import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { CapabilityDto, RoleDirectorBindingDto, RoleInput } from "../../lib/ipc/bindings.gen";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { fieldClass as field } from "../../components/ui/Field";
import { PresetRolePicker } from "./PresetRolePicker";
import type { PresetRoleSelection } from "./PresetRolePicker";
import { RoleDirectorBindingPanel } from "./RoleDirectorBindingPanel";

/** Binding mode of the role form: unbound ("默认"), provider or CLI agent profile. */
export type BindingMode = "none" | "provider" | "cli";

/** Capability choices shown as checkboxes (reasoning/image/voice/video). */
export const CAPABILITY_KEYS: ReadonlyArray<{ key: CapabilityDto; labelKey: string }> = [
  { key: "reasoning", labelKey: "capability.reasoning" },
  { key: "image", labelKey: "capability.image" },
  { key: "voice", labelKey: "capability.voice" },
  { key: "video", labelKey: "capability.video" },
];

export { readAgentProfileId } from "../../lib/conversation/roleReady";

/** Role Director dialog: plain-language description → generated Role. */
function RoleDirectorDialog({ onClose }: { onClose: () => void }): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [description, setDescription] = useState("");
  const [binding, setBinding] = useState<RoleDirectorBindingDto | null>(null);

  const generateMut = useMutation({
    mutationFn: (desc: string) => {
      if (!binding) {
        return Promise.reject(new Error("role.no_binding"));
      }
      return ipc.generateRole(desc, binding);
    },
    onSuccess: (role) => {
      void qc.invalidateQueries({ queryKey: ["roles"] });
      toast.success(
        t("settings.roles.directorSuccess", { name: role.name }) +
          " " +
          t("settings.roles.directorBindReminder"),
      );
      onClose();
    },
    onError: (e) => {
      const msg = describeError(e);
      if (msg.includes("role.no_binding")) {
        toast.error(t("settings.roles.directorNoBinding"));
      } else {
        toast.error(`${t("settings.roles.directorFailed")}: ${msg}`);
      }
    },
  });

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
      role="dialog"
      aria-modal="true"
      aria-label={t("settings.roles.directorTitle")}
    >
      <div className="relative w-full max-w-md rounded border border-ink-muted/40 bg-surface-raised p-4">
        {/* Self-binding panel: top-right of the dialog. */}
        <div className="absolute right-2 top-2 w-44">
          <RoleDirectorBindingPanel onBindingChange={setBinding} />
        </div>
        <h4 className="mb-2 text-sm font-semibold text-ink">
          {t("settings.roles.directorTitle")}
        </h4>
        <p className="mb-2 text-xs text-ink-muted">{t("settings.roles.directorHint")}</p>
        <textarea
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          rows={4}
          autoFocus
          placeholder={t("settings.roles.directorPlaceholder") ?? ""}
          aria-label={t("settings.roles.directorTitle")}
          className="w-full rounded border border-ink-muted/40 bg-surface px-2 py-1 text-sm text-ink placeholder:text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent"
        />
        {binding === null && (
          <p className="mt-1 text-xs text-ink-muted/70">
            {t("settings.roles.directorBindingRequired")}
          </p>
        )}
        <div className="mt-3 flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded border border-ink-muted px-3 py-1 text-sm text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            {t("common.cancel")}
          </button>
          <button
            type="button"
            disabled={generateMut.isPending || description.trim().length === 0 || binding === null}
            onClick={() => generateMut.mutate(description.trim())}
            className="pixel-fill-accent px-3 py-1 text-sm text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
          >
            {generateMut.isPending ? t("settings.roles.directorGenerating") : t("settings.roles.directorGenerate")}
          </button>
        </div>
      </div>
    </div>
  );
}

/** Add form for roles — pure template definition (no binding). */
export function RoleForm(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [name, setName] = useState("");
  const [systemPromptOverride, setSystemPromptOverride] = useState("");
  const [requiredCapabilities, setRequiredCapabilities] = useState<CapabilityDto[]>([]);
  const [presetOpen, setPresetOpen] = useState(false);
  const [directorOpen, setDirectorOpen] = useState(false);

  const saveMut = useMutation({
    mutationFn: (input: RoleInput) => ipc.upsertRole(input),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["roles"] });
      toast.success(t("settings.roles.saved"));
      setName("");
      setSystemPromptOverride("");
      setRequiredCapabilities([]);
    },
    onError: (e) => {
      toast.error(`${t("settings.roles.saveFailed")}: ${describeError(e)}`);
    },
  });

  const applyPreset = (preset: PresetRoleSelection) => {
    setName(preset.name);
    setSystemPromptOverride(preset.systemPromptOverride ?? "");
    setRequiredCapabilities(preset.requiredCapabilities);
  };

  return (
    <form
      aria-label={t("settings.roles.heading")}
      className="mb-3 rounded border border-ink-muted/40 bg-surface-raised p-3"
      onSubmit={(e) => {
        e.preventDefault();
        if (!name.trim() || saveMut.isPending) return;
        saveMut.mutate({
          name: name.trim(),
          providerId: null,
          providerIds: [],
          systemPromptOverride:
            systemPromptOverride.trim().length > 0 ? systemPromptOverride.trim() : null,
          toolAllowlist: [],
          requiredCapabilities,
          temperature: null,
          maxTokens: null,
          params: {},
        });
      }}
    >
      <div className="mb-2 flex items-center justify-between">
        <p className="text-xs text-ink-muted">{t("settings.roles.idempotentHint")}</p>
        <div className="flex gap-2">
          <button
            type="button"
            onClick={() => setPresetOpen(true)}
            className="rounded border border-ink-muted px-2 py-0.5 text-xs text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            {t("settings.roles.presetOpen")}
          </button>
          <button
            type="button"
            onClick={() => setDirectorOpen(true)}
            className="rounded border border-ink-accent px-2 py-0.5 text-xs text-ink-accent hover:bg-ink-accent/10 focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            {t("settings.roles.directorOpen")}
          </button>
        </div>
      </div>
      <div className="flex flex-wrap items-end gap-2">
        <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
          {t("settings.roles.name")}
          <input value={name} onChange={(e) => setName(e.target.value)} required className={`${field} bg-surface text-sm w-40`} />
        </label>
      </div>
      <div className="mt-2 flex flex-wrap items-center gap-1.5">
        <span className="text-xs text-ink-muted">{t("settings.roles.requiredCapabilities")}</span>
        {CAPABILITY_KEYS.map((cap) => {
          const active = requiredCapabilities.includes(cap.key);
          return (
            <label
              key={cap.key}
              className={`flex cursor-pointer items-center gap-1 rounded-full border px-2 py-0.5 text-xs focus-within:ring-2 focus-within:ring-ink-accent ${
                active
                  ? "border-ink-accent bg-ink-accent/20 text-ink"
                  : "border-ink-muted/40 text-ink-muted hover:bg-surface-overlay"
              }`}
            >
              <input
                type="checkbox"
                className="sr-only"
                checked={active}
                onChange={() =>
                  setRequiredCapabilities((prev) =>
                    prev.includes(cap.key)
                      ? prev.filter((c) => c !== cap.key)
                      : [...prev, cap.key],
                  )
                }
              />
              {t(cap.labelKey)}
            </label>
          );
        })}
      </div>
      <label className="mt-2 flex min-w-48 flex-1 flex-col gap-0.5 text-xs text-ink-muted">
        {t("settings.roles.systemPromptOverride")}
        <textarea
          value={systemPromptOverride}
          onChange={(e) => setSystemPromptOverride(e.target.value)}
          rows={3}
          spellCheck={false}
          className={`${field} bg-surface text-sm font-mono`}
        />
      </label>
      <button
        type="submit"
        disabled={saveMut.isPending}
        className="mt-2 pixel-fill-accent px-3 py-1.5 text-sm text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
      >
        {saveMut.isPending ? t("settings.roles.saving") : t("settings.roles.save")}
      </button>
      {presetOpen && (
        <PresetRolePicker
          onSelect={applyPreset}
          onClose={() => setPresetOpen(false)}
        />
      )}
      {directorOpen && <RoleDirectorDialog onClose={() => setDirectorOpen(false)} />}
    </form>
  );
}
