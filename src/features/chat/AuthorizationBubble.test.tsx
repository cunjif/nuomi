import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach } from "vitest";
import { AuthorizationBubble } from "./AuthorizationBubble";
import type { AuthorizationRequest } from "./useAuthorizationStream";
import { useToastStore } from "../../lib/store/toastStore";
import { ipc } from "../../lib/ipc/client";
import "../../test/helpers";
import "../../i18n";

function request(overrides: Partial<AuthorizationRequest> = {}): AuthorizationRequest {
  return {
    authorizationId: "fa-1",
    sessionId: "s1",
    roleId: "role_1",
    roleDisplayName: "Researcher",
    op: "write",
    targetPath: "C:\\proj\\a.txt",
    state: "pending",
    requestedAt: 1_000,
    expiresAt: 10_000,
    ...overrides,
  };
}

describe("AuthorizationBubble — three-option interaction (spec 5.4.1-4a)", () => {
  beforeEach(() => {
    useToastStore.setState({ toasts: [] });
  });

  it("pending state renders the three options and request details", () => {
    render(<AuthorizationBubble request={request()} now={5_000} />);
    expect(screen.getByTestId("authorization-bubble").getAttribute("data-state")).toBe("pending");
    expect(screen.getByText("Researcher")).not.toBeNull();
    expect(screen.getByText("C:\\proj\\a.txt")).not.toBeNull();
    expect(screen.getByRole("button", { name: "仅一次授权" })).not.toBeNull();
    expect(screen.getByRole("button", { name: "会话内授权" })).not.toBeNull();
    expect(screen.getByRole("button", { name: "拒绝" })).not.toBeNull();
  });

  it("submitting a decision calls resolveAuthorization", async () => {
    const calls: { authorizationId: string; decision: string }[] = [];
    const resolve = vi.fn(async (input: { authorizationId: string; decision: string }) => {
      calls.push(input);
      return null;
    });
    vi.spyOn(ipc, "resolveAuthorization").mockImplementation(resolve);
    render(<AuthorizationBubble request={request()} now={5_000} />);
    fireEvent.click(screen.getByRole("button", { name: "仅一次授权" }));
    await waitFor(() => expect(resolve).toHaveBeenCalledTimes(1));
    expect(calls).toEqual([
      {
        authorizationId: "fa-1",
        decision: "grant_once",
      },
    ]);
  });

  it("terminal states render outcomes without actions", () => {
    for (const state of ["granted_once", "granted_session", "denied", "timeout_default", "voided"] as const) {
      const { unmount } = render(<AuthorizationBubble request={request({ state })} />);
      expect(screen.getByTestId("authorization-terminal")).not.toBeNull();
      expect(screen.queryByRole("button", { name: "拒绝" })).toBeNull();
      unmount();
    }
  });

  it("pending past the deadline shows the timeout reminder (spec 5.4.1-8a)", () => {
    render(<AuthorizationBubble request={request()} now={20_000} />);
    expect(screen.getByTestId("authorization-timeout-reminder")).not.toBeNull();
    // Still actionable (the timeout_notified state keeps the options).
    expect(screen.getByRole("button", { name: "会话内授权" })).not.toBeNull();
  });

  it("before the deadline no reminder shows", () => {
    render(<AuthorizationBubble request={request()} now={5_000} />);
    expect(screen.queryByTestId("authorization-timeout-reminder")).toBeNull();
  });

  it("resolve failure toasts and keeps the bubble actionable", async () => {
    vi.spyOn(ipc, "resolveAuthorization").mockRejectedValue(new Error("boom"));
    render(<AuthorizationBubble request={request()} now={5_000} />);
    fireEvent.click(screen.getByRole("button", { name: "拒绝" }));
    await waitFor(() => expect(useToastStore.getState().toasts.length).toBe(1));
    expect(screen.getByRole("button", { name: "拒绝" })).not.toBeNull();
  });
});
