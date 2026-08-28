import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

type UpdateInfo = Awaited<ReturnType<typeof check>>;
type ProgressFn = (message: string) => void;

/**
 * Check whether an update is available (best-effort). Returns the update info,
 * or `null` when there is nothing to update or the updater is not configured
 * (e.g. a dev build without a signing key — errors are swallowed silently).
 */
export async function peekUpdate(): Promise<UpdateInfo | null> {
  try {
    return await check();
  } catch {
    return null;
  }
}

/**
 * Download + install the pending update (if any). Reports progress through
 * `onStatus`. Returns `true` when an update was downloaded and installed, in
 * which case the caller should offer to relaunch the app.
 */
export async function downloadAndInstall(onStatus?: ProgressFn): Promise<boolean> {
  try {
    const update = await check();
    if (!update) return false;
    onStatus?.(`正在下载 v${update.version}…`);
    await update.downloadAndInstall((event) => {
      if (!onStatus) return;
      const ev = event.event;
      if (ev === "Started") {
        onStatus("开始下载…");
      } else if (ev === "Progress") {
        const p = (event.data as { progress?: number } | undefined)?.progress ?? 0;
        onStatus(`下载进度 ${Math.round(p * 100)}%`);
      } else if (ev === "Finished") {
        onStatus("下载完成，正在安装…");
      }
    });
    return true;
  } catch {
    return false;
  }
}

/** Relaunch the app (used after an update was installed). */
export async function relaunchApp(): Promise<void> {
  try {
    await relaunch();
  } catch {
    /* ignore */
  }
}
