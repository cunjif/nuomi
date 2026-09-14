/**
 * ExtensionsPanel tests: the enable toggles and — the reported bug — the
 * "Extensions Center" button must actually leave the editor area, otherwise
 * Shell keeps rendering <EditorArea /> and nothing appears to happen.
 */
import { fireEvent, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { i18n } from "../../i18n";
import { useUiStore } from "../store/uiStore";
import {
  getRegisteredEditorExtensions,
  registerEditorExtension,
  resetEditorExtensionsForTest,
} from "./index";
import { ExtensionsPanel } from "./ExtensionsPanel";

/** The toolbar trigger (the popover's own buttons also contain "Extensions"). */
const TRIGGER_NAME = "🧩 Extensions";

beforeEach(async () => {
  await i18n.changeLanguage("en");
  resetEditorExtensionsForTest();
  useUiStore.setState({ view: "chat", activeArea: "workbench", workbenchSubTab: "editor" });
});

describe("ExtensionsPanel", () => {
  it("opens the Extensions Center (leaving the editor area) from the editor toolbar", () => {
    renderWithProviders(<ExtensionsPanel />);
    fireEvent.click(screen.getByRole("button", { name: TRIGGER_NAME }));
    fireEvent.click(screen.getByRole("button", { name: /extensions center/i }));

    // Regression: activeArea must flip too — Shell renders the editor while
    // it is "editor" and would ignore the new view.
    expect(useUiStore.getState().view).toBe("plugins");
    expect(useUiStore.getState().activeArea).toBe("chat");
  });

  it("lists registered extensions with an enable toggle", () => {
    registerEditorExtension({
      id: "builtin.preview-markdown",
      titleI18nKey: "editor.ext.previewMarkdown",
      contribute: () => {},
    });
    renderWithProviders(<ExtensionsPanel />);
    fireEvent.click(screen.getByRole("button", { name: TRIGGER_NAME }));

    expect(screen.getByText("Markdown preview")).toBeInTheDocument();
    expect(getRegisteredEditorExtensions().map((e) => e.id)).toContain("builtin.preview-markdown");
    // Toggle flips the shared enable state (app_settings-backed).
    fireEvent.click(screen.getByRole("button", { name: /^on$/i }));
    expect(screen.getByRole("button", { name: /^off$/i })).toBeInTheDocument();
  });
});
