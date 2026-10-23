import { describe, expect, it, beforeEach } from "vitest";
import { openExternalUrl, setOpenUrlImpl } from "./openUrl";
import { useToastStore } from "./store/toastStore";
import "../i18n";

describe("openExternalUrl — opener channel (spec 5.2.3-3)", () => {
  beforeEach(() => {
    useToastStore.setState({ toasts: [] });
    setOpenUrlImpl(async () => false);
  });

  it("toasts and resolves false when the opener fails", async () => {
    const ok = await openExternalUrl("https://example.com");
    expect(ok).toBe(false);
    const state = useToastStore.getState();
    expect(state.toasts.length).toBe(1);
    expect(state.toasts[0]?.kind).toBe("error");
  });

  it("no toast on success", async () => {
    setOpenUrlImpl(async () => true);
    const ok = await openExternalUrl("https://example.com");
    expect(ok).toBe(true);
    expect(useToastStore.getState().toasts.length).toBe(0);
  });
});
