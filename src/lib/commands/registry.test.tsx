/**
 * Slash-command registry + composer integration tests. Renders Composer
 * directly with a hand-built CommandContext (renderWithProviders pattern);
 * IPC goes through the test double / injected overrides.
 */
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { QueryClient } from "@tanstack/react-query";
import { i18n } from "../../i18n";
import { Composer } from "../../features/conversation/composer/Composer";
import { injectIpcCommands, ipc } from "../../lib/ipc/client";
import { tdState } from "../../lib/ipc/test-double";
import { renderWithProviders } from "../../test/helpers";
import { parseInput, suggestCommands, type CommandContext } from "./registry";

// Importing Composer registers the built-in commands (side effect).
const LABEL = "输入消息，Enter 发送（Shift+Enter 换行）";

/** Inspectable vitest mock function. */
type MockFn = ReturnType<typeof vi.fn>;

/** Mock hooks handed out next to the context for call assertions. */
interface CommandMocks {
  navigate: MockFn;
  selectSession: MockFn;
  toggleTheme: MockFn;
  toastError: MockFn;
  toastSuccess: MockFn;
  toastWarn: MockFn;
}

function makeContext(overrides: Partial<CommandContext> = {}): { ctx: CommandContext; mocks: CommandMocks } {
  const navigate = vi.fn();
  const selectSession = vi.fn();
  const toggleTheme = vi.fn();
  const toastError = vi.fn();
  const toastSuccess = vi.fn();
  const toastWarn = vi.fn();
  const ctx: CommandContext = {
    sessionId: "s1",
    ipc,
    queryClient: new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } }),
    navigate,
    selectSession,
    toggleTheme,
    toast: { error: toastError, success: toastSuccess, warn: toastWarn },
    t: i18n.t,
    ...overrides,
  };
  return { ctx, mocks: { navigate, selectSession, toggleTheme, toastError, toastSuccess, toastWarn } };
}

function typeIntoTextarea(value: string): HTMLTextAreaElement {
  const textarea = screen.getByLabelText(LABEL) as HTMLTextAreaElement;
  fireEvent.change(textarea, { target: { value } });
  return textarea;
}

function pressKey(textarea: HTMLTextAreaElement, key: string): void {
  fireEvent.keyDown(textarea, { key });
}

describe("parseInput", () => {
  it("parses a registered command with args", () => {
    const parsed = parseInput("/workspace D:/x");
    expect(parsed).not.toBeNull();
    expect(parsed?.name).toBe("workspace");
    expect(parsed?.args).toBe("D:/x");
    expect(parsed?.command?.name).toBe("workspace");
  });

  it("tolerates surrounding and repeated whitespace", () => {
    const parsed = parseInput("  /cmd   arg1  arg2 ");
    expect(parsed?.name).toBe("cmd");
    expect(parsed?.args).toBe("arg1 arg2");
    expect(parsed?.command).toBeUndefined();
  });

  it("returns null for plain text and a bare slash", () => {
    expect(parseInput("hello world")).toBeNull();
    expect(parseInput("/")).toBeNull();
    expect(parseInput("/ ")).toBeNull();
  });

  it("suggests closest commands by prefix or contains match", () => {
    expect(suggestCommands("hel").map((c) => c.name)).toContain("help");
    expect(suggestCommands("zzz")).toHaveLength(0);
  });
});

describe("Composer — completion panel keyboard navigation", () => {
  it("lists commands for /, completes with Enter, dismisses with Escape", () => {
    renderWithProviders(<Composer disabled={false} pending={false} commandContext={makeContext().ctx} onSubmit={vi.fn()} />);
    const textarea = typeIntoTextarea("/");
    expect(screen.getAllByRole("option").length).toBeGreaterThanOrEqual(6);

    // ArrowDown moves the highlight from the first to the second command.
    pressKey(textarea, "ArrowDown");
    const options = screen.getAllByRole("option");
    expect(options[0]).toHaveAttribute("aria-selected", "false");
    expect(options[1]).toHaveAttribute("aria-selected", "true");

    pressKey(textarea, "Enter");
    expect((screen.getByLabelText(LABEL) as HTMLTextAreaElement).value).toBe("/sessions ");

    // Typing again reopens; Escape closes the panel until the draft changes.
    fireEvent.change(screen.getByLabelText(LABEL), { target: { value: "/the" } });
    expect(screen.getByRole("listbox")).toBeInTheDocument();
    pressKey(screen.getByLabelText(LABEL), "Escape");
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("completes with Tab and filters while typing", () => {
    renderWithProviders(<Composer disabled={false} pending={false} commandContext={makeContext().ctx} onSubmit={vi.fn()} />);
    const textarea = typeIntoTextarea("/se");
    const options = screen.getAllByRole("option");
    expect(options).toHaveLength(1);
    pressKey(textarea, "Tab");
    expect((screen.getByLabelText(LABEL) as HTMLTextAreaElement).value).toBe("/sessions ");
  });
});

describe("Composer — command execution", () => {
  it("/workspace switches workspace, toasts success and clears the draft", async () => {
    const { ctx, mocks } = makeContext();
    const calls: string[] = [];
    injectIpcCommands({
      setWorkspace: async (path: string) => {
        calls.push(path);
        tdState.workspaceRoot = path;
        return { status: "ok", data: { root: path, configured: true } };
      },
    });
    renderWithProviders(<Composer disabled={false} pending={false} commandContext={ctx} onSubmit={vi.fn()} />);

    const textarea = typeIntoTextarea("/workspace D:/tmp");
    pressKey(textarea, "Enter");

    await waitFor(() => expect(mocks.toastSuccess).toHaveBeenCalled());
    expect(String(mocks.toastSuccess.mock.calls[0]?.[0])).toContain("D:/tmp");
    expect(calls).toEqual(["D:/tmp"]);
    expect((screen.getByLabelText(LABEL) as HTMLTextAreaElement).value).toBe("");
  });

  it("/workspace keeps the draft and toasts the error when IPC fails", async () => {
    const { ctx, mocks } = makeContext();
    injectIpcCommands({
      setWorkspace: async () => {
        throw new Error("boom");
      },
    });
    renderWithProviders(<Composer disabled={false} pending={false} commandContext={ctx} onSubmit={vi.fn()} />);

    const textarea = typeIntoTextarea("/workspace D:/tmp");
    pressKey(textarea, "Enter");

    await waitFor(() => expect(mocks.toastError).toHaveBeenCalled());
    expect(String(mocks.toastError.mock.calls[0]?.[0])).toContain("boom");
    expect((screen.getByLabelText(LABEL) as HTMLTextAreaElement).value).toBe("/workspace D:/tmp");
  });

  it("/workspace without a path toasts the usage hint", async () => {
    const { ctx, mocks } = makeContext();
    renderWithProviders(<Composer disabled={false} pending={false} commandContext={ctx} onSubmit={vi.fn()} />);

    const textarea = typeIntoTextarea("/workspace ");
    pressKey(textarea, "Enter");

    await waitFor(() => expect(mocks.toastError).toHaveBeenCalled());
    expect(String(mocks.toastError.mock.calls[0]?.[0])).toContain("/workspace <path>");
  });

  it("unknown command toasts the closest matches", async () => {
    const { ctx, mocks } = makeContext();
    renderWithProviders(<Composer disabled={false} pending={false} commandContext={ctx} onSubmit={vi.fn()} />);

    const textarea = typeIntoTextarea("/helpme");
    pressKey(textarea, "Enter");

    await waitFor(() => expect(mocks.toastError).toHaveBeenCalled());
    const message = String(mocks.toastError.mock.calls[0]?.[0]);
    expect(message).toContain("helpme");
    expect(message).toContain("/help");
  });

  it("/help lists every registered command with descriptions", async () => {
    const { ctx, mocks } = makeContext();
    renderWithProviders(<Composer disabled={false} pending={false} commandContext={ctx} onSubmit={vi.fn()} />);

    const textarea = typeIntoTextarea("/help ");
    pressKey(textarea, "Enter");

    await waitFor(() => expect(mocks.toastSuccess).toHaveBeenCalled());
    const message = String(mocks.toastSuccess.mock.calls[0]?.[0]);
    for (const name of ["workspace", "new", "sessions", "clear", "help", "theme"]) {
      expect(message).toContain(`/${name}`);
    }
  });

  it("/clear empties the draft", async () => {
    const { ctx, mocks } = makeContext();
    renderWithProviders(<Composer disabled={false} pending={false} commandContext={ctx} onSubmit={vi.fn()} />);

    const textarea = typeIntoTextarea("/clear ");
    pressKey(textarea, "Enter");

    await waitFor(() => expect((screen.getByLabelText(LABEL) as HTMLTextAreaElement).value).toBe(""));
    expect(mocks.toastError).not.toHaveBeenCalled();
  });

  it("/new creates a session and selects it", async () => {
    const { ctx, mocks } = makeContext();
    renderWithProviders(<Composer disabled={false} pending={false} commandContext={ctx} onSubmit={vi.fn()} />);

    const textarea = typeIntoTextarea("/new ");
    pressKey(textarea, "Enter");

    await waitFor(() => expect(mocks.toastSuccess).toHaveBeenCalled());
    expect(mocks.selectSession).toHaveBeenCalledTimes(1);
    expect(mocks.selectSession.mock.calls[0]?.[0]).toMatch(/^s-/);
  });
});
