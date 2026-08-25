/**
 * Facade over the split test double (state + commands). Kept so existing
 * imports keep working: `injectIpcCommands(testDoubleCommands())`.
 */
export { tdReset, tdSeedFiles, tdState } from "./test-double-state";
export { testDoubleCommands } from "./test-double-commands";
