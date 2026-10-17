import { act, fireEvent, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { scanRawPaletteClasses } from "../../test/theme-tokens";
import { StatusPill } from "./StatusPill";
import type { StatusPillEntry } from "./StatusPill";

const START = 1_000_000;

function makeEntries(...texts: string[]): StatusPillEntry[] {
  return texts.map((text, i) => ({ text, at: START + i * 1000 }));
}

describe("StatusPill — in-flow status capsule", () => {
  beforeEach(() => {
    vi.useFakeTimers({ now: START });
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("renders dots, current status, timer from 0s, and expand toggle", () => {
    renderWithProviders(
      <StatusPill entries={makeEntries("正在酝酿一个绝妙的回答")} startedAt={START} />,
    );

    const pill = screen.getByTestId("status-pill");
    expect(screen.getByText("正在酝酿一个绝妙的回答")).toBeTruthy();
    expect(screen.getByText("0s")).toBeTruthy();
    expect(screen.getByRole("status")).toBeTruthy();
    expect(screen.getByLabelText("展开状态详情")).toBeTruthy();
    // Three staggered loading dots live inside the aria-hidden wrapper.
    const dots = screen.getByTestId("status-pill").querySelectorAll(".animate-pulse");
    expect(dots).toHaveLength(3);
    // In-flow by contract (spec 5.8.1-6): never a floating overlay.
    expect(pill.className).not.toMatch(/\b(fixed|absolute)\b/);
  });

  it("renders nothing when entries is empty", () => {
    renderWithProviders(<StatusPill entries={[]} startedAt={START} />);
    expect(screen.queryByTestId("status-pill")).toBeNull();
  });

  it("ticks the elapsed timer up as time passes", () => {
    renderWithProviders(
      <StatusPill entries={makeEntries("托腮沉思中……")} startedAt={START} />,
    );

    expect(screen.getByText("0s")).toBeTruthy();
    act(() => {
      vi.advanceTimersByTime(8000);
    });
    expect(screen.getByText("8s")).toBeTruthy();
    expect(screen.getByLabelText("已等待 8 秒")).toBeTruthy();
  });

  it("shows the latest entry as the current status", () => {
    renderWithProviders(
      <StatusPill entries={makeEntries("正在唤醒沉睡的代码之魂……", "正在翻阅之前的对话笔记")} startedAt={START} />,
    );
    expect(screen.getByText("正在翻阅之前的对话笔记")).toBeTruthy();
    expect(screen.queryByText("正在唤醒沉睡的代码之魂……")).toBeNull();
  });

  it("expands the history list and collapses it again", () => {
    renderWithProviders(
      <StatusPill
        entries={makeEntries("正在唤醒沉睡的代码之魂……", "正在翻阅之前的对话笔记", "托腮沉思中……")}
        startedAt={START}
      />,
    );

    const toggle = screen.getByLabelText("展开状态详情");
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(toggle);

    expect(screen.getByLabelText("收起状态详情")).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("正在唤醒沉睡的代码之魂……")).toBeTruthy();
    expect(screen.getByText("正在翻阅之前的对话笔记")).toBeTruthy();
    // The current status text appears in BOTH the pill row and the history
    // list (it is the newest history entry).
    expect(screen.getAllByText("托腮沉思中……")).toHaveLength(2);
    expect(screen.getByText("当前")).toBeTruthy();

    fireEvent.click(screen.getByLabelText("收起状态详情"));
    expect(screen.queryByText("正在唤醒沉睡的代码之魂……")).toBeNull();
  });

  it("renders a stop button that calls onStop when provided", () => {
    const onStop = vi.fn();
    renderWithProviders(
      <StatusPill entries={makeEntries("托腮沉思中……")} startedAt={START} onStop={onStop} />,
    );

    fireEvent.click(screen.getByLabelText("停止"));
    expect(onStop).toHaveBeenCalledTimes(1);
  });

  it("omits the stop button when onStop is not provided", () => {
    renderWithProviders(
      <StatusPill entries={makeEntries("托腮沉思中……")} startedAt={START} />,
    );
    expect(screen.queryByLabelText("停止")).toBeNull();
  });

  it("applies the closing fade (transform/opacity only) for the exit phase", () => {
    renderWithProviders(
      <StatusPill entries={makeEntries("托腮沉思中……")} startedAt={START} closing={true} />,
    );
    const pill = screen.getByTestId("status-pill");
    expect(pill.className).toContain("opacity-0");
    expect(pill.className).toContain("scale-95");
  });

  it("uses only semantic theme tokens (no raw palette classes)", () => {
    const { container } = renderWithProviders(
      <StatusPill
        entries={makeEntries("正在酝酿一个绝妙的回答", "工具调用中")}
        startedAt={START}
        onStop={() => {}}
      />,
    );
    expect(scanRawPaletteClasses(container)).toEqual([]);
  });
});
