import { rarityColor, type Rarity } from "@/lib/rarity";
import { cn } from "@/lib/utils";

/** One small dot per rarity the item comes in. */
export function RarityDots({ rarities, className }: { rarities: Rarity[]; className?: string }) {
  return (
    <span className={cn("inline-flex items-center gap-1", className)} aria-label={rarities.join(", ")}>
      {rarities.map((rarity) => (
        <span key={rarity} title={rarity} className="size-1.5 rounded-full" style={{ background: rarityColor(rarity) }} />
      ))}
    </span>
  );
}
