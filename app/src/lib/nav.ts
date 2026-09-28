import {
  Archive,
  ChartCandlestick,
  LayoutDashboard,
  LayoutGrid,
  ScanEye,
  ScrollText,
  Tags,
  type LucideIcon,
} from "lucide-react";

export interface NavItem {
  to: string;
  label: string;
  icon: LucideIcon;
}

export const NAV: { group: string; items: NavItem[] }[] = [
  {
    group: "Trade",
    items: [
      { to: "/", label: "Overview", icon: LayoutDashboard },
      { to: "/market", label: "Market", icon: ChartCandlestick },
      { to: "/lister", label: "Auto lister", icon: Tags },
    ],
  },
  {
    group: "In game",
    items: [
      { to: "/overlay", label: "Hover values", icon: ScanEye },
      { to: "/sorter", label: "Stash sorter", icon: LayoutGrid },
      { to: "/stash", label: "Stash", icon: Archive },
      { to: "/quests", label: "Quests", icon: ScrollText },
    ],
  },
];

/** Whether `pathname` is inside the section at `to` ("/" only matches itself). */
export function isActivePath(pathname: string, to: string): boolean {
  return to === "/" ? pathname === "/" : pathname === to || pathname.startsWith(`${to}/`);
}
