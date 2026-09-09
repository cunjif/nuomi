import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import App from "./App";
import "./i18n";
// Hand-drawn line-art fonts (Patrick Hand = titles, Kalam = emphasis,
// Caveat = scribbled annotations). Swap + system-ui fallback keeps zh-CN
// readable since these Latin faces have no CJK glyphs.
import "@fontsource/patrick-hand/400.css";
import "@fontsource/kalam/400.css";
import "@fontsource/caveat/400.css";
import "./styles/global.css";

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
