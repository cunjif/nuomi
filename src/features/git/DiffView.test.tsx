/**
 * DiffView colour semantics: added rows are blue (`diff-add`), removed rows
 * are red (`danger`). Pinned because it is an explicit product decision —
 * red/blue stays separable under red-green colour-vision deficiencies —
 * while the palette values themselves are guarded separately by
 * src/styles/themeContrast.test.ts.
 */
import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { i18n } from "../../i18n";
import { DiffView } from "./DiffView";

const DIFF = `diff --git a/a.txt b/a.txt
--- a/a.txt
+++ b/a.txt
@@ -1,3 +1,3 @@
 context line
-removed line
+added line
`;

beforeEach(async () => {
  await i18n.changeLanguage("en");
});

describe("DiffView", () => {
  it("renders added rows in blue and removed rows in red", () => {
    const { container } = render(<DiffView diffText={DIFF} />);
    const rows = Array.from(container.querySelectorAll("div.flex.items-start"));
    expect(rows).toHaveLength(3);

    const [context, removed, added] = rows as [HTMLElement, HTMLElement, HTMLElement];

    // Added → blue sign + blue tint.
    expect(added.className).toContain("bg-diff-add/10");
    expect(added.className).not.toContain("state-ok");
    expect(added.querySelector("span.text-diff-add")?.textContent).toBe("+");

    // Removed → red sign + red tint.
    expect(removed.className).toContain("bg-danger/10");
    expect(removed.querySelector("span.text-danger")?.textContent).toBe("-");

    // Context rows stay untinted.
    expect(context.className).not.toContain("bg-diff-add");
    expect(context.className).not.toContain("bg-danger");
  });

  it("renders the empty state for an empty diff", () => {
    render(<DiffView diffText="" />);
    expect(screen.getByText("No changes")).toBeInTheDocument();
  });

  it("drops the no-newline marker without disturbing line numbers", () => {
    // Marker lines sit between real lines on the old and new side; treating
    // them as content used to render a bogus row AND bump both counters, so
    // the following added line was numbered one too high.
    const withMarkers = `diff --git a/a.txt b/a.txt
--- a/a.txt
+++ b/a.txt
@@ -1,2 +1,2 @@
 context
-old last
\\ No newline at end of file
+new last
\\ No newline at end of file
`;
    const { container } = render(<DiffView diffText={withMarkers} />);

    expect(screen.queryByText(/No newline at end of file/)).toBeNull();

    const rows = Array.from(container.querySelectorAll("div.flex.items-start"));
    expect(rows).toHaveLength(3); // context + removed + added, no marker rows

    const added = rows[2] as HTMLElement;
    const numbers = Array.from(added.querySelectorAll("span")).slice(0, 2).map((s) => s.textContent);
    // new-side number is 2 (1=context, 2=added) — the old bug produced 3.
    expect(numbers).toEqual(["", "2"]);
  });
});
