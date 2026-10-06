import { screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { injectIpcCommands } from "../../lib/ipc/client";
import { testDoubleCommands } from "../../lib/ipc/test-double";
import { renderWithProviders } from "../../test/helpers";
import { GateInbox } from "./GateInbox";

describe("GateInbox", () => {
  it("renders heading", async () => {
    const base = testDoubleCommands();
    injectIpcCommands(base);

    renderWithProviders(<GateInbox />);

    expect(await screen.findByText("验收门收件箱")).toBeInTheDocument();
  });
});
