import { screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { injectIpcCommands } from "../../lib/ipc/client";
import { testDoubleCommands } from "../../lib/ipc/test-double";
import { renderWithProviders } from "../../test/helpers";
import { StewardSettingsPanel } from "./StewardSettingsPanel";

/** Raw Tailwind palette classes (red-500, bg-black/10, text-white, …) never
 *  resolve through the theme CSS vars, so they break the dual-theme contract
 *  and the themeContrast gate's assumptions. Everything inside the panel must
 *  ride the semantic tokens (surface/ink/state/divider). */
const RAW_PALETTE_CLASS =
  /\b(?:text|bg|border|ring|fill|stroke|shadow|from|to|via|outline|decoration|divide|caret|placeholder)-(?:red|blue|yellow|green|black|white|gray|grey|slate|zinc|neutral|stone|amber|lime|emerald|teal|cyan|sky|indigo|violet|purple|fuchsia|pink|rose|orange)(?:-\d+)?\b/;

function scanRawPaletteClasses(root: HTMLElement): string[] {
  const offenders: string[] = [];
  for (const el of root.querySelectorAll("*")) {
    const cls = el.getAttribute("class");
    if (cls !== null && RAW_PALETTE_CLASS.test(cls)) offenders.push(cls);
  }
  return offenders;
}

describe("StewardSettingsPanel — slide-over panel", () => {
  it("keeps the panel mounted but hidden off-canvas when open is false", () => {
    renderWithProviders(
      <StewardSettingsPanel sessionId="s1" open={false} onClose={() => {}} />,
    );
    const panel = screen.getByRole("complementary", { hidden: true });
    expect(panel).toHaveAttribute("aria-hidden", "true");
    expect(panel.className).toContain("translate-x-full");
    expect(screen.queryByRole("presentation")).toBeNull();
  });

  it("renders panel header and slides in when open", async () => {
    renderWithProviders(
      <StewardSettingsPanel sessionId="s1" open={true} onClose={() => {}} />,
    );
    expect(screen.getByText("管家设置")).toBeTruthy();
    const panel = screen.getByRole("complementary");
    expect(panel).toHaveAttribute("aria-hidden", "false");
    expect(panel.className).toContain("translate-x-0");
  });

  it("calls onClose when close button is clicked", () => {
    const onClose = vi.fn();
    renderWithProviders(
      <StewardSettingsPanel sessionId="s1" open={true} onClose={onClose} />,
    );
    fireEvent.click(screen.getByLabelText("关闭"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("calls onClose when backdrop is clicked", () => {
    const onClose = vi.fn();
    renderWithProviders(
      <StewardSettingsPanel sessionId="s1" open={true} onClose={onClose} />,
    );
    const backdrop = screen.getByRole("presentation");
    fireEvent.click(backdrop);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("calls onClose when Escape key is pressed", () => {
    const onClose = vi.fn();
    renderWithProviders(
      <StewardSettingsPanel sessionId="s1" open={true} onClose={onClose} />,
    );
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("does not close on Escape when panel is closed", () => {
    const onClose = vi.fn();
    renderWithProviders(
      <StewardSettingsPanel sessionId="s1" open={false} onClose={onClose} />,
    );
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
  });

  it("shows loading state while settings are loading", async () => {
    const base = testDoubleCommands();
    const getSettings = vi.fn(
      () => new Promise<never>(() => {}),
    ) as unknown as typeof base.stewardGetSettings;
    injectIpcCommands({ ...base, stewardGetSettings: getSettings });

    renderWithProviders(
      <StewardSettingsPanel sessionId="s1" open={true} onClose={() => {}} />,
    );
    expect(screen.getByText("加载中...")).toBeTruthy();
  });

  it("shows section-level error placeholders when settings load fails", async () => {
    const base = testDoubleCommands();
    const getSettings = vi.fn(async () => {
      throw new Error("store unavailable");
    });
    injectIpcCommands({ ...base, stewardGetSettings: getSettings });

    renderWithProviders(
      <StewardSettingsPanel sessionId="s1" open={true} onClose={() => {}} />,
    );

    await waitFor(() =>
      expect(screen.getAllByText("加载失败，点击重试")).toHaveLength(5),
    );
    expect(screen.getByRole("region", { name: "SOUL 人设" })).toBeTruthy();
    expect(screen.getByRole("region", { name: "待办" })).toBeTruthy();
  });

  it("recovers all sections via section-level retry after settings load failure", async () => {
    const base = testDoubleCommands();
    let fail = true;
    const getSettings = vi.fn(async () => {
      if (fail) throw new Error("store unavailable");
      return base.stewardGetSettings("s1");
    });
    injectIpcCommands({ ...base, stewardGetSettings: getSettings });

    renderWithProviders(
      <StewardSettingsPanel sessionId="s1" open={true} onClose={() => {}} />,
    );

    await waitFor(() =>
      expect(screen.getAllByText("加载失败，点击重试")).toHaveLength(5),
    );
    fail = false;
    fireEvent.click(screen.getAllByText("加载失败，点击重试")[0]!);

    await waitFor(() => expect(screen.getByText("SOUL.md")).toBeTruthy());
    expect(screen.getByText("绑定")).toBeTruthy();
    expect(screen.getByText("聊天记录")).toBeTruthy();
    expect(screen.getByText("运行状态")).toBeTruthy();
    expect(screen.getByText("待办")).toBeTruthy();
  });

  it("renders all five sections when data loads", async () => {
    renderWithProviders(
      <StewardSettingsPanel sessionId="s1" open={true} onClose={() => {}} />,
    );

    await waitFor(() => expect(screen.getByText("SOUL.md")).toBeTruthy());
    expect(screen.getByText("绑定")).toBeTruthy();
    expect(screen.getByText("聊天记录")).toBeTruthy();
    expect(screen.getByText("运行状态")).toBeTruthy();
    expect(screen.getByText("待办")).toBeTruthy();
    expect(
      screen.getByRole("region", { name: "Provider 绑定" }),
    ).toBeTruthy();
    expect(screen.getByRole("region", { name: "运行状态" })).toBeTruthy();
  });

  it("uses only semantic theme tokens in both light and dark themes (no raw palette classes)", async () => {
    const { container } = renderWithProviders(
      <StewardSettingsPanel sessionId="s1" open={true} onClose={() => {}} />,
    );
    await waitFor(() => expect(screen.getByText("聊天记录")).toBeTruthy());

    // Light theme (paper-light, the :root default).
    expect(scanRawPaletteClasses(container)).toEqual([]);

    // Dark theme (chalkboard-dark rides the .dark class).
    document.documentElement.classList.add("dark");
    try {
      await waitFor(() => expect(screen.getByText("管家设置")).toBeTruthy());
      expect(scanRawPaletteClasses(container)).toEqual([]);
    } finally {
      document.documentElement.classList.remove("dark");
    }
  });
});
