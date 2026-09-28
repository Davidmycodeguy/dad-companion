import { useMatches, useParams } from "react-router";
import { ChevronRight, Search } from "lucide-react";

import { useOpenItemSearch } from "@/components/item-search";
import { WindowControls } from "@/components/window-controls";
import { Kbd, KbdGroup } from "@/components/ui/kbd";
import { SidebarTrigger } from "@/components/ui/sidebar";
import { useAppStatus } from "@/lib/queries";
import { cn } from "@/lib/utils";

/** The window's title bar: drag to move, page title, item search, game status, caption buttons. */
export function TitleBar() {
  const matches = useMatches();
  const { name } = useParams();
  const openSearch = useOpenItemSearch();
  const { data: status } = useAppStatus();
  const title = [...matches]
    .reverse()
    .map((match) => (match.handle as { title?: string } | undefined)?.title)
    .find(Boolean);

  return (
    <header
      data-tauri-drag-region
      className="flex h-(--titlebar-height) shrink-0 items-center gap-2 border-b border-border/70 pl-2"
    >
      <SidebarTrigger className="text-muted-foreground" />
      <div data-tauri-drag-region className="flex min-w-0 items-center gap-1.5 font-display text-[15px]">
        <span data-tauri-drag-region className={name ? "text-muted-foreground" : "text-foreground"}>
          {title}
        </span>
        {name && (
          <>
            <ChevronRight className="size-3.5 shrink-0 text-muted-foreground/60" />
            <span data-tauri-drag-region className="truncate">
              {name}
            </span>
          </>
        )}
      </div>
      <div data-tauri-drag-region className="h-full min-w-4 flex-1" />
      <button
        type="button"
        onClick={openSearch}
        className="flex h-7 w-60 items-center gap-2 rounded-md border border-input/60 bg-muted/50 px-2 text-sm text-muted-foreground transition-colors hover:border-input hover:text-foreground"
      >
        <Search className="size-3.5" />
        <span className="flex-1 text-left">Search items…</span>
        <KbdGroup>
          <Kbd>Ctrl</Kbd>
          <Kbd>K</Kbd>
        </KbdGroup>
      </button>
      <GameStatus running={status?.gameRunning ?? false} />
      <WindowControls />
    </header>
  );
}

function GameStatus({ running }: { running: boolean }) {
  return (
    <div
      data-tauri-drag-region
      className="flex items-center gap-1.5 px-2 text-xs whitespace-nowrap text-muted-foreground"
    >
      <span
        className={cn(
          "pointer-events-none size-2 rounded-full",
          running ? "bg-profit shadow-[0_0_6px_var(--profit)]" : "bg-muted-foreground/40",
        )}
      />
      {running ? "Game running" : "Game not running"}
    </div>
  );
}
