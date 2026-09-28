import { useState, type ReactNode } from "react";
import { TriangleAlert } from "lucide-react";

import { Gold } from "@/components/gold";
import { PriceChart } from "@/components/market/price-chart";
import { Rolls } from "@/components/market/rolls";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import type { ItemMarket, ListingView } from "@/lib/api";
import { formatAge } from "@/lib/format";
import { useItemMarket } from "@/lib/queries";

/** Rows shown before "Show all". */
const FIRST_ROWS = 25;

/** What the market says about one item: asks, spread, daily prices and probable sales. */
export function MarketSection({ itemId, tradable }: { itemId: string; tradable: boolean }) {
  const { data: market, isPending, error } = useItemMarket(itemId);

  if (!tradable) {
    return <Note>This item can't be sold on the Marketplace.</Note>;
  }
  if (isPending) return <MarketSkeleton />;
  if (error) {
    return (
      <Alert variant="destructive">
        <TriangleAlert />
        <AlertTitle>Market prices could not be read</AlertTitle>
        <AlertDescription>{String(error)}</AlertDescription>
      </Alert>
    );
  }
  if (market.listingCount === 0 && market.sales.length === 0 && market.days.length === 0) {
    return <Note>No listings of this item have been seen yet. They show up here once the market is read.</Note>;
  }

  return (
    <div className="flex flex-col gap-6">
      <Summary market={market} />
      <section className="rounded-lg border bg-card p-4">
        <PriceChart listings={market.listings} days={market.days} />
      </section>
      <ListingTable
        title="Open listings"
        count={market.listingCount}
        rows={market.listings}
        ageHeading="Seen"
        empty="Nothing is listed right now."
      />
      <ListingTable
        title="Probably sold this week"
        count={market.sales.length}
        rows={market.sales}
        ageHeading="Sold"
        empty={market.salesTracked ? "No sales seen this week." : "Sales show up after the market has been read twice."}
      />
    </div>
  );
}

function Summary({ market }: { market: ItemMarket }) {
  return (
    <dl className="grid grid-cols-2 gap-px overflow-hidden rounded-lg border bg-border md:grid-cols-4">
      <Fact label="Lowest ask">{market.lowest !== null ? <Gold value={market.lowest} className="text-lg" /> : "—"}</Fact>
      <Fact label="Median ask">{market.median !== null ? <Gold value={market.median} className="text-lg" /> : "—"}</Fact>
      <Fact label="Open listings" detail={market.newestAgeS !== null ? `read ${formatAge(market.newestAgeS)}` : undefined}>
        <span className="font-num text-lg tabular">{market.listingCount.toLocaleString()}</span>
      </Fact>
      <Fact label="Sold this week" detail={market.salesTracked ? "listings that vanished early" : "not read twice yet"}>
        <span className="font-num text-lg tabular">{market.salesTracked ? market.sales.length : "—"}</span>
      </Fact>
    </dl>
  );
}

function Fact({ label, detail, children }: { label: string; detail?: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-0.5 bg-card px-4 py-3">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd>{children}</dd>
      {detail && <dd className="text-xs text-muted-foreground/80">{detail}</dd>}
    </div>
  );
}

function ListingTable({
  title,
  count,
  rows,
  ageHeading,
  empty,
}: {
  title: string;
  count: number;
  rows: ListingView[];
  ageHeading: string;
  empty: string;
}) {
  const [showAll, setShowAll] = useState(false);
  const shown = showAll ? rows : rows.slice(0, FIRST_ROWS);
  return (
    <section className="flex flex-col gap-2">
      <h3 className="flex items-baseline gap-2 font-display text-base">
        {title}
        <span className="font-sans text-xs text-muted-foreground">{count.toLocaleString()}</span>
      </h3>
      {rows.length === 0 ? (
        <p className="rounded-lg border border-dashed px-4 py-3 text-sm text-muted-foreground">{empty}</p>
      ) : (
        <div className="overflow-hidden rounded-lg border bg-card">
          <Table>
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead className="w-44 pl-4">Price</TableHead>
                <TableHead>Rolls</TableHead>
                <TableHead className="w-24 pr-4 text-right">{ageHeading}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {shown.map((row) => (
                <TableRow key={row.id}>
                  <TableCell className="pl-4 align-top">
                    <Gold value={row.price} />
                    {row.count > 1 && (
                      <div className="text-xs text-muted-foreground">
                        ×{row.count} · <Gold value={row.unitPrice} className="text-muted-foreground" /> each
                      </div>
                    )}
                  </TableCell>
                  <TableCell className="align-top whitespace-normal">
                    <Rolls rolls={row.rolls} />
                  </TableCell>
                  <TableCell className="pr-4 text-right align-top text-muted-foreground">{formatAge(row.ageS)}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
          {rows.length > FIRST_ROWS && (
            <div className="border-t px-4 py-2">
              <Button variant="ghost" size="sm" onClick={() => setShowAll((value) => !value)}>
                {showAll ? "Show fewer" : `Show all ${rows.length.toLocaleString()}`}
              </Button>
            </div>
          )}
        </div>
      )}
    </section>
  );
}

function Note({ children }: { children: ReactNode }) {
  return <p className="rounded-lg border border-dashed px-4 py-3 text-sm text-muted-foreground">{children}</p>;
}

function MarketSkeleton() {
  return (
    <div className="flex flex-col gap-6">
      <Skeleton className="h-20 w-full" />
      <Skeleton className="h-64 w-full" />
      <Skeleton className="h-48 w-full" />
    </div>
  );
}
