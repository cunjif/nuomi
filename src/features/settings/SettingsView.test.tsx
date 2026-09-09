/**
 * SettingsView tab bar tests: sections switch through the top nav instead of
 * stacking on one long page (and only the active section mounts).
 */
import { fireEvent, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { i18n } from "../../i18n";
import { SettingsView } from "./SettingsView";

beforeEach(async () => {
  await i18n.changeLanguage("en");
});

describe("SettingsView — top tab navigation", () => {
  it("renders the section tabs and mounts only the active panel", async () => {
    renderWithProviders(<SettingsView />);

    const tablist = await screen.findByRole("tablist", { name: /settings sections/i });
    expect(tablist).toBeInTheDocument();
    // Default tab: Providers — the provider list panel is visible.
    expect(await screen.findByRole("button", { name: /\+ add provider/i })).toBeInTheDocument();

    // Switch to Roles: the Roles panel replaces the provider list.
    fireEvent.click(screen.getByRole("tab", { name: /^Roles$/ }));
    expect(await screen.findByRole("heading", { name: "Roles" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /\+ add provider/i })).toBeNull();

    // Tools & Access hosts the sensitive-tools allowlist.
    fireEvent.click(screen.getByRole("tab", { name: /tools & access/i }));
    expect(await screen.findByRole("heading", { name: /sensitive tools/i })).toBeInTheDocument();
  });
});
