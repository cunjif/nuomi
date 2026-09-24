/**
 * Lightweight performance instrumentation for multi-workspace operations.
 * Records timing to console.debug in dev and fire-and-forgets to the backend
 * event log (topic `perf.*`) for telemetry sink export (SPEC bots-telemetry-m1).
 */

export interface PerfMetric {
  readonly operation: string;
  readonly durationMs: number;
  readonly metadata?: Record<string, unknown>;
}

function emit(metric: PerfMetric): void {
  console.debug(`[perf] ${metric.operation}: ${metric.durationMs.toFixed(1)}ms`, metric.metadata ?? "");
  reportToBackend(metric).catch(() => { /* fire-and-forget */ });
}

async function reportToBackend(metric: PerfMetric): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("app_setting_set", {
    key: `perf.${metric.operation}`,
    value: JSON.stringify({
      durationMs: Math.round(metric.durationMs * 10) / 10,
      ts: Date.now(),
      ...metric.metadata,
    }),
  });
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
