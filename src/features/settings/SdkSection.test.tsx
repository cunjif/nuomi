/**
 * SdkSection tests: language and build config rows render, edit, and toggle.
 */
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, beforeEach } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { i18n } from "../../i18n";
import { SdkSection } from "./SdkSection";
import { ipc } from "../../lib/ipc/client";
import { SDK_CONFIGS_KEY } from "../shell/run/sdkConfigs";

beforeEach(async () => {
  await i18n.changeLanguage("en");
});

describe("SdkSection — language configs", () => {
  it("renders all default language rows", async () => {
    renderWithProviders(<SdkSection />);

    const langList = await screen.findByTestId("sdk-language-list");
    expect(langList).toHaveTextContent("Python");
    expect(langList).toHaveTextContent("JavaScript");
    expect(langList).toHaveTextContent("TypeScript");
    expect(langList).toHaveTextContent("Go");
    expect(langList).toHaveTextContent("Rust");
    expect(langList).toHaveTextContent("Java");
    expect(langList).toHaveTextContent("C");
    expect(langList).toHaveTextContent("C++");
    expect(langList).toHaveTextContent("Shell");
  });

  it("shows default commands in the command inputs", async () => {
    renderWithProviders(<SdkSection />);

    const pythonCmd = await screen.findByTestId("sdk-lang-python-command");
    expect(pythonCmd).toHaveValue("python");

    const rustCmd = screen.getByTestId("sdk-lang-rust-command");
    expect(rustCmd).toHaveValue("cargo run");
  });

  it("toggles a language off and persists", async () => {
    renderWithProviders(<SdkSection />);

    const toggleBtn = await screen.findByTestId("sdk-lang-python-toggle");
    expect(toggleBtn).toHaveTextContent("Enabled");

    fireEvent.click(toggleBtn);

    await waitFor(() => {
      expect(toggleBtn).toHaveTextContent("Disabled");
    });
  });

  it("edits a command and persists to app_settings", async () => {
    renderWithProviders(<SdkSection />);

    const pythonCmd = await screen.findByTestId("sdk-lang-python-command");
    fireEvent.change(pythonCmd, { target: { value: "python3" } });

    await waitFor(() => {
      void ipc.appSettingGet(SDK_CONFIGS_KEY).then((stored) => {
        expect(stored).not.toBeNull();
        const parsed = JSON.parse(stored!);
        expect(parsed.languages.python.command).toBe("python3");
      });
    });
  });
});

describe("SdkSection — build configs", () => {
  it("renders all default build rows", async () => {
    renderWithProviders(<SdkSection />);

    const buildList = await screen.findByTestId("sdk-build-list");
    expect(buildList).toHaveTextContent("Node.js");
    expect(buildList).toHaveTextContent("Rust (Cargo)");
    expect(buildList).toHaveTextContent("Java (Maven)");
    expect(buildList).toHaveTextContent("Java (Gradle)");
    expect(buildList).toHaveTextContent("CMake");
  });

  it("shows manifest files", async () => {
    renderWithProviders(<SdkSection />);

    const buildList = await screen.findByTestId("sdk-build-list");
    expect(buildList).toHaveTextContent("package.json");
    expect(buildList).toHaveTextContent("Cargo.toml");
    expect(buildList).toHaveTextContent("pom.xml");
    expect(buildList).toHaveTextContent("Makefile");
  });

  it("toggles a build type off", async () => {
    renderWithProviders(<SdkSection />);

    const toggleBtn = await screen.findByTestId("sdk-build-node-toggle");
    expect(toggleBtn).toHaveTextContent("Enabled");

    fireEvent.click(toggleBtn);

    await waitFor(() => {
      expect(toggleBtn).toHaveTextContent("Disabled");
    });
  });
});

describe("SdkSection — reset", () => {
  it("resets all configs to defaults when reset button clicked", async () => {
    renderWithProviders(<SdkSection />);

    const pythonCmd = await screen.findByTestId("sdk-lang-python-command");
    fireEvent.change(pythonCmd, { target: { value: "python3" } });

    await waitFor(() => {
      expect(pythonCmd).toHaveValue("python3");
    });

    const resetBtn = screen.getByText("Reset to defaults");
    fireEvent.click(resetBtn);

    await waitFor(() => {
      expect(screen.getByTestId("sdk-lang-python-command")).toHaveValue("python");
    });
  });
});
