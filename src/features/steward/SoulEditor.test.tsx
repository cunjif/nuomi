import { screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { injectIpcCommands } from "../../lib/ipc/client";
import { testDoubleCommands } from "../../lib/ipc/test-double";
import { renderWithProviders } from "../../test/helpers";
import { useToastStore } from "../../lib/store/toastStore";
import { SoulEditor } from "./SoulEditor";
import type { SoulConfigDto } from "../../lib/ipc/bindings.gen";

describe("SoulEditor — SOUL.md editing", () => {
  it("renders textarea with saved content", () => {
    const soul: SoulConfigDto = { content: "You are a helpful steward.", maxLength: 16000 };
    renderWithProviders(<SoulEditor sessionId="s1" soul={soul} />);
    expect(screen.getByDisplayValue("You are a helpful steward.")).toBeTruthy();
  });

  it("renders empty textarea when soul is null", () => {
    renderWithProviders(<SoulEditor sessionId="s1" soul={null} />);
    const textarea = screen.getByPlaceholderText("输入管家人格提示词（留空使用默认提示词）...");
    expect(textarea).toBeTruthy();
  });

  it("disables save button when content is unchanged", () => {
    const soul: SoulConfigDto = { content: "existing", maxLength: 16000 };
    renderWithProviders(<SoulEditor sessionId="s1" soul={soul} />);
    const saveBtn = screen.getByText("保存");
    expect(saveBtn).toBeTruthy();
    expect((saveBtn as HTMLButtonElement).disabled).toBe(true);
  });

  it("enables save button when content is modified", () => {
    const soul: SoulConfigDto = { content: "existing", maxLength: 16000 };
    renderWithProviders(<SoulEditor sessionId="s1" soul={soul} />);
    const textarea = screen.getByDisplayValue("existing");
    fireEvent.change(textarea, { target: { value: "modified content" } });
    expect(screen.getByText("保存")).toBeTruthy();
    expect((screen.getByText("保存") as HTMLButtonElement).disabled).toBe(false);
  });

  it("shows '有未保存的修改' indicator when dirty", () => {
    const soul: SoulConfigDto = { content: "existing", maxLength: 16000 };
    renderWithProviders(<SoulEditor sessionId="s1" soul={soul} />);
    const textarea = screen.getByDisplayValue("existing");
    fireEvent.change(textarea, { target: { value: "modified" } });
    expect(screen.getByText("有未保存的修改")).toBeTruthy();
  });

  it("calls stewardUpdateSoul on save", async () => {
    const base = testDoubleCommands();
    const updateSoul = vi.fn(base.stewardUpdateSoul);
    injectIpcCommands({ ...base, stewardUpdateSoul: updateSoul });

    const soul: SoulConfigDto = { content: "old", maxLength: 16000 };
    renderWithProviders(<SoulEditor sessionId="s1" soul={soul} />);

    const textarea = screen.getByDisplayValue("old");
    fireEvent.change(textarea, { target: { value: "new content" } });
    fireEvent.click(screen.getByText("保存"));

    await waitFor(() => expect(updateSoul).toHaveBeenCalledWith("s1", "new content"));
  });

  it("shows character count", () => {
    const soul: SoulConfigDto = { content: "12345", maxLength: 16000 };
    renderWithProviders(<SoulEditor sessionId="s1" soul={soul} />);
    expect(screen.getByText("5 / 16000")).toBeTruthy();
  });

  it("toasts success and invalidates queries on save", async () => {
    const base = testDoubleCommands();
    const updateSoul = vi.fn(base.stewardUpdateSoul);
    injectIpcCommands({ ...base, stewardUpdateSoul: updateSoul });

    const soul: SoulConfigDto = { content: "old", maxLength: 16000 };
    renderWithProviders(<SoulEditor sessionId="s1" soul={soul} />);

    fireEvent.change(screen.getByDisplayValue("old"), {
      target: { value: "new content" },
    });
    fireEvent.click(screen.getByText("保存"));

    await waitFor(() =>
      expect(useToastStore.getState().toasts).toHaveLength(1),
    );
    expect(useToastStore.getState().toasts[0]).toMatchObject({
      kind: "success",
      message: "SOUL.md 已保存",
    });
  });

  it("toasts failure and keeps the draft when save fails", async () => {
    const base = testDoubleCommands();
    const updateSoul = vi.fn(async () => {
      throw new Error("db locked");
    });
    injectIpcCommands({ ...base, stewardUpdateSoul: updateSoul });

    const soul: SoulConfigDto = { content: "old", maxLength: 16000 };
    renderWithProviders(<SoulEditor sessionId="s1" soul={soul} />);

    fireEvent.change(screen.getByDisplayValue("old"), {
      target: { value: "unsaved draft" },
    });
    fireEvent.click(screen.getByText("保存"));

    await waitFor(() =>
      expect(useToastStore.getState().toasts).toHaveLength(1),
    );
    expect(useToastStore.getState().toasts[0]?.kind).toBe("error");
    expect(useToastStore.getState().toasts[0]?.message).toContain("保存失败");
    expect(screen.getByDisplayValue("unsaved draft")).toBeTruthy();
  });

  it("rejects over-length content with error toast and skips the save call", async () => {
    const base = testDoubleCommands();
    const updateSoul = vi.fn(base.stewardUpdateSoul);
    injectIpcCommands({ ...base, stewardUpdateSoul: updateSoul });

    const soul: SoulConfigDto = { content: "", maxLength: 16000 };
    renderWithProviders(<SoulEditor sessionId="s1" soul={soul} />);

    fireEvent.change(screen.getByPlaceholderText(/输入管家人格提示词/), {
      target: { value: "x".repeat(16001) },
    });
    fireEvent.click(screen.getByText("保存"));

    await waitFor(() =>
      expect(useToastStore.getState().toasts).toHaveLength(1),
    );
    expect(useToastStore.getState().toasts[0]?.kind).toBe("error");
    expect(useToastStore.getState().toasts[0]?.message).toContain("内容过长");
    expect(updateSoul).not.toHaveBeenCalled();
    expect(screen.getByText(/16001 \/ 16000/)).toBeTruthy();
  });
});
