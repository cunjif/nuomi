import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { IntegrationDto } from "../../lib/ipc/bindings.gen";
import { injectIpcCommands } from "../../lib/ipc/client";
import { useToastStore } from "../../lib/store/toastStore";
import { renderWithProviders } from "../../test/helpers";
import { i18n } from "../../i18n";
import { IntegrationsSection } from "./IntegrationsSection";

const ok = <T,>(data: T) => ({ status: "ok" as const, data });

const toastText = (): string =>
  useToastStore
    .getState()
    .toasts.map((toast) => toast.message)
    .join("\n");

function integration(overrides: Partial<IntegrationDto>): IntegrationDto {
  return {
    id: "integ-x",
    name: "feishu-main",
    kind: "feishu_bot",
    webhookUrlMasked: "https://open.feishu.cn/***abcd",
    events: [],
    enabled: true,
    createdAt: 1,
    updatedAt: 1,
    ...overrides,
  };
}

describe("IntegrationsSection (SPEC bots-telemetry-m1 B5 / AC7)", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("renders integrations with kind badges and verbatim masked URLs", async () => {
    injectIpcCommands({
      listIntegrations: vi.fn().mockResolvedValue(
        ok([
          integration({
            id: "i1",
            name: "feishu-bot",
            kind: "feishu_bot",
            webhookUrlMasked: "https://open.feishu.cn/***abcd",
          }),
          integration({
            id: "i2",
            name: "tele-gw",
            kind: "telemetry",
            webhookUrlMasked: "https://telemetry.internal/***ef01",
            enabled: false,
          }),
        ]),
      ),
      upsertIntegration: vi.fn(),
      deleteIntegration: vi.fn(),
      testIntegration: vi.fn(),
    } as never);
    renderWithProviders(<IntegrationsSection />);

    const feishuRow = (await screen.findByText("feishu-bot")).closest("li");
    expect(feishuRow?.textContent).toContain("Feishu bot");
    expect(feishuRow?.textContent).toContain("https://open.feishu.cn/***abcd");
    expect(feishuRow?.textContent).toContain("Enabled");

    const teleRow = (await screen.findByText("tele-gw")).closest("li");
    expect(teleRow?.textContent).toContain("Telemetry");
    expect(teleRow?.textContent).toContain("https://telemetry.internal/***ef01");
    expect(teleRow?.textContent).toContain("Disabled");
  });

  it("submits the raw webhookUrl + secret and the checked event whitelist", async () => {
    const created = integration({
      id: "i9",
      name: "hook",
      events: ["run.state_changed", "approval.requested"],
    });
    const upsertIntegration = vi.fn().mockResolvedValue(ok(created));
    injectIpcCommands({
      listIntegrations: vi.fn().mockResolvedValueOnce(ok([])).mockResolvedValueOnce(ok([created])),
      upsertIntegration,
      deleteIntegration: vi.fn(),
      testIntegration: vi.fn(),
    } as never);
    renderWithProviders(<IntegrationsSection />);

    fireEvent.change(await screen.findByLabelText(/^name$/i), { target: { value: "hook" } });
    // Default kind stays feishu_bot; fill URL + secret and tick two topics.
    fireEvent.change(screen.getByLabelText(/webhook url/i), {
      target: { value: "https://open.feishu.cn/open-apis/bot/v2/hook/raw-token" },
    });
    fireEvent.change(screen.getByLabelText(/signing secret/i), { target: { value: "s3cret" } });
    fireEvent.click(screen.getByLabelText("run.state_changed"));
    fireEvent.click(screen.getByLabelText("approval.requested"));
    fireEvent.click(screen.getByRole("button", { name: /^save$/i }));

    await waitFor(() =>
      expect(upsertIntegration).toHaveBeenCalledWith({
        name: "hook",
        kind: "feishu_bot",
        webhookUrl: "https://open.feishu.cn/open-apis/bot/v2/hook/raw-token",
        secret: "s3cret",
        headers: null,
        events: ["run.state_changed", "approval.requested"],
        enabled: true,
      }),
    );
    expect(await screen.findByText("hook")).toBeInTheDocument();
  });

  it("shows a success toast when the test send reports ok:true", async () => {
    const testIntegration = vi.fn().mockResolvedValue(ok({ ok: true, error: null }));
    injectIpcCommands({
      listIntegrations: vi.fn().mockResolvedValue(ok([integration({ id: "i1", name: "feishu-bot" })])),
      upsertIntegration: vi.fn(),
      deleteIntegration: vi.fn(),
      testIntegration,
    } as never);
    renderWithProviders(<IntegrationsSection />);

    fireEvent.click(await screen.findByRole("button", { name: /send test feishu-bot/i }));

    await waitFor(() => expect(testIntegration).toHaveBeenCalledWith("i1"));
    await waitFor(() => expect(toastText()).toMatch(/test message sent/i));
  });

  it("keeps the form input and toasts the error when the backend rejects an invalid URL", async () => {
    const upsertIntegration = vi.fn().mockResolvedValue({
      status: "error" as const,
      error: { generic: { code: "integration.invalid", message: "not http(s)" } },
    });
    injectIpcCommands({
      listIntegrations: vi.fn().mockResolvedValue(ok([])),
      upsertIntegration,
      deleteIntegration: vi.fn(),
      testIntegration: vi.fn(),
    } as never);
    renderWithProviders(<IntegrationsSection />);

    fireEvent.change(await screen.findByLabelText(/^name$/i), { target: { value: "bad-hook" } });
    fireEvent.change(screen.getByLabelText(/webhook url/i), { target: { value: "ftp://example.com/hook" } });
    fireEvent.click(screen.getByRole("button", { name: /^save$/i }));

    await waitFor(() =>
      expect(toastText()).toMatch(/failed to save integration.*invalid webhook url/i),
    );
    expect(screen.getByLabelText(/^name$/i)).toHaveValue("bad-hook");
    expect(screen.getByLabelText(/webhook url/i)).toHaveValue("ftp://example.com/hook");
    expect(upsertIntegration).toHaveBeenCalledTimes(1);
  });
});
