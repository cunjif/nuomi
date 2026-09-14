import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { usePanelMutex } from "./usePanelMutex";

describe("usePanelMutex — panel mutual exclusion", () => {
  it("starts with no panel open", () => {
    const { result } = renderHook(() => usePanelMutex());
    expect(result.current.openPanel).toBe(null);
  });

  it("opens a panel", () => {
    const { result } = renderHook(() => usePanelMutex());
    act(() => result.current.open("function"));
    expect(result.current.openPanel).toBe("function");
    expect(result.current.isOpen("function")).toBe(true);
  });

  it("opening a new panel closes the previous one", () => {
    const { result } = renderHook(() => usePanelMutex());
    act(() => result.current.open("function"));
    expect(result.current.openPanel).toBe("function");
    act(() => result.current.open("command"));
    expect(result.current.openPanel).toBe("command");
    expect(result.current.isOpen("function")).toBe(false);
    expect(result.current.isOpen("command")).toBe(true);
  });

  it("toggle closes the panel if it is already open", () => {
    const { result } = renderHook(() => usePanelMutex());
    act(() => result.current.toggle("mention"));
    expect(result.current.openPanel).toBe("mention");
    act(() => result.current.toggle("mention"));
    expect(result.current.openPanel).toBe(null);
  });

  it("toggle opens a new panel and closes the previous", () => {
    const { result } = renderHook(() => usePanelMutex());
    act(() => result.current.toggle("function"));
    act(() => result.current.toggle("command"));
    expect(result.current.openPanel).toBe("command");
  });

  it("close resets to null", () => {
    const { result } = renderHook(() => usePanelMutex());
    act(() => result.current.open("function"));
    act(() => result.current.close());
    expect(result.current.openPanel).toBe(null);
  });

  it("isOpen returns false for non-open panels", () => {
    const { result } = renderHook(() => usePanelMutex());
    act(() => result.current.open("function"));
    expect(result.current.isOpen("command")).toBe(false);
    expect(result.current.isOpen("mention")).toBe(false);
  });
});
