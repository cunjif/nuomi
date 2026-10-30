import { describe, expect, it } from "vitest";
import { injectIpcCommands } from "../../../lib/ipc/client";
import { testDoubleCommands } from "../../../lib/ipc/test-double";
import { tdReset, tdSeedFiles } from "../../../lib/ipc/test-double";
import { checkFileSize, classifyPath, isOutsideWorkspaces, MAX_TEMP_FILE_BYTES } from "./pathClassifier";

describe("pathClassifier", () => {
  it("classifies a directory via listDir success", async () => {
    injectIpcCommands(testDoubleCommands());
    tdReset();
    const kind = await classifyPath("C:\\some-dir");
    expect(kind).toBe("directory");
  });

  it("classifies a file when listDir fails", async () => {
    const base = testDoubleCommands();
    injectIpcCommands({
      ...base,
      listDir: async () => ({ status: "error", error: { generic: { code: "not_a_dir", message: "" } } } as never),
    });
    tdReset();
    const kind = await classifyPath("C:\\file.txt");
    expect(kind).toBe("file");
  });

  it("detects paths inside and outside workspaces", () => {
    const roots = ["C:\\projects\\nuomi", "C:\\projects\\other"];
    expect(isOutsideWorkspaces("C:\\external\\file.txt", roots)).toBe(true);
    expect(isOutsideWorkspaces("C:\\projects\\nuomi\\src\\main.ts", roots)).toBe(false);
    expect(isOutsideWorkspaces("C:\\projects\\nuomi", roots)).toBe(false);
    // Boundary: C:\projects\nuomiX is NOT inside C:\projects\nuomi
    expect(isOutsideWorkspaces("C:\\projects\\nuomiX\\file.ts", roots)).toBe(true);
  });

  it("accepts files under the 10MB cap and rejects oversized files", async () => {
    injectIpcCommands(testDoubleCommands());
    tdReset();
    tdSeedFiles({ "small.txt": "hello" });
    const small = await checkFileSize("small.txt");
    expect(small.ok).toBe(true);
    expect(small.size).toBe(5);

    // Simulate an oversized file by overriding readExternalFile.
    const base = testDoubleCommands();
    const huge = "x".repeat(MAX_TEMP_FILE_BYTES + 1);
    injectIpcCommands({
      ...base,
      readExternalFile: async () => ({ status: "ok", data: huge } as never),
    });
    const big = await checkFileSize("huge.txt");
    expect(big.ok).toBe(false);
  });
});
