import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PanelResizer } from "./PanelResizer";

afterEach(() => cleanup());

function setup(side: "left" | "right", width = 224) {
  const onWidthChange = vi.fn();
  const onCollapse = vi.fn();
  const onReset = vi.fn();
  render(
    <div className="relative">
      <PanelResizer
        side={side}
        width={width}
        minWidth={180}
        maxWidth={480}
        onWidthChange={onWidthChange}
        onCollapse={onCollapse}
        onReset={onReset}
        resizeLabel="resize"
        collapseLabel="collapse"
      />
    </div>,
  );
  return { onWidthChange, onCollapse, onReset };
}

describe("PanelResizer", () => {
  it("renders a vertical separator handle", () => {
    setup("left");
    expect(screen.getByRole("separator", { name: "resize" })).toHaveAttribute("aria-orientation", "vertical");
  });

  it("dragging right widens a left panel; mouseup ends the drag", () => {
    const { onWidthChange } = setup("left", 224);
    fireEvent.mouseDown(screen.getByTestId("panel-resizer-left"), { button: 0, clientX: 100 });
    fireEvent.mouseMove(window, { clientX: 150 });
    expect(onWidthChange).toHaveBeenLastCalledWith(274);
    fireEvent.mouseUp(window);
    fireEvent.mouseMove(window, { clientX: 300 });
    expect(onWidthChange).toHaveBeenCalledTimes(1);
  });

  it("dragging left widens a right panel", () => {
    const { onWidthChange } = setup("right", 224);
    fireEvent.mouseDown(screen.getByTestId("panel-resizer-right"), { button: 0, clientX: 300 });
    fireEvent.mouseMove(window, { clientX: 250 });
    expect(onWidthChange).toHaveBeenLastCalledWith(274);
  });

  it("clamps the dragged width to min/max", () => {
    const { onWidthChange } = setup("left", 224);
    const handle = screen.getByTestId("panel-resizer-left");
    fireEvent.mouseDown(handle, { button: 0, clientX: 0 });
    fireEvent.mouseMove(window, { clientX: -5000 });
    expect(onWidthChange).toHaveBeenLastCalledWith(180);
    fireEvent.mouseMove(window, { clientX: 5000 });
    expect(onWidthChange).toHaveBeenLastCalledWith(480);
  });

  it("double-click resets the width", () => {
    const { onReset } = setup("left");
    fireEvent.dblClick(screen.getByTestId("panel-resizer-left"));
    expect(onReset).toHaveBeenCalledTimes(1);
  });

  it("the chevron collapses without triggering a drag", () => {
    const { onCollapse, onWidthChange } = setup("left");
    fireEvent.mouseDown(screen.getByTestId("panel-collapse-left"));
    fireEvent.click(screen.getByTestId("panel-collapse-left"));
    expect(onCollapse).toHaveBeenCalledTimes(1);
    expect(onWidthChange).not.toHaveBeenCalled();
  });
});
