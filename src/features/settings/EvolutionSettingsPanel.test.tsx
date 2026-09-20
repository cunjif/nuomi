/**
 * EvolutionSettingsPanel tests: four-dimension config panel renders all
 * cards and the authorization toggle interacts with the IPC layer.
 */
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { injectIpcCommands } from "../../lib/ipc/client";
import { renderWithProviders } from "../../test/helpers";
import { i18n } from "../../i18n";
import { EvolutionSettingsPanel } from "./EvolutionSettingsPanel";

const ok = <T,>(data: T) => ({ status: "ok" as const, data });

beforeEach(async () => {
  await i18n.changeLanguage("en");
});

describe("EvolutionSettingsPanel — four-dimension config", () => {
  it("renders all four dimension cards", async () => {
    injectIpcCommands({
      getOnlineAuthorized: vi.fn().mockResolvedValue(ok(false)),
      setOnlineAuthorized: vi.fn().mockResolvedValue(ok(null)),
    } as never);
    renderWithProviders(<EvolutionSettingsPanel />);

    expect(await screen.findByText("Online Learning Sources")).toBeInTheDocument();
    expect(screen.getByText("Reflection Parameters")).toBeInTheDocument();
    expect(screen.getByText("Auto Skill Creation")).toBeInTheDocument();
    expect(screen.getByText("Persistent Memory Policy")).toBeInTheDocument();
  });

  it("renders the authorization toggle and fires IPC on change", async () => {
    const setAuth = vi.fn().mockResolvedValue(ok(null));
    injectIpcCommands({
      getOnlineAuthorized: vi.fn().mockResolvedValue(ok(false)),
      setOnlineAuthorized: setAuth,
    } as never);
    renderWithProviders(<EvolutionSettingsPanel />);

    const checkbox = await screen.findByRole("checkbox", { name: /allow online learning/i });
    expect(checkbox).not.toBeChecked();

    fireEvent.click(checkbox);
    await waitFor(() => expect(setAuth).toHaveBeenCalledWith(true));
  });

  it("renders refine parameter controls with default values", async () => {
    injectIpcCommands({
      getOnlineAuthorized: vi.fn().mockResolvedValue(ok(false)),
      setOnlineAuthorized: vi.fn().mockResolvedValue(ok(null)),
    } as never);
    renderWithProviders(<EvolutionSettingsPanel />);

    const triggerInput = await screen.findByLabelText(/trigger failures/i);
    expect(triggerInput).toHaveValue(3);

    const strategySelect = screen.getByLabelText(/min edit strategy/i);
    expect(strategySelect).toHaveValue("prompt_note");

    const rollbackCheckbox = screen.getByRole("checkbox", { name: /enable rollback/i });
    expect(rollbackCheckbox).toBeChecked();
  });

  it("renders memory policy controls with default values", async () => {
    injectIpcCommands({
      getOnlineAuthorized: vi.fn().mockResolvedValue(ok(false)),
      setOnlineAuthorized: vi.fn().mockResolvedValue(ok(null)),
    } as never);
    renderWithProviders(<EvolutionSettingsPanel />);

    const retentionInput = await screen.findByLabelText(/retention days/i);
    expect(retentionInput).toHaveValue(90);

    const retrievalSelect = screen.getByLabelText(/retrieval/i);
    expect(retrievalSelect).toHaveValue("keyword");
  });
});
