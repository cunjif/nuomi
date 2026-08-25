import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { injectIpcCommands } from "../../lib/ipc/client";
import { renderWithProviders } from "../../test/helpers";
import { i18n } from "../../i18n";
import { GitView } from "./GitView";

const ok = <T,>(data: T) => ({ status: "ok" as const, data });

function base() {
  return {
    gitStatus: vi.fn().mockResolvedValue(ok([
      { indexStatus: " ", worktreeStatus: "M", path: "src/main.rs" },
      { indexStatus: "A", worktreeStatus: " ", path: "new.txt" },
    ])),
    gitStage: vi.fn().mockResolvedValue(ok(null)),
    gitCommit: vi.fn().mockResolvedValue(ok("abc123")),
    gitPush: vi.fn().mockResolvedValue(ok("pushed")),
    gitLog: vi.fn().mockResolvedValue(ok([
      { hash: "abcdef1234567890", subject: "feat: init", author: "james" },
    ])),
  };
}

describe("GitView (SPEC D5/US5)", () => {
  beforeEach(async () => {
    await i18n.changeLanguage("en");
  });
  it("lists changes and stages selected paths", async () => {
    const overrides = base();
    const stage = overrides.gitStage;
    injectIpcCommands(overrides as never);
    renderWithProviders(<GitView />);

    expect(await screen.findByText("src/main.rs")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("checkbox", { name: /new\.txt/ }));
    fireEvent.click(screen.getByRole("button", { name: /stage/i }));
    await waitFor(() => expect(stage).toHaveBeenCalledWith(["new.txt"]));
  });

  it("commits with the typed message", async () => {
    const overrides = base();
    const commit = overrides.gitCommit;
    injectIpcCommands(overrides as never);
    renderWithProviders(<GitView />);

    fireEvent.change(await screen.findByRole("textbox", { name: /message/i }), {
      target: { value: "fix(core): x" },
    });
    fireEvent.click(screen.getByRole("button", { name: /^commit$/i }));
    await waitFor(() => expect(commit).toHaveBeenCalledWith("fix(core): x"));
  });

  it("pushes to the remote/branch inputs", async () => {
    const overrides = base();
    const push = overrides.gitPush;
    injectIpcCommands(overrides as never);
    renderWithProviders(<GitView />);

    fireEvent.click(await screen.findByRole("button", { name: /push/i }));
    await waitFor(() => expect(push).toHaveBeenCalledWith("origin", "main"));
  });
});
