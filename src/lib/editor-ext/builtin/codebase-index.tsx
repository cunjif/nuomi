/**
 * Builtin codebase-index extension — the UI mount point for MCP integration.
 * The button invokes the agreed `codebase_index` backend command; the
 * current Rust kernel does not implement it, so the failure path toasts
 * guidance instead: configure codebase-memory-mcp via MCP and let the Agent
 * run `index_repository`. MCP INTEGRATION POINT: when the kernel (or an MCP
 * bridge plugin) exposes the command, this button starts working without
 * any UI change.
 */
import type { ReactNode } from "react";
import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import type { EditorExtContext } from "../types";
import { toast } from "../../../lib/store/toastStore";

export const EXT_ID = "builtin.codebase-index";

function IndexButton(): ReactNode {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  return (
    <button
      type="button"
      disabled={busy}
      onClick={() => {
        setBusy(true);
        void invoke("codebase_index")
          .then(() => toast.success(t("editor.indexStarted")))
          .catch(() => toast.error(t("editor.indexUnavailable")))
          .finally(() => setBusy(false));
      }}
      className="rounded border border-ink-muted/40 px-2 py-0.5 text-xs text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
    >
      🔍 {t("editor.indexButton")}
    </button>
  );
}

export const codebaseIndexExtension = {
  id: EXT_ID,
  titleI18nKey: "editor.ext.codebaseIndex",
  contribute(ctx: EditorExtContext): void {
    ctx.registerToolbarAction({ id: `${EXT_ID}.index`, Component: IndexButton });
    ctx.reportCapability("codebase.index");
  },
} as const;
