import { screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { TodoView } from "./TodoView";
import type { TodoItemDto } from "../../lib/ipc/bindings.gen";

describe("TodoView — readonly todo list", () => {
  it("shows empty state when todos is empty", () => {
    renderWithProviders(<TodoView todos={[]} />);
    expect(screen.getByText("暂无待办")).toBeTruthy();
  });

  it("renders unchecked items with ☐", () => {
    const todos: TodoItemDto[] = [
      { id: "t1", description: "Review PR", completed: false },
    ];
    renderWithProviders(<TodoView todos={todos} />);
    expect(screen.getByText("☐")).toBeTruthy();
    expect(screen.getByText("Review PR")).toBeTruthy();
  });

  it("renders completed items with ☑ and line-through", () => {
    const todos: TodoItemDto[] = [
      { id: "t1", description: "Done task", completed: true },
    ];
    renderWithProviders(<TodoView todos={todos} />);
    expect(screen.getByText("☑")).toBeTruthy();
    const desc = screen.getByText("Done task");
    expect(desc.parentElement?.className).toContain("line-through");
  });

  it("renders multiple items in order", () => {
    const todos: TodoItemDto[] = [
      { id: "t1", description: "First", completed: false },
      { id: "t2", description: "Second", completed: true },
      { id: "t3", description: "Third", completed: false },
    ];
    renderWithProviders(<TodoView todos={todos} />);
    expect(screen.getByText("First")).toBeTruthy();
    expect(screen.getByText("Second")).toBeTruthy();
    expect(screen.getByText("Third")).toBeTruthy();
  });
});
