import { useEffect, useMemo, useState } from "react";
import { Link, useParams } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { ChartCandlestick, Search } from "lucide-react";

import { Gold } from "@/components/gold";
import { MarketOverviewPanel } from "@/components/market/market-overview";
import { MarketSection } from "@/components/market/market-section";
import { ItemGroupSummary } from "@/components/item-group-summary";
import { ItemSlot } from "@/components/item-slot";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { api, type ItemGroup, type ItemView } from "@/lib/api";
import { useItemSearch, useOpenListingCounts } from "@/lib/queries";
import { RARITY_TEXT, rarityColor } from "@/lib/rarity";
import { forgetBrokenRecent, rememberRecent, useRecentItems } from "@/lib/recent";

export function MarketPage() {
  const { name } = useParams();
  return name ? <ItemDetail key={name} name={name} /> : <MarketHome />;
}

function MarketHome() {
  const [query, setQuery] = useState("");
  const { data: groups = [], isFetching } = useItemSearch(query, 40);
  const recent = useRecentItems();
  const trimmed = query.trim();

  return (
    <div className="mx-auto flex max-w-4xl flex-col gap-6 p-6">
      <div className="relative">
        <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
        <Input
          autoFocus
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          placeholder="Search any item to see what it sells for…"
          className="h-11 pl-9 text-base"
        />
      </div>

      {trimmed ? (
        groups.length > 0 ? (
          <ul className="divide-y divide-border/60 overflow-hidden rounded-lg border bg-card">
            {groups.map((group) => (
              <ResultRow key={group.name} group={group} />
            ))}
          </ul>
        ) : (
          !isFetching && <p className="px-1 text-sm text-muted-foreground">No item is called “{trimmed}”.</p>
        )
      ) : (
        <>
          {recent.length > 0 && (
            <section className="flex flex-col gap-2">
              <h2 className="px-1 text-xs tracking-wider text-muted-foreground uppercase">Recently viewed</h2>
              <div className="flex flex-wrap gap-2">
                {recent.map((name) => (
                  <Link
                    key={name}
                    to={`/market/${encodeURIComponent(name)}`}
                    className="rounded-md border bg-card px-3 py-1.5 font-display text-sm transition-colors hover:border-primary/40 hover:bg-accent"
                  >
                    {name}
                  </Link>
                ))}
              </div>
            </section>
          )}
          <MarketOverviewPanel
            fallback={
              recent.length === 0 && (
                <Empty className="border border-dashed">
                  <EmptyHeader>
                    <EmptyMedia variant="icon">
                      <ChartCandlestick />
                    </EmptyMedia>
                    <EmptyTitle className="font-display text-lg">Look up any item</EmptyTitle>
                    <EmptyDescription>Search above, or press Ctrl+K anywhere in the app.</EmptyDescription>
                  </EmptyHeader>
                </Empty>
              )
            }
          />
        </>
      )}
    </div>
  );
}

function ResultRow({ group }: { group: ItemGroup }) {
  return (
    <li>
      <Link
        to={`/market/${encodeURIComponent(group.name)}`}
        className="flex items-center gap-3 px-3 py-2 transition-colors hover:bg-accent/60"
      >
        <ItemGroupSummary group={group} />
      </Link>
    </li>
  );
}

function ItemDetail({ name }: { name: string }) {
  const { data: group, isPending } = useQuery({
    queryKey: ["items", "group", name],
    queryFn: async () => (await api.searchItems(name, 1)).find((hit) => hit.name === name) ?? null,
    staleTime: Infinity,
  });
  const [picked, setPicked] = useState<string | null>(null);
  const variantIds = useMemo(() => group?.variants.map((v) => v.id) ?? [], [group]);
  const { data: counts } = useOpenListingCounts(variantIds);
  // Until one is picked, show the rarity with the most listings (else the highest).
  const variant = useMemo(() => {
    const variants = group?.variants ?? [];
    const busiest = counts
      ? variants.reduce<ItemView | undefined>((best, v) => ((counts[v.id] ?? 0) > (best ? (counts[best.id] ?? 0) : 0) ? v : best), undefined)
      : undefined;
    return variants.find((v) => v.id === picked) ?? busiest ?? variants[variants.length - 1];
  }, [group, picked, counts]);

  useEffect(() => {
    if (group) rememberRecent(group.name);
    else if (group === null) forgetBrokenRecent(name);
  }, [group, name]);

  if (isPending) return <DetailSkeleton />;
  if (!group || !variant) {
    return (
      <div className="p-6">
        <Empty className="border border-dashed">
          <EmptyHeader>
            <EmptyTitle className="font-display text-lg">No item is called “{name}”</EmptyTitle>
            <EmptyDescription>It may have been renamed in a game update.</EmptyDescription>
          </EmptyHeader>
        </Empty>
      </div>
    );
  }

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-6 p-6">
      <header className="flex items-start gap-5">
        <ItemSlot icon={variant.icon} rarity={variant.rarity} size={60} cells={cellsFor(variant)} />
        <div className="flex min-w-0 flex-1 flex-col gap-2 pt-1">
          <h1 className={`font-display text-[28px] leading-tight ${RARITY_TEXT[variant.rarity]}`}>{group.name}</h1>
          <div className="rarity-rule max-w-md" style={{ "--rule-rarity": rarityColor(variant.rarity) } as React.CSSProperties} />
          <p className="text-sm text-muted-foreground">
            {[variant.rarity, variant.kind || variant.itemType || "Loot"].filter(Boolean).join(" · ")}
          </p>
          {group.variants.length > 1 && (
            <ToggleGroup
              type="single"
              value={variant.id}
              onValueChange={(id) => id && setPicked(id)}
              variant="outline"
              size="sm"
              spacing={1}
              className="mt-2 flex-wrap justify-start"
            >
              {group.variants.map((v) => (
                <ToggleGroupItem
                  key={v.id}
                  value={v.id}
                  title={`${v.rarity}: ${counts?.[v.id] ?? 0} open listings`}
                  className="gap-1.5 px-2.5 text-xs"
                >
                  <span className="size-2 rounded-full" style={{ background: rarityColor(v.rarity) }} />
                  {v.rarity}
                  {counts && (
                    <span className="font-num text-muted-foreground tabular">{counts[v.id] ?? 0}</span>
                  )}
                </ToggleGroupItem>
              ))}
            </ToggleGroup>
          )}
        </div>
      </header>

      <dl className="grid grid-cols-2 gap-px overflow-hidden rounded-lg border bg-border sm:grid-cols-4">
        <Fact label="Merchant pays">
          {variant.vendorPrice > 0 ? <Gold value={variant.vendorPrice} /> : <span className="text-muted-foreground">Nothing</span>}
        </Fact>
        <Fact label="Size">
          <span className="font-num tabular">
            {variant.width} × {variant.height}
          </span>
        </Fact>
        <Fact label="Stacks to">
          <span className="font-num tabular">{variant.maxStack}</span>
        </Fact>
        <Fact label="Marketplace">
          {variant.tradable ? "Tradable" : <span className="text-muted-foreground">Not tradable</span>}
        </Fact>
      </dl>

      <MarketSection itemId={variant.id} tradable={variant.tradable} />
    </div>
  );
}

function Fact({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1 bg-card px-4 py-3">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="text-[15px]">{children}</dd>
    </div>
  );
}

/** Inventory cells to draw the icon at, scaled down for very tall or wide items. */
function cellsFor(item: ItemView) {
  const longest = Math.max(item.width, item.height);
  const scale = longest > 3 ? 3 / longest : 1;
  return { width: item.width * scale, height: item.height * scale };
}

function DetailSkeleton() {
  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-6 p-6">
      <div className="flex gap-5">
        <Skeleton className="size-16" />
        <div className="flex flex-1 flex-col gap-3 pt-1">
          <Skeleton className="h-7 w-64" />
          <Skeleton className="h-1 w-96" />
          <Skeleton className="h-4 w-40" />
        </div>
      </div>
      <Skeleton className="h-16 w-full" />
    </div>
  );
}
