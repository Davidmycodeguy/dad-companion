import { useMemo } from "react";
import { Bar, BarChart, CartesianGrid, Line, LineChart, XAxis, YAxis } from "recharts";

import { ChartContainer, ChartTooltip, ChartTooltipContent, type ChartConfig } from "@/components/ui/chart";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import type { DayView, ListingView } from "@/lib/api";
import { formatGold } from "@/lib/format";
import { priceBins } from "@/lib/price-bins";

const spreadConfig = { count: { label: "Listings", color: "var(--chart-1)" } } satisfies ChartConfig;
const historyConfig = { median: { label: "Median ask", color: "var(--chart-1)" } } satisfies ChartConfig;

/** Where the open listings sit (price spread) and how the median ask moved day by day. */
export function PriceChart({ listings, days }: { listings: ListingView[]; days: DayView[] }) {
  const bins = useMemo(
    () =>
      priceBins(listings.map((listing) => listing.unitPrice)).map((bin) => ({
        label: bin.overflow ? `${formatGold(bin.from)}+` : formatGold(bin.from),
        range: bin.overflow ? `${formatGold(bin.from)} and up` : `${formatGold(bin.from)} – ${formatGold(bin.to)}`,
        count: bin.count,
      })),
    [listings],
  );
  const history = useMemo(
    () =>
      days.map((day) => ({
        label: new Date(day.day * 1000).toLocaleDateString(undefined, { month: "short", day: "numeric" }),
        median: Math.round(day.median),
        listings: day.listings,
      })),
    [days],
  );

  return (
    <Tabs defaultValue="spread" className="gap-3">
      <div className="flex items-center justify-between gap-4">
        <h3 className="font-display text-base">Prices</h3>
        <TabsList>
          <TabsTrigger value="spread">Spread</TabsTrigger>
          <TabsTrigger value="history">Daily</TabsTrigger>
        </TabsList>
      </div>
      <TabsContent value="spread">
        {bins.length === 0 ? (
          <ChartNote>No open listings to chart.</ChartNote>
        ) : (
          <ChartContainer config={spreadConfig} className="aspect-auto h-52 w-full">
            <BarChart data={bins} margin={{ left: 4, right: 4, top: 8 }}>
              <CartesianGrid vertical={false} />
              <XAxis dataKey="label" tickLine={false} axisLine={false} tickMargin={8} minTickGap={16} />
              <YAxis allowDecimals={false} tickLine={false} axisLine={false} width={28} />
              <ChartTooltip
                cursor={false}
                content={
                  <ChartTooltipContent
                    labelFormatter={(_, payload) => payload?.[0]?.payload?.range ?? ""}
                    formatter={(value) => `${value} listings`}
                    hideIndicator
                  />
                }
              />
              <Bar dataKey="count" fill="var(--color-count)" radius={[3, 3, 0, 0]} maxBarSize={48} />
            </BarChart>
          </ChartContainer>
        )}
      </TabsContent>
      <TabsContent value="history">
        {history.length < 2 ? (
          <ChartNote>The daily price line starts once the market has been read on two different days.</ChartNote>
        ) : (
          <ChartContainer config={historyConfig} className="aspect-auto h-52 w-full">
            <LineChart data={history} margin={{ left: 4, right: 12, top: 8 }}>
              <CartesianGrid vertical={false} />
              <XAxis dataKey="label" tickLine={false} axisLine={false} tickMargin={8} minTickGap={24} />
              <YAxis
                tickLine={false}
                axisLine={false}
                width={52}
                tickFormatter={(value: number) => formatGold(value)}
                domain={["auto", "auto"]}
              />
              <ChartTooltip
                content={
                  <ChartTooltipContent
                    formatter={(value, _name, item) =>
                      `${formatGold(Number(value))} gold · ${item.payload.listings} listings`
                    }
                  />
                }
              />
              <Line dataKey="median" stroke="var(--color-median)" strokeWidth={2} dot={{ r: 2.5 }} type="monotone" />
            </LineChart>
          </ChartContainer>
        )}
      </TabsContent>
    </Tabs>
  );
}

function ChartNote({ children }: { children: React.ReactNode }) {
  return <div className="grid h-52 place-items-center text-sm text-muted-foreground">{children}</div>;
}
