import { fireEvent, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { ViewScopeToggle } from "./ViewScopeToggle";

describe("ViewScopeToggle", () => {
  it("renders both buttons with the surface attribute", () => {
    renderWithProviders(
      <ViewScopeToggle surface="board" scope="all" onScopeChange={vi.fn()} />,
    );
    expect(screen.getByText("仅看聚焦")).toBeInTheDocument();
    expect(screen.getByText("全部工作区")).toBeInTheDocument();
  });

  it("calls onScopeChange with 'focused' when the focused button is clicked", () => {
    const onScopeChange = vi.fn();
    renderWithProviders(
      <ViewScopeToggle surface="board" scope="all" onScopeChange={onScopeChange} />,
    );
    fireEvent.click(screen.getByText("仅看聚焦"));
    expect(onScopeChange).toHaveBeenCalledWith("focused");
  });

  it("calls onScopeChange with 'all' when the all button is clicked", () => {
    const onScopeChange = vi.fn();
    renderWithProviders(
      <ViewScopeToggle surface="approvals" scope="focused" onScopeChange={onScopeChange} />,
    );
    fireEvent.click(screen.getByText("全部工作区"));
    expect(onScopeChange).toHaveBeenCalledWith("all");
  });
});
