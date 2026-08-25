import "@testing-library/jest-dom/vitest";

// jsdom lacks ResizeObserver; @tanstack/react-virtual needs it.
class ResizeObserverStub {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}
if (typeof globalThis.ResizeObserver === "undefined") {
  globalThis.ResizeObserver = ResizeObserverStub as unknown as typeof ResizeObserver;
}

// jsdom lacks requestAnimationFrame timing guarantees for rAF batching —
// polyfill only when missing (modern jsdom implements it).
if (typeof globalThis.requestAnimationFrame === "undefined") {
  globalThis.requestAnimationFrame = ((cb: FrameRequestCallback) =>
    setTimeout(() => cb(Date.now()), 16) as unknown as number) as typeof requestAnimationFrame;
  globalThis.cancelAnimationFrame = ((handle: number) => clearTimeout(handle)) as typeof cancelAnimationFrame;
}

// Monaco's clipboard contribution probes this API at import time;
// jsdom does not implement it.
Object.defineProperty(document, "queryCommandSupported", { value: () => true });
