import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ProviderDto } from "../../lib/ipc/bindings.gen";
import { injectIpcCommands } from "../../lib/ipc/client";
import { renderWithProviders } from "../../test/helpers";
import { i18n } from "../../i18n";
import { ProvidersSection } from "./ProvidersSection";

const ok = <T,>(data: T) => ({ status: "ok" as const, data });

function provider(overrides: Partial<ProviderDto>): ProviderDto {
  return {
    id: "prov-1",
    name: "alpha",
    protocol: "open_ai_compatible",
    baseUrl: "https://api.alpha.test/v1",
    hasKey: true,
    capabilities: [],
    isMaster: false,
    settings: {
      models: [
        { id: "model-a", capabilities: ["reasoning"] },
        { id: "model-b", capabilities: [] },
      ],
      defaultModel: "model-a",
      temperature: 0.7,
      topP: 1,
      maxTokens: null,
      timeoutSecs: null,
      retry: null,
      maxConcurrency: null,
      priority: 5,
      roles: ["code"],
      enabled: true,
    },
    ...overrides,
  };
}

describe("ProvidersSection", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("renders the provider list with count, model subtitle and status dot", async () => {
    injectIpcCommands({
      listProviders: vi.fn().mockResolvedValue(ok([
        provider({ id: "prov-1", name: "alpha" }),
        provider({ id: "prov-2", name: "beta", settings: { models: [], enabled: true } }),
      ])),
    } as never);
    renderWithProviders(<ProvidersSection />);

    expect(await screen.findByText("alpha")).toBeInTheDocument();
    expect(screen.getByText("beta")).toBeInTheDocument();
    expect(screen.getByText(/Model model-a/)).toBeInTheDocument();
    expect(screen.getByText("beta").closest("li")?.textContent).toContain("No model configured");
    expect(screen.getAllByLabelText("Untested").length).toBe(2);
    expect(screen.getByText("2")).toBeInTheDocument(); // count badge
  });

  it("filters the list through the search box", async () => {
    injectIpcCommands({
      listProviders: vi.fn().mockResolvedValue(ok([
        provider({ id: "prov-1", name: "alpha" }),
        provider({ id: "prov-2", name: "beta" }),
      ])),
    } as never);
    renderWithProviders(<ProvidersSection />);

    fireEvent.change(await screen.findByPlaceholderText(/search providers/i), {
      target: { value: "alp" },
    });
    expect(screen.getByText("alpha")).toBeInTheDocument();
    expect(screen.queryByText("beta")).not.toBeInTheDocument();
  });

  it("opens the editor with prefilled values and saves changes in place", async () => {
    const upsertProvider = vi.fn().mockResolvedValue(ok(null));
    injectIpcCommands({
      listProviders: vi.fn().mockResolvedValue(ok([provider({})])),
      upsertProvider,
      deleteProvider: vi.fn(),
      testProviderConnection: vi.fn(),
    } as never);
    renderWithProviders(<ProvidersSection />);

    fireEvent.click(await screen.findByText("alpha"));
    const nameInput = await screen.findByLabelText(/display name/i);
    expect(nameInput).toHaveValue("alpha");
    fireEvent.change(nameInput, { target: { value: "alpha renamed" } });
    fireEvent.click(screen.getByRole("button", { name: /^save$/i }));

    await waitFor(() => expect(upsertProvider).toHaveBeenCalled());
    const input = upsertProvider.mock.calls[0]?.[0];
    expect(input).toMatchObject({
      id: "prov-1",
      name: "alpha renamed",
      protocol: "open_ai_compatible",
      baseUrl: "https://api.alpha.test/v1",
      settings: expect.objectContaining({ defaultModel: "model-a", roles: ["code"] }),
    });
  });

  it("adds a model chip and creates a new provider without an id", async () => {
    const upsertProvider = vi.fn().mockResolvedValue(ok(null));
    injectIpcCommands({
      listProviders: vi.fn().mockResolvedValue(ok([])),
      upsertProvider,
      deleteProvider: vi.fn(),
      testProviderConnection: vi.fn(),
    } as never);
    renderWithProviders(<ProvidersSection />);

    fireEvent.click(await screen.findByRole("button", { name: /\+ add provider/i }));
    fireEvent.change(screen.getByLabelText(/display name/i), { target: { value: "deep" } });
    // DeepSeek preset fills the base URL when empty.
    const typeSelect = screen.getByLabelText(/provider type/i);
    fireEvent.change(typeSelect, { target: { value: "deepseek" } });
    expect(screen.getByLabelText(/api base url/i)).toHaveValue("https://api.deepseek.com");
    fireEvent.change(screen.getByLabelText(/api base url/i), {
      target: { value: "https://api.deepseek.com" },
    });

    const modelInput = screen.getByPlaceholderText(/\+ add model id/i);
    fireEvent.change(modelInput, { target: { value: "deepseek-chat" } });
    fireEvent.keyDown(modelInput, { key: "Enter" });
    // The chip appears (the <option> in the default-model select shares the text).
    expect(
      screen.getAllByText("deepseek-chat").some((el) => el.textContent === "deepseek-chat"),
    ).toBe(true);
    expect(screen.getAllByRole("option", { name: "deepseek-chat" }).length).toBe(1);

    fireEvent.click(screen.getByRole("button", { name: /^save$/i }));
    await waitFor(() => expect(upsertProvider).toHaveBeenCalled());
    const input = upsertProvider.mock.calls[0]?.[0];
    expect(input).toMatchObject({
      id: null,
      name: "deep",
      protocol: "open_ai_compatible",
      settings: expect.objectContaining({
        models: [{ id: "deepseek-chat", capabilities: ["reasoning"] }],
      }),
    });
  });

  it("reports the latency line after a successful connection test", async () => {
    const testProviderConnection = vi
      .fn()
      .mockResolvedValue(ok({ ok: true, latencyMs: 128, error: null }));
    injectIpcCommands({
      listProviders: vi.fn().mockResolvedValue(ok([provider({})])),
      upsertProvider: vi.fn(),
      deleteProvider: vi.fn(),
      testProviderConnection,
    } as never);
    renderWithProviders(<ProvidersSection />);

    fireEvent.click(await screen.findByText("alpha"));
    fireEvent.click(await screen.findByRole("button", { name: /test connection/i }));
    await waitFor(() =>
      expect(testProviderConnection).toHaveBeenCalledWith(
        expect.objectContaining({
          providerId: "prov-1",
          protocol: "open_ai_compatible",
          baseUrl: "https://api.alpha.test/v1",
        }),
      ),
    );
    expect(await screen.findByText(/✓ Connected · 128ms/)).toBeInTheDocument();
    // The list dot flips to connected.
    expect(screen.getByLabelText("Connected")).toBeInTheDocument();
  });

  it("deletes a provider via the two-step confirm and clears the selection", async () => {
    const deleteProvider = vi.fn().mockResolvedValue(ok(null));
    injectIpcCommands({
      listProviders: vi.fn().mockResolvedValue(ok([provider({})])),
      upsertProvider: vi.fn(),
      deleteProvider,
      testProviderConnection: vi.fn(),
    } as never);
    renderWithProviders(<ProvidersSection />);

    fireEvent.click(await screen.findByText("alpha"));
    fireEvent.click(screen.getByRole("button", { name: /^delete$/i }));
    fireEvent.click(screen.getByRole("button", { name: /delete this provider/i }));
    await waitFor(() => expect(deleteProvider).toHaveBeenCalledWith("prov-1"));
    expect(await screen.findByText(/select a provider/i)).toBeInTheDocument();
  });
});
