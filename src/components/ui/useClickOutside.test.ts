import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RefObject } from "react";
import { useClickOutside } from "./useClickOutside";

function mutableRef<T extends Node>(value: T): RefObject<T> {
  return { current: value } as RefObject<T>;
}

function dispatchDown(target: Node): void {
  target.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true }));
}

function dispatchEscape(): void {
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
}

describe("useClickOutside", () => {
  let inside: HTMLDivElement;
  let outside: HTMLDivElement;

  beforeEach(() => {
    inside = document.createElement("div");
    outside = document.createElement("div");
    document.body.appendChild(inside);
    document.body.appendChild(outside);
  });

  afterEach(() => {
    inside.remove();
    outside.remove();
  });

  it("calls onOutside when mousedown lands outside the ref", () => {
    const ref = mutableRef(inside);
    const onOutside = vi.fn();
    renderHook(() => useClickOutside([ref], { enabled: true, onOutside }));

    dispatchDown(outside);
    expect(onOutside).toHaveBeenCalledTimes(1);
  });

  it("does not call onOutside when mousedown lands inside the ref", () => {
    const ref = mutableRef(inside);
    const onOutside = vi.fn();
    renderHook(() => useClickOutside([ref], { enabled: true, onOutside }));

    dispatchDown(inside);
    expect(onOutside).not.toHaveBeenCalled();
  });

  it("does not bind the listener when enabled is false", () => {
    const ref = mutableRef(inside);
    const onOutside = vi.fn();
    renderHook(() => useClickOutside([ref], { enabled: false, onOutside }));

    dispatchDown(outside);
    expect(onOutside).not.toHaveBeenCalled();
  });

  it("treats multiple refs as a union — inside any ref suppresses the call", () => {
    const second = document.createElement("div");
    document.body.appendChild(second);
    const refA = mutableRef(inside);
    const refB = mutableRef(second);
    const onOutside = vi.fn();
    renderHook(() => useClickOutside([refA, refB], { enabled: true, onOutside }));

    dispatchDown(second);
    expect(onOutside).not.toHaveBeenCalled();
    second.remove();
  });

  it("invokes onOutside on Escape when escape is true", () => {
    const ref = mutableRef(inside);
    const onOutside = vi.fn();
    renderHook(() => useClickOutside([ref], { enabled: true, onOutside, escape: true }));

    dispatchEscape();
    expect(onOutside).toHaveBeenCalledTimes(1);
  });

  it("ignores Escape when escape is false (default)", () => {
    const ref = mutableRef(inside);
    const onOutside = vi.fn();
    renderHook(() => useClickOutside([ref], { enabled: true, onOutside }));

    dispatchEscape();
    expect(onOutside).not.toHaveBeenCalled();
  });

  it("unbinds the listener when enabled flips from true to false", () => {
    const ref = mutableRef(inside);
    const onOutside = vi.fn();
    const { rerender } = renderHook(({ enabled }) => useClickOutside([ref], { enabled, onOutside }), {
      initialProps: { enabled: true },
    });

    rerender({ enabled: false });
    dispatchDown(outside);
    expect(onOutside).not.toHaveBeenCalled();
  });

  it("uses the latest onOutside without re-binding", () => {
    const ref = mutableRef(inside);
    const first = vi.fn();
    const second = vi.fn();
    const { rerender } = renderHook(({ cb }) => useClickOutside([ref], { enabled: true, onOutside: cb }), {
      initialProps: { cb: first },
    });

    rerender({ cb: second });
    dispatchDown(outside);
    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledTimes(1);
  });
});
