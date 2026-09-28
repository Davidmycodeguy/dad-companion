import { useState } from "react";
import { RotateCcw, TriangleAlert } from "lucide-react";

import { Gold } from "@/components/gold";
import { ItemSlot } from "@/components/item-slot";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import type { Confidence, PlanEntry, PlanItemInfo } from "@/lib/lister-api";
import { parsePrice, rarityName, withPrice } from "@/lib/lister";
import { formatGold } from "@/lib/format";
import { RARITY_TEXT } from "@/lib/rarity";
import { cn } from "@/lib/utils";

interface PlanTableProps {
  entries: PlanEntry[];
  items: Record<string, PlanItemInfo>;
  ticked: Set<string>;
  onTick: (uniqueId: string, on: boolean) => void;
  onTickAll: (on: boolean) => void;
  onEntry: (entry: PlanEntry) => void;
  /** Prices can't be edited while a run is going or before the game has priced the plan. */
  locked: boolean;
}

/** The items a run will list: tick to include, edit a price to override the suggestion. */
export function PlanTable({ entries, items, ticked, onTick, onTickAll, onEntry, locked }: PlanTableProps) {
  const allTicked = entries.length > 0 && entries.every((e) => ticked.has(e.uniqueId));
  const someTicked = entries.some((e) => ticked.has(e.uniqueId));
  return (
    <Table>
      <TableHeader>
        <TableRow className="hover:bg-transparent">
          <TableHead className="w-8">
            <Checkbox
              checked={allTicked ? true : someTicked ? "indeterminate" : false}
              onCheckedChange={(on) => onTickAll(on === true)}
              disabled={locked}
              aria-label="Tick every item"
            />
          </TableHead>
          <TableHead>Item</TableHead>
          <TableHead>From</TableHead>
          <TableHead>Confidence</TableHead>
          <TableHead className="text-right">Price</TableHead>
          <TableHead className="text-right">Fee</TableHead>
          <TableHead className="text-right">You get</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {entries.map((entry) => (
          <PlanRow
            key={entry.uniqueId}
            entry={entry}
            info={items[entry.uniqueId]}
            ticked={ticked.has(entry.uniqueId)}
            onTick={(on) => onTick(entry.uniqueId, on)}
            onEntry={onEntry}
            locked={locked}
          />
        ))}
      </TableBody>
    </Table>
  );
}

interface PlanRowProps {
  entry: PlanEntry;
  info: PlanItemInfo | undefined;
  ticked: boolean;
  onTick: (on: boolean) => void;
  onEntry: (entry: PlanEntry) => void;
  locked: boolean;
}

function PlanRow({ entry, info, ticked, onTick, onEntry, locked }: PlanRowProps) {
  const rarity = info?.rarity ?? rarityName(entry.rarity);
  const edited = entry.recommended > 0 && entry.price !== entry.recommended;
  return (
    <TableRow data-state={ticked ? "selected" : undefined} className={cn(!ticked && "opacity-55")}>
      <TableCell>
        <Checkbox checked={ticked} onCheckedChange={(on) => onTick(on === true)} disabled={locked} aria-label={`List ${entry.name}`} />
      </TableCell>
      {/* Takes the width the other columns leave; long roll lists are cut short (full text on hover). */}
      <TableCell className="w-full max-w-0">
        <div className="flex min-w-0 items-center gap-2.5">
          <ItemSlot icon={info?.icon ?? null} rarity={rarity} size={34} />
          <div className="flex min-w-0 flex-col">
            <span className={cn("flex items-center gap-1.5 truncate", RARITY_TEXT[rarity])}>
              {entry.name}
              {entry.quantity > 1 && <span className="font-num text-xs text-muted-foreground">×{entry.quantity}</span>}
              {entry.flag && <PriceFlag flag={entry.flag} compared={entry.compared} />}
            </span>
            {info && info.rolls.length > 0 && (
              <span className="truncate text-xs text-roll" title={info.rolls.join(", ")}>
                {info.rolls.join(" · ")}
              </span>
            )}
          </div>
        </div>
      </TableCell>
      <TableCell className="text-muted-foreground">{info?.stashLabel ?? entry.stashId}</TableCell>
      <TableCell>
        <ConfidenceBadge confidence={entry.confidence} compared={entry.compared} />
      </TableCell>
      <TableCell className="text-right">
        <div className="flex items-center justify-end gap-1">
          {edited && !locked && (
            <button
              type="button"
              title={`Back to the suggested ${formatGold(entry.recommended)}`}
              onClick={() => onEntry(withPrice(entry, entry.recommended))}
              className="text-muted-foreground hover:text-foreground"
            >
              <RotateCcw className="size-3.5" />
            </button>
          )}
          <PriceInput entry={entry} onEntry={onEntry} locked={locked} />
        </div>
      </TableCell>
      <TableCell className="text-right">
        <span className="font-num text-muted-foreground tabular">{entry.price > 0 ? formatGold(entry.fee) : "—"}</span>
      </TableCell>
      <TableCell className="text-right">{entry.price > 0 ? <Gold value={entry.price - entry.fee} /> : "—"}</TableCell>
    </TableRow>
  );
}

/** The listing price; a typed price takes effect when it is a whole number within the game's limits. */
function PriceInput({ entry, onEntry, locked }: { entry: PlanEntry; onEntry: (entry: PlanEntry) => void; locked: boolean }) {
  const [draft, setDraft] = useState<string | null>(null);
  if (entry.price <= 0) return <span className="text-xs text-muted-foreground">priced in game</span>;
  const invalid = draft !== null && parsePrice(draft) === null;
  const commit = () => {
    const price = draft === null ? null : parsePrice(draft);
    if (price !== null && price !== entry.price) onEntry(withPrice(entry, price));
    setDraft(null);
  };
  return (
    <Input
      value={draft ?? formatGold(entry.price)}
      disabled={locked}
      inputMode="numeric"
      aria-invalid={invalid}
      aria-label={`Price for ${entry.name}`}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={commit}
      onKeyDown={(event) => {
        if (event.key === "Enter") commit();
        if (event.key === "Escape") setDraft(null);
      }}
      className="h-8 w-24 text-right font-num text-gold tabular"
    />
  );
}

const CONFIDENCE_STYLE: Record<Exclude<Confidence, "">, string> = {
  high: "border-profit/40 bg-profit/10 text-profit",
  medium: "border-gold/40 bg-gold/10 text-gold",
  low: "border-loss/40 bg-loss/10 text-loss",
};

function ConfidenceBadge({ confidence, compared }: { confidence: Confidence; compared: string }) {
  if (!confidence) return <span className="text-muted-foreground">—</span>;
  const badge = (
    <Badge variant="outline" className={cn("capitalize", CONFIDENCE_STYLE[confidence])}>
      {confidence}
    </Badge>
  );
  if (!compared) return badge;
  return (
    <Tooltip>
      <TooltipTrigger type="button" className="rounded-md focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none">
        {badge}
      </TooltipTrigger>
      <TooltipContent className="max-w-80">{compared}</TooltipContent>
    </Tooltip>
  );
}

function PriceFlag({ flag, compared }: { flag: string; compared: string }) {
  return (
    <Tooltip>
      <TooltipTrigger type="button" aria-label="Check this price" className="rounded-sm focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none">
        <TriangleAlert className="size-3.5 shrink-0 text-gold" />
      </TooltipTrigger>
      <TooltipContent className="max-w-80">
        <p>{flag}</p>
        {compared && <p className="mt-1 text-muted-foreground">{compared}</p>}
      </TooltipContent>
    </Tooltip>
  );
}
