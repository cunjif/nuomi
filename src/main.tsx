import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { openUrl as openerOpenUrl } from "@tauri-apps/plugin-opener";
import App from "./App";
import "./i18n";
// Hand-drawn line-art fonts (Patrick Hand = titles, Kalam = emphasis,
// Caveat = scribbled annotations). Swap + system-ui fallback keeps zh-CN
// readable since these Latin faces have no CJK glyphs.
import "@fontsource/patrick-hand/400.css";
import "@fontsource/kalam/400.css";
import "@fontsource/caveat/400.css";
import "./styles/global.css";
import { setOpenUrlImpl } from "./lib/openUrl";

// System-browser channel (chat_rich_bind spec 5.2.3): every external link
// in the app routes through the opener plugin — in-app rendering of
// arbitrary pages is forbidden (spec 4.3-4).
setOpenUrlImpl(async (url) => {
  if (!/^https?:\/\//i.test(url)) return false;
  try {
    await openerOpenUrl(url);
    return true;
  } catch {
    return false;
  }
});

const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: 1, staleTime: 5_000 } },
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  </React.StrictMode>,
);

// Signal the Rust-side watchdog that the webview mounted successfully.
// If this line never executes (JS crash / import failure), the watchdog
// auto-reloads the webview after a grace period.
invoke("__nuomi_heartbeat").catch(() => {});
