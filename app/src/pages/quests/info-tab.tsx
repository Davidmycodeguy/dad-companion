import type { ReactNode } from "react";
import { CircleCheck, RadioTower } from "lucide-react";

import { Badge } from "@/components/ui/badge";
import { StateBadge } from "@/pages/quests/parts";

/** What the page's marks mean, and where its numbers come from. */
export function InfoTab() {
  return (
    <div className="grid gap-4 lg:grid-cols-2">
      <Section title="Merchants">
        <Entry mark={<RadioTower className="size-4 text-profit" />}>
          The app has read this merchant's quests from the game. They update each time you open the merchant's quests
          or your quest log.
        </Entry>
        <Entry mark={<CircleCheck className="size-4 text-profit" />}>Every quest of this merchant is done.</Entry>
        <Entry mark={<Badge variant="outline" className="border-gold/40 bg-gold/10 font-normal text-gold">Turn in</Badge>}>
          A quest is ready to hand in here.
        </Entry>
        <Entry mark={<Badge variant="outline" className="border-profit/30 font-normal text-profit">2 to bring</Badge>}>
          You have looted items that open quests here still need.
        </Entry>
      </Section>

      <Section title="Quests">
        <Entry mark={<StateBadge state="available" source="game" />}>Offered, not accepted yet.</Entry>
        <Entry mark={<StateBadge state="active" source="game" />}>Accepted, or some progress recorded.</Entry>
        <Entry mark={<StateBadge state="ready" source="game" />}>Every objective is met: hand it in.</Entry>
        <Entry mark={<StateBadge state="done" source="game" />}>Handed in, or ticked off.</Entry>
        <Entry mark={<StateBadge state="locked" source={null} />}>Waits for the quest before it in the chain.</Entry>
      </Section>

      <Section title="Counting items">
        <p>
          <span className="text-foreground">Have</span> counts looted items across all your characters: stashes, bag and
          gear. Items you bought, crafted or traded for show as <span className="text-foreground">not looted</span>;
          quests don't take them.
        </p>
        <p>When a quest names a rarity, items of that rarity or higher count.</p>
        <p>The locked seasonal stash never counts: its items are only a preview.</p>
      </Section>

      <Section title="Progress">
        <p>
          Progress comes from the game's own messages when you open a merchant's quests or your quest log, accept a
          quest or hand one in. It is read only: nothing is sent to the game or anywhere else.
        </p>
        <p>
          Ticks and counts you set yourself stand until the game reports more. Once a later quest of a chain is open,
          every quest before it counts as done.
        </p>
        <p>Daily and weekly quests show while the game offers them, or while they're in progress.</p>
      </Section>
    </div>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-3 rounded-lg border bg-card p-4 text-sm text-muted-foreground">
      <h3 className="font-display text-base text-foreground">{title}</h3>
      {children}
    </section>
  );
}

function Entry({ mark, children }: { mark: ReactNode; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[8.5rem_1fr] items-start gap-3">
      <span className="flex min-h-5 items-center">{mark}</span>
      <span>{children}</span>
    </div>
  );
}
