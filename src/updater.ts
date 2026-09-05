import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { relaunch } from "@tauri-apps/plugin-process";
import { useTasksStore } from "./stores/tasks";
import { error as devError } from "./utils/logger";

/**
 * 应用自更新检查结果（由 Rust 端 `updater::check_for_update` 返回）。
 * `available` 为 `false` 表示当前所选更新源没有新版本。
 */
export interface UpdateInfo {
  available: boolean;
  version: string | null;
  currentVersion: string | null;
  body: string | null;
  downloadUrl: string | null;
  /** 实际使用的更新源："bucket" | "github" */
  source: "bucket" | "github";
}

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
 * Check whether an update is available on the **currently selected update
 * source** (存储桶 / GitHub). Returns the update info, or `null` when there is
 * nothing to update or the updater is not configured (errors are swallowed
 * silently so a network blip never blocks the launcher).
 */
export async function peekUpdate(strict = false): Promise<UpdateInfo | null> {
  try {
    const info = await invoke<UpdateInfo>("check_for_update");
    return info.available ? info : null;
  } catch (err) {
    // 默认吞掉错误：启动时的静默检查不希望网络抖动阻塞启动。
    // strict（如用户手动点「检查更新」）时把真实错误抛给调用方，
    // 以便区分「确实没有新版本」和「更新源不可用」，避免误报已是最新。
    if (strict) throw err;
    return null;
  }
}

/**
 * Download the pending update from the selected source **without installing
 * it**. The download+signature-check happen on the Rust side, which streams
 * progress back through the `update://progress` event.
 *
 * Important: on Windows, installing means running the NSIS installer, which
 * forcibly kills the running process and then relaunches the app (`/R`). So the
 * install step must NOT happen right after the download — `applyUpdateNow()`
 * does that only once the user clicks「重启以更新」.
 *
 * Reports progress through `onStatus` AND registers a task in the Download
 * Center (`tasks` store). Returns `true` when an update was downloaded.
 */
export async function downloadUpdate(onStatus?: ProgressFn): Promise<boolean> {
  const tasks = useTasksStore();
  const taskId = Date.now();
  let registered = false;
  // The updater emits a progress event for *every* network chunk, which can be
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

  let unlisten: UnlistenFn | undefined;
  let version: string | null = null;
  try {
    unlisten = await listen<{ downloaded: number; total: number | null }>(
      "app-update-progress",
      (event) => {
        const { downloaded, total } = event.payload;
        const fraction = total && total > 0 ? downloaded / total : undefined;
        emit({
          message:
            fraction != null
              ? `下载中 ${Math.round(fraction * 100)}%…`
              : "下载中…",
          fraction,
          bytesDone: downloaded,
          bytesTotal: total ?? undefined,
        });
      }
    );

    registered = true;
    tasks.upsert(taskId, (t) => {
      t.activity = "download";
      t.source = "启动器更新";
      t.stage = "download";
      t.message = "准备下载更新…";
    });

    const ok = await invoke<boolean>("download_update");
    if (!ok) return false;

    const info = await peekUpdate();
    version = info?.version ?? null;

    tasks.upsert(taskId, (t) => {
      t.stage = "done";
      t.finished = true;
      t.ok = true;
      t.fraction = 1;
      t.speed = 0;
      t.samples = [];
      t.message = `v${version ?? ""} 已下载，点击标题栏「重启以更新」安装`;
    });
    // Downloaded, not installed: the user decides when to restart.
    updateReadyVersion.value = version;
    updateReady.value = true;
    return true;
  } catch (err) {
    // Surface the real reason (404, signature mismatch, permissions...) so the
    // UI can show something actionable instead of a generic "update failed".
    devError("[updater] downloadUpdate failed:", err);
    const detail = err instanceof Error && err.message ? err.message : String(err);
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
  } finally {
    unlisten?.();
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
  updateInstalling.value = true;
  try {
    await invoke("apply_app_update");
  } catch (err) {
    // 没有已下载的更新（或安装失败）→ 兜底直接重启，避免卡在旧版本
    devError("[updater] apply_update failed:", err);
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
