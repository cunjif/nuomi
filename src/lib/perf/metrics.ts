/**
 * Lightweight performance instrumentation for multi-workspace operations.
 * Records timing to console.debug in dev; structured events for telemetry
 * sinks can be added later by replacing `emit`.
 */

export interface PerfMetric {
  readonly operation: string;
  readonly durationMs: number;
  readonly metadata?: Record<string, unknown>;
}

function emit(metric: PerfMetric): void {
  console.debug(`[perf] ${metric.operation}: ${metric.durationMs.toFixed(1)}ms`, metric.metadata ?? "");
}

export async function measureAsync<T>(
  operation: string,
  fn: () => Promise<T>,
  metadata?: Record<string, unknown>,
): Promise<T> {
  const start = performance.now();
  try {
    return await fn();
  } finally {
    emit({ operation, durationMs: performance.now() - start, metadata });
  }
}

export function measureSync<T>(
  operation: string,
  fn: () => T,
  metadata?: Record<string, unknown>,
): T {
  const start = performance.now();
  try {
    return fn();
  } finally {
    emit({ operation, durationMs: performance.now() - start, metadata });
  }
}
