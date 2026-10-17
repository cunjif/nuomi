import type { ReactNode } from "react";
import type { TodoItemDto } from "../../lib/ipc/bindings.gen";

export interface TodoViewProps {
  todos: TodoItemDto[];
}

export function TodoView({ todos }: TodoViewProps): ReactNode {
  if (todos.length === 0) {
    return (
      <div className="text-xs text-ink-muted">暂无待办</div>
    );
  }

  return (
    <ul className="space-y-1">
      {todos.map((todo) => (
        <li
          key={todo.id}
          className={`flex items-start gap-2 text-sm ${
            todo.completed
              ? "text-ink-muted line-through"
              : "text-ink"
          }`}
        >
          <span className="shrink-0">
            {todo.completed ? "☑" : "☐"}
          </span>
          <span className="break-words">{todo.description}</span>
        </li>
      ))}
    </ul>
  );
}
