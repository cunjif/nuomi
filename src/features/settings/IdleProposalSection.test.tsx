/**
 * IdleProposalSection tests: the two user-facing settings keys round-trip
 * through the IPC test double (ADR 0023 — enabled + timeout only; backlog
 * threshold and cooldown stay backend defaults).
 */
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { i18n } from "../../i18n";
import { tdState } from "../../lib/ipc/test-double";
import { IdleProposalSection } from "./IdleProposalSection";

beforeEach(async () => {
  await i18n.changeLanguage("en");
});

describe("IdleProposalSection — two-key settings", () => {
  it("renders the persisted defaults", async () => {
    renderWithProviders(<IdleProposalSection />);

    const enabled = await screen.findByLabelText(/draft team proposals while idle/i);
    await waitFor(() => expect(enabled).toBeChecked());
    expect(screen.getByLabelText(/idle threshold/i)).toHaveValue(15);
  });

  it("persists the enabled toggle through setIdleProposalSettings", async () => {
    renderWithProviders(<IdleProposalSection />);

    const enabled = await screen.findByLabelText(/draft team proposals while idle/i);
    await waitFor(() => expect(enabled).toBeChecked());
    fireEvent.click(enabled);
    await waitFor(() => expect(tdState.idleProposalSettings.enabled).toBe(false));
    await waitFor(() => expect(enabled).not.toBeChecked());
  });

  it("persists the timeout value", async () => {
    renderWithProviders(<IdleProposalSection />);

    const timeout = await screen.findByLabelText(/idle threshold/i);
    await waitFor(() => expect(timeout).toHaveValue(15));
    fireEvent.change(timeout, { target: { value: "25" } });
    await waitFor(() => expect(tdState.idleProposalSettings.timeoutMinutes).toBe(25));
    await waitFor(() => expect(screen.getByLabelText(/idle threshold/i)).toHaveValue(25));
  });

  it("ignores non-positive timeout input", async () => {
    renderWithProviders(<IdleProposalSection />);

    const timeout = await screen.findByLabelText(/idle threshold/i);
    await waitFor(() => expect(timeout).toHaveValue(15));
    fireEvent.change(timeout, { target: { value: "0" } });
    await waitFor(() => expect(timeout).toHaveValue(15));
    expect(tdState.idleProposalSettings.timeoutMinutes).toBe(15);
  });
});
