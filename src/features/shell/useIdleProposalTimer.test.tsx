/**
 * useIdleProposalTimer tests (ADR 0023): liveness measurement lives in the
 * frontend, the decision in Rust. Fake timers drive the timeout; injected
 * IPC doubles observe the single advisory `idle_check` call.
 */
import { act, fireEvent, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { injectIpcCommands } from "../../lib/ipc/client";
import "../../test/helpers";
import { useIdleProposalTimer } from "./useIdleProposalTimer";

const ok = <T,>(data: T) => ({ status: "ok" as const, data });
const MINUTE = 60_000;

const w = window as unknown as Record<string, unknown>;

function injectIdleDoubles(settings: { enabled: boolean; timeoutMinutes: number }) {
  const idleCheck = vi.fn().mockResolvedValue(ok(null));
  injectIpcCommands({
    getIdleProposalSettings: vi.fn().mockResolvedValue(ok(settings)),
    idleCheck,
  } as never);
  return idleCheck;
}

describe("useIdleProposalTimer", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    w.__TAURI_INTERNALS__ = {};
  });
  afterEach(() => {
    vi.useRealTimers();
    delete w.__TAURI_INTERNALS__;
  });

  it("fires idle_check once after the configured timeout and never re-fires without activity", async () => {
    const idleCheck = injectIdleDoubles({ enabled: true, timeoutMinutes: 15 });
    renderHook(() => useIdleProposalTimer());

    await act(async () => { await vi.advanceTimersByTimeAsync(15 * MINUTE); });
    expect(idleCheck).toHaveBeenCalledTimes(1);

    await act(async () => { await vi.advanceTimersByTimeAsync(60 * MINUTE); });
    expect(idleCheck).toHaveBeenCalledTimes(1);
  });

  it("resets the timer on user activity (return-to-keyboard stops the check)", async () => {
    const idleCheck = injectIdleDoubles({ enabled: true, timeoutMinutes: 15 });
    renderHook(() => useIdleProposalTimer());

    await act(async () => { await vi.advanceTimersByTimeAsync(10 * MINUTE); });
    fireEvent(window, new Event("pointerdown"));
    await act(async () => { await vi.advanceTimersByTimeAsync(10 * MINUTE); });
    expect(idleCheck).not.toHaveBeenCalled();

    await act(async () => { await vi.advanceTimersByTimeAsync(5 * MINUTE); });
    expect(idleCheck).toHaveBeenCalledTimes(1);
  });

  it("stays off when idle proposals are disabled", async () => {
    const idleCheck = injectIdleDoubles({ enabled: false, timeoutMinutes: 15 });
    renderHook(() => useIdleProposalTimer());

    await act(async () => { await vi.advanceTimersByTimeAsync(60 * MINUTE); });
    expect(idleCheck).not.toHaveBeenCalled();
  });

  it("disarms on unmount (HMR remount safety)", async () => {
    const idleCheck = injectIdleDoubles({ enabled: true, timeoutMinutes: 15 });
    const { unmount } = renderHook(() => useIdleProposalTimer());

    await act(async () => { await vi.advanceTimersByTimeAsync(10 * MINUTE); });
    unmount();
    await act(async () => { await vi.advanceTimersByTimeAsync(60 * MINUTE); });
    expect(idleCheck).not.toHaveBeenCalled();
  });

  it("does nothing outside the real Tauri runtime", async () => {
    delete w.__TAURI_INTERNALS__;
    const idleCheck = injectIdleDoubles({ enabled: true, timeoutMinutes: 15 });
    renderHook(() => useIdleProposalTimer());

    await act(async () => { await vi.advanceTimersByTimeAsync(60 * MINUTE); });
    expect(idleCheck).not.toHaveBeenCalled();
  });
});
