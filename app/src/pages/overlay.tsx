import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Search } from "lucide-react";
import { toast } from "sonner";

import { HoverValueCard } from "@/components/hover-card";
import { ItemGroupSummary } from "@/components/item-group-summary";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { hoverApi, type ItemGroup } from "@/lib/api";
import { formatGold } from "@/lib/format";
import { useItemSearch, useOpenListingCounts, useSetSetting, useSettings } from "@/lib/queries";
import { rarityColor } from "@/lib/rarity";

const DEFAULT_ITEM = "Hounskull";

export function OverlayPage() {
  const [query, setQuery] = useState("");
  const [name, setName] = useState(DEFAULT_ITEM);
  const { data: results = [] } = useItemSearch(query, 8);
  const { data: picked } = useItemSearch(name, 1);
  const group = picked?.find((g) => g.name === name);

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-6 p-6">
      <HoverSwitch />

      <section className="flex flex-col gap-2">
        <h2 className="font-display text-lg">The value card</h2>
        <p className="max-w-2xl text-sm text-muted-foreground">
          When you hover an item in game, this card shows beside the game's tooltip. Pick any item to see its card
          from your market data, with the rolls of one of its listings.
        </p>
      </section>

      <div className="grid gap-6 md:grid-cols-[1fr_auto]">
        <div className="flex flex-col gap-3">
          <div className="relative">
            <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
            <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Preview another item…" className="pl-9" />
          </div>
          {query.trim() && results.length > 0 && (
            <ul className="divide-y divide-border/60 overflow-hidden rounded-lg border bg-card">
              {results.map((result) => (
                <li key={result.name}>
                  <button
                    type="button"
                    onClick={() => {
                      setName(result.name);
                      setQuery("");
                    }}
                    className="flex w-full items-center gap-3 px-3 py-2 text-left transition-colors hover:bg-accent/60"
                  >
                    <ItemGroupSummary group={result} slotSize={32} />
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
        <div className="flex flex-col gap-3">{group && <Preview key={group.name} group={group} />}</div>
      </div>
    </div>
  );
}

/** Hover values on or off (on by default), saved in the settings. */
function HoverSwitch() {
  const { data: settings } = useSettings();
  const setSetting = useSetSetting();
  const on = (settings?.hoverValues as boolean | undefined) ?? true;
  return (
    <section className="flex items-center gap-6 rounded-lg border bg-card px-4 py-3.5">
      <div className="min-w-0 flex-1">
        <div className="text-[15px]">Show values when I hover items in game</div>
        <div className="mt-0.5 text-sm text-muted-foreground">
          Reads the game's tooltip from the screen with Windows' own text recognition. Nothing leaves your PC.
        </div>
      </div>
      <Switch
        checked={on}
        onCheckedChange={(value) =>
          setSetting.mutate(
            { key: "hoverValues", value },
            { onError: (error) => toast.error("Could not save the setting", { description: String(error) }) },
          )
        }
      />
    </section>
  );
}

function Preview({ group }: { group: ItemGroup }) {
  const ids = useMemo(() => group.variants.map((v) => v.id), [group]);
  const { data: counts } = useOpenListingCounts(ids);
  const [pickedId, setPickedId] = useState<string | null>(null);
  const busiest = counts ? [...group.variants].sort((a, b) => (counts[b.id] ?? 0) - (counts[a.id] ?? 0))[0] : undefined;
  const variantId = pickedId ?? busiest?.id ?? group.variants[group.variants.length - 1].id;
  const { data, isPending, error } = useQuery({
    queryKey: ["hover", "preview", variantId],
    queryFn: () => hoverApi.preview(variantId),
  });

  return (
    <>
      {group.variants.length > 1 && (
        <ToggleGroup type="single" value={variantId} onValueChange={(id) => id && setPickedId(id)} variant="outline" size="sm" spacing={1} className="flex-wrap justify-start">
          {group.variants.map((v) => (
            <ToggleGroupItem key={v.id} value={v.id} className="gap-1.5 px-2 text-xs" title={v.rarity}>
              <span className="size-2 rounded-full" style={{ background: rarityColor(v.rarity) }} />
              {v.rarity}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      )}
      <div className="rounded-xl border bg-[radial-gradient(ellipse_at_top,#1a1510,#07060500)] p-6">
        {isPending ? (
          <Skeleton className="h-[420px] w-[304px]" />
        ) : error ? (
          <p className="w-[304px] text-sm text-destructive">{String(error)}</p>
        ) : (
          <HoverValueCard card={data.card} />
        )}
      </div>
      {data?.listingPrice != null && (
        <p className="max-w-[352px] text-xs text-muted-foreground">
          Rolls from a listing asking {formatGold(data.listingPrice)} gold.
        </p>
      )}
    </>
  );
}
