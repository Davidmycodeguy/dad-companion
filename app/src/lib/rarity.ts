export const RARITIES = ["Poor", "Common", "Uncommon", "Rare", "Epic", "Legendary", "Unique", "Artifact"] as const;

export type Rarity = (typeof RARITIES)[number] | "Unknown";

/** The rarity's colour as a CSS value, for inline styles and CSS variables. */
export function rarityColor(rarity: Rarity): string {
  return `var(--rarity-${rarity.toLowerCase()})`;
}

/** Text in the rarity's colour. */
export const RARITY_TEXT: Record<Rarity, string> = {
  Poor: "text-rarity-poor",
  Common: "text-rarity-common",
  Uncommon: "text-rarity-uncommon",
  Rare: "text-rarity-rare",
  Epic: "text-rarity-epic",
  Legendary: "text-rarity-legendary",
  Unique: "text-rarity-unique",
  Artifact: "text-rarity-artifact",
  Unknown: "text-rarity-unknown",
};

/** The highest rarity among `rarities` (Unknown when empty). */
export function highestRarity(rarities: Rarity[]): Rarity {
  let best: Rarity = "Unknown";
  let bestIndex = -1;
  for (const rarity of rarities) {
    const index = RARITIES.indexOf(rarity as (typeof RARITIES)[number]);
    if (index > bestIndex) {
      best = rarity;
      bestIndex = index;
    }
  }
  return best;
}

/** The rarity when all of `rarities` are the same one, otherwise null. */
export function singleRarity(rarities: Rarity[]): Rarity | null {
  const first = rarities[0];
  return first !== undefined && rarities.every((rarity) => rarity === first) ? first : null;
}
