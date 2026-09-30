import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";
import type { ViewSurface, ViewScope } from "./ViewScopeToggle";

export function useViewScope(surface: ViewSurface) {
  const queryClient = useQueryClient();
  const scopeQuery = useQuery({
    queryKey: ["view_scope", surface],
    queryFn: async () => {
      const data = await ipc.getViewScope(surface);
      return data as ViewScope;
    },
    staleTime: 30_000,
  });

  const setScope = async (scope: ViewScope) => {
    await ipc.setViewScope(surface, scope);
    await queryClient.invalidateQueries({ queryKey: ["view_scope", surface] });
  };

  return {
    scope: scopeQuery.data ?? "all",
    setScope,
    isLoading: scopeQuery.isLoading,
  };
}
