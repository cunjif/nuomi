import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { fitChips, SourceChipRow } from "./SourceChipRow";
import { SourceListDialog } from "./SourceListDialog";
import { SourceEvidence } from "./SourceEvidence";
import type { SourceEntryDto } from "../../lib/events/types";
import "../../i18n";

function source(kind: SourceEntryDto["kind"], label: string, target = label): SourceEntryDto {
  return { kind, target, label, agentId: "r1", agentName: "R" };
}

describe("fitChips — 50% budget overflow math", () => {
  it("fits all labels within the budget", () => {
    const got = fitChips([source("workspace_file", "a.rs"), source("web_url", "b.com")], 400);
    expect(got).toEqual(["a.rs", "b.com"]);
  });

  it("collapses overflowing labels into a single ··· chip", () => {
    const many = Array.from({ length: 20 }, (_, i) => source("workspace_file", `file${i}.rs`));
    const got = fitChips(many, 180);
    expect(got.length).toBeGreaterThan(0);
    expect(got.length).toBeLessThan(20);
    expect(got.at(-1)).toBe("···");
    expect(got.slice(0, -1).every((l) => l !== "···")).toBe(true);
  });

  it("shows at least the overflow chip when nothing fits", () => {
    const got = fitChips([source("workspace_file", "averylongfilename.rs")], 10);
    expect(got).toEqual(["···"]);
  });
});

describe("SourceChipRow — right-aligned clickable strip", () => {
  it("renders chips and is one clickable whole", () => {
    const onOpenList = vi.fn();
    render(
      <SourceChipRow
        sources={[source("workspace_file", "a.rs"), source("web_url", "b.com")]}
        bubbleWidth={400}
        onOpenList={onOpenList}
      />,
    );
    const row = screen.getByTestId("source-chip-row");
    expect(row.className).toContain("justify-end");
    expect(row.className).toContain("max-w-[50%]");
    expect(screen.getAllByTestId("source-chip").length).toBe(2);
    fireEvent.click(row);
    expect(onOpenList).toHaveBeenCalledTimes(1);
  });

  it("renders nothing for an empty list", () => {
    const { container } = render(
      <SourceChipRow sources={[]} bubbleWidth={400} onOpenList={() => {}} />,
    );
    expect(container.firstChild).toBeNull();
  });
});

describe("SourceListDialog — per-entry list with open actions", () => {
  it("lists every source with full targets and open buttons", () => {
    const onNavigate = vi.fn();
    render(
      <SourceListDialog
        open={true}
        sources={[
          source("workspace_file", "a.rs", "C:\\proj\\src\\a.rs"),
          source("web_url", "docs", "https://example.com/docs"),
          source("cli_agent", "Codex", "ap-1"),
        ]}
        onClose={() => {}}
        onNavigate={onNavigate}
      />,
    );
    expect(screen.getByTestId("source-list").children.length).toBe(3);
    expect(screen.getByText("C:\\proj\\src\\a.rs")).not.toBeNull();
    expect(screen.getByText("https://example.com/docs")).not.toBeNull();
    const openButtons = screen.getAllByRole("button", { name: "打开" });
    expect(openButtons.length).toBe(3);
    // cli_agent entry is disabled (no jump target).
    expect(openButtons[2]?.hasAttribute("disabled")).toBe(true);
    const firstOpen = openButtons[0];
    if (firstOpen === undefined) throw new Error("first open button missing");
    fireEvent.click(firstOpen);
    expect(onNavigate).toHaveBeenCalledTimes(1);
    expect(onNavigate.mock.calls[0]?.[0].target).toBe("C:\\proj\\src\\a.rs");
  });
});

describe("SourceEvidence — chip row opens the dialog (≤200ms mount)", () => {
  it("opens the list dialog on strip click", () => {
    const start = performance.now();
    render(<SourceEvidence sources={[source("workspace_file", "a.rs")]} />);
    fireEvent.click(screen.getByTestId("source-chip-row"));
    expect(screen.getByTestId("source-list")).not.toBeNull();
    expect(performance.now() - start).toBeLessThan(200);
  });

  it("renders nothing without sources", () => {
    const { container } = render(<SourceEvidence sources={[]} />);
    expect(container.firstChild).toBeNull();
  });
});
