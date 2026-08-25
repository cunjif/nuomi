import type { ReactNode } from "react";
import { Toaster } from "./components/ui/Toaster";
import { Shell } from "./features/shell/Shell";

/** App root: the U8 shell plus the global toast region. */
export default function App(): ReactNode {
  return (
    <>
      <Shell />
      <Toaster />
    </>
  );
}
