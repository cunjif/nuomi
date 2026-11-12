import { describe, expect, it } from "vitest";
import { fireEvent, screen } from "@testing-library/react";
import { renderWithProviders } from "../../test/helpers";
import "../../test/helpers";
import { useUiStore, type TerminalInstance } from "../../lib/store/uiStore";
import { TerminalTabs } from "./TerminalTabs";
import { nextTerminalTitle } from "./terminalTitle";

const ws = "ws-term-test";

function seed(terminals: TerminalInstance[]): void {
  useUiStore.setState({
    terminalByWorkspace: {
      [ws]: { terminals, activeTerminalId: terminals[0]?.id ?? null },
    },
  });
}

function tab(over: Partial<TerminalInstance> = {}): TerminalInstance {
  return {
    id: "term-1",
    title: "Terminal 1",
    createdAt: 1,
    cwd: "C:\\ws",
    cwdSource: "workspace",
    ...over,
  };
}

function currentTitle(): string | undefined {
  return useUiStore.getState().terminalByWorkspace[ws]?.terminals[0]?.title;
}

describe("TerminalTabs — rename", () => {
  it("renames the tab inline on double-click + Enter", () => {
    seed([tab()]);
    renderWithProviders(<TerminalTabs workspaceId={ws} />);

    fireEvent.doubleClick(screen.getByText("Terminal 1"));
    const input = screen.getByRole("textbox") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "  build  " } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(currentTitle()).toBe("build");
    expect(screen.queryByRole("textbox")).toBeNull();
    expect(screen.getByText("build")).toBeTruthy();
  });

  it("commits the draft on blur", () => {
    seed([tab()]);
    renderWithProviders(<TerminalTabs workspaceId={ws} />);

    fireEvent.doubleClick(screen.getByText("Terminal 1"));
    const input = screen.getByRole("textbox");
    fireEvent.change(input, { target: { value: "dev server" } });
    fireEvent.blur(input);

    expect(currentTitle()).toBe("dev server");
  });

  it("keeps the old name on Escape", () => {
    seed([tab()]);
    renderWithProviders(<TerminalTabs workspaceId={ws} />);

    fireEvent.doubleClick(screen.getByText("Terminal 1"));
    const input = screen.getByRole("textbox");
    fireEvent.change(input, { target: { value: "discarded" } });
    fireEvent.keyDown(input, { key: "Escape" });

    expect(currentTitle()).toBe("Terminal 1");
    expect(screen.queryByRole("textbox")).toBeNull();
  });

  it("ignores a blank name", () => {
    seed([tab()]);
    renderWithProviders(<TerminalTabs workspaceId={ws} />);

    fireEvent.doubleClick(screen.getByText("Terminal 1"));
    const input = screen.getByRole("textbox");
    fireEvent.change(input, { target: { value: "   " } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(currentTitle()).toBe("Terminal 1");
  });
});

describe("TerminalTabs — reorder", () => {
  const dragTransfer = (id: string): DataTransfer =>
    ({
      setData: vi.fn(),
      getData: vi.fn(() => id),
      effectAllowed: "move",
    }) as unknown as DataTransfer;

  function order(): string[] {
    return useUiStore
      .getState()
      .terminalByWorkspace[ws]!.terminals.map((t) => t.id);
  }

  it("reorders the tab list when a tab is dropped onto another", () => {
    seed([tab(), tab({ id: "term-2", title: "Terminal 2" })]);
    renderWithProviders(<TerminalTabs workspaceId={ws} />);

    const from = screen.getByText("Terminal 1").closest('[role="tab"]')!;
    const to = screen.getByText("Terminal 2").closest('[role="tab"]')!;
    const dt = dragTransfer("term-1");
    fireEvent.dragStart(from, { dataTransfer: dt });
    fireEvent.dragOver(to, { dataTransfer: dt });
    fireEvent.drop(to, { dataTransfer: dt });

    expect(order()).toEqual(["term-2", "term-1"]);
  });

  it("keeps the order when a tab is dropped onto itself", () => {
    seed([tab(), tab({ id: "term-2", title: "Terminal 2" })]);
    renderWithProviders(<TerminalTabs workspaceId={ws} />);

    const from = screen.getByText("Terminal 1").closest('[role="tab"]')!;
    const dt = dragTransfer("term-1");
    fireEvent.dragStart(from, { dataTransfer: dt });
    fireEvent.drop(from, { dataTransfer: dt });

    expect(order()).toEqual(["term-1", "term-2"]);
  });

  it("ignores drops carrying an unknown drag id", () => {
    seed([tab(), tab({ id: "term-2", title: "Terminal 2" })]);
    renderWithProviders(<TerminalTabs workspaceId={ws} />);

    const to = screen.getByText("Terminal 2").closest('[role="tab"]')!;
    const dt = dragTransfer("term-ghost");
    fireEvent.drop(to, { dataTransfer: dt });

    expect(order()).toEqual(["term-1", "term-2"]);
  });

  it("reorderTerminal keeps the active terminal selected", () => {
    seed([tab(), tab({ id: "term-2", title: "Terminal 2" })]);
    useUiStore.getState().reorderTerminal(ws, "term-2", "term-1");

    expect(order()).toEqual(["term-2", "term-1"]);
    expect(useUiStore.getState().terminalByWorkspace[ws]!.activeTerminalId).toBe("term-1");
  });
});

describe("nextTerminalTitle", () => {
  it("skips names already taken in the bucket", () => {
    expect(nextTerminalTitle([])).toBe("Terminal 1");
    expect(nextTerminalTitle([tab()])).toBe("Terminal 2");
    expect(
      nextTerminalTitle([tab(), tab({ id: "term-2", title: "Terminal 2" })]),
    ).toBe("Terminal 3");
    expect(
      nextTerminalTitle([tab({ title: "Terminal 2" }), tab({ id: "term-2", title: "logs" })]),
    ).toBe("Terminal 3");
  });
});