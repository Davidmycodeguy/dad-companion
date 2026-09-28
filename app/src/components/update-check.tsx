import { useEffect, useState } from "react";
import { Download, RefreshCw } from "lucide-react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { findUpdate, installUpdate, type Update } from "@/lib/updates";

/** Wait after startup before asking GitHub, so the check never slows the app down. */
const STARTUP_CHECK_DELAY_MS = 8_000;

/** Offers an update once per start when GitHub has a newer release; silent otherwise. */
export function StartupUpdateCheck() {
  useEffect(() => {
    const timer = window.setTimeout(() => {
      findUpdate()
        .then((update) => {
          if (!update) return;
          toast(`Version ${update.version} is available`, {
            description: "Installs in the background, then the app restarts.",
            duration: Infinity,
            action: { label: "Install", onClick: () => void install(update) },
          });
        })
        .catch(() => {});
    }, STARTUP_CHECK_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, []);
  return null;
}

function install(update: Update): Promise<void> {
  const id = toast.loading(`Downloading version ${update.version}…`);
  return installUpdate(update, (share) => {
    if (share !== null) toast.loading(`Downloading version ${update.version}… ${Math.round(share * 100)}%`, { id });
  }).catch((error: unknown) => {
    toast.error("The update could not be installed", { id, description: String(error) });
  });
}

type CheckState = { kind: "idle" } | { kind: "checking" } | { kind: "latest" } | { kind: "found"; update: Update } | { kind: "failed"; error: string };

/** The Settings row's button: check GitHub now, then install what it found. */
export function UpdateButton({ onState }: { onState: (text: string) => void }) {
  const [state, setState] = useState<CheckState>({ kind: "idle" });
  const [installing, setInstalling] = useState(false);

  useEffect(() => {
    if (state.kind === "latest") onState("You have the latest version.");
    else if (state.kind === "found") onState(`Version ${state.update.version} is available.`);
    else if (state.kind === "failed") onState(`Could not check: ${state.error}`);
  }, [state, onState]);

  if (state.kind === "found") {
    return (
      <Button
        size="sm"
        disabled={installing}
        onClick={() => {
          setInstalling(true);
          void install(state.update).finally(() => setInstalling(false));
        }}
      >
        {installing ? <Spinner /> : <Download />}
        Install and restart
      </Button>
    );
  }
  return (
    <Button
      variant="outline"
      size="sm"
      disabled={state.kind === "checking"}
      onClick={() => {
        setState({ kind: "checking" });
        findUpdate()
          .then((update) => setState(update ? { kind: "found", update } : { kind: "latest" }))
          .catch((error: unknown) => setState({ kind: "failed", error: String(error) }));
      }}
    >
      {state.kind === "checking" ? <Spinner /> : <RefreshCw />}
      Check for updates
    </Button>
  );
}
