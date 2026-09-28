import { NavLink, useLocation } from "react-router";
import { Settings } from "lucide-react";

import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarRail,
} from "@/components/ui/sidebar";
import { NAV, isActivePath, type NavItem } from "@/lib/nav";
import { useAppStatus } from "@/lib/queries";

const ACTIVE_ENTRY =
  "relative data-[active=true]:font-normal data-[active=true]:text-foreground " +
  "data-[active=true]:[&>svg]:text-primary data-[active=true]:before:absolute data-[active=true]:before:inset-y-1.5 " +
  "data-[active=true]:before:left-0 data-[active=true]:before:w-0.5 data-[active=true]:before:rounded-full " +
  "data-[active=true]:before:bg-primary";

export function AppSidebar() {
  const { pathname } = useLocation();
  const { data: status } = useAppStatus();
  return (
    <Sidebar collapsible="icon">
      <SidebarHeader
        data-tauri-drag-region
        className="h-(--titlebar-height) flex-row items-center gap-2.5 px-2.5 py-0"
      >
        <BrandMark />
        <span
          data-tauri-drag-region
          className="truncate font-display text-[15px] tracking-wide text-foreground group-data-[collapsible=icon]:hidden"
        >
          {status?.name ?? "DaD Companion"}
        </span>
      </SidebarHeader>
      <SidebarContent>
        {NAV.map((section) => (
          <SidebarGroup key={section.group}>
            <SidebarGroupLabel className="text-[11px] tracking-wider text-muted-foreground/70 uppercase">
              {section.group}
            </SidebarGroupLabel>
            <SidebarMenu>
              {section.items.map((item) => (
                <NavEntry key={item.to} item={item} active={isActivePath(pathname, item.to)} />
              ))}
            </SidebarMenu>
          </SidebarGroup>
        ))}
      </SidebarContent>
      <SidebarFooter>
        <SidebarMenu>
          <NavEntry
            item={{ to: "/settings", label: "Settings", icon: Settings }}
            active={isActivePath(pathname, "/settings")}
          />
        </SidebarMenu>
        {status && (
          <div className="px-2 pb-1 text-[11px] text-muted-foreground/60 group-data-[collapsible=icon]:hidden">
            Version {status.version}
          </div>
        )}
      </SidebarFooter>
      <SidebarRail />
    </Sidebar>
  );
}

function NavEntry({ item, active }: { item: NavItem; active: boolean }) {
  const Icon = item.icon;
  return (
    <SidebarMenuItem>
      <SidebarMenuButton asChild isActive={active} tooltip={item.label} className={ACTIVE_ENTRY}>
        <NavLink to={item.to}>
          <Icon />
          <span>{item.label}</span>
        </NavLink>
      </SidebarMenuButton>
    </SidebarMenuItem>
  );
}

function BrandMark() {
  return (
    <img
      data-tauri-drag-region
      src="/logo.png"
      alt=""
      draggable={false}
      className="size-7 shrink-0 rounded-md"
    />
  );
}
