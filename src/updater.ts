import { ref } from "vue";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { useTasksStore } from "./stores/tasks";

type UpdateInfo = Awaited<ReturnType<typeof check>>;

/**
 * `true` once an update has been downloaded + installed and is only waiting for
 * a restart to take effect. The title bar shows a「重启以更新」button while this
 * is set — the app never restarts on its own.
 */
export const updateReady = ref(false);
/** Version that was just downloaded (shown in the title-bar button tooltip). */
export const updateReadyVersion = ref<string | null>(null);
/** `true` while `install()` is running (guards double clicks on the button). */
export const updateInstalling = ref(false);
/**
 * The update whose package has been downloaded and is waiting for the user to
 * confirm the install. Kept as module state because `Update` is a Tauri
 * resource — dropping it would release the downloaded bytes.
 */
let pending: UpdateInfo | null = null;

export interface UpdateProgress {
  message: string;
  /** 0..1 download fraction while downloading */
  fraction?: number;
  /** bytes downloaded so far (optional, used for the speed indicator) */
  bytesDone?: number;
  /** total bytes to download (optional) */
  bytesTotal?: number;
}
type ProgressFn = (p: UpdateProgress) => void;

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
 * Download the pending update (if any) **without installing it**.
 *
 * Important: on Windows, installing means running the NSIS installer, which
 * forcibly kills the running process (`taskkill` in `hooks.nsi`) and then
 * relaunches the app because the updater passes `/R`. So the install step must
 * NOT happen right after the download — otherwise the launcher silently
 * restarts itself and the user never gets a say. `Update` conveniently splits
 * `download()` and `install()`, so we only do the former here and let
 * `applyUpdateNow()` do the latter once the user clicks「重启以更新」.
 *
 * Reports progress through `onStatus` and ALSO registers a task in the Download
 * Center (`tasks` store). Returns `true` when an update was downloaded.
 */
export async function downloadUpdate(onStatus?: ProgressFn): Promise<boolean> {
  const tasks = useTasksStore();
  const taskId = Date.now();
  let registered = false;
  // The updater fires a Progress event for *every* network chunk, which can be
  // hundreds per second — throttle the store writes (and the resulting UI
  // re-renders) to ~4/s so the Download Center stays responsive.
  let lastPush = 0;

  const emit = (p: UpdateProgress, force = false) => {
    onStatus?.(p);
    if (!registered) return;
    const now = Date.now();
    if (!force && p.bytesDone != null && now - lastPush < 250) return;
    lastPush = now;
    tasks.upsert(taskId, (t) => {
      t.message = p.message;
      if (p.bytesTotal && p.bytesTotal > 0) {
        t.bytesTotal = Math.max(t.bytesTotal, p.bytesTotal);
      }
      if (p.bytesDone != null) {
        t.activity = "download";
        t.bytesDone = Math.max(t.bytesDone, p.bytesDone);
        // feed the shared rolling-window speed sampler, so the Download Center
        // shows a live MB/s figure just like the game downloads do
        tasks.sampleSpeed(t, p.bytesDone);
      }
      if (p.fraction != null) t.fraction = p.fraction;
    });
  };

  try {
    const update = await check();
    if (!update) return false;

    registered = true;
    tasks.upsert(taskId, (t) => {
      t.activity = "download";
      t.source = "启动器更新";
      t.stage = "download";
      t.message = `准备下载 v${update.version}…`;
    });

    // The plugin's Progress event only carries `chunkLength` (bytes of the
    // current callback), NOT a `progress` field — so accumulate the bytes and
    // derive the fraction from `contentLength` ourselves.
    // Hold on to the resource: `install()` (called from `applyUpdateNow`)
    // needs this exact instance to find the downloaded package.
    pending = update;

    let downloaded = 0;
    let total = 0;
    await update.download((event) => {
      const ev = event.event;
      if (ev === "Started") {
        total = event.data.contentLength ?? 0;
        downloaded = 0;
        emit({ message: "开始下载…", bytesDone: 0, bytesTotal: total }, true);
      } else if (ev === "Progress") {
        downloaded += event.data.chunkLength;
        const fraction = total > 0 ? downloaded / total : undefined;
        emit({
          message:
            fraction != null
              ? `下载中 ${Math.round(fraction * 100)}%…`
              : "下载中…",
          fraction,
          bytesDone: downloaded,
          bytesTotal: total,
        });
      } else if (ev === "Finished") {
        emit(
          { message: "下载完成，等待重启后安装…", fraction: 1, bytesDone: downloaded, bytesTotal: total },
          true
        );
      }
    });

    tasks.upsert(taskId, (t) => {
      t.stage = "done";
      t.finished = true;
      t.ok = true;
      t.fraction = 1;
      t.speed = 0;
      t.samples = [];
      t.message = `v${update.version} 已下载，点击标题栏「重启以更新」安装`;
    });
    // Downloaded, not installed: the user decides when to restart.
    updateReadyVersion.value = update.version;
    updateReady.value = true;
    return true;
  } catch (err) {
    // Surface the real reason (404, signature mismatch, permissions...) so the
    // UI can show something actionable instead of a generic "update failed".
    console.error("[updater] downloadUpdate failed:", err);
    const detail =
      err instanceof Error && err.message ? err.message : String(err);
    pending = null;
    if (registered) {
      tasks.upsert(taskId, (t) => {
        t.finished = true;
        t.ok = false;
        t.fraction = 0;
        t.speed = 0;
        t.samples = [];
        t.message = `更新失败：${detail}`;
      });
    }
    throw new Error(`更新失败：${detail}`);
  }
}

/**
 * Install the downloaded package and restart — what the title-bar
 *「重启以更新」button is wired to.
 *
 * Note: on Windows the installer kills this process as part of the install and
 * relaunches the app itself (the updater passes `/R`), so the `relaunchApp()`
 * below normally never runs. It is only a fallback for platforms/installers
 * that leave the process alive.
 */
export async function applyUpdateNow(): Promise<void> {
  if (updateInstalling.value) return;
  if (!pending) {
    // Nothing downloaded in this session — just restart.
    await relaunchApp();
    return;
  }
  updateInstalling.value = true;
  try {
    await pending.install();
    pending = null;
  } finally {
    updateInstalling.value = false;
  }
  await relaunchApp();
}

/** Relaunch the app (used after an update was installed). */
export async function relaunchApp(): Promise<void> {
  try {
    await relaunch();
  } catch {
    /* ignore */
  }
}
