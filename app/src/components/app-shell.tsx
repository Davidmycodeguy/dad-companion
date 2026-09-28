import type { CSSProperties } from "react";
import { Outlet } from "react-router";

import { AppSidebar } from "@/components/app-sidebar";
import { ItemSearchProvider } from "@/components/item-search";
import { TitleBar } from "@/components/title-bar";
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar";

export function AppShell() {
  return (
    <ItemSearchProvider>
      <SidebarProvider className="h-svh min-h-0" style={{ "--sidebar-width": "14rem" } as CSSProperties}>
        <AppSidebar />
        <SidebarInset className="min-w-0 overflow-hidden">
          <TitleBar />
          <div className="min-h-0 flex-1 overflow-y-auto">
            <Outlet />
          </div>
        </SidebarInset>
      </SidebarProvider>
    </ItemSearchProvider>
  );
}
