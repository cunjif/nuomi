import { screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { injectIpcCommands } from "../../lib/ipc/client";
import { testDoubleCommands } from "../../lib/ipc/test-double";
import { seedProvider } from "../../lib/ipc/test-double-state";
import { renderWithProviders } from "../../test/helpers";
import { useToastStore } from "../../lib/store/toastStore";
import { BindingEditor } from "./BindingEditor";
import type { BindingConfigDto } from "../../lib/ipc/bindings.gen";

describe("BindingEditor — provider/CLI binding", () => {
  it("renders binding mode select with current mode", () => {
    const binding: BindingConfigDto = { mode: "none", providerId: null, modelId: null, agentProfileId: null };
    renderWithProviders(<BindingEditor sessionId="s1" binding={binding} />);

    const select = screen.getByDisplayValue("未绑定");
    expect(select).toBeTruthy();
  });

  it("shows Provider select when mode is provider", () => {
    const binding: BindingConfigDto = { mode: "provider", providerId: "p1", modelId: null, agentProfileId: null };
    renderWithProviders(<BindingEditor sessionId="s1" binding={binding} />);
    expect(screen.getByText("选择 Provider...")).toBeTruthy();
  });

  it("shows CLI Agent select when mode is cli", () => {
    const binding: BindingConfigDto = { mode: "cli", providerId: null, modelId: null, agentProfileId: "a1" };
    renderWithProviders(<BindingEditor sessionId="s1" binding={binding} />);
    expect(screen.getByText("选择 CLI Agent...")).toBeTruthy();
  });

  it("does not show Provider or CLI Agent select when mode is none", () => {
    const binding: BindingConfigDto = { mode: "none", providerId: null, modelId: null, agentProfileId: null };
    renderWithProviders(<BindingEditor sessionId="s1" binding={binding} />);
    expect(screen.queryByText("选择 Provider...")).toBeNull();
    expect(screen.queryByText("选择 CLI Agent...")).toBeNull();
  });

  it("disables save button when binding is unchanged", () => {
    const binding: BindingConfigDto = { mode: "none", providerId: null, modelId: null, agentProfileId: null };
    renderWithProviders(<BindingEditor sessionId="s1" binding={binding} />);
    expect((screen.getByText("保存") as HTMLButtonElement).disabled).toBe(true);
  });

  it("enables save button when mode is changed", () => {
    const binding: BindingConfigDto = { mode: "none", providerId: null, modelId: null, agentProfileId: null };
    renderWithProviders(<BindingEditor sessionId="s1" binding={binding} />);

    const modeSelect = screen.getByDisplayValue("未绑定");
    fireEvent.change(modeSelect, { target: { value: "provider" } });

    expect((screen.getByText("保存") as HTMLButtonElement).disabled).toBe(false);
  });

  it("calls stewardUpdateBinding on save", async () => {
    const base = testDoubleCommands();
    const updateBinding = vi.fn(base.stewardUpdateBinding);
    injectIpcCommands({ ...base, stewardUpdateBinding: updateBinding });

    const binding: BindingConfigDto = { mode: "none", providerId: null, modelId: null, agentProfileId: null };
    renderWithProviders(<BindingEditor sessionId="s1" binding={binding} />);

    const modeSelect = screen.getByDisplayValue("未绑定");
    fireEvent.change(modeSelect, { target: { value: "cli" } });
    fireEvent.click(screen.getByText("保存"));

    await waitFor(() =>
      expect(updateBinding).toHaveBeenCalledWith("s1", "cli", null, null, null),
    );
  });

  it("toasts success on binding save", async () => {
    const base = testDoubleCommands();
    const updateBinding = vi.fn(base.stewardUpdateBinding);
    injectIpcCommands({ ...base, stewardUpdateBinding: updateBinding });

    const binding: BindingConfigDto = { mode: "none", providerId: null, modelId: null, agentProfileId: null };
    renderWithProviders(<BindingEditor sessionId="s1" binding={binding} />);

    fireEvent.change(screen.getByDisplayValue("未绑定"), {
      target: { value: "provider" },
    });
    fireEvent.click(screen.getByText("保存"));

    await waitFor(() =>
      expect(useToastStore.getState().toasts).toHaveLength(1),
    );
    expect(useToastStore.getState().toasts[0]).toMatchObject({
      kind: "success",
      message: "绑定已更新",
    });
  });

  it("toasts failure and keeps edits when binding save fails", async () => {
    const base = testDoubleCommands();
    const updateBinding = vi.fn(async () => {
      throw new Error("registry unavailable");
    });
    injectIpcCommands({ ...base, stewardUpdateBinding: updateBinding });

    const binding: BindingConfigDto = { mode: "none", providerId: null, modelId: null, agentProfileId: null };
    renderWithProviders(<BindingEditor sessionId="s1" binding={binding} />);

    fireEvent.change(screen.getByDisplayValue("未绑定"), {
      target: { value: "provider" },
    });
    fireEvent.click(screen.getByText("保存"));

    await waitFor(() =>
      expect(useToastStore.getState().toasts).toHaveLength(1),
    );
    expect(useToastStore.getState().toasts[0]?.kind).toBe("error");
    expect(useToastStore.getState().toasts[0]?.message).toContain("绑定失败");
    expect(screen.getByDisplayValue("Provider")).toBeTruthy();
  });

  it("shows a model dropdown from the provider's models and submits the pin", async () => {
    seedProvider({
      id: "p1",
      name: "Primary",
      settings: {
        models: [
          { id: "gpt-4o", capabilities: ["reasoning"], temperature: null, topP: null, maxTokens: null },
          { id: "gpt-4o-mini", capabilities: ["reasoning"], temperature: null, topP: null, maxTokens: null },
        ],
        defaultModel: null,
        temperature: null,
        topP: null,
        maxTokens: null,
        timeoutSecs: null,
        retry: null,
        maxConcurrency: null,
        priority: null,
        roles: [],
        proxy: null,
        enabled: true,
      },
    });
    const base = testDoubleCommands();
    const updateBinding = vi.fn(base.stewardUpdateBinding);
    injectIpcCommands({ ...base, stewardUpdateBinding: updateBinding });

    const binding: BindingConfigDto = { mode: "provider", providerId: "p1", modelId: null, agentProfileId: null };
    renderWithProviders(<BindingEditor sessionId="s1" binding={binding} />);

    // Model dropdown lists both models; the default option carries the
    // provider's first model (query resolves async — wait for it).
    const modelSelect = (await screen.findByDisplayValue(
      "默认（gpt-4o）",
    )) as HTMLSelectElement;
    expect(modelSelect).toBeTruthy();
    fireEvent.change(modelSelect, { target: { value: "gpt-4o-mini" } });
    fireEvent.click(screen.getByText("保存"));

    await waitFor(() =>
      expect(updateBinding).toHaveBeenCalledWith("s1", "provider", "p1", "gpt-4o-mini", null),
    );
  });

  it("falls back to a free-form model input when the provider has no models", async () => {
    seedProvider({ id: "p1", name: "Bare", settings: {} });
    const base = testDoubleCommands();
    const updateBinding = vi.fn(base.stewardUpdateBinding);
    injectIpcCommands({ ...base, stewardUpdateBinding: updateBinding });

    const binding: BindingConfigDto = { mode: "provider", providerId: "p1", modelId: null, agentProfileId: null };
    renderWithProviders(<BindingEditor sessionId="s1" binding={binding} />);

    const modelInput = screen.getByPlaceholderText(
      "该 Provider 未配置模型列表，可手动填写 ModelId",
    ) as HTMLInputElement;
    expect(modelInput).toBeTruthy();
    fireEvent.change(modelInput, { target: { value: "manual-model" } });
    fireEvent.click(screen.getByText("保存"));

    await waitFor(() =>
      expect(updateBinding).toHaveBeenCalledWith("s1", "provider", "p1", "manual-model", null),
    );
  });

  it("resets the model pin when the provider changes", async () => {
    seedProvider({
      id: "p1",
      name: "Primary",
      settings: {
        models: [{ id: "m-one", capabilities: ["reasoning"], temperature: null, topP: null, maxTokens: null }],
        defaultModel: null, temperature: null, topP: null, maxTokens: null,
        timeoutSecs: null, retry: null, maxConcurrency: null, priority: null, roles: [], proxy: null, enabled: true,
      },
    });
    seedProvider({
      id: "p2",
      name: "Secondary",
      settings: {
        models: [{ id: "m-two", capabilities: ["reasoning"], temperature: null, topP: null, maxTokens: null }],
        defaultModel: null, temperature: null, topP: null, maxTokens: null,
        timeoutSecs: null, retry: null, maxConcurrency: null, priority: null, roles: [], proxy: null, enabled: true,
      },
    });
    const base = testDoubleCommands();
    const updateBinding = vi.fn(base.stewardUpdateBinding);
    injectIpcCommands({ ...base, stewardUpdateBinding: updateBinding });

    const binding: BindingConfigDto = { mode: "provider", providerId: "p1", modelId: "m-one", agentProfileId: null };
    renderWithProviders(<BindingEditor sessionId="s1" binding={binding} />);

    expect((await screen.findByDisplayValue("m-one") as HTMLSelectElement).value).toBe("m-one");
    fireEvent.change(await screen.findByDisplayValue("Primary"), { target: { value: "p2" } });
    // Provider switch clears the stale model pin.
    expect((await screen.findByDisplayValue("默认（m-two）") as HTMLSelectElement).value).toBe("");
    fireEvent.click(screen.getByText("保存"));

    await waitFor(() =>
      expect(updateBinding).toHaveBeenCalledWith("s1", "provider", "p2", null, null),
    );
  });
});
