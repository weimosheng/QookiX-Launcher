import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { useTasksStore } from "./stores/tasks";

type UpdateInfo = Awaited<ReturnType<typeof check>>;

export interface UpdateProgress {
  message: string;
  /** 0..1 download fraction while downloading */
  fraction?: number;
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
 * Download + install the pending update (if any). Reports progress through
 * `onStatus` and ALSO registers a task in the Download Center (`tasks` store)
 * so users can watch the download progress there. Returns `true` when an update
 * was downloaded and installed, in which case the caller should offer to
 * relaunch the app.
 */
export async function downloadAndInstall(onStatus?: ProgressFn): Promise<boolean> {
  const tasks = useTasksStore();
  const taskId = Date.now();
  let registered = false;

  const emit = (p: UpdateProgress) => {
    onStatus?.(p);
    if (!registered) return;
    tasks.upsert(taskId, (t) => {
      t.message = p.message;
      if (p.fraction != null) {
        t.activity = "download";
        t.fraction = p.fraction;
      }
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
    let downloaded = 0;
    let total = 0;
    await update.downloadAndInstall((event) => {
      const ev = event.event;
      if (ev === "Started") {
        total = event.data.contentLength ?? 0;
        downloaded = 0;
        emit({ message: "开始下载…" });
      } else if (ev === "Progress") {
        downloaded += event.data.chunkLength;
        const fraction = total > 0 ? downloaded / total : undefined;
        emit({
          message:
            fraction != null
              ? `下载中 ${Math.round(fraction * 100)}%…`
              : "下载中…",
          fraction,
        });
      } else if (ev === "Finished") {
        tasks.upsert(taskId, (t) => {
          t.activity = "install";
          t.fraction = 1;
        });
        emit({ message: "下载完成，正在安装…", fraction: 1 });
      }
    });

    tasks.upsert(taskId, (t) => {
      t.stage = "done";
      t.finished = true;
      t.ok = true;
      t.fraction = 1;
      t.message = `v${update.version} 已下载并安装，重启后生效`;
    });
    return true;
  } catch (err) {
    // Surface the real reason (404, signature mismatch, permissions...) so the
    // UI can show something actionable instead of a generic "update failed".
    console.error("[updater] downloadAndInstall failed:", err);
    const detail =
      err instanceof Error && err.message ? err.message : String(err);
    if (registered) {
      tasks.upsert(taskId, (t) => {
        t.finished = true;
        t.ok = false;
        t.fraction = 0;
        t.message = `更新失败：${detail}`;
      });
    }
    throw new Error(`更新失败：${detail}`);
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
