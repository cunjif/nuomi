import { fireEvent, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { AgentOptionDto, RoleDto } from "../../../lib/ipc/bindings.gen";
import { injectIpcCommands } from "../../../lib/ipc/client";
import { renderWithProviders } from "../../../test/helpers";
import { i18n } from "../../../i18n";
import { AgentPickerPopover } from "./AgentPickerPopover";

const ok = <T,>(data: T) => ({ status: "ok" as const, data });

function agentOption(overrides: Partial<AgentOptionDto>): AgentOptionDto {
  return {
    kind: "role",
    id: "r1",
    name: "coder",
    enabled: true,
    builtin: false,
    role: null,
    responsibility: null,
    boundModel: null,
    provider: null,
    ...overrides,
  };
}

function role(overrides: Partial<RoleDto>): RoleDto {
  return {
    id: "r1",
    name: "coder",
    providerId: null,
    providerIds: [],
    systemPromptOverride: null,
    toolAllowlist: [],
    requiredCapabilities: [],
    temperature: null,
    maxTokens: null,
    params: {},
    builtin: false,
    generated: false,
    ephemeral: false,
    source: null,
    createdAt: 1,
    updatedAt: 1,
    ...overrides,
  };
}

describe("AgentPickerPopover — ready/unbound role split", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("lists ready roles as selectable and unbound roles as greyed with goBind", async () => {
    injectIpcCommands({
      listAgentOptions: vi.fn().mockResolvedValue(
        ok([
          agentOption({ id: "r1", name: "translator", kind: "role" }),
          agentOption({ id: "r2", name: "summarizer", kind: "role" }),
        ]),
      ),
      listRoles: vi.fn().mockResolvedValue(
        ok([
          role({ id: "r1", name: "translator", providerId: "prov-1" }),
          role({ id: "r2", name: "summarizer", providerId: null }),
        ]),
      ),
    } as never);
    renderWithProviders(
      <AgentPickerPopover sessionId="s1" onSelect={vi.fn()} onClose={vi.fn()} />,
    );

    expect(await screen.findByText("translator")).toBeInTheDocument();
    expect(screen.getByText("summarizer")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /configure binding/i })).toBeInTheDocument();
  });

  it("clicking a ready role calls onSelect with kind and id", async () => {
    const onSelect = vi.fn().mockResolvedValue(undefined);
    injectIpcCommands({
      listAgentOptions: vi.fn().mockResolvedValue(
        ok([agentOption({ id: "r1", name: "translator", kind: "role" })]),
      ),
      listRoles: vi.fn().mockResolvedValue(
        ok([role({ id: "r1", name: "translator", providerId: "prov-1" })]),
      ),
    } as never);
    renderWithProviders(
      <AgentPickerPopover sessionId="s1" onSelect={onSelect} onClose={vi.fn()} />,
    );

    fireEvent.click(await screen.findByText("translator"));
    expect(onSelect).toHaveBeenCalledWith("role", "r1");
  });

  it("clicking goBind on an unbound role calls onGoToSettings", async () => {
    const onGoToSettings = vi.fn();
    injectIpcCommands({
      listAgentOptions: vi.fn().mockResolvedValue(
        ok([agentOption({ id: "r2", name: "summarizer", kind: "role" })]),
      ),
      listRoles: vi.fn().mockResolvedValue(
        ok([role({ id: "r2", name: "summarizer", providerId: null })]),
      ),
    } as never);
    renderWithProviders(
      <AgentPickerPopover
        sessionId="s1"
        onSelect={vi.fn()}
        onClose={vi.fn()}
        onGoToSettings={onGoToSettings}
      />,
    );

    fireEvent.click(await screen.findByRole("button", { name: /configure binding/i }));
    expect(onGoToSettings).toHaveBeenCalled();
  });
});
