import { renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { AgentRefDto } from "../../../lib/ipc/client";
import { useAtRoute } from "./useAtRoute";

const participants: AgentRefDto[] = [
  { kind: "cli", id: "agent-1", name: "Planner" },
  { kind: "role", id: "role-1", name: "Coder" },
  { kind: "role", id: "role-2", name: "Reviewer" },
];

describe("useAtRoute — @ Agent routing parser", () => {
  it("returns default mode when text has no @ markers", () => {
    const { result } = renderHook(() => useAtRoute("hello world", participants));
    expect(result.current.routeMode).toBe("default");
    expect(result.current.targetAgentIds).toEqual([]);
    expect(result.current.unresolved).toEqual([]);
  });

  it("parses a single @ mention matching a participant", () => {
    const { result } = renderHook(() => useAtRoute("@Planner please help", participants));
    expect(result.current.routeMode).toBe("at_directed");
    expect(result.current.targetAgentIds).toEqual(["agent-1"]);
    expect(result.current.unresolved).toEqual([]);
  });

  it("parses multiple @ mentions matching participants", () => {
    const { result } = renderHook(() => useAtRoute("@Planner and @Coder please collaborate", participants));
    expect(result.current.routeMode).toBe("at_directed");
    expect(result.current.targetAgentIds).toEqual(["agent-1", "role-1"]);
    expect(result.current.unresolved).toEqual([]);
  });

  it("reports unresolved for @ markers not matching any participant", () => {
    const { result } = renderHook(() => useAtRoute("@UnknownAgent help", participants));
    expect(result.current.routeMode).toBe("default");
    expect(result.current.targetAgentIds).toEqual([]);
    expect(result.current.unresolved).toEqual(["unknownagent"]);
  });

  it("deduplicates repeated @ mentions of the same agent", () => {
    const { result } = renderHook(() => useAtRoute("@Coder @Coder @Coder", participants));
    expect(result.current.targetAgentIds).toEqual(["role-1"]);
  });

  it("handles mixed resolved and unresolved @ markers", () => {
    const { result } = renderHook(() => useAtRoute("@Planner @Ghost @Coder", participants));
    expect(result.current.routeMode).toBe("at_directed");
    expect(result.current.targetAgentIds).toEqual(["agent-1", "role-1"]);
    expect(result.current.unresolved).toEqual(["ghost"]);
  });

  it("handles empty text", () => {
    const { result } = renderHook(() => useAtRoute("", participants));
    expect(result.current.routeMode).toBe("default");
    expect(result.current.targetAgentIds).toEqual([]);
  });

  it("handles empty participants list", () => {
    const { result } = renderHook(() => useAtRoute("@Anyone help", []));
    expect(result.current.routeMode).toBe("default");
    expect(result.current.unresolved).toEqual(["anyone"]);
  });
});
