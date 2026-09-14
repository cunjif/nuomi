import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { EmptyState } from "../../components/ui/EmptyState";

type LineType = "context" | "added" | "removed";

interface DiffLine {
  type: LineType;
  content: string;
  oldLineNum: number | null;
  newLineNum: number | null;
}

interface DiffHunk {
  header: string;
  lines: DiffLine[];
}

interface ParsedDiff {
  oldPath: string;
  newPath: string;
  hunks: DiffHunk[];
  isBinary: boolean;
}

export interface DiffViewProps {
  diffText: string;
  className?: string;
}

function parseDiff(diffText: string): ParsedDiff | null {
  if (!diffText.trim()) return null;
  const lines = diffText.split("\n");
  let oldPath = "";
  let newPath = "";
  let isBinary = false;
  const hunks: DiffHunk[] = [];
  let currentHunk: DiffHunk | null = null;
  let oldLineNum = 0;
  let newLineNum = 0;

  for (const line of lines) {
    // A bare "" only ever comes from the trailing newline of the diff text —
    // git renders an empty source line as " " (space + empty), so this is the
    // split artifact, not content. Without this it would render one extra
    // numbered blank row at the end of every hunk.
    if (line === "") continue;
    // "\ No newline at end of file" is a marker, not content: git only ever
    // emits a raw backslash here (real content is prefixed with " "/"+").
    // Skipping it without touching the counters keeps every following line's
    // number correct.
    if (line.startsWith("\\")) continue;
    if (line.startsWith("diff --git")) {
      continue;
    } else if (line.startsWith("--- ")) {
      oldPath = line.slice(4);
    } else if (line.startsWith("+++ ")) {
      newPath = line.slice(4);
    } else if (line.startsWith("Binary files")) {
      isBinary = true;
    } else if (line.startsWith("@@")) {
      const match = line.match(/@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
      if (match) {
        oldLineNum = parseInt(match[1] ?? "0", 10);
        newLineNum = parseInt(match[2] ?? "0", 10);
      }
      currentHunk = { header: line, lines: [] };
      hunks.push(currentHunk);
    } else if (currentHunk && !isBinary) {
      if (line.startsWith("-")) {
        currentHunk.lines.push({
          type: "removed",
          content: line.slice(1),
          oldLineNum: oldLineNum,
          newLineNum: null,
        });
        oldLineNum++;
      } else if (line.startsWith("+")) {
        currentHunk.lines.push({
          type: "added",
          content: line.slice(1),
          oldLineNum: null,
          newLineNum: newLineNum,
        });
        newLineNum++;
      } else {
        const content = line.startsWith(" ") ? line.slice(1) : line;
        currentHunk.lines.push({
          type: "context",
          content,
          oldLineNum: oldLineNum,
          newLineNum: newLineNum,
        });
        oldLineNum++;
        newLineNum++;
      }
    }
  }

  if (hunks.length === 0 && !isBinary) return null;
  return { oldPath, newPath, hunks, isBinary };
}

// Row tint + sign ink per line kind. Added = blue (`diff-add`), not the green
// `state-ok`: red/blue survives the common red-green colour-vision
// deficiencies, so a change cannot be read as "unchanged". Removed keeps the
// app's single red (`state.danger`) for consistency with every other
// destructive affordance.
const LINE_STYLES: Record<LineType, string> = {
  context: "",
  added: "bg-diff-add/10",
  removed: "bg-danger/10",
};

const SIGN_STYLES: Record<LineType, string> = {
  context: "text-ink-muted/40",
  added: "text-diff-add",
  removed: "text-danger",
};

const SIGNS: Record<LineType, string> = {
  context: " ",
  added: "+",
  removed: "-",
};

export function DiffView({ diffText, className = "" }: DiffViewProps): ReactNode {
  const { t } = useTranslation();
  const parsed = parseDiff(diffText);

  if (parsed === null) {
    return (
      <EmptyState
        icon="diff"
        title={t("git.diffEmpty")}
        hint={t("git.diffEmptyHint")}
        className={className}
      />
    );
  }

  if (parsed.isBinary) {
    return (
      <EmptyState
        icon="file"
        title={t("git.diffBinary")}
        className={className}
      />
    );
  }

  return (
    <div className={`overflow-x-auto font-mono text-xs leading-relaxed ${className}`}>
      {parsed.hunks.map((hunk, hi) => (
        <div key={hi}>
          <div className="border-y border-dashed border-ink-muted/30 bg-surface-overlay px-3 py-1 text-ink-muted">
            {hunk.header}
          </div>
          {hunk.lines.map((line, li) => (
            <div
              key={li}
              className={`flex items-start ${LINE_STYLES[line.type]} px-3`}
            >
              <span className="w-10 shrink-0 select-none pr-2 text-right text-ink-muted/60">
                {line.oldLineNum ?? ""}
              </span>
              <span className="w-10 shrink-0 select-none pr-2 text-right text-ink-muted/60">
                {line.newLineNum ?? ""}
              </span>
              <span className={`w-4 shrink-0 select-none ${SIGN_STYLES[line.type]}`}>
                {SIGNS[line.type]}
              </span>
              <span className="whitespace-pre text-ink">{line.content}</span>
            </div>
          ))}
        </div>
      ))}
    </div>
  );
}
