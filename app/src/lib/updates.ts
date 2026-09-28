import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";

export type { Update };

/** The newer release on GitHub, or null when this is the latest. */
export function findUpdate(): Promise<Update | null> {
  return check();
}

/** Downloads and installs `update`, reporting progress (0..1 once the size is known), then restarts the app. */
export async function installUpdate(update: Update, onProgress?: (share: number | null) => void): Promise<void> {
  let total: number | null = null;
  let done = 0;
  await update.downloadAndInstall((event) => {
    if (event.event === "Started") {
      total = event.data.contentLength ?? null;
    } else if (event.event === "Progress") {
      done += event.data.chunkLength;
      onProgress?.(total ? Math.min(1, done / total) : null);
    }
  });
  await relaunch();
}
