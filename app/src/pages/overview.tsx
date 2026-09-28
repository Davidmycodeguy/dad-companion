import type { ReactNode } from "react";
import { Link } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { ChevronRight, Coins, Database, Gamepad2, Store, type LucideIcon } from "lucide-react";

import { Gold } from "@/components/gold";
import { LiveDataAlert, useLiveStatus } from "@/components/live-data-alert";
import { stashApi, type CharacterWealth } from "@/lib/api";
import { formatAge, formatGold } from "@/lib/format";
import { listerApi } from "@/lib/lister-api";
import { NAV } from "@/lib/nav";
import { useAppStatus, useSettings } from "@/lib/queries";
import { cn } from "@/lib/utils";

interface Imported {
  files: string[];
  at: number;
}

/** One line under each feature link, saying what it's for. */
const FEATURE_NOTES: Record<string, string> = {
  "/market": "Prices, listings and sales for any item",
  "/lister": "Price your stash and list it in one go",
  "/overlay": "An item's value beside the game's tooltip",
  "/sorter": "Tidy a stash tab the way you like it",
  "/stash": "Every tab, with gold and item values",
  "/quests": "Quest progress and what to bring",
};

export function OverviewPage() {
  const { data: status } = useAppStatus();
  const { data: live } = useLiveStatus();
  const { data: wealth } = useQuery({ queryKey: ["stash", "wealth"], queryFn: stashApi.wealth });
  const { data: lister } = useQuery({ queryKey: ["lister", "status"], queryFn: listerApi.status });
  const totalGold = wealth?.reduce((sum, c) => sum + c.gold, 0) ?? 0;
  const totalValue = wealth?.reduce((sum, c) => sum + c.value, 0) ?? 0;
  const listings = lister?.listings;

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-7 p-6">
      <section className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
        <Tile icon={Gamepad2} label="Game" tone={status?.gameRunning ? "good" : "idle"} detail={liveLabel(live?.state)}>
          {status?.gameRunning ? "Running" : "Not running"}
        </Tile>
        <Tile icon={Coins} label="Wealth" detail={wealth ? `plus ${formatGold(totalValue)} in items` : undefined}>
          {wealth ? <Gold value={totalGold} /> : "…"}
        </Tile>
        <Tile
          icon={Store}
          label="Listings"
          detail={listings?.seen && listings.ageS !== null ? `read ${formatAge(listings.ageS)}` : "open My Listings in game"}
        >
          {listings?.seen ? (
            <span>
              {listings.free ?? "?"} free
              {listings.payouts > 0 && <span className="text-gold"> · {listings.payouts} sold</span>}
            </span>
          ) : (
            "Not seen yet"
          )}
        </Tile>
        <MarketTile />
      </section>

      <LiveDataAlert />

      {wealth && wealth.length > 0 && (
        <section className="flex flex-col gap-3">
          <h2 className="font-display text-lg">Characters</h2>
          <div className="grid gap-3 md:grid-cols-2">
            {wealth.map((character) => (
              <CharacterCard key={character.id} character={character} />
            ))}
          </div>
        </section>
      )}

      <section className="flex flex-col gap-3">
        <h2 className="font-display text-lg">Tools</h2>
        <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
          {NAV.flatMap((group) => group.items)
            .filter((item) => item.to !== "/")
            .map((item) => (
              <Link
                key={item.to}
                to={item.to}
                className="group flex items-center gap-3 rounded-lg border bg-card px-4 py-3 transition-colors hover:border-primary/40 hover:bg-accent/40"
              >
                <item.icon className="size-4.5 shrink-0 text-muted-foreground group-hover:text-primary" />
                <span className="flex min-w-0 flex-1 flex-col">
                  <span className="text-[15px]">{item.label}</span>
                  <span className="truncate text-xs text-muted-foreground">{FEATURE_NOTES[item.to]}</span>
                </span>
                <ChevronRight className="size-4 text-muted-foreground/50 group-hover:text-primary" />
              </Link>
            ))}
        </div>
      </section>
    </div>
  );
}

function liveLabel(state: string | undefined): string {
  if (state === "on") return "Live data on";
  if (state === "off") return "Live data off";
  return "Live data starting";
}

function MarketTile() {
  const { data: settings } = useSettings();
  const { data: market } = useQuery({ queryKey: ["market", "summary"], queryFn: listerApi.marketData });
  const imported = settings?.importedFromDndtools as Imported | undefined;
  const starter = settings?.starterData as Imported | undefined;
  const origin = imported ? "from DnDTools" : starter ? "starter data" : "as you browse";
  return (
    <Tile icon={Database} label="Market data" detail={market ? `${market.items.toLocaleString()} items, ${origin}` : origin}>
      {market ? `${market.listings.toLocaleString()} listings` : "…"}
    </Tile>
  );
}

function CharacterCard({ character }: { character: CharacterWealth }) {
  return (
    <Link
      to="/stash"
      className="group flex items-center gap-4 rounded-lg border bg-card p-4 transition-colors hover:border-primary/40 hover:bg-accent/30"
    >
      <img
        src={`/classes/${character.class.toLowerCase()}.png`}
        alt=""
        draggable={false}
        onError={(event) => (event.currentTarget.style.visibility = "hidden")}
        className="size-12 shrink-0 rounded-md border border-border bg-muted object-cover"
      />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <div className="flex items-baseline gap-2">
          <span className="truncate font-display text-base">{character.name}</span>
          <span className="text-xs text-muted-foreground">
            {character.class} · level {character.level}
          </span>
        </div>
        <div className="flex flex-wrap gap-x-5 gap-y-1 text-sm">
          <Gold value={character.gold} />
          <span className="text-muted-foreground">
            items <span className="font-num text-foreground tabular">{formatGold(character.value)}</span>
          </span>
          <span className="text-muted-foreground">
            {character.items} item{character.items === 1 ? "" : "s"}
          </span>
        </div>
      </div>
      <ChevronRight className="size-4 text-muted-foreground/50 group-hover:text-primary" />
    </Link>
  );
}

function Tile({
  icon: Icon,
  label,
  detail,
  tone = "idle",
  children,
}: {
  icon: LucideIcon;
  label: string;
  detail?: string;
  tone?: "good" | "idle";
  children: ReactNode;
}) {
  return (
    <div className="flex items-start gap-3 rounded-lg border bg-card p-4">
      <div
        className={cn(
          "grid size-9 shrink-0 place-items-center rounded-md border",
          tone === "good" ? "border-profit/40 bg-profit/10 text-profit" : "border-border bg-muted text-muted-foreground",
        )}
      >
        <Icon className="size-4.5" />
      </div>
      <div className="min-w-0">
        <div className="text-xs text-muted-foreground">{label}</div>
        <div className="mt-0.5 text-[15px]">{children}</div>
        {detail && <div className="mt-0.5 truncate text-xs text-muted-foreground">{detail}</div>}
      </div>
    </div>
  );
}
