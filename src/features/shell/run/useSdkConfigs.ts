/**
 * Hook: read and write SDK configs from the app_settings KV store.
 * Returns the parsed SdkConfigs (merged with defaults) and a setter
 * that persists the full blob back to the backend.
 */
import { useCallback, useMemo } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../../lib/ipc/client";
import {
  DEFAULT_SDK_CONFIGS,
  SDK_CONFIGS_KEY,
  parseSdkConfigs,
  serializeSdkConfigs,
  type SdkConfigs,
} from "./sdkConfigs";

export function useSdkConfigs(): {
  configs: SdkConfigs;
  isLoading: boolean;
  updateLanguage: (language: string, patch: Partial<SdkConfigs["languages"][string]>) => void;
  updateBuild: (buildKey: string, patch: Partial<SdkConfigs["builds"][string]>) => void;
  resetToDefaults: () => void;
} {
  const qc = useQueryClient();

  const query = useQuery({
    queryKey: ["app-setting", SDK_CONFIGS_KEY],
    queryFn: () => ipc.appSettingGet(SDK_CONFIGS_KEY),
  });

  const configs = useMemo(() => parseSdkConfigs(query.data ?? null), [query.data]);

  const persist = useCallback(
    (next: SdkConfigs): void => {
      const serialized = serializeSdkConfigs(next);
      qc.setQueryData(["app-setting", SDK_CONFIGS_KEY], serialized);
      void ipc.appSettingSet(SDK_CONFIGS_KEY, serialized);
    },
    [qc],
  );

  const updateLanguage = useCallback(
    (language: string, patch: Partial<SdkConfigs["languages"][string]>): void => {
      const current = configs.languages[language];
      if (current === undefined) return;
      const next: SdkConfigs = {
        ...configs,
        languages: { ...configs.languages, [language]: { ...current, ...patch } },
      };
      persist(next);
    },
    [configs, persist],
  );

  const updateBuild = useCallback(
    (buildKey: string, patch: Partial<SdkConfigs["builds"][string]>): void => {
      const current = configs.builds[buildKey];
      if (current === undefined) return;
      const next: SdkConfigs = {
        ...configs,
        builds: { ...configs.builds, [buildKey]: { ...current, ...patch } },
      };
      persist(next);
    },
    [configs, persist],
  );

  const resetToDefaults = useCallback((): void => {
    persist(DEFAULT_SDK_CONFIGS);
  }, [persist]);

  return {
    configs,
    isLoading: query.isLoading,
    updateLanguage,
    updateBuild,
    resetToDefaults,
  };
}
