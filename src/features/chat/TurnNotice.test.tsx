import { fireEvent, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { scanRawPaletteClasses } from "../../test/theme-tokens";
import { TurnNotice } from "./TurnNotice";

describe("TurnNotice — terminal outcome notice", () => {
  it("renders the stopped statement without a retry button", () => {
    renderWithProviders(<TurnNotice kind="stopped" />);
    const notice = screen.getByTestId("turn-notice");
    expect(notice).toHaveAttribute("data-kind", "stopped");
    expect(screen.getByRole("status")).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("shows the error summary and wires the retry affordance for failed", () => {
    const onRetry = vi.fn();
    renderWithProviders(<TurnNotice kind="failed" summary="provider 超时" onRetry={onRetry} />);
    expect(screen.getByTestId("turn-notice")).toHaveAttribute("data-kind", "failed");
    expect(screen.getByRole("status").textContent).toContain("provider 超时");
    fireEvent.click(screen.getByRole("button"));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });

  it("renders interrupted with a retry affordance", () => {
    renderWithProviders(<TurnNotice kind="interrupted" onRetry={() => {}} />);
    expect(screen.getByTestId("turn-notice")).toHaveAttribute("data-kind", "interrupted");
    expect(screen.getByRole("button")).toBeTruthy();
  });

  it("uses only semantic theme tokens across all kinds (no raw palette classes)", () => {
    const { container } = renderWithProviders(
      <div>
        <TurnNotice kind="stopped" />
        <TurnNotice kind="failed" summary="x" onRetry={() => {}} />
        <TurnNotice kind="interrupted" onRetry={() => {}} />
      </div>,
    );
    expect(scanRawPaletteClasses(container)).toEqual([]);
  });
});
