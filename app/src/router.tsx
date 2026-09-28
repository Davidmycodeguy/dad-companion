import { Navigate, createHashRouter } from "react-router";

import { AppShell } from "@/components/app-shell";
import { ListerPage } from "@/pages/lister";
import { MarketPage } from "@/pages/market";
import { OverlayPage } from "@/pages/overlay";
import { OverviewPage } from "@/pages/overview";
import { QuestsPage } from "@/pages/quests";
import { SettingsPage } from "@/pages/settings";
import { SorterPage } from "@/pages/sorter";
import { StashPage } from "@/pages/stash";

export const router = createHashRouter([
  {
    path: "/",
    element: <AppShell />,
    children: [
      { index: true, element: <OverviewPage />, handle: { title: "Overview" } },
      { path: "market", element: <MarketPage />, handle: { title: "Market" } },
      { path: "market/:name", element: <MarketPage />, handle: { title: "Market" } },
      { path: "lister", element: <ListerPage />, handle: { title: "Auto lister" } },
      { path: "overlay", element: <OverlayPage />, handle: { title: "Hover values" } },
      { path: "sorter", element: <SorterPage />, handle: { title: "Stash sorter" } },
      { path: "stash", element: <StashPage />, handle: { title: "Stash" } },
      { path: "quests", element: <QuestsPage />, handle: { title: "Quests" } },
      { path: "settings", element: <SettingsPage />, handle: { title: "Settings" } },
      { path: "*", element: <Navigate to="/" replace /> },
    ],
  },
]);
