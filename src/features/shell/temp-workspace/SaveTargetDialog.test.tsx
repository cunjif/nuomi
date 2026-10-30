import { screen, fireEvent } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { renderWithProviders } from "../../../test/helpers";
import { Toaster } from "../../../components/ui/Toaster";
import { SaveTargetDialog } from "./SaveTargetDialog";

describe("SaveTargetDialog", () => {
  it("prefills the target path with the original absPath", async () => {
    renderWithProviders(
      <>
        <SaveTargetDialog
          open={true}
          absPath="/external/notes.txt"
          content="hello"
          onClose={() => {}}
        />
        <Toaster />
      </>,
    );
    const input = await screen.findByLabelText("目标路径");
    expect(input).toHaveValue("/external/notes.txt");
  });

  it("allows the user to edit the target path", async () => {
    renderWithProviders(
      <>
        <SaveTargetDialog
          open={true}
          absPath="/external/notes.txt"
          content="hello"
          onClose={() => {}}
        />
        <Toaster />
      </>,
    );
    const input = await screen.findByLabelText("目标路径");
    fireEvent.change(input, { target: { value: "/new-path/notes.txt" } });
    expect(input).toHaveValue("/new-path/notes.txt");
  });

  it("cancel preserves the dialog content without saving", async () => {
    const onClose = vi.fn();
    renderWithProviders(
      <>
        <SaveTargetDialog open={true} absPath="/x.txt" content="hello" onClose={onClose} />
        <Toaster />
      </>,
    );
    await screen.findByLabelText("目标路径");
    fireEvent.click(screen.getByText("取消"));
    expect(onClose).toHaveBeenCalled();
  });
});
