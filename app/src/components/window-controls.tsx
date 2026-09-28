import { useEffect, useState, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-react";

import { cn } from "@/lib/utils";

/** Minimize, maximize and close, drawn like Windows' own caption buttons. */
export function WindowControls() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const appWindow = getCurrentWindow();
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const refresh = () =>
      appWindow
        .isMaximized()
        .then((value) => !disposed && setMaximized(value))
        .catch(() => {});
    refresh();
    appWindow
      .onResized(refresh)
      .then((stop) => (disposed ? stop() : (unlisten = stop)))
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const appWindow = getCurrentWindow();
  return (
    <div className="flex h-full items-stretch">
      <CaptionButton label="Minimize" onClick={() => appWindow.minimize()}>
        <Minus className="size-4" />
      </CaptionButton>
      <CaptionButton label={maximized ? "Restore" : "Maximize"} onClick={() => appWindow.toggleMaximize()}>
        {maximized ? <Copy className="size-3.5 -scale-x-100" /> : <Square className="size-3.5" />}
      </CaptionButton>
      <CaptionButton label="Close" onClick={() => appWindow.close()} danger>
        <X className="size-4" />
      </CaptionButton>
    </div>
  );
}

function CaptionButton({
  label,
  onClick,
  danger,
  children,
}: {
  label: string;
  onClick: () => void;
  danger?: boolean;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className={cn(
        "grid w-11 place-items-center text-muted-foreground transition-colors",
        danger ? "hover:bg-[#c42b1c] hover:text-white" : "hover:bg-accent hover:text-foreground",
      )}
    >
      {children}
    </button>
  );
}
