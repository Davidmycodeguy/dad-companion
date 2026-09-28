import { useMemo, useState, type CSSProperties } from "react";
import { useQuery } from "@tanstack/react-query";
import { Archive, Lock } from "lucide-react";

import { Gold } from "@/components/gold";
import { HoverValueCard } from "@/components/hover-card";
import { ItemSlot } from "@/components/item-slot";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { stashApi, type StashItem, type StashView } from "@/lib/api";
import { RARITY_TEXT } from "@/lib/rarity";
import { cn } from "@/lib/utils";

/** Side of one stash cell in CSS pixels. */
const CELL = 38;
/** Most valuable items listed beside a tab. */
const TOP_ITEMS = 6;

export function StashPage() {
  const { data: characters, isPending } = useQuery({ queryKey: ["characters"], queryFn: stashApi.characters });
  const [characterId, setCharacterId] = useState<string | null>(null);
  const character = characters?.find((c) => c.id === characterId) ?? characters?.[0];
  const firstTab = character?.stashes.find((s) => !s.locked && s.id >= 4) ?? character?.stashes[0];
  const [tab, setTab] = useState<number | null>(null);
  const [picked, setPicked] = useState<string | null>(null);
  const inventoryId = tab ?? firstTab?.id ?? null;

  const { data: view } = useQuery({
    queryKey: ["stash", character?.id, inventoryId],
    queryFn: () => stashApi.view(character!.id, inventoryId!),
    enabled: !!character && inventoryId !== null,
  });

  if (isPending) return <Skeleton className="m-6 h-96" />;
  if (!character) {
    return (
      <div className="grid h-full place-items-center p-8">
        <Empty className="max-w-md border border-dashed">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <Archive />
            </EmptyMedia>
            <EmptyTitle className="font-display text-lg">No characters yet</EmptyTitle>
            <EmptyDescription>Your stash shows up here once the game has sent your character.</EmptyDescription>
          </EmptyHeader>
        </Empty>
      </div>
    );
  }

  return (
    <div className="mx-auto flex max-w-6xl flex-col gap-5 p-6">
      <header className="flex flex-wrap items-center justify-between gap-3">
        {characters && characters.length > 1 ? (
          <Select
            value={character.id}
            onValueChange={(id) => {
              setCharacterId(id);
              setTab(null);
              setPicked(null);
            }}
          >
            <SelectTrigger className="h-auto min-w-56 py-1.5" aria-label="Character">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {characters.map((c) => (
                <SelectItem key={c.id} value={c.id}>
                  <span className="font-display text-base">{c.name}</span>
                  <span className="text-muted-foreground">
                    {c.class} · level {c.level}
                  </span>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        ) : (
          <div>
            <h2 className="font-display text-xl">{character.name}</h2>
            <p className="text-sm text-muted-foreground">
              {character.class} · level {character.level}
            </p>
          </div>
        )}
        <ToggleGroup
          type="single"
          value={inventoryId !== null ? String(inventoryId) : undefined}
          onValueChange={(value) => {
            if (!value) return;
            setTab(Number(value));
            setPicked(null);
          }}
          variant="outline"
          size="sm"
          spacing={1}
          className="flex-wrap justify-end"
        >
          {character.stashes.map((stash) => (
            <ToggleGroupItem key={stash.id} value={String(stash.id)} className="gap-1.5 px-2.5 text-xs">
              {stash.locked && <Lock className="size-3" />}
              {stash.label}
              <span className="font-num text-muted-foreground tabular">{stash.items}</span>
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      </header>

      {view ? (
        <div className="grid items-start gap-6 lg:grid-cols-[auto_1fr]">
          <StashGrid view={view} picked={picked} onPick={setPicked} />
          <SidePanel characterId={character.id} view={view} picked={picked} />
        </div>
      ) : (
        <Skeleton className="h-[480px] w-[456px]" />
      )}
    </div>
  );
}

function StashGrid({ view, picked, onPick }: { view: StashView; picked: string | null; onPick: (id: string) => void }) {
  const [columns, rows] = view.grid ?? [Math.max(1, view.items.length * 2), 2];
  return (
    <div
      className="relative rounded-md border border-[#2a241c] bg-[#0b0a08]"
      style={{
        width: columns * CELL + 2,
        height: rows * CELL + 2,
        backgroundImage: "linear-gradient(#1a1712 1px, transparent 1px), linear-gradient(90deg, #1a1712 1px, transparent 1px)",
        backgroundSize: `${CELL}px ${CELL}px`,
        backgroundPosition: "1px 1px",
      }}
    >
      {view.items.map((item) => (
        <button
          key={item.uniqueId}
          type="button"
          title={`${item.name}${item.count > 1 ? ` ×${item.count}` : ""}`}
          onClick={() => onPick(item.uniqueId)}
          className={cn("absolute p-px transition-transform hover:z-10 hover:scale-[1.04]", picked === item.uniqueId && "z-10")}
          style={{ left: item.x * CELL + 1, top: item.y * CELL + 1, width: item.width * CELL, height: item.height * CELL } as CSSProperties}
        >
          <ItemSlot
            icon={item.icon}
            rarity={item.rarity}
            size={CELL - 2}
            cells={{ width: item.width, height: item.height }}
            className={cn("size-full!", picked === item.uniqueId && "outline outline-2 outline-primary")}
          />
          {(item.count > 1 || (item.gold !== null && item.itemId !== "GoldCoins")) && (
            <span className="absolute right-1 bottom-0.5 font-num text-[11px] text-foreground tabular [text-shadow:0_1px_2px_#000]">
              {(item.gold !== null && item.itemId !== "GoldCoins" ? item.gold : item.count).toLocaleString()}
            </span>
          )}
        </button>
      ))}
    </div>
  );
}

function SidePanel({ characterId, view, picked }: { characterId: string; view: StashView; picked: string | null }) {
  const top = useMemo(
    () => [...view.items].filter((i) => i.value !== null).sort((a, b) => (b.value ?? 0) - (a.value ?? 0)).slice(0, TOP_ITEMS),
    [view],
  );
  const { data: card, error } = useQuery({
    queryKey: ["stash", "card", characterId, picked],
    queryFn: () => stashApi.card(characterId, picked!),
    enabled: !!picked && !view.locked,
  });

  if (view.locked) {
    return (
      <div className="flex max-w-sm flex-col gap-2 rounded-lg border border-dashed px-4 py-4 text-sm text-muted-foreground">
        <span className="flex items-center gap-2 font-display text-base text-foreground">
          <Lock className="size-4" /> Locked seasonal stash
        </span>
        These items are only a preview of what unlocking this stash gives. They aren't yours, so they aren't counted,
        priced, sorted or sold.
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      <dl className="grid max-w-md grid-cols-3 gap-px overflow-hidden rounded-lg border bg-border">
        <div className="bg-card px-4 py-3">
          <dt className="text-xs text-muted-foreground">Gold</dt>
          <dd className="mt-0.5">
            <Gold value={view.totalGold} className="text-lg" />
          </dd>
        </div>
        <div className="bg-card px-4 py-3">
          <dt className="text-xs text-muted-foreground">Items worth</dt>
          <dd className="mt-0.5">
            <Gold value={view.totalValue} className="text-lg" />
          </dd>
        </div>
        <div className="bg-card px-4 py-3">
          <dt className="text-xs text-muted-foreground">Merchants pay</dt>
          <dd className="mt-0.5">
            <Gold value={view.totalMerchant} className="text-lg" />
          </dd>
        </div>
      </dl>
      {picked && card ? (
        <HoverValueCard card={card} />
      ) : picked && error ? (
        <p className="text-sm text-muted-foreground">{String(error)}</p>
      ) : (
        <section className="flex max-w-md flex-col gap-2">
          <h3 className="font-display text-base">Most valuable here</h3>
          {top.length === 0 ? (
            <p className="text-sm text-muted-foreground">Nothing here has market data yet.</p>
          ) : (
            <ul className="divide-y divide-border/60 overflow-hidden rounded-lg border bg-card">
              {top.map((item) => (
                <TopRow key={item.uniqueId} item={item} />
              ))}
            </ul>
          )}
          <p className="text-xs text-muted-foreground">Click an item to see its value card.</p>
        </section>
      )}
    </div>
  );
}

function TopRow({ item }: { item: StashItem }) {
  return (
    <li className="flex items-center gap-3 px-3 py-2">
      <ItemSlot icon={item.icon} rarity={item.rarity} size={32} />
      <span className={cn("min-w-0 flex-1 truncate font-display text-[15px]", RARITY_TEXT[item.rarity])}>{item.name}</span>
      {item.value !== null && <Gold value={item.value} />}
    </li>
  );
}
