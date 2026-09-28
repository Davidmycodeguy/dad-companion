import type { ReactNode } from "react";
import { Link } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { Flame, Layers } from "lucide-react";

import { Gold } from "@/components/gold";
import { ItemSlot } from "@/components/item-slot";
import { Skeleton } from "@/components/ui/skeleton";
import { marketApi, type ItemActivity } from "@/lib/api";
import { formatAge } from "@/lib/format";
import { RARITY_TEXT } from "@/lib/rarity";
import { cn } from "@/lib/utils";

/** Market data older than this gets its age shown: prices may have moved since. */
const STALE_AFTER_S = 3600;
const OVERVIEW_STALE_MS = 60_000;

/** The market at a glance: what is listed most right now, and what sold fastest this week.
 * `fallback` shows instead while there is no market data at all. */
export function MarketOverviewPanel({ fallback }: { fallback?: ReactNode }) {
  const { data, isPending } = useQuery({ queryKey: ["market", "overview"], queryFn: marketApi.overview, staleTime: OVERVIEW_STALE_MS });
  if (isPending) return <Skeleton className="h-80" />;
  if (!data || (data.listed.length === 0 && data.sold.length === 0)) return <>{fallback}</>;
  const stale = data.newestAgeS !== null && data.newestAgeS > STALE_AFTER_S;
  return (
    <section className="flex flex-col gap-3">
      <div className="flex items-baseline justify-between px-1">
        <h2 className="text-xs tracking-wider text-muted-foreground uppercase">The market now</h2>
        {stale && <span className="text-xs text-muted-foreground">newest listing seen {formatAge(data.newestAgeS!)}</span>}
      </div>
      <div className="grid gap-4 md:grid-cols-2">
        <ActivityList title="Most listed" icon={Layers} unit="listed" items={data.listed} />
        <ActivityList title="Selling fastest this week" icon={Flame} unit="sold" items={data.sold} />
      </div>
    </section>
  );
}

interface ActivityListProps {
  title: string;
  icon: typeof Layers;
  unit: string;
  items: ItemActivity[];
}

function ActivityList({ title, icon: Icon, unit, items }: ActivityListProps) {
  return (
    <div className="overflow-hidden rounded-lg border bg-card">
      <div className="flex items-center gap-2 border-b px-4 py-2.5">
        <Icon className="size-4 text-muted-foreground" />
        <h3 className="font-display text-[15px]">{title}</h3>
        <span className="ml-auto text-xs text-muted-foreground">median each</span>
      </div>
      {items.length === 0 ? (
        <p className="px-4 py-6 text-sm text-muted-foreground">Nothing yet: it fills in as the app sees the Marketplace.</p>
      ) : (
        <ol className="divide-y divide-border/60">
          {items.map((item) => (
            <li key={item.itemId}>
              <Link
                to={`/market/${encodeURIComponent(item.name)}`}
                className="flex items-center gap-3 px-4 py-2 transition-colors hover:bg-accent/40"
              >
                <ItemSlot icon={item.icon} rarity={item.rarity} size={30} />
                <span className={cn("min-w-0 flex-1 truncate text-sm", RARITY_TEXT[item.rarity])}>{item.name}</span>
                <span className="w-16 text-right text-xs text-muted-foreground">
                  <span className="font-num text-foreground tabular">{item.count}</span> {unit}
                </span>
                <span className="w-20 text-right text-sm">
                  <Gold value={item.median} />
                </span>
              </Link>
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}
