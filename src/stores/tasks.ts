import { defineStore } from "pinia";
import { listen } from "@tauri-apps/api/event";
import { useInstancesStore } from "./instances";
import type {
  ActiveFile,
  DownloadProgressEvent,
  InstallProgressEvent,
  LaunchLogEvent,
  LaunchStateEvent,
} from "../types";

interface LogEntry {
  stream: "out" | "err";
  line: string;
}

export interface TaskFile {
  name: string;
  ok: boolean;
}

export interface TaskEntry {
  id: number;
  stage: string;
  message: string;
  // download phase (files + bytes)
  fileDone: number;
  fileTotal: number;
  bytesDone: number;
  bytesTotal: number;
  // install phase (steps)
  stepDone: number;
  stepTotal: number;
  current?: string;
  lastCurrent?: string;
  activeFiles: ActiveFile[];
  speed: number; // bytes/sec (average over a rolling window)
  samples: { ts: number; bytes: number }[];
  files: TaskFile[];
  instanceId?: string;
  instanceName?: string;
  source?: string;
  startedAt: number;
  finished: boolean;
  ok?: boolean;
  activity: "download" | "install";
  /** 0..1 overall progress (used e.g. by the app updater, which has no per-file/byte accounting). */
  fraction?: number;
}

function nowMs() {
  return Date.now();
}

/** Install-side stages that are not pure downloads. */
const INSTALL_STAGES = ["manifest", "natives", "modpack-install", "done"];

export const useTasksStore = defineStore("tasks", {
  state: () => ({
    tasks: {} as Record<number, TaskEntry>,
    order: [] as number[],
    logs: {} as Record<string, LogEntry[]>,
    runningInstances: [] as string[],
    lastExit: null as { instanceId: string; code: number | null } | null,
  }),
  getters: {
    taskList(): TaskEntry[] {
      return this.order
        .map((id) => this.tasks[id])
        .filter(Boolean)
        .sort((a, b) => b.startedAt - a.startedAt);
    },
    activeCount(): number {
      return this.taskList.filter((t) => !t.finished).length;
    },
    gameRunning(): boolean {
      return this.runningInstances.length > 0;
    },
    runningInstance(): string | null {
      return this.runningInstances[0] ?? null;
    },
  },
  actions: {
    init() {
      listen<InstallProgressEvent>("install://progress", (e) => {
        const p = e.payload;
        this.upsert(p.taskId, (t) => {
          t.stage = p.stage;
          t.message = p.message;
          t.stepDone = p.done;
          t.stepTotal = p.total;
          t.activity = INSTALL_STAGES.includes(p.stage) ? "install" : "download";
          if (p.instanceId) t.instanceId = p.instanceId;
          if (p.instanceName) t.instanceName = p.instanceName;
          if (p.source) t.source = p.source;
          if (p.stage === "done") {
            t.finished = true;
            t.ok = p.ok !== false;
            t.speed = 0;
            // flush the last tracked file
            if (t.lastCurrent) {
              t.files.push({ name: t.lastCurrent, ok: t.ok ?? true });
              t.lastCurrent = undefined;
            }
          }
        });
      });
      listen<DownloadProgressEvent>("download://progress", (e) => {
        const p = e.payload;
        this.upsert(p.taskId, (t) => {
          t.activity = "download";
          t.stage = p.phase;
          t.fileDone = p.done;
          t.fileTotal = p.total;
          t.bytesDone = Math.max(t.bytesDone, p.bytesDone ?? 0);
          if (p.bytesTotal && p.bytesTotal > 0) t.bytesTotal = Math.max(t.bytesTotal, p.bytesTotal);
          t.ok = p.ok;
          if (p.activeFiles) {
            t.activeFiles = p.activeFiles;
          }
          if (p.current) {
            t.files.push({ name: p.current, ok: p.ok ?? true });
            if (t.files.length > 100) t.files.splice(0, t.files.length - 100);
            t.current = p.current;
          }
          // rolling average speed over the last ~5s with smoothing
          this.sampleSpeed(t, p.bytesDone ?? 0);
        });
      });
      listen<LaunchLogEvent>("launch://log", (e) => {
        const { instanceId, stream, line } = e.payload;
        if (!this.logs[instanceId]) this.logs[instanceId] = [];
        const buf = this.logs[instanceId];
        buf.push({ stream, line });
        if (buf.length > 4000) buf.splice(0, buf.length - 4000);
      });
      listen<LaunchStateEvent>("launch://state", (e) => {
        const p = e.payload;
        if (p.state === "running") {
          if (!this.runningInstances.includes(p.instanceId)) {
            this.runningInstances = [...this.runningInstances, p.instanceId];
          }
        } else {
          this.runningInstances = this.runningInstances.filter((id) => id !== p.instanceId);
          this.lastExit = { instanceId: p.instanceId, code: p.code };
          // 游戏退出后后端已累加游玩时长/更新 last_played，刷新实例列表让
          // 卡片上的「上次游玩/累计时长」立即反映最新数据（后端先写盘再发事件）。
          void useInstancesStore().load();
        }
      });
    },
    /**
     * Record a byte-counter sample and refresh the rolling-window average speed
     * (bytes/sec). Shared by the backend downloader and the in-frontend app
     * updater, which keeps its own byte counters.
     */
    sampleSpeed(t: TaskEntry, bytesDone: number) {
      const ts = nowMs();
      t.samples.push({ ts, bytes: bytesDone });
      t.samples = t.samples.filter((s) => ts - s.ts <= 5000);
      if (t.samples.length >= 3) {
        const first = t.samples[0];
        const last = t.samples[t.samples.length - 1];
        const dt = (last.ts - first.ts) / 1000;
        if (dt > 0.5) {
          const newSpeed = Math.max(0, (last.bytes - first.bytes) / dt);
          t.speed = t.speed > 0 ? t.speed * 0.7 + newSpeed * 0.3 : newSpeed;
        }
      }
      if (t.samples.length > 60) t.samples.splice(0, t.samples.length - 60);
    },
    upsert(id: number, patch: (t: TaskEntry) => void) {
      if (!this.tasks[id]) {
        this.tasks[id] = {
          id,
          stage: "",
          message: "",
          fileDone: 0,
          fileTotal: 0,
          bytesDone: 0,
          bytesTotal: 0,
          stepDone: 0,
          stepTotal: 0,
          activeFiles: [],
          speed: 0,
          samples: [],
          files: [],
          startedAt: nowMs(),
          finished: false,
          activity: "download",
        };
        this.order.push(id);
        if (this.order.length > 60) {
          const old = this.order.shift();
          if (old !== undefined) delete this.tasks[old];
        }
      }
      patch(this.tasks[id]);
    },
    clearFinished() {
      const keep = this.order.filter((id) => {
        const t = this.tasks[id];
        if (t && t.finished) {
          delete this.tasks[id];
          return false;
        }
        return true;
      });
      this.order = keep;
    },
    clearAll() {
      this.tasks = {};
      this.order = [];
    },
    clearLogs(instanceId: string) {
      this.logs[instanceId] = [];
    },
  },
});
