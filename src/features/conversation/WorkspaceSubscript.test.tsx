import { fireEvent, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { WorkspaceSubscript } from "./WorkspaceSubscript";

describe("WorkspaceSubscript", () => {
  it("workspaceName null → renders nothing", () => {
    const { container } = renderWithProviders(
      <WorkspaceSubscript workspaceName={null} onOpenWorkspaceList={() => {}} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("workspaceName empty string → renders nothing (no empty parens)", () => {
    const { container } = renderWithProviders(
      <WorkspaceSubscript workspaceName="" onOpenWorkspaceList={() => {}} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("workspaceName whitespace-only → renders nothing", () => {
    const { container } = renderWithProviders(
      <WorkspaceSubscript workspaceName="   " onOpenWorkspaceList={() => {}} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders (workspaceName) with truncate class for long names", () => {
    const longName = "very-long-workspace-name-that-should-truncate";
    renderWithProviders(
      <WorkspaceSubscript workspaceName={longName} onOpenWorkspaceList={() => {}} />,
    );
    const btn = screen.getByRole("button");
    expect(btn.textContent).toBe(`(${longName})`);
    expect(btn.className).toContain("truncate");
  });

  it("click triggers onOpenWorkspaceList", () => {
    const onOpen = vi.fn();
    renderWithProviders(
      <WorkspaceSubscript workspaceName="my-project" onOpenWorkspaceList={onOpen} />,
    );
    fireEvent.click(screen.getByRole("button"));
    expect(onOpen).toHaveBeenCalledOnce();
  });
});
