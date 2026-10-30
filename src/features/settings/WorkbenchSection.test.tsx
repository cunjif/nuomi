/**
 * WorkbenchSection tests: three independent feature toggles render and
 * flip via the editor-ext enable mechanism (workbench_enhance DFX 4.4.2).
 */
import { fireEvent, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { i18n } from "../../i18n";
import {
  isEditorExtensionEnabled,
  resetEditorExtensionsForTest,
  setEditorExtensionEnabled,
} from "../../lib/editor-ext";
import { WorkbenchSection } from "./WorkbenchSection";

beforeEach(async () => {
  resetEditorExtensionsForTest();
  await i18n.changeLanguage("en");
});

describe("WorkbenchSection — feature toggles", () => {
  it("renders three toggle entries, all enabled by default", async () => {
    renderWithProviders(<WorkbenchSection />);

    expect(await screen.findByText("In-editor compile & run")).toBeInTheDocument();
    expect(screen.getByText("Drag external files to temp workspace")).toBeInTheDocument();
    expect(screen.getByText("Enhanced hover annotations")).toBeInTheDocument();

    // All three default to enabled.
    const enabledLabels = screen.getAllByText("Enabled");
    expect(enabledLabels).toHaveLength(3);
  });

  it("toggles a feature off and persists via the enable mechanism", async () => {
    renderWithProviders(<WorkbenchSection />);

    expect(isEditorExtensionEnabled("workbench.runPanel")).toBe(true);

    // Click the "In-editor compile & run" toggle button.
    const toggleBtn = screen.getAllByRole("button", { name: "In-editor compile & run" })[0]!;
    fireEvent.click(toggleBtn);

    expect(isEditorExtensionEnabled("workbench.runPanel")).toBe(false);
    // The other two remain enabled.
    expect(isEditorExtensionEnabled("workbench.tempWorkspace")).toBe(true);
    expect(isEditorExtensionEnabled("workbench.enhancedHover")).toBe(true);
  });

  it("toggles are independent — disabling one does not affect others", async () => {
    setEditorExtensionEnabled("workbench.tempWorkspace", false);
    renderWithProviders(<WorkbenchSection />);

    expect(isEditorExtensionEnabled("workbench.runPanel")).toBe(true);
    expect(isEditorExtensionEnabled("workbench.tempWorkspace")).toBe(false);
    expect(isEditorExtensionEnabled("workbench.enhancedHover")).toBe(true);

    // Toggle enhancedHover off — tempWorkspace stays off, runPanel stays on.
    const hoverBtn = screen.getAllByRole("button", { name: "Enhanced hover annotations" })[0]!;
    fireEvent.click(hoverBtn);

    expect(isEditorExtensionEnabled("workbench.runPanel")).toBe(true);
    expect(isEditorExtensionEnabled("workbench.tempWorkspace")).toBe(false);
    expect(isEditorExtensionEnabled("workbench.enhancedHover")).toBe(false);
  });
});
