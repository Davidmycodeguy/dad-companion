import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "react-router";

import { CardWindow } from "@/card-window";
import { Toaster } from "@/components/ui/sonner";
import { StartupUpdateCheck } from "@/components/update-check";
import { TooltipProvider } from "@/components/ui/tooltip";
import { LiveDataRefresher } from "@/lib/live-data";
import { router } from "@/router";
import "./index.css";

const queryClient = new QueryClient({
  defaultOptions: { queries: { staleTime: 30_000, refetchOnWindowFocus: false, retry: 1 } },
});

// A right click shows the browser's menu (Back, Reload, Inspect) unless it is on text the user can edit.
window.addEventListener("contextmenu", (event) => {
  const target = event.target as HTMLElement | null;
  if (!target?.closest("input, textarea, [contenteditable]")) event.preventDefault();
});

const root = ReactDOM.createRoot(document.getElementById("root") as HTMLElement);

// The overlay's card window loads the same page at #/card: just the card, on a transparent page.
if (window.location.hash.startsWith("#/card")) {
  document.documentElement.classList.add("overlay");
  root.render(
    <React.StrictMode>
      <CardWindow />
    </React.StrictMode>,
  );
} else {
  root.render(
    <React.StrictMode>
      <QueryClientProvider client={queryClient}>
        <LiveDataRefresher />
        {import.meta.env.PROD && <StartupUpdateCheck />}
        <TooltipProvider delayDuration={300}>
          <RouterProvider router={router} />
          <Toaster position="bottom-right" />
        </TooltipProvider>
      </QueryClientProvider>
    </React.StrictMode>,
  );
}
