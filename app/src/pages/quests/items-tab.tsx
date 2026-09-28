import { useMemo, useState } from "react";
import { Check, Lock, PackageSearch, Search, X } from "lucide-react";

import { ItemSlot } from "@/components/item-slot";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from "@/components/ui/input-group";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { checklistRows, type NeedRow } from "@/lib/quests";
import { RARITY_TEXT } from "@/lib/rarity";
import { cn } from "@/lib/utils";
import { Places } from "@/pages/quests/objective-row";
import { isBoolean, useQuestItems, useStoredState } from "@/pages/quests/use-quests";

export function ItemsTab({ hideLocked }: { hideLocked: boolean }) {
  const { data, isPending, isError, error } = useQuestItems();
  const [search, setSearch] = useState("");
  const [ownedFirst, setOwnedFirst] = useStoredState("quests.ownedFirst", false, isBoolean);
  const rows = useMemo(() => checklistRows(data?.items ?? [], { search, hideLocked, ownedFirst }), [data, search, hideLocked, ownedFirst]);

  if (isPending) return <ChecklistSkeleton />;
  if (isError) return <p className="text-sm text-muted-foreground">{String(error)}</p>;

  const needed = rows.reduce((sum, row) => sum + row.needed, 0);
  const covered = rows.filter((row) => row.need.owned.usable >= row.needed).length;
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-4">
        <InputGroup className="h-9 w-72">
          <InputGroupAddon>
            <Search />
          </InputGroupAddon>
          <InputGroupInput value={search} onChange={(event) => setSearch(event.target.value)} placeholder="Item, merchant or quest" />
          {search && (
            <InputGroupAddon align="inline-end">
              <InputGroupButton size="icon-xs" aria-label="Clear" onClick={() => setSearch("")}>
                <X />
              </InputGroupButton>
            </InputGroupAddon>
          )}
        </InputGroup>
        <Label className="font-normal text-muted-foreground">
          <Switch checked={ownedFirst} onCheckedChange={setOwnedFirst} />
          Owned first
        </Label>
        <p className="ml-auto text-xs text-muted-foreground">
          <span className="font-num text-foreground tabular">{rows.length}</span> items ·{" "}
          <span className="font-num text-foreground tabular">{needed}</span> to bring ·{" "}
          <span className={cn("font-num tabular", covered > 0 ? "text-profit" : "text-foreground")}>{covered}</span> covered
        </p>
      </div>
      {data.characters === 0 && (
        <p className="rounded-lg border border-dashed px-4 py-2.5 text-sm text-muted-foreground">
          Owned counts show once the game has sent your character.
        </p>
      )}
      {rows.length === 0 ? <NoItems searching={search.trim().length > 0} /> : <Checklist rows={rows} />}
    </div>
  );
}

function Checklist({ rows }: { rows: NeedRow[] }) {
  return (
    <div className="overflow-hidden rounded-lg border bg-card">
      <div className="grid grid-cols-[minmax(0,1.2fr)_3.5rem_8.5rem_minmax(0,1.6fr)] gap-4 border-b px-3 py-2 text-xs text-muted-foreground">
        <span>Item</span>
        <span className="text-right">Need</span>
        <span>Have</span>
        <span>For</span>
      </div>
      <ul className="divide-y divide-border/60">
        {rows.map((row) => (
          <ChecklistRow key={row.need.key} row={row} />
        ))}
      </ul>
    </div>
  );
}

function ChecklistRow({ row }: { row: NeedRow }) {
  const { need } = row;
  const covered = need.owned.usable >= row.needed;
  const notes = [need.minRarity && `${need.minRarity} or higher`, need.useInDungeon && "Used in the dungeon"].filter(Boolean);
  return (
    <li className="grid grid-cols-[minmax(0,1.2fr)_3.5rem_8.5rem_minmax(0,1.6fr)] items-center gap-4 px-3 py-2">
      <div className="flex min-w-0 items-center gap-3">
        <ItemSlot icon={need.item.icon} rarity={need.item.rarity} size={38} />
        <div className="flex min-w-0 flex-col">
          <span className={cn("truncate font-display text-[15px]", RARITY_TEXT[need.item.rarity])}>{need.item.name}</span>
          {notes.length > 0 && <span className="truncate text-xs text-muted-foreground">{notes.join(" · ")}</span>}
        </div>
      </div>
      <span className="text-right font-num text-base tabular">{row.needed}</span>
      <Tooltip>
        <TooltipTrigger asChild>
          <span className="flex cursor-default items-center gap-1.5 text-sm">
            <span className={cn("font-num text-base tabular", covered ? "text-profit" : need.owned.usable > 0 ? "text-foreground" : "text-muted-foreground")}>
              {need.owned.usable}
            </span>
            {covered && <Check className="size-3.5 text-profit" />}
            {need.owned.other > 0 && <span className="text-xs text-muted-foreground">+{need.owned.other} not looted</span>}
          </span>
        </TooltipTrigger>
        <TooltipContent className="max-w-80">
          <Places owned={need.owned} />
        </TooltipContent>
      </Tooltip>
      <div className="flex min-w-0 flex-col gap-0.5 text-xs">
        {row.quests.map((quest) => (
          <span key={quest.questId} className={cn("flex min-w-0 items-center gap-1.5", quest.state === "locked" && "text-muted-foreground")}>
            {quest.state === "locked" && <Lock className="size-3 shrink-0" />}
            <span className="shrink-0 text-muted-foreground">{quest.merchant}</span>
            <span className="truncate">{quest.title}</span>
            <span className="shrink-0 font-num text-muted-foreground tabular">×{quest.remaining}</span>
          </span>
        ))}
        {row.hidden > 0 && (
          <span className="text-muted-foreground">
            {row.hidden} locked quest{row.hidden === 1 ? "" : "s"} hidden
          </span>
        )}
      </div>
    </li>
  );
}

function NoItems({ searching }: { searching: boolean }) {
  return (
    <Empty className="border border-dashed py-10">
      <EmptyHeader>
        <EmptyMedia variant="icon">
          <PackageSearch />
        </EmptyMedia>
        <EmptyTitle className="font-display text-lg">{searching ? "No item matches" : "No items needed"}</EmptyTitle>
        {!searching && <EmptyDescription>Open quests ask for no items right now.</EmptyDescription>}
      </EmptyHeader>
    </Empty>
  );
}

function ChecklistSkeleton() {
  return (
    <div className="flex flex-col gap-3">
      <Skeleton className="h-9 w-72" />
      <Skeleton className="h-80" />
    </div>
  );
}
