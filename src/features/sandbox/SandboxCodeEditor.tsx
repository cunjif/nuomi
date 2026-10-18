import type { ReactNode } from "react";
import { lazy, Suspense } from "react";
import { Spinner } from "../../components/ui/Spinner";
import { useTheme } from "../../lib/store/useTheme";
import { defineNuomiThemes, monacoThemeFor } from "../shell/monacoThemes";

// Self-hosted Monaco (offline desktop): the setup module is pulled in
// lazily together with the editor chunk — mirrors MonacoTab's wiring.
const MonacoEditor = lazy(async () => {
  await import("../shell/monacoSetup");
  return import("@monaco-editor/react");
});

interface SandboxCodeEditorProps {
  value: string;
  onChange: (value: string) => void;
  /** Monaco language id, e.g. "python" / "javascript". */
  language: string;
  /** Editor height (Tailwind class), default `h-64`. */
  heightClass?: string;
  ariaLabel: string;
}

/**
 * Lightweight code editor for sandbox forms: a controlled Monaco instance
 * with the nuomi themes applied. No file binding, outline or save flow —
 * those belong to the workspace editor (MonacoTab).
 */
export function SandboxCodeEditor({
  value,
  onChange,
  language,
  heightClass = "h-64",
  ariaLabel,
}: SandboxCodeEditorProps): ReactNode {
  const { theme } = useTheme();
  return (
    <div
      className={`overflow-hidden rounded-[12px_255px_15px_225px/225px_15px_255px_12px] border border-dashed border-ink-muted/40 ${heightClass}`}
      role="textbox"
      aria-label={ariaLabel}
    >
      <Suspense fallback={<div className="m-4"><Spinner /></div>}>
        <MonacoEditor
          value={value}
          onChange={(v) => onChange(v ?? "")}
          language={language}
          theme={monacoThemeFor(theme)}
          beforeMount={(monaco) => defineNuomiThemes(monaco)}
          options={{
            minimap: { enabled: false },
            scrollBeyondLastLine: false,
            fontSize: 12,
            lineNumbers: "on",
            automaticLayout: true,
            wordWrap: "on",
            padding: { top: 8 },
          }}
          height="100%"
        />
      </Suspense>
    </div>
  );
}
