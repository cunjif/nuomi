import { screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { RunStatusView } from "./RunStatusView";

describe("RunStatusView — state label mapping", () => {
  it("shows '空闲' for idle state", () => {
    renderWithProviders(<RunStatusView state="idle" label="idle" />);
    expect(screen.getByText("空闲")).toBeTruthy();
  });

  it("shows '运行中' for running state", () => {
    renderWithProviders(<RunStatusView state="running" label="running" />);
    expect(screen.getByText("运行中")).toBeTruthy();
  });

  it("shows '等待审批' for awaiting_approval state", () => {
    renderWithProviders(<RunStatusView state="awaiting_approval" label="awaiting_approval" />);
    expect(screen.getByText("等待审批")).toBeTruthy();
  });

  it("shows '已失败' for failed state", () => {
    renderWithProviders(<RunStatusView state="failed" label="failed" />);
    expect(screen.getByText("已失败")).toBeTruthy();
  });

  it("renders the raw label alongside the mapped text", () => {
    renderWithProviders(<RunStatusView state="running" label="evolution-cycle-42" />);
    expect(screen.getByText("(evolution-cycle-42)")).toBeTruthy();
  });
});
