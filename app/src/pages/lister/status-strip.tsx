import { HandCoins, MousePointerClick, Store } from "lucide-react";

import { Gold } from "@/components/gold";
import { Button } from "@/components/ui/button";
import { Kbd, KbdGroup } from "@/components/ui/kbd";
import type { ListingsInfo } from "@/lib/lister-api";
import { formatAge } from "@/lib/format";

/** The risk notice with the stop keys, and what the game last showed of the player's listings. */
export function StatusStrip({ listings, onCollect, busy }: { listings: ListingsInfo | undefined; onCollect: () => void; busy: boolean }) {
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-start gap-3 rounded-lg border border-gold/25 bg-gold/5 px-4 py-2.5 text-sm">
        <MousePointerClick className="mt-0.5 size-4 shrink-0 text-gold" />
        <p className="flex-1 text-muted-foreground">
          Listing moves your mouse and clicks in the game. Automated input may break Dark and Darker's Terms of Service, so
          use it at your own risk. Stop at any time with{" "}
          <KbdGroup>
            <Kbd>Ctrl</Kbd>
            <Kbd>F12</Kbd>
          </KbdGroup>
          .
        </p>
      </div>
      <div className="flex min-h-11 items-center gap-3 rounded-lg border bg-card px-4 py-2 text-sm">
        <Store className="size-4 shrink-0 text-muted-foreground" />
        {listings?.seen ? (
          <>
            <span>
              <span className="font-num tabular">{listings.free ?? "?"}</span> listing spot{listings.free === 1 ? "" : "s"} free
            </span>
            {listings.ageS !== null && <span className="text-xs text-muted-foreground">read {formatAge(listings.ageS)}</span>}
            {listings.payouts > 0 && (
              <span className="ml-auto flex items-center gap-3">
                <span>
                  {listings.payouts} sold · <Gold value={listings.payoutGold} /> to collect
                </span>
                <Button size="sm" onClick={onCollect} disabled={busy}>
                  <HandCoins />
                  Collect
                </Button>
              </span>
            )}
          </>
        ) : (
          <span className="text-muted-foreground">
            Open Trade → Marketplace → My Listings in the game so the app can see your free listing spots.
          </span>
        )}
      </div>
    </div>
  );
}
