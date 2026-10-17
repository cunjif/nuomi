import { useState, useEffect } from "react";
import type { ReactNode } from "react";
import { useMutation, useQueryClient, useQuery } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import type { BindingConfigDto, BindingModeDto } from "../../lib/ipc/bindings.gen";

export interface BindingEditorProps {
  sessionId: string;
  binding: BindingConfigDto;
}

export function BindingEditor({ sessionId, binding }: BindingEditorProps): ReactNode {
  const qc = useQueryClient();
  const [mode, setMode] = useState<BindingModeDto>(binding.mode);
  const [providerId, setProviderId] = useState<string | null>(binding.providerId);
  const [agentProfileId, setAgentProfileId] = useState<string | null>(binding.agentProfileId);

  useEffect(() => {
    setMode(binding.mode);
    setProviderId(binding.providerId);
    setAgentProfileId(binding.agentProfileId);
  }, [binding]);

  const providersQuery = useQuery({
    queryKey: ["providers"],
    queryFn: () => ipc.listProviders(),
  });

  const agentProfilesQuery = useQuery({
    queryKey: ["agentProfiles"],
    queryFn: () => ipc.listAgentProfiles(),
  });

  const saveMut = useMutation({
    mutationFn: () => ipc.stewardUpdateBinding(sessionId, mode, providerId, agentProfileId),
    onSuccess: () => {
      toast.success("绑定已更新");
      void qc.invalidateQueries({ queryKey: ["steward"] });
    },
    onError: (e) => toast.error(`绑定失败: ${e instanceof Error ? e.message : String(e)}`),
  });

  const isDirty =
    mode !== binding.mode ||
    providerId !== binding.providerId ||
    agentProfileId !== binding.agentProfileId;

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium text-ink">绑定</h3>
        <button
          type="button"
          className="pixel-fill-accent rounded px-2 py-0.5 text-xs text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
          onClick={() => saveMut.mutate()}
          disabled={saveMut.isPending || !isDirty}
        >
          {saveMut.isPending ? "保存中..." : "保存"}
        </button>
      </div>

      <div>
        <label className="mb-1 block text-xs text-ink-muted">绑定模式</label>
        <select
          className="sketch-input w-full bg-surface px-2 py-1 text-sm text-ink outline-none"
          value={mode}
          onChange={(e) => setMode(e.target.value as BindingModeDto)}
        >
          <option value="none">未绑定</option>
          <option value="provider">Provider</option>
          <option value="cli">CLI Agent</option>
        </select>
      </div>

      {mode === "provider" && (
        <div>
          <label className="mb-1 block text-xs text-ink-muted">Provider</label>
          <select
            className="sketch-input w-full bg-surface px-2 py-1 text-sm text-ink outline-none"
            value={providerId ?? ""}
            onChange={(e) => setProviderId(e.target.value || null)}
          >
            <option value="">选择 Provider...</option>
            {(providersQuery.data ?? []).map((p) => (
              <option key={p.id} value={p.id}>{p.name}</option>
            ))}
          </select>
          {providersQuery.isLoading && (
            <p className="mt-1 text-xs text-ink-muted">加载中...</p>
          )}
        </div>
      )}

      {mode === "cli" && (
        <div>
          <label className="mb-1 block text-xs text-ink-muted">CLI Agent</label>
          <select
            className="sketch-input w-full bg-surface px-2 py-1 text-sm text-ink outline-none"
            value={agentProfileId ?? ""}
            onChange={(e) => setAgentProfileId(e.target.value || null)}
          >
            <option value="">选择 CLI Agent...</option>
            {(agentProfilesQuery.data ?? []).map((a) => (
              <option key={a.id} value={a.id}>{a.name}</option>
            ))}
          </select>
          {agentProfilesQuery.isLoading && (
            <p className="mt-1 text-xs text-ink-muted">加载中...</p>
          )}
        </div>
      )}
    </div>
  );
}
