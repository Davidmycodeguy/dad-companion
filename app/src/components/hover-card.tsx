import type { CSSProperties, ReactNode } from "react";

import { ItemSlot } from "@/components/item-slot";
import { COIN_ICON, iconUrl, type CardRoll, type HoverCard } from "@/lib/api";
import { formatAge, formatGold } from "@/lib/format";
import { rarityColor } from "@/lib/rarity";
import { cn } from "@/lib/utils";

/** Width of the card in CSS pixels; the game's tooltip beside it is about this wide at 4K. */
export const CARD_WIDTH = 304;
/** Segments in a roll's quality bar. */
const QUALITY_SEGMENTS = 10;
/** The ladder's price range ends at this share of the asks (the rest are marked with an arrow), so a
 * few extreme asks don't squash the curve. */
const LADDER_TOP_SHARE = 0.9;
/** Points along the ladder's curve. */
const LADDER_STEPS = 120;
/** Height of the ladder in CSS pixels. */
const LADDER_HEIGHT = 40;

const VERDICTS = {
  list: { text: "List it", className: "border-profit/50 bg-profit/15 text-profit" },
  merchant: { text: "Merchant", className: "border-[#f2c170]/45 bg-[#5c3c10]/60 text-[#f6c470]" },
  unknown: { text: "No data", className: "border-border bg-muted text-muted-foreground" },
} as const;

/**
 * The value card shown beside the game's tooltip: what the item is worth for its exact rolls, the
 * price that sells quickly, where that sits among the other listings, and what each roll adds.
 */
export function HoverValueCard({ card, className }: { card: HoverCard; className?: string }) {
  const color = rarityColor(card.rarity);
  return (
    <div
      className={cn(
        "overflow-hidden rounded-[10px] border bg-linear-to-b from-[#1b1612] to-[#0c0a08] text-[13px] text-foreground shadow-[0_10px_28px_rgb(0_0_0/0.55)]",
        className,
      )}
      style={{ width: CARD_WIDTH, borderColor: `color-mix(in oklab, ${color} 45%, #2b251d)` } as CSSProperties}
    >
      <Header card={card} color={color} />
      <div className="flex flex-col gap-2.5 px-3.5 pt-2.5 pb-3">
        <Hero card={card} />
        {card.asks.length > 0 && card.fastSale !== null && <Ladder asks={card.asks} fastSale={card.fastSale} value={card.value} low={card.low} high={card.high} />}
        <MarketLine card={card} />
        {card.rolls.length > 0 && <RollList card={card} />}
      </div>
      <footer className="flex justify-between border-t border-border/60 px-3.5 py-1.5 text-[10.5px] text-muted-foreground/70">
        <span>DaD Companion</span>
        <span>{card.listings > 0 ? `local model · ${card.listings.toLocaleString()} listings` : "local model"}</span>
      </footer>
    </div>
  );
}

function Header({ card, color }: { card: HoverCard; color: string }) {
  const verdict = VERDICTS[card.verdict];
  return (
    <header
      className="relative flex items-center gap-2.5 px-3.5 pt-2.5 pb-3"
      style={{ background: `linear-gradient(to bottom, color-mix(in oklab, ${color} 30%, transparent), transparent)` }}
    >
      <ItemSlot icon={card.icon} rarity={card.rarity} size={44} />
      <div className="min-w-0 flex-1">
        <div
          className="truncate font-display text-[17px] leading-tight"
          style={{ color: `color-mix(in oklab, ${color} 88%, white)` }}
        >
          {card.name}
        </div>
        <div
          className="mt-0.5 truncate text-[10px] font-semibold tracking-wide uppercase"
          style={{ color: `color-mix(in oklab, ${color} 45%, var(--muted-foreground))` }}
        >
          {[card.rarity, card.kind].filter(Boolean).join(" · ")}
        </div>
      </div>
      <span className={cn("shrink-0 self-start rounded-full border px-2 py-0.5 text-[10px] font-bold tracking-wide uppercase", verdict.className)}>
        {verdict.text}
      </span>
      <div className="rarity-rule absolute inset-x-0 bottom-0" style={{ "--rule-rarity": color } as CSSProperties} />
    </header>
  );
}

function Coin({ className }: { className?: string }) {
  return <img src={iconUrl(COIN_ICON)} alt="" draggable={false} className={cn("shrink-0", className)} />;
}

function Hero({ card }: { card: HoverCard }) {
  return (
    <section className="flex flex-col gap-1">
      <div className="flex items-end justify-between gap-3">
        <span className="flex items-center gap-1.5">
          <Coin className="size-6" />
          <span className={cn("font-num text-[30px] leading-none font-bold tabular", card.value !== null ? "text-gold" : "text-muted-foreground")}>
            {formatGold(card.value ?? card.merchant)}
          </span>
        </span>
        {card.value !== null && card.confidence ? (
          <Confidence level={card.confidence} />
        ) : (
          <span className="pb-0.5 text-[11px] text-muted-foreground">merchant price</span>
        )}
      </div>
      {card.value === null ? (
        <p className="text-[12px] text-muted-foreground">No market data for this item yet.</p>
      ) : card.verdict === "merchant" ? (
        <p className="text-[12px] text-muted-foreground">
          Merchant pays <Num className="text-[#f2b35a]">{card.merchant}</Num> · a fast sale nets only{" "}
          <Num>{card.net ?? 0}</Num>
        </p>
      ) : (
        <p className="text-[12px] text-muted-foreground">
          Fast sale <Num className="text-profit">{card.fastSale ?? 0}</Num> · nets <Num>{card.net ?? 0}</Num> after fee
        </p>
      )}
    </section>
  );
}

function Num({ children, className }: { children: number; className?: string }) {
  return <span className={cn("font-num text-[13px] font-semibold text-foreground tabular", className)}>{formatGold(children)}</span>;
}

function Confidence({ level }: { level: "high" | "medium" | "low" }) {
  const bars = level === "high" ? 3 : level === "medium" ? 2 : 1;
  return (
    <span className="flex items-end gap-1.5 pb-0.5 text-[11px] text-muted-foreground">
      {level} confidence
      <span className="flex items-end gap-0.5" aria-hidden>
        {[0, 1, 2].map((i) => (
          <span key={i} className={cn("w-[3px] rounded-[1px]", i < bars ? "bg-gold" : "bg-border")} style={{ height: 5 + 3 * i }} />
        ))}
      </span>
    </span>
  );
}

/** Smoothed share of `values` (log prices) at each of `xs`: a kernel density estimate. */
function density(values: number[], xs: number[]): number[] {
  const n = values.length;
  const mean = values.reduce((sum, v) => sum + v, 0) / n;
  const spread = n > 1 ? Math.sqrt(values.reduce((sum, v) => sum + (v - mean) ** 2, 0) / n) : 0.1;
  const width = Math.min(Math.max(0.9 * spread * n ** -0.2, 0.035), 0.25);
  return xs.map((x) => values.reduce((sum, v) => sum + Math.exp(-0.5 * ((x - v) / width) ** 2), 0));
}

/**
 * Where this item lands among the live asks, on a log price scale: the curve is where listings
 * cluster and its bright part the model's typical range for these rolls; the lowest ask is a white
 * tick, the fast sale a green marker and the value a gold diamond. Underneath: how far above the
 * cheapest listing these rolls sell.
 */
function Ladder({ asks, fastSale, value, low, high }: { asks: number[]; fastSale: number; value: number | null; low: number | null; high: number | null }) {
  const lowest = asks[0];
  const points = [lowest, asks[Math.min(asks.length - 1, Math.floor(asks.length * LADDER_TOP_SHARE))], fastSale, value, low, high].filter(
    (v): v is number => v !== null && v > 0,
  );
  const margin = (Math.log(Math.max(...points, 2)) - Math.log(Math.max(Math.min(...points), 1))) * 0.06 || 0.1;
  const lo = Math.log(Math.max(Math.min(...points), 1)) - margin;
  const hi = Math.log(Math.max(...points, 2)) + margin;
  const width = CARD_WIDTH - 28;
  const right = width - 10;
  const top = 2;
  const base = LADDER_HEIGHT - 8;
  const x = (v: number) => (right * (Math.min(Math.max(Math.log(Math.max(v, 1)), lo), hi) - lo)) / (hi - lo);
  const logs = asks.map((a) => Math.log(Math.max(a, 1))).filter((v) => v <= hi);
  const beaten = asks.filter((ask) => ask > fastSale).length;
  const above = fastSale - lowest;

  let area = "";
  let lit = "";
  let line = "";
  if (logs.length >= 2) {
    const xs = Array.from({ length: LADDER_STEPS + 1 }, (_, i) => lo + ((hi - lo) * i) / LADDER_STEPS);
    const dens = density(logs, xs);
    const peak = Math.max(...dens) || 1;
    const curve = dens.map((d, i) => [(right * i) / LADDER_STEPS, base - ((base - top) * d) / peak] as const);
    line = curve.map(([cx, cy], i) => `${i ? "L" : "M"}${cx.toFixed(1)} ${cy.toFixed(1)}`).join(" ");
    area = `M0 ${base} ${curve.map(([cx, cy]) => `L${cx.toFixed(1)} ${cy.toFixed(1)}`).join(" ")} L${right} ${base} Z`;
    if (low !== null && high !== null) {
      const band = curve.filter(([cx]) => cx >= x(low) && cx <= x(high));
      if (band.length >= 2) {
        lit = `M${band[0][0].toFixed(1)} ${base} ${band.map(([cx, cy]) => `L${cx.toFixed(1)} ${cy.toFixed(1)}`).join(" ")} L${band[band.length - 1][0].toFixed(1)} ${base} Z`;
      }
    }
  }
  const fx = x(fastSale);
  return (
    <section className="flex flex-col gap-1">
      <svg width={width} height={LADDER_HEIGHT} viewBox={`0 0 ${width} ${LADDER_HEIGHT}`} role="img" aria-label="Where listings cluster by price">
        {area && <path d={area} fill="color-mix(in oklab, var(--gold) 18%, transparent)" />}
        {lit && <path d={lit} fill="color-mix(in oklab, var(--gold) 28%, transparent)" />}
        {line && <path d={line} fill="none" stroke="color-mix(in oklab, var(--gold) 75%, transparent)" strokeWidth={1.3} strokeLinejoin="round" />}
        {!line && low !== null && high !== null && (
          <rect x={x(low)} y={base - 5} width={Math.max(2, x(high) - x(low))} height={5} rx={2} fill="color-mix(in oklab, var(--gold) 28%, transparent)" />
        )}
        <line x1={0} x2={right} y1={base} y2={base} stroke="var(--border)" strokeWidth={1} />
        {asks.some((a) => Math.log(Math.max(a, 1)) > hi) && (
          <text x={right + 3} y={base + 3} fontSize={11} fill="var(--muted-foreground)">
            ›
          </text>
        )}
        <line x1={x(lowest)} x2={x(lowest)} y1={base - 8} y2={base + 3} stroke="#faf6ec" strokeWidth={1.6} />
        <line x1={fx} x2={fx} y1={top + 4} y2={base} stroke="color-mix(in oklab, var(--profit) 60%, transparent)" strokeWidth={1} />
        <path d={`M ${fx} ${base + 1.5} l -4 6.5 h 8 z`} fill="var(--profit)" />
        {value !== null && (
          <>
            <line x1={x(value)} x2={x(value)} y1={top} y2={base} stroke="color-mix(in oklab, var(--gold) 65%, transparent)" strokeWidth={1} />
            <path d={`M ${x(value)} ${base - 5} l 5 5 l -5 5 l -5 -5 z`} fill="var(--gold)" stroke="#2c2010" strokeWidth={1} />
          </>
        )}
      </svg>
      <div className="flex justify-between text-[10.5px] text-muted-foreground">
        <span>
          lowest <Num className="text-[11px] text-foreground/85">{lowest}</Num>
        </span>
        {above > 0 ? (
          <span>
            these rolls sell <span className="font-num text-profit">+{formatGold(above)}</span> above it
          </span>
        ) : (
          <span>
            fast sale beats <span className="font-num text-foreground/85">{Math.round((beaten / asks.length) * 100)}%</span> of asks
          </span>
        )}
      </div>
    </section>
  );
}

function MarketLine({ card }: { card: HoverCard }) {
  const parts: ReactNode[] = [];
  if (card.asks.length > 0) parts.push(`${card.asks.length.toLocaleString()} listed`);
  if (card.seenAgoS !== null) parts.push(`read ${formatAge(card.seenAgoS)}`);
  if (card.soldWeek > 0) parts.push(`${card.soldWeek} sold this week`);
  if (card.pace) parts.push(card.pace);
  if (card.trendPct !== null && Math.abs(card.trendPct) >= 1) {
    parts.push(
      <span key="trend" className={card.trendPct > 0 ? "text-profit" : "text-loss"}>
        {card.trendPct > 0 ? "▲" : "▼"} {Math.abs(Math.round(card.trendPct))}%
      </span>,
    );
  }
  return (
    <section className="flex flex-col gap-0.5 text-[11.5px] text-muted-foreground">
      {parts.length > 0 && (
        <div className="flex flex-wrap gap-x-1.5">
          {parts.flatMap((part, i) => [
            ...(i > 0 ? [<span key={`dot${i}`}>·</span>] : []),
            <span key={i}>{part}</span>,
          ])}
        </div>
      )}
      <div>
        Merchant <Num className="text-[12px] text-foreground/85">{card.merchant}</Num>
        {card.slots > 1 && (
          <>
            {" "}· <Num className="text-[12px] text-foreground/85">{card.perSlot}</Num> per slot · {card.slots} slots
          </>
        )}
      </div>
    </section>
  );
}

function RollList({ card }: { card: HoverCard }) {
  return (
    <section className="flex flex-col gap-1.5 border-t border-border/60 pt-2">
      <div className="flex items-center justify-between text-[10px] font-semibold tracking-wider text-muted-foreground uppercase">
        Random rolls
        {card.quality !== null && (
          <span className="flex items-center gap-1.5 font-normal tracking-normal normal-case">
            roll quality
            <span className="grid size-5 place-items-center rounded-full border border-border font-num text-[10px] text-foreground tabular">
              {card.quality}
            </span>
          </span>
        )}
      </div>
      {card.rolls.map((roll, i) => (
        <RollRow key={i} roll={roll} />
      ))}
      {strongestPairs(card.pairs).map((pair) => (
        <div key={pair.label} className="flex justify-between text-[11px] text-muted-foreground">
          <span className="truncate">{pair.label}</span>
          <span className={cn("font-num tabular", pair.pct > 0 ? "text-profit" : "text-loss")}>
            {pair.pct > 0 ? "+" : ""}
            {Math.round(pair.pct)}% together
          </span>
        </div>
      ))}
      {card.unread > 0 && (
        <div className="text-[10.5px] text-muted-foreground/70">
          {card.unread} {card.unread === 1 ? "line" : "lines"} not read
        </div>
      )}
    </section>
  );
}

/** Pair bonuses shown on the card; the strongest say the most and the card stays short. */
const MAX_PAIRS = 3;

function strongestPairs(pairs: HoverCard["pairs"]) {
  return [...pairs].sort((a, b) => Math.abs(b.pct) - Math.abs(a.pct)).slice(0, MAX_PAIRS);
}

function RollRow({ roll }: { roll: CardRoll }) {
  const lit = roll.quality === null ? 0 : Math.max(1, Math.round(roll.quality * QUALITY_SEGMENTS));
  return (
    <div className="grid grid-cols-[1fr_auto_auto_3rem] items-center gap-2 text-[12px]">
      <span className="truncate">{roll.label}</span>
      <span className="flex items-center gap-1 font-num text-roll tabular">
        {roll.isMax && <span className="rounded-sm bg-gold/20 px-1 text-[9px] font-bold text-gold">MAX</span>}
        {roll.value}
      </span>
      <span className="flex gap-px" aria-label={roll.quality === null ? "quality unknown" : `quality ${Math.round(roll.quality * 100)}%`}>
        {Array.from({ length: QUALITY_SEGMENTS }, (_, i) => (
          <span
            key={i}
            className={cn("h-2 w-1.5 rounded-[1px]", i < lit ? (roll.isMax ? "bg-gold" : "bg-roll/80") : "bg-border")}
          />
        ))}
      </span>
      <span
        className={cn(
          "text-right font-num text-[11.5px] tabular",
          roll.gold === null ? "text-muted-foreground/60" : roll.gold > 0 ? "text-profit" : roll.gold < 0 ? "text-loss" : "text-muted-foreground",
        )}
      >
        {roll.gold === null ? "—" : `${roll.gold > 0 ? "+" : ""}${formatGold(roll.gold)}`}
      </span>
    </div>
  );
}
