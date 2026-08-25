/** Task lifecycle columns — mirrors the Rust run/task state machine. */
export const TASK_STATUSES = ["backlog", "queued", "running", "done", "cancelled"] as const;

export type TaskStatus = (typeof TASK_STATUSES)[number];

export function isTaskStatus(value: string): value is TaskStatus {
  return (TASK_STATUSES as readonly string[]).includes(value);
}

export const statusLabelKey = (status: string): string => `board.status_${status}`;
