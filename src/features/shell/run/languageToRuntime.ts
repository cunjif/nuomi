/**
 * Pure mapping from a Monaco language id to a terminal run command.
 * Uses SDK configs (user-configurable) to build the command; falls back
 * to built-in defaults when no SDK config is available.
 */
import {
  DEFAULT_LANGUAGE_CONFIGS,
  DEFAULT_BUILD_CONFIGS,
  type SdkConfigs,
} from "./sdkConfigs";

/**
 * Build a terminal run command for a single file from SDK configs.
 * The command format is: `{command} {args} "{filePath}"` when args are
 * present, or `{command} "{filePath}"` when args are empty.
 *
 * For compiled languages (C/C++), args may contain `${file}` placeholder
 * which is replaced with the file path without extension.
 *
 * Returns null when the language has no SDK config or is disabled.
 */
export function languageToRunCommand(
  language: string,
  filePath: string,
  configs?: SdkConfigs | null,
): string | null {
  const langConfig = (configs?.languages ?? DEFAULT_LANGUAGE_CONFIGS)[language];
  if (langConfig === undefined || !langConfig.enabled) return null;

  const { command, args } = langConfig;
  if (args.trim() === "") {
    return `${command} "${filePath}"`;
  }

  const fileNoExt = filePath.replace(/\.[^.]+$/, "");
  const expandedArgs = args.replace(/\$\{file\}/g, fileNoExt);
  return `${command} ${expandedArgs} "${filePath}"`;
}

/**
 * Resolve a build command for a project directory based on detected project type.
 * Uses SDK configs (user-configurable) to determine the build command;
 * checks for well-known manifest files to determine the build tool.
 */
export function detectBuildCommand(
  projectFiles: string[],
  configs?: SdkConfigs | null,
): string | null {
  const buildConfigs = configs?.builds ?? DEFAULT_BUILD_CONFIGS;
  for (const buildConfig of Object.values(buildConfigs)) {
    if (buildConfig.enabled && projectFiles.includes(buildConfig.manifestFile)) {
      return buildConfig.command;
    }
  }
  return null;
}
