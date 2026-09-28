import { useState, type ReactNode } from "react";
import { openPath, openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink, FolderOpen } from "lucide-react";
import { toast } from "sonner";

import { LiveDataAlert, useLiveStatus } from "@/components/live-data-alert";
import { UpdateButton } from "@/components/update-check";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { useAppStatus, useSetSetting, useSettings } from "@/lib/queries";

interface Imported {
  files: string[];
  at: number;
}

export function SettingsPage() {
  const { data: settings } = useSettings();
  const { data: status } = useAppStatus();
  const setSetting = useSetSetting();
  const closeToTray = (settings?.closeToTray as boolean | undefined) ?? true;
  const imported = settings?.importedFromDndtools as Imported | undefined;
  const { data: live } = useLiveStatus();
  const [updateText, setUpdateText] = useState("New versions come from the project's GitHub releases.");

  const open = (action: Promise<void>, what: string) =>
    action.catch((error: unknown) => toast.error(`Could not open ${what}`, { description: String(error) }));

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-8 p-6">
      <Section title="General">
        <Row
          title="Keep running in the tray"
          description="Closing the window leaves the app in the tray, so hover values keep working. Quit from the tray menu."
        >
          <Switch
            aria-label="Keep running in the tray"
            checked={closeToTray}
            onCheckedChange={(value) =>
              setSetting.mutate(
                { key: "closeToTray", value },
                { onError: (error) => toast.error("Could not save the setting", { description: String(error) }) },
              )
            }
          />
        </Row>
      </Section>

      <Section title="Live game data">
        <Row
          title={live?.state === "on" ? "On" : live?.state === "off" ? "Off" : "Starting"}
          description={
            live?.state === "on"
              ? `Reading the game's traffic on ${live.adapter ?? "the network adapter"}. Nothing is ever sent anywhere.`
              : live?.state === "off"
                ? (live.error ?? "Live game data is off.")
                : "Looking for the network adapter."
          }
        />
      </Section>
      <LiveDataAlert />

      <Section title="Data">
        <Row title="Data folder" description={status?.dataDir ?? "…"}>
          <Button
            variant="outline"
            size="sm"
            disabled={!status}
            onClick={() => status && open(openPath(status.dataDir), "the data folder")}
          >
            <FolderOpen />
            Open
          </Button>
        </Row>
        <Row
          title="Imported from DnDTools"
          description={
            imported
              ? `${imported.files.join(", ")} on ${new Date(imported.at * 1000).toLocaleString()}`
              : "Nothing yet. Market history and trained models are copied on the first run when DnDTools is installed."
          }
        />
      </Section>

      <Section title="About">
        <Row title={status?.name ?? "DaD Companion"} description={status ? `Version ${status.version}` : "…"}>
          <Button
            variant="outline"
            size="sm"
            disabled={!status}
            onClick={() => status && open(openUrl(`https://github.com/${status.repo}`), "the repository")}
          >
            <ExternalLink />
            Repository
          </Button>
        </Row>
        <Row title="Updates" description={updateText}>
          <UpdateButton onState={setUpdateText} />
        </Row>
      </Section>
    </div>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-3">
      <h2 className="font-display text-lg">{title}</h2>
      <div className="divide-y divide-border/60 overflow-hidden rounded-lg border bg-card">{children}</div>
    </section>
  );
}

function Row({ title, description, children }: { title: string; description?: string; children?: ReactNode }) {
  return (
    <div className="flex items-center gap-6 px-4 py-3.5">
      <div className="min-w-0 flex-1">
        <div className="text-[15px]">{title}</div>
        {description && <div className="mt-0.5 text-sm break-words text-muted-foreground">{description}</div>}
      </div>
      {children}
    </div>
  );
}
