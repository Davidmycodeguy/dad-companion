import { COIN_ICON, iconUrl } from "@/lib/api";
import { formatGold } from "@/lib/format";
import { cn } from "@/lib/utils";

/** A gold amount with the game's coin, in tabular figures. */
export function Gold({ value, className }: { value: number; className?: string }) {
  return (
    <span className={cn("inline-flex items-center gap-1 font-num tabular text-gold", className)}>
      <img src={iconUrl(COIN_ICON)} alt="" draggable={false} className="size-[1.1em] shrink-0" />
      {formatGold(value)}
    </span>
  );
}
