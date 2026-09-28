import { ScrollText } from "lucide-react";

import { LiveDataAlert } from "@/components/live-data-alert";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { formatAge } from "@/lib/format";
import { secondsSince } from "@/lib/quests";
import { cn } from "@/lib/utils";
import { InfoTab } from "@/pages/quests/info-tab";
import { ItemsTab } from "@/pages/quests/items-tab";
import { MerchantsTab } from "@/pages/quests/merchants-tab";
import { QuestsTab } from "@/pages/quests/quests-tab";
import {
  isBoolean,
  isString,
  useNowSeconds,
  useQuestLiveRefresh,
  useQuestsOverview,
  useStoredState,
} from "@/pages/quests/use-quests";

const TABS = ["merchants", "quests", "items", "info"] as const;
type Tab = (typeof TABS)[number];
const isTab = (value: unknown): value is Tab => TABS.includes(value as Tab);

/** How recent the game's last word must be for the tracking mark to glow. */
const FRESH_S = 120;

export function QuestsPage() {
  useQuestLiveRefresh();
  const now = useNowSeconds();
  const [tab, setTab] = useStoredState<Tab>("quests.tab", "merchants", isTab);
  const [merchant, setMerchant] = useStoredState<string>("quests.merchant", "", isString);
  const [hideLocked, setHideLocked] = useStoredState("quests.hideLocked", false, isBoolean);
  const { data: overview, isPending, isError, error } = useQuestsOverview();

  if (isPending) return <PageSkeleton />;
  if (isError) return <Unavailable reason={String(error)} />;

  const names = overview.merchants.map((entry) => entry.name);
  const selected = names.includes(merchant) ? merchant : (names[0] ?? null);
  const openMerchant = (name: string) => {
    setMerchant(name);
    setTab("quests");
  };

  return (
    <div className="mx-auto flex max-w-6xl flex-col gap-5 p-6">
      <LiveDataAlert />
      <Tabs value={tab} onValueChange={(value) => isTab(value) && setTab(value)} className="gap-5">
        <div className="flex flex-wrap items-center gap-x-5 gap-y-3">
          <TabsList>
            <TabsTrigger value="merchants" className="px-3">
              Merchants
            </TabsTrigger>
            <TabsTrigger value="quests" className="px-3">
              Quests
            </TabsTrigger>
            <TabsTrigger value="items" className="px-3">
              Item checklist
            </TabsTrigger>
            <TabsTrigger value="info" className="px-3">
              Info
            </TabsTrigger>
          </TabsList>
          <Tracking lastUpdate={overview.lastUpdate} now={now} />
          {(tab === "quests" || tab === "items") && (
            <Label className="ml-auto font-normal text-muted-foreground">
              <Switch checked={hideLocked} onCheckedChange={setHideLocked} />
              Hide locked quests
            </Label>
          )}
        </div>
        <TabsContent value="merchants">
          <MerchantsTab overview={overview} now={now} onOpen={openMerchant} />
        </TabsContent>
        <TabsContent value="quests">
          <QuestsTab merchants={overview.merchants} merchant={selected} onMerchant={setMerchant} hideLocked={hideLocked} now={now} />
        </TabsContent>
        <TabsContent value="items">
          <ItemsTab hideLocked={hideLocked} />
        </TabsContent>
        <TabsContent value="info">
          <InfoTab />
        </TabsContent>
      </Tabs>
    </div>
  );
}

/** When the game last reported quests. */
function Tracking({ lastUpdate, now }: { lastUpdate: number | null; now: number }) {
  if (lastUpdate === null) return <span className="text-xs text-muted-foreground">Not read from the game yet</span>;
  const age = secondsSince(lastUpdate, now);
  return (
    <span className="flex items-center gap-2 text-xs text-muted-foreground">
      <span className={cn("size-1.5 rounded-full", age < FRESH_S ? "animate-pulse bg-profit" : "bg-profit/50")} />
      Read from the game {formatAge(age)}
    </span>
  );
}

function PageSkeleton() {
  return (
    <div className="mx-auto flex max-w-6xl flex-col gap-5 p-6">
      <Skeleton className="h-8 w-96" />
      <Skeleton className="h-24" />
      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
        {Array.from({ length: 6 }, (_, index) => (
          <Skeleton key={index} className="h-32" />
        ))}
      </div>
    </div>
  );
}

function Unavailable({ reason }: { reason: string }) {
  return (
    <div className="grid h-full place-items-center p-8">
      <Empty className="max-w-md border border-dashed">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <ScrollText />
          </EmptyMedia>
          <EmptyTitle className="font-display text-lg">Quests are unavailable</EmptyTitle>
          <EmptyDescription>{reason}</EmptyDescription>
        </EmptyHeader>
      </Empty>
    </div>
  );
}
