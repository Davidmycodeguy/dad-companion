import { useState, type CSSProperties } from "react";
import { Package } from "lucide-react";

import { iconUrl } from "@/lib/api";
import { rarityColor, type Rarity } from "@/lib/rarity";
import { cn } from "@/lib/utils";

interface ItemSlotProps {
  icon: string | null;
  rarity: Rarity;
  /** Size of one inventory cell in px. */
  size?: number;
  /** Inventory cells the item takes; drawn to scale when given, square otherwise. */
  cells?: { width: number; height: number };
  className?: string;
}

/** An item icon in an inventory slot lit by its rarity, like the game's inventory. */
export function ItemSlot({ icon, rarity, size = 40, cells, className }: ItemSlotProps) {
  const [broken, setBroken] = useState(false);
  const style = {
    width: size * (cells?.width ?? 1),
    height: size * (cells?.height ?? 1),
    "--slot-rarity": rarityColor(rarity),
  } as CSSProperties;
  return (
    <div className={cn("item-slot", className)} style={style}>
      {icon && !broken ? (
        <img
          src={iconUrl(icon)}
          alt=""
          draggable={false}
          loading="lazy"
          onError={() => setBroken(true)}
          className="absolute inset-0 m-auto size-[88%] object-contain drop-shadow-[0_2px_3px_rgb(0_0_0/0.6)]"
        />
      ) : (
        <Package className="size-1/2 text-muted-foreground/40" />
      )}
    </div>
  );
}
