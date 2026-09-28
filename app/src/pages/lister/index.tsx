import { useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { ListChecks, Users } from "lucide-react";
import { toast } from "sonner";

import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Skeleton } from "@/components/ui/skeleton";
import { stashApi } from "@/lib/api";
import { listerApi, type ListerRules, type PlanEntry, type PlanResponse } from "@/lib/lister-api";
import { mergePriced } from "@/lib/lister";
import { DataPanel } from "@/pages/lister/data-panel";
import { MerchantPanel } from "@/pages/lister/merchant-panel";
import { PlanPanel } from "@/pages/lister/plan-panel";
import { RulesPanel } from "@/pages/lister/rules-panel";
import { RunPanel } from "@/pages/lister/run-panel";
import { StatusStrip } from "@/pages/lister/status-strip";
import {
  isRunning,
  useAutosaveRules,
  useListerAction,
  useListerRules,
  useListerSources,
  useListerStatus,
} from "@/pages/lister/use-lister";

export function ListerPage() {
  const { data: characters, isPending } = useQuery({ queryKey: ["characters"], queryFn: stashApi.characters });
  const { data: savedRules } = useListerRules();
  const { data: status } = useListerStatus();
  const [characterId, setCharacterId] = useState<string>();
  const [rules, setRules] = useState<ListerRules | null>(null);
  const [response, setResponse] = useState<PlanResponse | null>(null);
  const [ticked, setTicked] = useState<Set<string>>(new Set());
  const [recheck, setRecheck] = useState(true);
  const [building, setBuilding] = useState(false);
  // Each built plan gets a number; a pricing run remembers the plan it was started for, so its
  // result is merged into that plan only, and only once.
  const planId = useRef(0);
  const pricingFor = useRef<number | null>(null);

  const character = characterId ?? characters?.[0]?.id;
  const { data: sources = [] } = useListerSources(character);
  const running = isRunning(status);
  const canRun = status?.canRun ?? false;

  useEffect(() => {
    if (savedRules && !rules) setRules(savedRules);
  }, [savedRules, rules]);
  useAutosaveRules(rules);

  // A finished pricing run hands back prices for the entries it was given: they merge into the
  // plan it was started for (a plan rebuilt since then is left alone).
  const pricedPlan = status?.mode === "price" && status.state === "done" ? status.plan : null;
  useEffect(() => {
    if (!pricedPlan || pricingFor.current === null) return;
    const forCurrentPlan = pricingFor.current === planId.current;
    pricingFor.current = null;
    if (!forCurrentPlan) return;
    const skipped = new Set(pricedPlan.skipped.map((s) => s.uniqueId));
    setResponse((current) =>
      current ? { ...current, plan: mergePriced(current.plan, pricedPlan), needsGamePricing: false } : current,
    );
    setTicked((current) => new Set([...current].filter((id) => !skipped.has(id))));
  }, [pricedPlan]);

  const priceFromGame = useListerAction(
    (entries: PlanEntry[]) => listerApi.priceFromGame(entries).then(() => (pricingFor.current = planId.current)),
    "Could not price from the game",
  );
  const start = useListerAction(
    ({ entries, dryRun }: { entries: PlanEntry[]; dryRun: boolean }) => listerApi.start(character!, entries, dryRun, recheck),
    "Could not start listing",
  );
  const stop = useListerAction(() => listerApi.stop(), "Could not stop");
  const collect = useListerAction(() => listerApi.collect(), "Could not collect");
  const crawl = useListerAction((deep: boolean) => listerApi.crawl(deep), "Could not read the market");
  const hoverTest = useListerAction(() => listerApi.hoverTest(), "Could not start the hover test");
  const sell = useListerAction(
    ({ ids, dryRun }: { ids: string[]; dryRun: boolean }) => listerApi.sellToMerchant(character!, ids, dryRun),
    "Could not sell to the merchant",
  );

  const buildPlan = async () => {
    if (!character || !rules) return;
    setBuilding(true);
    try {
      const built = await listerApi.buildPlan(character, rules);
      planId.current += 1;
      setResponse(built);
      setTicked(new Set(built.plan.entries.map((e) => e.uniqueId)));
      setRecheck(rules.priceSource === "live");
    } catch (error) {
      toast.error("Could not build the plan", { description: String(error) });
    } finally {
      setBuilding(false);
    }
  };

  const tickedEntries = () => response?.plan.entries.filter((e) => ticked.has(e.uniqueId)) ?? [];
  const replaceEntry = (entry: PlanEntry) =>
    setResponse((current) =>
      current && {
        ...current,
        plan: { ...current.plan, entries: current.plan.entries.map((e) => (e.uniqueId === entry.uniqueId ? entry : e)) },
      },
    );

  if (isPending || !rules) return <Skeleton className="m-6 h-96" />;
  if (!characters?.length) return <NoCharacters />;

  return (
    <div className="mx-auto flex max-w-6xl flex-col gap-5 p-6">
      <StatusStrip listings={status?.listings} onCollect={() => collect.mutate(undefined)} busy={running || !canRun} />
      <div className="grid items-start gap-5 lg:grid-cols-[320px_minmax(0,1fr)]">
        <RulesPanel
          characters={characters}
          characterId={character}
          onCharacter={(id) => {
            setCharacterId(id);
            planId.current += 1;
            setResponse(null);
          }}
          sources={sources}
          rules={rules}
          onRules={setRules}
          onBuild={buildPlan}
          building={building}
          disabled={running}
        />
        <div className="flex min-w-0 flex-col gap-5">
          {status && <RunPanel status={status} />}
          {response ? (
            <>
              <PlanPanel
                response={response}
                ticked={ticked}
                onTicked={setTicked}
                onEntry={replaceEntry}
                recheck={recheck}
                onRecheck={setRecheck}
                running={running}
                canRun={canRun}
                onPriceFromGame={() => priceFromGame.mutate(tickedEntries())}
                onStart={(dryRun) => start.mutate({ entries: tickedEntries(), dryRun })}
                onStop={() => stop.mutate(undefined)}
              />
              {character && (
                <MerchantPanel
                  characterId={character}
                  response={response}
                  running={running}
                  canRun={canRun}
                  onSell={(ids, dryRun) => sell.mutate({ ids, dryRun })}
                />
              )}
            </>
          ) : (
            <NoPlanYet />
          )}
          <DataPanel
            running={running}
            canRun={canRun}
            onCrawl={(deep) => crawl.mutate(deep)}
            onHoverTest={() => hoverTest.mutate(undefined)}
          />
        </div>
      </div>
    </div>
  );
}

function NoPlanYet() {
  return (
    <Empty className="border border-dashed py-10">
      <EmptyHeader>
        <EmptyMedia variant="icon">
          <ListChecks />
        </EmptyMedia>
        <EmptyTitle className="font-display text-lg">No plan yet</EmptyTitle>
        <EmptyDescription>
          Set the rules and build a plan. You see every item, its price and the fees before anything is listed.
        </EmptyDescription>
      </EmptyHeader>
    </Empty>
  );
}

function NoCharacters() {
  return (
    <div className="grid h-full place-items-center p-8">
      <Empty className="max-w-md border border-dashed">
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <Users />
          </EmptyMedia>
          <EmptyTitle className="font-display text-lg">No characters yet</EmptyTitle>
          <EmptyDescription>Log in to the game with the app running: your stash shows up here, ready to list.</EmptyDescription>
        </EmptyHeader>
      </Empty>
    </div>
  );
}
