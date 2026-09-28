import { ItemSlot } from "@/components/item-slot";
import { RarityDots } from "@/components/rarity-dots";
import type { ItemGroup } from "@/lib/api";
import { RARITY_TEXT, singleRarity } from "@/lib/rarity";

/**
 * An item name as a search result: icon, name, kind and the rarities it comes in. The name takes
 * its rarity's colour only when the item has a single rarity; the dots carry the rest.
 */
export function ItemGroupSummary({ group, slotSize = 36 }: { group: ItemGroup; slotSize?: number }) {
  const rarities = group.variants.map((variant) => variant.rarity);
  const only = singleRarity(rarities);
  const shown = group.variants[group.variants.length - 1];
  return (
    <>
      <ItemSlot icon={shown?.icon ?? null} rarity={only ?? "Unknown"} size={slotSize} />
      <div className="min-w-0 flex-1">
        <div className={`truncate font-display text-[15px] ${only ? RARITY_TEXT[only] : "text-foreground"}`}>
          {group.name}
        </div>
        <div className="truncate text-xs text-muted-foreground">{shown?.kind || shown?.itemType || "Loot"}</div>
      </div>
      <RarityDots rarities={rarities} />
    </>
  );
}
