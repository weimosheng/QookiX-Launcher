<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch, type Ref } from "vue";
import { useI18n } from "vue-i18n";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useRouter } from "vue-router";
import {
  NButton,
  NInput,
  NInputNumber,
  NModal,
  NPopconfirm,
  NSelect,
  NSwitch,
  useDialog,
  useMessage,
} from "naive-ui";
import { api } from "../api";
import { useInstancesStore } from "../stores/instances";
import { useTasksStore } from "../stores/tasks";
import {
  IconCamera,
  IconChevronLeft,
  IconChevronRight,
  IconClose,
  IconGithub,
  IconPlay,
  IconRefresh,
  IconSliders,
  IconTrash,
  IconUploadCloud,
} from "../components/icons";
import { fmtDuration } from "../utils/format";
import type { PlaySession, TimelineShot } from "../types";

const { t } = useI18n();
const message = useMessage();
const dialog = useDialog();
const router = useRouter();
const instances = useInstancesStore();
const tasks = useTasksStore();

// ---- 状态 / 配置 ----
const loading = ref(true);
const connected = ref(false);
const account = ref("");
const repoName = ref("");
const enabled = ref(false);
const intervalSecs = ref(300);
const keepPerInstance = ref(500);
const onInterval = ref(true);
const onWorldEnter = ref(true);
const onWorldExit = ref(true);
const onDeath = ref(true);
const onAdvancement = ref(true);
const foregroundOnly = ref(true);
const keepLocalCopy = ref(true);
interface Trigger {
  id: string;
  label: string;
  pattern: string;
  enabled: boolean;
  cooldownSecs: number;
}
const custom = ref<Trigger[]>([]);

const intervalValue = ref(5);
const intervalUnit = ref<"sec" | "min" | "hour">("min");
const unitOptions = computed(() => [
  { label: t("timemachine.sec"), value: "sec" },
  { label: t("timemachine.min"), value: "min" },
  { label: t("timemachine.hour"), value: "hour" },
]);
const unitFactor = { sec: 1, min: 60, hour: 3600 } as const;

const quotaValue = ref<number | null>(500);

// ---- 截图列表（本地 + 云端合并）----
const shots = ref<TimelineShot[]>([]);
/** 时间轴条目 id → 可直接显示的地址（本地路径直出，云端条目下载后缓存） */
const thumbs = ref<Record<string, string>>({});
const busy = ref(false);
/** 正在删除的截图（该卡片按钮转圈，防重复点） */
const deletingShot = ref<string | null>(null);
/** 刚截到的新图：卡片上来一圈呼吸光 + "新"标，几秒后自己散掉 */
const freshIds = ref<Set<string>>(new Set());
/** 是否启用 GitHub 云保存（关掉＝纯本地，不需要授权） */
const cloudEnabled = ref(false);
const batchSize = ref(10);
/** 攒批张数的可编辑副本 */
const batchValue = ref<number | null>(10);
/** 待上传张数（所有实例合计） */
const pending = ref(0);
/** 正在手动上传待传截图 */
const flushing = ref(false);

/** 单次游玩会话（「章节」按它分组） */
const sessions = ref<PlaySession[]>([]);
/** 视图：回忆流（不分组的流畅瀑布） / 章节（按这次游玩） / 按天 */
const viewMode = ref<"stream" | "session" | "day">("stream");
const viewOptions = computed<{ label: string; value: "stream" | "session" | "day" }[]>(() => [
  { label: t("timemachine.viewStream"), value: "stream" },
  { label: t("timemachine.viewLog"), value: "session" },
  { label: t("timemachine.viewDay"), value: "day" },
]);
/** 控制台抽屉 */
const ctlOpen = ref(false);
/** 卡片密度：舒适 / 紧凑（记住上次的选择） */
const density = ref<"cozy" | "compact">(
  localStorage.getItem("tm.density") === "compact" ? "compact" : "cozy"
);
watch(density, (v) => localStorage.setItem("tm.density", v));

/** 筛选；空串 = 全部 */
const filterInstance = ref("");
const filterTrigger = ref("");

/**
 * 筛选指纹：拼进卡片 key，让筛选变化时卡片被当成**新元素**重新挂载，
 * 于是 `data-reveal` 那套错落淡入会重播一遍。
 * 不这么做的话，留下来的卡片元素没变、动画不会重跑，切换看起来就是"闪一下"。
 */
const filterFingerprint = computed(
  () => `${viewMode.value}|${filterTrigger.value}|${filterInstance.value}`
);

/** 正在运行的实例：以后端 `game_pids` 为准，再并上前端本次会话记下的（重启就丢） */
const runningIds = ref<string[]>([]);
const knownRunning = computed(
  () => new Set<string>([...runningIds.value, ...tasks.runningInstances])
);
async function refreshRunning() {
  try {
    runningIds.value = await api.timemachineRunningInstances();
  } catch {
    /* 查不到就退回前端自己记的那份 */
  }
}

function toDate(v: string | number | null | undefined): Date | null {
  if (v == null) return null;
  const d =
    typeof v === "number"
      ? new Date(v * 1000)
      : /^\d+$/.test(v)
        ? new Date(Number(v) * 1000)
        : new Date(v);
  return Number.isNaN(d.getTime()) ? null : d;
}

function fmtTime(v: string | number | null | undefined): string {
  const d = toDate(v);
  if (!d) return String(v ?? "-");
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}

function dayKey(v: string | number | null | undefined): string {
  const d = toDate(v);
  if (!d) return "-";
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/** "21:05" */
function clock(sec: number): string {
  return fmtTime(sec).slice(-5);
}

// ---- 触发类型的显示 ----
function triggerLabel(tr: string | null | undefined): string {
  if (!tr) return t("timemachine.trigger.manual");
  if (tr.startsWith("custom:")) {
    const id = tr.slice(7);
    return custom.value.find((c) => c.id === id)?.label ?? t("timemachine.trigger.custom");
  }
  return t(`timemachine.trigger.${tr}`);
}

/** 触发类型 → 语义色（时间轴上的"故事符号"：死亡红、进度金、进出世界青、自定义紫） */
function triggerClass(tr: string | null | undefined): "red" | "gold" | "teal" | "purple" | "gray" {
  if (tr === "death") return "red";
  if (tr === "advancement") return "gold";
  if (tr === "world_enter" || tr === "world_exit") return "teal";
  if (tr && tr.startsWith("custom:")) return "purple";
  return "gray";
}

// ---- 实例名 ----
function instanceName(id: string | null): string {
  if (!id) return "-";
  const inst = instances.instances.find((i) => i.id === id);
  return inst?.name ?? id;
}

const instanceOptions = computed<{ label: string; value: string }[]>(() => {
  const ids = new Set<string>();
  for (const s of shots.value) if (s.instanceId) ids.add(s.instanceId);
  for (const i of instances.instances) ids.add(i.id);
  const running = knownRunning.value;
  return [
    { label: t("timemachine.allInstances"), value: "" },
    ...[...ids].map((id) => ({
      // 标出哪个实例的游戏正在跑，方便挑对截图目标
      label: running.has(id)
        ? `${instanceName(id)}（${t("timemachine.runningTag")}）`
        : instanceName(id),
      value: id,
    })),
  ];
});

/** 出现过的触发类型（死亡 / 进度排前面，其余按名排序）→ 筛选 chips */
const triggerTypes = computed(() => {
  const set = new Set<string>();
  for (const s of shots.value) if (s.trigger) set.add(s.trigger);
  const rank = (tr: string) => {
    if (tr === "death") return 0;
    if (tr === "advancement") return 1;
    if (tr === "world_enter") return 2;
    if (tr === "world_exit") return 3;
    if (tr === "interval") return 4;
    if (tr.startsWith("custom:")) return 5;
    return 6;
  };
  return [...set].sort((a, b) => rank(a) - rank(b) || a.localeCompare(b));
});

const filteredShots = computed(() =>
  shots.value.filter(
    (s) =>
      (!filterInstance.value || s.instanceId === filterInstance.value) &&
      (!filterTrigger.value || s.trigger === filterTrigger.value)
  )
);

/** 页面副标题："142 张截图 · 3 个世界" */
const summaryCount = computed(() => ({
  shots: shots.value.length,
  worlds: new Set(shots.value.map((s) => s.instanceId)).size,
}));

/** 一场游玩的窗口（含"没记录结束"的封口规则） */
interface SessionWindow {
  s: PlaySession;
  end: number;
}

/**
 * 会话窗口，按开始时间正序。
 *
 * 没记录结束时间（被强杀 / 还在跑）时，用"下一场的开始"或"当前时间"封口，
 * 不然它会把后面所有截图都吞进去。
 */
function sessionWindows(): SessionWindow[] {
  const now = Math.floor(Date.now() / 1000);
  const list = [...sessions.value].sort((a, b) => a.startedAt - b.startedAt);
  return list.map((s, i) => ({ s, end: s.endedAt ?? (list[i + 1]?.startedAt ?? now) }));
}

/** 某场游玩里的全部截图（走**未筛选**的列表：封面的统计不该被筛选影响） */
function shotsOfWindow(w: SessionWindow): TimelineShot[] {
  return shots.value.filter(
    (x) => x.instanceId === w.s.instanceId && x.createdAt >= w.s.startedAt && x.createdAt <= w.end
  );
}

/** 一场游玩：把截图按会话时间窗口归组 */
interface SessionGroup {
  key: string;
  session: PlaySession | null;
  startedAt: number;
  endedAt: number | null;
  shots: TimelineShot[];
}

/**
 * 「章节」分组：截图落进哪一场游玩，看它的时间是否在该实例的会话窗口内。
 * 窗口外的截图（手动截图、功能上线前的旧图）统一归到"未归类"。
 *
 * 先按实例把窗口分桶：每张截图只跟自己实例的那几场比，不跟全部会话比
 * （会话多了以后 O(截图 × 全部会话) 会明显变慢）。
 */
const sessionGroups = computed<SessionGroup[]>(() => {
  const windows = sessionWindows();
  const byInstance = new Map<string, SessionWindow[]>();
  for (const w of windows) {
    const arr = byInstance.get(w.s.instanceId) ?? [];
    arr.push(w);
    byInstance.set(w.s.instanceId, arr);
  }
  const buckets = new Map<SessionWindow, TimelineShot[]>();
  const rest: TimelineShot[] = [];
  for (const shot of filteredShots.value) {
    let hit: SessionWindow | null = null;
    for (const w of byInstance.get(shot.instanceId) ?? []) {
      if (shot.createdAt >= w.s.startedAt && shot.createdAt <= w.end) hit = w;
    }
    if (!hit) {
      rest.push(shot);
      continue;
    }
    const arr = buckets.get(hit) ?? [];
    arr.push(shot);
    buckets.set(hit, arr);
  }
  const out: SessionGroup[] = windows
    .filter((w) => (buckets.get(w)?.length ?? 0) > 0)
    .map((w) => ({
      key: `s${w.s.id}`,
      session: w.s,
      startedAt: w.s.startedAt,
      endedAt: w.end,
      shots: buckets.get(w) ?? [],
    }))
    .reverse();
  if (rest.length) {
    out.push({ key: "orphan", session: null, startedAt: 0, endedAt: null, shots: rest });
  }
  return out;
});

/** 这批截图里某种触发发生了几次（死亡/进度…） */
function countTrigger(list: TimelineShot[], trigger: string): number {
  return list.filter((s) => s.trigger === trigger).length;
}

/** 两种分组视图（章节 / 按天）统一成的形态，模板只写一遍 */
interface TimelineGroup {
  key: string;
  title: string;
  /** 时段 "21:05 – 23:18" */
  range: string | null;
  /** "2.2 小时" */
  duration: string | null;
  instance: string | null;
  stats: string;
  shots: TimelineShot[];
}

const timelineGroups = computed<TimelineGroup[]>(() => {
  if (viewMode.value === "day") {
    return dayGroups.value.map((g) => ({
      key: `d${g.day}`,
      title: g.day,
      range: null,
      duration: null,
      instance: null,
      stats: t("timemachine.shotCount", { n: g.shots.length }),
      shots: g.shots,
    }));
  }
  return sessionGroups.value.map((g) => {
    const s = g.session;
    if (!s) {
      return {
        key: g.key,
        title: t("timemachine.orphanGroup"),
        range: null,
        duration: null,
        instance: null,
        stats: t("timemachine.shotCount", { n: g.shots.length }),
        shots: g.shots,
      };
    }
    const end = g.endedAt ?? s.startedAt;
    return {
      key: g.key,
      title: dayKey(s.startedAt),
      range: s.endedAt
        ? `${clock(s.startedAt)} – ${clock(s.endedAt)}`
        : `${clock(s.startedAt)} – ${t("timemachine.sessionOngoing")}`,
      duration: fmtDuration(Math.max(0, end - s.startedAt)),
      instance: instanceName(s.instanceId),
      stats: t("timemachine.sessionStats", {
        shots: g.shots.length,
        deaths: countTrigger(g.shots, "death"),
        adv: countTrigger(g.shots, "advancement"),
      }),
      shots: g.shots,
    };
  });
});

/** 回忆流：把所有截图摊平，遇到换天就插一条日期分隔（网格里横跨整行） */
type StreamItem =
  | { kind: "day"; key: string; label: string }
  | { kind: "shot"; key: string; shot: TimelineShot; i: number };

const streamItems = computed<StreamItem[]>(() => {
  const out: StreamItem[] = [];
  let last = "";
  let i = 0;
  for (const s of filteredShots.value) {
    const k = dayKey(s.createdAt);
    if (k !== last) {
      out.push({ kind: "day", key: `d-${k}`, label: k });
      last = k;
    }
    out.push({ kind: "shot", key: s.id, shot: s, i });
    i++;
  }
  return out;
});

// ---- 封面：最近一次冒险 ----
const heroShot = computed(() => shots.value[0] ?? null);
/**
 * 最新那张图所属的游玩会话。
 *
 * 注意**不能**从 `sessionGroups` 里找：那是按筛选后的列表分组的，一筛类型/实例，
 * 封面的时长和死亡数就跟着变了（甚至找不到）。这里直接按会话窗口算。
 */
const heroWindow = computed(() => {
  const h = heroShot.value;
  if (!h) return null;
  let hit: SessionWindow | null = null;
  for (const w of sessionWindows()) {
    if (w.s.instanceId !== h.instanceId) continue;
    if (h.createdAt >= w.s.startedAt && h.createdAt <= w.end) hit = w;
  }
  return hit;
});
const heroCover = computed(() => (heroShot.value ? thumbs.value[heroShot.value.id] ?? "" : ""));
const heroInfo = computed(() => {
  const h = heroShot.value;
  if (!h) return null;
  const w = heroWindow.value;
  if (w) {
    const list = shotsOfWindow(w);
    return {
      instance: instanceName(w.s.instanceId),
      shots: list.length,
      duration: fmtDuration(Math.max(0, w.end - w.s.startedAt)),
      deaths: countTrigger(list, "death"),
      adv: countTrigger(list, "advancement"),
      ongoing: !w.s.endedAt,
    };
  }
  // 没有会话记录（功能上线前的旧图）：退回按天计数，不给时长
  return {
    instance: instanceName(h.instanceId),
    shots: shots.value.filter(
      (s) => s.instanceId === h.instanceId && dayKey(s.createdAt) === dayKey(h.createdAt)
    ).length,
    duration: "",
    deaths: 0,
    adv: 0,
    ongoing: false,
  };
});

async function loadSessions() {
  try {
    sessions.value = (await api.playSessions(200)).sessions;
  } catch {
    /* 拉不到就都归到"未归类"，不影响看图 */
  }
}

/** 按天分组（倒序） */
const dayGroups = computed(() => {
  const map = new Map<string, TimelineShot[]>();
  for (const s of filteredShots.value) {
    const k = dayKey(s.createdAt);
    const list = map.get(k) ?? [];
    list.push(s);
    map.set(k, list);
  }
  return [...map.entries()]
    .sort((a, b) => b[0].localeCompare(a[0]))
    .map(([day, list]) => ({ day, shots: list }));
});

// ---- 滚动揭示 / 回到最新 ----
/**
 * 卡片滚进视口时错落淡入（不做 transform 常驻，动画结束即归还合成层）。
 *
 * 标记**必须用 `data-*` 属性，不能用 class**：卡片带动态 `:class="{ fresh }"`，
 * 而 Vue 更新动态 class 时是整条 `el.className = "..."` 覆盖写入的——手写的
 * class 会被一起抹掉。之前用 `.is-in` 时就踩了这个坑：新图的黄色呼吸光结束
 * （`fresh` 变回 false）那一刻，Vue 重写 className 把 `.is-in` 抹掉，卡片又
 * 回到 `[data-reveal] { opacity: 0 }`，于是"闪光一消失，卡片也跟着消失"。
 */
const revealIo =
  typeof IntersectionObserver !== "undefined"
    ? new IntersectionObserver(
        (ents) => {
          for (const e of ents) {
            if (e.isIntersecting) {
              (e.target as HTMLElement).dataset.in = "1";
              revealIo?.unobserve(e.target);
            }
          }
        },
        { rootMargin: "0px 0px -6% 0px", threshold: 0.04 }
      )
    : null;
const revealed = new WeakSet<Element>();

function reveal(el: unknown) {
  if (!(el instanceof HTMLElement) || el.dataset.in) return;
  if (revealed.has(el)) return;
  revealed.add(el);
  if (revealIo) revealIo.observe(el);
  else el.dataset.in = "1";
}

/** 封面滚出视野后，右下角浮出「回到最新」 */
const heroEl = ref<HTMLElement | null>(null);
const showJump = ref(false);
const heroIo =
  typeof IntersectionObserver !== "undefined"
    ? new IntersectionObserver((ents) => {
        const e = ents[0];
        if (e) showJump.value = !e.isIntersecting;
      }, { threshold: 0 })
    : null;
watch(heroEl, (el) => {
  if (el && heroIo) heroIo.observe(el);
});
function backToLatest() {
  heroEl.value?.scrollIntoView({ behavior: "smooth", block: "start" });
}

// ---- 沉浸式看图 ----
/** 打开的那张图的 id（作为真源：列表刷新后下标会漂，id 不会） */
const lbId = ref<string | null>(null);
const lbIndex = computed(() =>
  lbId.value ? filteredShots.value.findIndex((s) => s.id === lbId.value) : -1
);
const lbOpen = computed(() => lbIndex.value >= 0);
const lbItem = computed<TimelineShot | null>(() =>
  lbIndex.value >= 0 ? filteredShots.value[lbIndex.value] : null
);
const lbUrl = computed(() => (lbItem.value ? thumbs.value[lbItem.value.id] ?? "" : ""));
const lbTotal = computed(() => filteredShots.value.length);

/** 大图右侧「那一刻」的日志：本地条目自带，云端条目按需下载 shots.json 附件 */
const lbLog = ref("");
const lbLogLoading = ref(false);
watch(lbId, async (id) => {
  lbLog.value = "";
  const s = lbItem.value;
  if (!id || !s) return;
  if (s.logSnippet) {
    lbLog.value = s.logSnippet;
    return;
  }
  if (!s.releaseId) return;
  lbLogLoading.value = true;
  try {
    lbLog.value = await api.timemachineShotLog(s.releaseId, s.file);
  } catch {
    /* 拉不到就当这张没有日志 */
  } finally {
    lbLogLoading.value = false;
  }
});

async function stepLb(d: number) {
  const n = lbTotal.value;
  if (!n) return;
  const cur = lbIndex.value < 0 ? 0 : lbIndex.value;
  const next = filteredShots.value[(cur + d + n) % n];
  if (!next) return;
  lbId.value = next.id;
  if (!thumbs.value[next.id]) await ensureThumb(next);
}

function onKey(e: KeyboardEvent) {
  if (lbOpen.value) {
    if (e.key === "Escape") lbId.value = null;
    else if (e.key === "ArrowRight") void stepLb(1);
    else if (e.key === "ArrowLeft") void stepLb(-1);
    return;
  }
  if (ctlOpen.value && e.key === "Escape") ctlOpen.value = false;
}

// ---- 加载 ----
function fillFromStatus(s: Awaited<ReturnType<typeof api.timemachineStatus>>) {
  connected.value = s.connected;
  account.value = s.account;
  repoName.value = s.repoName;
  cloudEnabled.value = s.cloudEnabled;
  batchSize.value = s.batchSize;
  batchValue.value = s.batchSize;
  pending.value = s.pending;
  enabled.value = s.enabled;
  intervalSecs.value = s.intervalSecs;
  keepPerInstance.value = s.keepPerInstance;
  quotaValue.value = s.keepPerInstance;
  onInterval.value = s.onInterval;
  onWorldEnter.value = s.onWorldEnter;
  onWorldExit.value = s.onWorldExit;
  onDeath.value = s.onDeath;
  onAdvancement.value = s.onAdvancement;
  foregroundOnly.value = s.foregroundOnly;
  keepLocalCopy.value = s.keepLocalCopy;
  custom.value = s.custom ?? [];
  if (s.intervalSecs % 3600 === 0 && s.intervalSecs >= 3600) {
    intervalUnit.value = "hour";
    intervalValue.value = s.intervalSecs / 3600;
  } else if (s.intervalSecs % 60 === 0 && s.intervalSecs >= 60) {
    intervalUnit.value = "min";
    intervalValue.value = s.intervalSecs / 60;
  } else {
    intervalUnit.value = "sec";
    intervalValue.value = s.intervalSecs;
  }
}

async function loadStatus() {
  try {
    fillFromStatus(await api.timemachineStatus());
  } catch (e) {
    message.error(String(e));
  }
}

/** 列表读取失败的提示节流（同一错误 60 秒内只弹一次） */
let listErrKey = "";
let listErrAt = 0;
/** 首次加载不做"新截图"高亮，否则一进页面满屏都在闪 */
let firstLoad = true;

/** 读取时间轴（本地 + 云端合并）；`quiet` 用于首次打开（仓库可能还没建，不必弹错） */
async function loadShots(quiet = false) {
  try {
    const r = await api.timemachineListAll();
    const before = new Set(shots.value.map((s) => s.id));
    shots.value = [...r.shots].sort((a, b) => (b.createdAt ?? 0) - (a.createdAt ?? 0));
    if (firstLoad) firstLoad = false;
    else markFresh(before);
    void loadThumbs();
  } catch (e) {
    // 截了图却列不出来时必须说出来（之前这里静默吞掉，看起来就像"没上传成功"）
    if (quiet) return;
    const key = String(e);
    const now = Date.now();
    if (key === listErrKey && now - listErrAt < 60_000) return;
    listErrKey = key;
    listErrAt = now;
    message.error(t("timemachine.listFail", { error: key }));
  }
}

/** 新出现的截图亮一会儿（不弹 toast，只在卡片上做提示，避免打断） */
function markFresh(before: Set<string>) {
  const added = shots.value.map((s) => s.id).filter((id) => !before.has(id));
  if (!added.length) return;
  const next = new Set(freshIds.value);
  added.forEach((id) => next.add(id));
  freshIds.value = next;
  window.setTimeout(() => {
    const after = new Set(freshIds.value);
    added.forEach((id) => after.delete(id));
    freshIds.value = after;
  }, 6000);
}

/** 缩略图地址：本地条目直出（不下载），云端条目才回源下载 */
async function ensureThumb(s: TimelineShot) {
  if (thumbs.value[s.id]) return;
  if (s.localPath) {
    thumbs.value = { ...thumbs.value, [s.id]: convertFileSrc(s.localPath) };
    return;
  }
  if (!s.releaseId || !s.assetId) return;
  try {
    const p = await api.timemachineDownload(s.releaseId, s.assetId);
    thumbs.value = { ...thumbs.value, [s.id]: convertFileSrc(p) };
  } catch {
    /* 单张失败不影响其它 */
  }
}

async function loadThumbs() {
  // 本地条目先一次性就位（纯内存操作，秒出）
  for (const s of shots.value) {
    if (s.localPath && !thumbs.value[s.id]) {
      thumbs.value = { ...thumbs.value, [s.id]: convertFileSrc(s.localPath) };
    }
  }
  // 只有云端条目需要下载；先弄前 60 张，其余点开时再下
  const queue = shots.value.slice(0, 60).filter((s) => !thumbs.value[s.id]);
  const workers = Array.from({ length: 4 }, async () => {
    while (queue.length) {
      const s = queue.shift();
      if (s) await ensureThumb(s);
    }
  });
  await Promise.all(workers);
}

async function openShot(s: TimelineShot) {
  if (!thumbs.value[s.id]) {
    if (s.localPath) {
      thumbs.value = { ...thumbs.value, [s.id]: convertFileSrc(s.localPath) };
    } else if (s.releaseId && s.assetId) {
      try {
        const p = await api.timemachineDownload(s.releaseId, s.assetId);
        thumbs.value = { ...thumbs.value, [s.id]: convertFileSrc(p) };
      } catch (e) {
        message.error(String(e));
        return;
      }
    }
  }
  if (!thumbs.value[s.id]) {
    message.error(t("timemachine.noImage"));
    return;
  }
  lbId.value = s.id;
}

/** 删除一张记忆点（本地文件 + 已上传的云端副本），删前二次确认 */
function removeShot(s: TimelineShot) {
  dialog.warning({
    title: t("timemachine.deleteShot"),
    content: t("timemachine.deleteShotConfirm", { time: fmtTime(s.createdAt) }),
    positiveText: t("common.delete"),
    negativeText: t("common.cancel"),
    onPositiveClick: async () => {
      deletingShot.value = s.id;
      try {
        await api.timemachineDeleteShot(s.instanceId, s.file, s.releaseId, s.assetId);
        shots.value = shots.value.filter((x) => x.id !== s.id);
        if (lbId.value === s.id) lbId.value = null;
        const next = { ...thumbs.value };
        delete next[s.id];
        thumbs.value = next;
        message.success(t("timemachine.deleted"));
        void loadStatus();
      } catch (e) {
        message.error(String(e));
      } finally {
        deletingShot.value = null;
      }
    },
  });
}

// ---- 配置 ----
async function saveConfig(patch: Record<string, unknown>) {
  try {
    await api.timemachineSetConfig(patch);
  } catch (e) {
    message.error(String(e));
    await loadStatus();
  }
}

async function toggleEnabled(v: boolean) {
  enabled.value = v;
  await saveConfig({ enabled: v });
  message.success(v ? t("timemachine.enabledOn") : t("timemachine.enabledOff"));
}

async function applyInterval() {
  const secs = Math.max(10, Math.round((intervalValue.value || 5) * unitFactor[intervalUnit.value]));
  intervalSecs.value = secs;
  await saveConfig({ intervalSecs: secs });
  message.success(t("timemachine.intervalOk"));
}

async function applyQuota() {
  const n = Math.max(1, Math.round(quotaValue.value || 500));
  keepPerInstance.value = n;
  await saveConfig({ keepPerInstance: n });
  message.success(t("timemachine.quotaOk"));
}

/** 打开/关闭 GitHub 云保存：关掉就是纯本地，不联网也不需要授权 */
async function toggleCloud(v: boolean) {
  cloudEnabled.value = v;
  await saveConfig({ cloudEnabled: v });
  message.success(v ? t("timemachine.cloudOn") : t("timemachine.cloudOff"));
  await loadStatus();
}

async function applyBatch() {
  const n = Math.min(500, Math.max(10, Math.round(batchValue.value || 10)));
  batchValue.value = n;
  batchSize.value = n;
  await saveConfig({ batchSize: n });
  message.success(t("timemachine.batchOk"));
}

/** 手动把攒着的截图传上去（不够一批也传） */
async function flushNow() {
  flushing.value = true;
  try {
    const r = await api.timemachineFlush();
    if (r.uploaded > 0) message.success(t("timemachine.uploadedN", { n: r.uploaded }));
    else if (r.failures.length) message.error(r.failures.join("；"));
    else message.info(t("timemachine.nothingToUpload"));
    await loadStatus();
    await loadShots();
  } catch (e) {
    message.error(String(e));
  } finally {
    flushing.value = false;
  }
}

async function initRepo() {
  busy.value = true;
  try {
    const r = await api.timemachineInitRepo();
    repoName.value = r.repo;
    message.success(t("timemachine.repoReady", { repo: r.repo }));
    await loadShots();
  } catch (e) {
    message.error(String(e));
  } finally {
    busy.value = false;
  }
}

/**
 * 测试截图的目标实例：筛选指定的优先，否则只挑正在运行的那个（截图靠给游戏窗口按 F2，
 * 没在跑的实例必然失败）。挑不出就返回空串，交给后端按 `game_pids` 决定。
 * 坑：不能用 `filterInstance.value ?? ...` —— 筛「全部实例」时它是空串，`??` 不兜空串。
 */
const captureInstance = computed(() => {
  if (filterInstance.value) return filterInstance.value;
  const known = knownRunning.value;
  const running = [...known]
    .filter((id) => instances.instances.some((i) => i.id === id))
    .sort(); // 与后端 running_instance_ids() 同序，免得提示的实例与实际截图对象不一致
  return running[0] || "";
});

async function testCapture() {
  if (!instances.instances.length) {
    message.warning(t("timemachine.noInstance"));
    return;
  }
  // 先问后端谁真的在跑（前端那份重启后就是空的）
  await refreshRunning();
  const id = captureInstance.value;
  // 明确筛了某个实例、但它没在跑：先提示，别去撞后端的错
  if (id && knownRunning.value.size && !knownRunning.value.has(id)) {
    message.warning(t("timemachine.notRunning", { name: instanceName(id) }));
    return;
  }
  busy.value = true;
  try {
    const r = await api.timemachineCapture(id, "manual"); // 空串 = 后端自动挑正在运行的
    if (r.uploadError) {
      // 本地已经存好了，只是没传上云——别让用户以为截图丢了
      message.warning(t("timemachine.captureSavedUploadFail", { error: r.uploadError }));
    } else if (r.uploaded > 0) {
      message.success(t("timemachine.captureUploaded", { n: r.uploaded }));
    } else {
      message.success(t("timemachine.captureSaved", { n: r.pending, batch: batchSize.value }));
    }
    await loadStatus();
    await loadShots();
  } catch (e) {
    // 带上实例名：多实例时能立刻看出是不是挑错了目标
    const who = id ? `（${instanceName(id)}）` : "";
    message.error(`${t("timemachine.captureFail")}${who}：${e}`);
  } finally {
    busy.value = false;
  }
}

// ---- 自定义触发器 ----
const adding = ref(false);
const draft = ref<Trigger>({ id: "", label: "", pattern: "", enabled: true, cooldownSecs: 30 });

function startAdd() {
  draft.value = { id: "", label: "", pattern: "", enabled: true, cooldownSecs: 30 };
  adding.value = true;
}
async function commitAdd() {
  if (!draft.value.pattern.trim()) {
    message.warning(t("timemachine.needPattern"));
    return;
  }
  const item: Trigger = {
    ...draft.value,
    id: draft.value.id.trim() || `t${Date.now()}`,
    label: draft.value.label.trim() || draft.value.pattern.slice(0, 20),
  };
  const next = [...custom.value, item];
  await saveConfig({ custom: next });
  custom.value = next;
  adding.value = false;
  message.success(t("timemachine.ruleAdded"));
}
async function removeTrigger(id: string) {
  const next = custom.value.filter((c) => c.id !== id);
  await saveConfig({ custom: next });
  custom.value = next;
  message.success(t("timemachine.ruleRemoved"));
}
async function toggleTrigger(c: Trigger, v: boolean) {
  c.enabled = v;
  await saveConfig({ custom: [...custom.value] });
}

const BUILTIN = computed(() => [
  { key: "onInterval", label: t("timemachine.tgInterval"), model: onInterval },
  { key: "onWorldEnter", label: t("timemachine.tgWorldEnter"), model: onWorldEnter },
  { key: "onWorldExit", label: t("timemachine.tgWorldExit"), model: onWorldExit },
  { key: "onDeath", label: t("timemachine.tgDeath"), model: onDeath },
  { key: "onAdvancement", label: t("timemachine.tgAdvancement"), model: onAdvancement },
]);

/** key → 本地 ref（键名与后端配置字段同名） */
const BUILTIN_MODELS: Record<string, Ref<boolean>> = {
  onInterval,
  onWorldEnter,
  onWorldExit,
  onDeath,
  onAdvancement,
};

async function toggleBuiltin(key: string, v: boolean) {
  // 开关是单向绑定（:value），保存后必须自己改本地状态，否则点了一下看不出变化
  const target = BUILTIN_MODELS[key];
  if (target) target.value = v;
  await saveConfig({ [key]: v });
}

// ---- 游戏里截了一张 → 节流刷新 ----
let lastShotReload = 0;
let unlisten: (() => void) | null = null;
let unlistenErr: (() => void) | null = null;
/** 引擎失败提示的节流：同一实例 + 同一错误，30 秒内只弹一次（引擎每 500ms 一轮） */
let lastErrKey = "";
let lastErrAt = 0;

/** 从游戏切回启动器（窗口重新获得焦点）时刷新一次，截图目标跟着变 */
const onWindowFocus = () => void refreshRunning();

onMounted(async () => {
  loading.value = true;
  if (!instances.instances.length) await instances.refresh();
  await loadStatus();
  await refreshRunning();
  // 本地模式没有云端也要列本地截图，所以这里无条件加载
  await Promise.all([loadShots(true), loadSessions()]);
  loading.value = false;
  window.addEventListener("focus", onWindowFocus);
  window.addEventListener("keydown", onKey);
  unlisten = await listen("timemachine://shot", () => {
    const now = Date.now();
    if (now - lastShotReload < 5000) return;
    lastShotReload = now;
    void loadShots();
    void loadSessions();
  });
  // 自动截图（定时 / 日志事件）失败时后端只打日志，用户完全看不到 —— 这里顶出来
  unlistenErr = await listen<{ instanceId: string; trigger: string; message: string }>(
    "timemachine://error",
    (e) => {
      const key = `${e.payload.instanceId}|${e.payload.message}`;
      const now = Date.now();
      if (key === lastErrKey && now - lastErrAt < 30_000) return;
      lastErrKey = key;
      lastErrAt = now;
      message.error(
        `${t("timemachine.captureFail")}（${instanceName(e.payload.instanceId)}）：${e.payload.message}`
      );
    }
  );
});
onBeforeUnmount(() => {
  window.removeEventListener("focus", onWindowFocus);
  window.removeEventListener("keydown", onKey);
  revealIo?.disconnect();
  heroIo?.disconnect();
  unlisten?.();
  unlistenErr?.();
});
</script>

<template>
  <div class="tm">
    <!-- 顶部工具栏：视图切换 + 快捷操作 + 控制台 -->
    <header class="tm-top">
      <div class="tm-title">
        <span class="tm-name">{{ t("nav.timemachine") }}</span>
        <span v-if="shots.length" class="tm-sub">
          {{ t("timemachine.summary", summaryCount) }}
        </span>
      </div>
      <div class="tm-tools">
        <button
          v-if="pending > 0 && cloudEnabled"
          class="btn pending"
          :class="{ spin: flushing }"
          :title="t('timemachine.pendingN', { n: pending })"
          :disabled="flushing"
          @click="flushNow"
        >
          <IconUploadCloud />
          {{ t("timemachine.pendingN", { n: pending }) }}
        </button>
        <div class="seg">
          <button
            v-for="v in viewOptions"
            :key="v.value"
            class="seg-i"
            :class="{ on: viewMode === v.value }"
            @click="viewMode = v.value"
          >
            {{ v.label }}
          </button>
        </div>
        <button class="btn ic" :title="t('timemachine.refresh')" :disabled="busy" @click="loadShots()">
          <IconRefresh />
        </button>
        <button
          class="btn"
          :disabled="busy"
          :title="
            captureInstance
              ? t('timemachine.captureTarget', { name: instanceName(captureInstance) })
              : t('timemachine.captureAuto')
          "
          @click="testCapture"
        >
          <IconCamera /> {{ t("timemachine.testCapture") }}
        </button>
        <button class="btn" @click="ctlOpen = true">
          <IconSliders /> {{ t("timemachine.console") }}
        </button>
      </div>
    </header>

    <!-- 开了云保存但还没授权：引导先连 GitHub（纯本地模式不需要授权） -->
    <section v-if="!loading && cloudEnabled && !connected" class="gate">
      <div class="gate-ico"><IconGithub /></div>
      <p class="gate-t">{{ t("timemachine.needGithub") }}</p>
      <p class="gate-p">{{ t("timemachine.needGithubHint") }}</p>
      <div class="gate-actions">
        <button class="btn" @click="router.push('/settings')">{{ t("nav.settings") }}</button>
        <button class="btn primary" @click="loadStatus">{{ t("timemachine.retry") }}</button>
      </div>
    </section>

    <template v-else>
      <!-- 封面：最近一次冒险 -->
      <section v-if="heroShot && heroInfo" ref="heroEl" class="hero">
        <img v-if="heroCover" :src="heroCover" class="hero-img" alt="" />
        <div v-else class="hero-ph"></div>
        <div class="hero-veil"></div>
        <div class="hero-body">
          <span class="hero-tag">
            {{ t("timemachine.heroLatest") }}
            <em v-if="heroInfo.ongoing">{{ t("timemachine.sessionOngoing") }}</em>
          </span>
          <h3 class="hero-h">
            {{ fmtTime(heroShot.createdAt).slice(0, 10) }}
            <span class="hero-sep">·</span>
            {{ heroInfo.instance }}
          </h3>
          <div class="hero-chips">
            <span class="hero-chip">{{ t("timemachine.heroShots", { n: heroInfo.shots }) }}</span>
            <span v-if="heroInfo.duration" class="hero-chip">{{ heroInfo.duration }}</span>
            <span v-if="heroInfo.deaths" class="hero-chip danger">
              {{ t("timemachine.heroDeaths", { n: heroInfo.deaths }) }}
            </span>
            <span v-if="heroInfo.adv" class="hero-chip gold">
              {{ t("timemachine.heroAdv", { n: heroInfo.adv }) }}
            </span>
          </div>
          <div class="hero-actions">
            <button class="btn hero-btn" @click="openShot(heroShot)">
              <IconPlay /> {{ t("timemachine.heroOpen") }}
            </button>
          </div>
        </div>
      </section>

      <!-- 空态 -->
      <section v-else-if="!loading" class="empty">
        <div class="empty-ico"><IconCamera /></div>
        <p class="empty-t">{{ t("timemachine.emptyTitle") }}</p>
        <div class="steps">
          <span><b>1</b> {{ t("timemachine.step1") }}</span>
          <span><b>2</b> {{ t("timemachine.step2") }}</span>
          <span><b>3</b> {{ t("timemachine.step3") }}</span>
        </div>
        <button class="btn" @click="ctlOpen = true">
          <IconSliders /> {{ t("timemachine.console") }}
        </button>
      </section>

      <!-- 筛选 chips -->
      <div v-if="shots.length" class="filters">
        <button class="chip" :class="{ on: !filterTrigger }" @click="filterTrigger = ''">
          {{ t("timemachine.allTriggers") }}
        </button>
        <button
          v-for="tr in triggerTypes"
          :key="tr"
          class="chip"
          :class="[triggerClass(tr), { on: filterTrigger === tr }]"
          @click="filterTrigger = filterTrigger === tr ? '' : tr"
        >
          {{ triggerLabel(tr) }}
        </button>
        <span class="filters-sep"></span>
        <NSelect
          v-model:value="filterInstance"
          size="small"
          style="width: 168px"
          :options="instanceOptions"
          :placeholder="t('timemachine.filterInstance')"
        />
        <span class="filters-count">{{ t("timemachine.timeline", { n: filteredShots.length }) }}</span>
        <button
          class="chip dens"
          :title="t('timemachine.densityCozy') + ' / ' + t('timemachine.densityCompact')"
          @click="density = density === 'cozy' ? 'compact' : 'cozy'"
        >
          {{ density === "cozy" ? t("timemachine.densityCompact") : t("timemachine.densityCozy") }}
        </button>
      </div>

      <!-- 回忆流：不分组，遇到换天插一条日期分隔 -->
      <div v-if="viewMode === 'stream'" class="stream" :class="density">
        <template v-for="it in streamItems" :key="`${it.key}@${filterFingerprint}`">
          <div v-if="it.kind === 'day'" class="day-div">{{ it.label }}</div>
          <article
            v-else
            class="shot"
            :class="{ fresh: freshIds.has(it.shot.id) }"
            data-reveal
            :ref="reveal"
            :style="{ '--d': `${(it.i % 12) * 28}ms` }"
          >
            <div class="shot-pic">
              <img
                v-if="thumbs[it.shot.id]"
                :src="thumbs[it.shot.id]"
                alt=""
                loading="lazy"
                @click="openShot(it.shot)"
              />
              <div v-else class="shot-skel" @click="openShot(it.shot)"></div>
              <span class="shot-badge" :class="triggerClass(it.shot.trigger)">
                {{ triggerLabel(it.shot.trigger) }}
              </span>
              <span v-if="freshIds.has(it.shot.id)" class="shot-new">
                {{ t("timemachine.newShot") }}
              </span>
              <span v-if="!it.shot.uploaded" class="shot-local">
                {{ t("timemachine.localOnly") }}
              </span>
              <button class="shot-del" :title="t('timemachine.deleteShot')" @click.stop="removeShot(it.shot)">
                <IconTrash />
              </button>
            </div>
            <div class="shot-meta">
              <span>{{ clock(it.shot.createdAt) }}</span>
              <span class="shot-inst">{{ instanceName(it.shot.instanceId) }}</span>
            </div>
          </article>
        </template>
      </div>

      <!-- 章节 / 按天：带时间轴主轴的分组 -->
      <div v-else class="timeline" :class="density">
        <div
          v-for="g in timelineGroups"
          :key="`${g.key}@${filterFingerprint}`"
          class="tl-group"
          data-reveal
          :ref="reveal"
        >
          <span class="tl-dot"></span>
          <div class="tl-head">
            <span class="tl-h">{{ g.title }}</span>
            <span v-if="g.range" class="tl-range">{{ g.range }}</span>
            <span v-if="g.duration" class="tl-dur">{{ g.duration }}</span>
            <span v-if="g.instance" class="tl-inst">{{ g.instance }}</span>
            <span class="tl-stats">{{ g.stats }}</span>
          </div>
          <div class="shots">
            <article
              v-for="(s, si) in g.shots"
              :key="s.id"
              class="shot"
              :class="{ fresh: freshIds.has(s.id) }"
              :style="{ '--i': si }"
            >
              <div class="shot-pic">
                <img
                  v-if="thumbs[s.id]"
                  :src="thumbs[s.id]"
                  alt=""
                  loading="lazy"
                  @click="openShot(s)"
                />
                <div v-else class="shot-skel" @click="openShot(s)"></div>
                <span class="shot-badge" :class="triggerClass(s.trigger)">
                  {{ triggerLabel(s.trigger) }}
                </span>
                <span v-if="freshIds.has(s.id)" class="shot-new">
                  {{ t("timemachine.newShot") }}
                </span>
                <span v-if="!s.uploaded" class="shot-local">{{ t("timemachine.localOnly") }}</span>
                <button class="shot-del" :title="t('timemachine.deleteShot')" @click.stop="removeShot(s)">
                  <IconTrash />
                </button>
              </div>
              <div class="shot-meta">
                <span>{{ clock(s.createdAt) }}</span>
                <span class="shot-inst">{{ instanceName(s.instanceId) }}</span>
              </div>
            </article>
          </div>
        </div>
      </div>
    </template>

    <!-- 回到最新 -->
    <transition name="pop">
      <button v-if="showJump" class="to-top" @click="backToLatest">
        ↑ {{ t("timemachine.jumpLatest") }}
      </button>
    </transition>
  <!-- 控制台：跟全项目一致用弹窗 -->
  <!-- 浮层必须留在这个根 div 内部：本页外面套着 <Transition mode="out-in">，
       组件多根时它拿不到 leave 结束信号，会导致切走本页后所有页面空白。 -->
  <NModal
    v-model:show="ctlOpen"
    preset="card"
    style="width: 620px; max-width: 92vw"
    :title="t('timemachine.console')"
  >
    <div class="ctl-repo">
      {{ cloudEnabled ? `${account} / ${repoName}` : t("timemachine.localMode") }}
    </div>

    <div class="ctl-body">
      <div class="row-line">
        <span class="row-label">{{ t("timemachine.enable") }}</span>
        <NSwitch :value="enabled" @update:value="toggleEnabled" />
      </div>

      <div class="row-line">
        <span class="row-label">{{ t("timemachine.cloudSave") }}</span>
        <NSwitch :value="cloudEnabled" @update:value="toggleCloud" />
      </div>

      <div v-if="cloudEnabled" class="row-line">
        <span class="row-label">{{ t("timemachine.batchSize") }}</span>
        <div class="inline-ctrl">
          <NInputNumber
            v-model:value="batchValue"
            size="small"
            :min="10"
            :max="500"
            style="width: 92px"
          />
          <NButton size="small" @click="applyBatch">{{ t("timemachine.apply") }}</NButton>
        </div>
      </div>

      <div class="row-line">
        <span class="row-label">{{ t("timemachine.interval") }}</span>
        <div class="inline-ctrl">
          <NInputNumber v-model:value="intervalValue" size="small" :min="1" style="width: 92px" />
          <NSelect
            v-model:value="intervalUnit"
            size="small"
            :options="unitOptions"
            style="width: 92px"
          />
          <NButton size="small" @click="applyInterval">{{ t("timemachine.apply") }}</NButton>
        </div>
      </div>

      <div class="row-line">
        <span class="row-label">{{ t("timemachine.keepQuota") }}</span>
        <div class="inline-ctrl">
          <NInputNumber v-model:value="quotaValue" size="small" :min="1" style="width: 92px" />
          <NButton size="small" @click="applyQuota">{{ t("timemachine.apply") }}</NButton>
        </div>
      </div>

      <div class="row-line">
        <span class="row-label">{{ t("timemachine.foregroundOnly") }}</span>
        <NSwitch
          :value="foregroundOnly"
          @update:value="
            (v: boolean) => {
              foregroundOnly = v;
              saveConfig({ foregroundOnly: v });
            }
          "
        />
      </div>
      <div class="row-line">
        <span class="row-label">{{ t("timemachine.keepLocal") }}</span>
        <NSwitch
          :value="keepLocalCopy"
          @update:value="
            (v: boolean) => {
              keepLocalCopy = v;
              saveConfig({ keepLocalCopy: v });
            }
          "
        />
      </div>

      <div class="sub-title">{{ t("timemachine.triggers") }}</div>
      <div class="triggers">
        <label v-for="b in BUILTIN" :key="b.key" class="tg">
          <NSwitch
            size="small"
            :value="b.model.value"
            @update:value="(v: boolean) => toggleBuiltin(b.key, v)"
          />
          <span>{{ b.label }}</span>
        </label>
      </div>

      <div class="sub-title">
        {{ t("timemachine.customTitle") }}
        <NButton quaternary size="tiny" type="primary" @click="startAdd">
          + {{ t("timemachine.addRule") }}
        </NButton>
      </div>
      <div v-if="!custom.length" class="hint">
        {{ t("timemachine.customEmpty") }}
        <code>\[CHAT\].*进入末地</code>
      </div>
      <div v-else class="custom-list">
        <div v-for="c in custom" :key="c.id" class="custom-row">
          <NSwitch
            size="small"
            :value="c.enabled"
            @update:value="(v: boolean) => toggleTrigger(c, v)"
          />
          <div class="c-info">
            <div class="c-name">{{ c.label }}</div>
            <code class="c-pattern">{{ c.pattern }}</code>
          </div>
          <span class="c-cool">{{ c.cooldownSecs }}s</span>
          <NPopconfirm @positive-click="removeTrigger(c.id)">
            <template #trigger>
              <NButton quaternary size="tiny" type="error">
                <IconTrash />
              </NButton>
            </template>
            {{ t("common.delete") }}?
          </NPopconfirm>
        </div>
      </div>
    </div>

    <template #footer>
      <div class="ctl-actions">
        <NButton v-if="cloudEnabled" size="small" :disabled="busy" @click="initRepo">
          {{ t("timemachine.initRepo") }}
        </NButton>
        <NButton size="small" type="primary" @click="ctlOpen = false">
          {{ t("common.close") }}
        </NButton>
      </div>
    </template>
  </NModal>

  <!-- 沉浸式看图：大图 + 那一刻的日志 + ← → 翻页 -->
  <Teleport to="body">
    <transition name="lb">
      <div v-if="lbOpen" class="lb">
        <button class="lb-x" @click="lbId = null"><IconClose /></button>
        <div class="lb-stage">
          <button class="lb-nav prev" :title="t('timemachine.lbPrev')" @click="stepLb(-1)">
            <IconChevronLeft />
          </button>
          <img v-if="lbUrl" :key="lbId ?? ''" :src="lbUrl" class="lb-img" alt="" />
          <div v-else class="lb-loading">···</div>
          <button class="lb-nav next" :title="t('timemachine.lbNext')" @click="stepLb(1)">
            <IconChevronRight />
          </button>
        </div>
        <aside v-if="lbItem" class="lb-side">
          <div class="lbs-row">
            <span class="lbs-label">{{ t("timemachine.bsInstance") }}</span>
            <span>{{ instanceName(lbItem.instanceId) }}</span>
          </div>
          <div class="lbs-row">
            <span class="lbs-label">{{ t("timemachine.bsTrigger") }}</span>
            <span class="lbs-badge" :class="triggerClass(lbItem.trigger)">
              {{ triggerLabel(lbItem.trigger) }}
            </span>
          </div>
          <div class="lbs-row">
            <span class="lbs-label">{{ t("timemachine.bsCloud") }}</span>
            <span>{{ lbItem.uploaded ? t("timemachine.bsUploaded") : t("timemachine.localOnly") }}</span>
          </div>
          <div class="lbs-row">
            <span class="lbs-label">{{ t("timemachine.bsMoment") }}</span>
            <span>{{ fmtTime(lbItem.createdAt) }}</span>
          </div>
          <div class="lbs-title">{{ t("timemachine.theMoment") }}</div>
          <pre v-if="lbLog" class="lbs-log">{{ lbLog }}</pre>
          <p v-else-if="lbLogLoading" class="lbs-nolog">···</p>
          <p v-else class="lbs-nolog">{{ t("timemachine.noLog") }}</p>
        </aside>
        <div class="lb-foot">
          <span>{{ t("timemachine.lbCounter", { i: lbIndex + 1, n: lbTotal }) }}</span>
          <span>{{ t("timemachine.lbHint") }}</span>
        </div>
      </div>
    </transition>
  </Teleport>

  <!-- 添加自定义规则 -->
  <NModal v-model:show="adding" preset="card" style="width: 520px" :title="t('timemachine.addRule')">
    <div class="form">
      <label>{{ t("timemachine.ruleName") }}</label>
      <NInput v-model:value="draft.label" :placeholder="t('timemachine.ruleNamePh')" />
      <label>{{ t("timemachine.rulePattern") }}</label>
      <NInput
        v-model:value="draft.pattern"
        class="mono"
        :placeholder="'\\[CHAT\\].*The End'"
        spellcheck="false"
      />
      <label>{{ t("timemachine.ruleCooldown") }}</label>
      <NInputNumber v-model:value="draft.cooldownSecs" :min="0" style="width: 140px" />
    </div>
    <template #footer>
      <div class="dlg-foot">
        <NButton size="small" @click="adding = false">{{ t("common.cancel") }}</NButton>
        <NButton size="small" type="primary" @click="commitAdd">{{ t("timemachine.addRule") }}</NButton>
      </div>
    </template>
  </NModal>
  </div>
</template>

<style scoped>
.tm {
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding-bottom: 10px;
}

/* ---------------- 顶部工具栏 ---------------- */
.tm-top {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
}
.tm-title {
  display: flex;
  align-items: baseline;
  gap: 10px;
  min-width: 0;
}
.tm-name {
  font-size: 16px;
  font-weight: 700;
  color: var(--text-1);
}
.tm-sub {
  font-size: 12px;
  color: var(--text-3);
}
.tm-tools {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  /* 统一高度：图标按钮的内容比文字矮，不锁死高度会"小一圈" */
  min-height: 34px;
  font-size: 12px;
  padding: 6px 12px;
  border-radius: 10px;
  border: 1px solid var(--border);
  background: var(--w-04);
  color: var(--text-2);
  cursor: pointer;
  transition: background 0.18s ease, border-color 0.18s ease, color 0.18s ease;
}
.btn:hover:not(:disabled) {
  background: var(--panel-hover);
  border-color: var(--accent-35);
  color: var(--text-1);
}
.btn:disabled {
  opacity: 0.5;
  cursor: default;
}
.btn.ic {
  min-width: 34px;
  padding: 0 9px;
}
.btn.primary,
.btn.hero-btn {
  background: var(--accent);
  border-color: var(--accent);
  color: #fff;
}
.btn.primary:hover,
.btn.hero-btn:hover {
  background: var(--accent-hover);
  border-color: var(--accent-hover);
  color: #fff;
}
.btn.pending {
  border-color: var(--accent-40);
  background: var(--accent-soft);
  color: var(--accent);
}
.btn.pending.spin svg {
  animation: spinY 1.1s linear infinite;
}
@keyframes spinY {
  to {
    transform: rotate(360deg);
  }
}

/* 视图切换胶囊 */
.seg {
  display: flex;
  gap: 2px;
  padding: 3px;
  border-radius: 999px;
  background: var(--w-06);
  border: 1px solid var(--border);
}
.seg-i {
  display: inline-flex;
  align-items: center;
  /* 和 .btn 同高（3+3 的外壳内边距 + 2 边框 = 34px） */
  min-height: 26px;
  border: 0;
  background: transparent;
  color: var(--text-3);
  font-size: 12px;
  padding: 0 14px;
  border-radius: 999px;
  cursor: pointer;
  transition: background 0.2s ease, color 0.2s ease;
}
.seg-i:hover {
  color: var(--text-1);
}
.seg-i.on {
  background: var(--accent);
  color: #fff;
}

/* ---------------- 未授权引导 / 空态 ---------------- */
.gate,
.empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  text-align: center;
  gap: 10px;
  padding: 46px 24px;
  border-radius: 16px;
  border: 1px solid var(--border);
  background: var(--w-02);
  animation: rise 0.45s cubic-bezier(0.22, 1, 0.36, 1);
}
.gate-ico,
.empty-ico {
  width: 52px;
  height: 52px;
  border-radius: 16px;
  background: var(--accent-soft);
  color: var(--accent);
  display: grid;
  place-items: center;
  font-size: 24px;
}
.gate-t,
.empty-t {
  margin: 0;
  font-size: 14px;
  color: var(--text-1);
}
.gate-p {
  max-width: 460px;
  margin: 0;
  font-size: 12.5px;
  line-height: 1.75;
  color: var(--text-3);
}
.gate-actions {
  display: flex;
  gap: 8px;
  margin-top: 4px;
}
.steps {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
  justify-content: center;
}
.steps > span {
  font-size: 11.5px;
  color: var(--text-3);
  padding: 5px 12px;
  border-radius: 999px;
  background: var(--w-04);
  border: 1px solid var(--border);
}
.steps b {
  color: var(--accent);
  margin-right: 4px;
}
@keyframes rise {
  from {
    opacity: 0;
    transform: translateY(10px);
  }
}

/* ---------------- 封面 ---------------- */
.hero {
  position: relative;
  border-radius: 16px;
  overflow: hidden;
  min-height: 208px;
  border: 1px solid var(--border);
  display: flex;
  align-items: flex-end;
  background: var(--w-06);
}
.hero-img {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: cover;
  animation: heroIn 1s cubic-bezier(0.22, 1, 0.36, 1);
}
.hero-ph {
  position: absolute;
  inset: 0;
  background: linear-gradient(135deg, var(--w-06), var(--w-02));
}
@keyframes heroIn {
  from {
    opacity: 0.3;
    transform: scale(1.06);
  }
  to {
    opacity: 1;
    transform: scale(1);
  }
}
.hero-veil {
  position: absolute;
  inset: 0;
  background: linear-gradient(
    180deg,
    rgba(0, 0, 0, 0.02) 0%,
    rgba(0, 0, 0, 0.16) 42%,
    rgba(0, 0, 0, 0.74) 100%
  );
}
.hero-body {
  position: relative;
  padding: 18px 20px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  width: 100%;
}
.hero-tag {
  align-self: flex-start;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
  padding: 3px 10px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.18);
  color: #fff;
}
.hero-tag em {
  font-style: normal;
  color: #9fe1cb;
}
.hero-h {
  margin: 0;
  font-size: 20px;
  font-weight: 700;
  color: #fff;
  text-shadow: 0 1px 10px rgba(0, 0, 0, 0.45);
}
.hero-sep {
  opacity: 0.5;
  margin: 0 4px;
}
.hero-chips {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}
.hero-chip {
  font-size: 11.5px;
  padding: 3px 9px;
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.16);
  color: #f2f3f7;
}
.hero-chip.danger {
  background: rgba(229, 83, 75, 0.88);
}
.hero-chip.gold {
  background: rgba(232, 154, 75, 0.92);
}
.hero-actions {
  margin-top: 2px;
}
.hero-btn {
  border-color: rgba(255, 255, 255, 0.2);
}

/* ---------------- 筛选 ---------------- */
.filters {
  display: flex;
  align-items: center;
  gap: 7px;
  flex-wrap: wrap;
}
.chip {
  font-size: 12px;
  padding: 5px 12px;
  border-radius: 999px;
  border: 1px solid var(--border);
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
  transition: background 0.18s ease, border-color 0.18s ease, color 0.18s ease;
}
.chip:hover {
  color: var(--text-1);
  border-color: var(--accent-35);
}
.chip.on {
  background: var(--accent-soft);
  border-color: var(--accent);
  color: var(--accent);
}
.chip.red.on {
  background: var(--danger-14);
  border-color: var(--danger-40);
  color: #e5534b;
}
.chip.gold.on {
  background: var(--accent-14);
  border-color: var(--accent-40);
  color: var(--accent);
}
.chip.teal.on {
  background: var(--success-12);
  border-color: rgba(78, 201, 160, 0.45);
  color: #4ec9a0;
}
.chip.purple.on {
  background: rgba(127, 119, 221, 0.16);
  border-color: rgba(127, 119, 221, 0.45);
  color: #7f77dd;
}
.filters-sep {
  width: 1px;
  height: 18px;
  background: var(--border);
}
.filters-count {
  margin-left: auto;
  font-size: 12px;
  color: var(--text-3);
}
.chip.dens {
  font-variant-numeric: tabular-nums;
}

/* ---------------- 网格 / 卡片 ---------------- */
.stream {
  display: grid;
  gap: 12px;
  grid-template-columns: repeat(auto-fill, minmax(232px, 1fr));
}
.timeline {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 20px;
  padding-left: 20px;
}
.timeline::before {
  content: "";
  position: absolute;
  left: 4px;
  top: 8px;
  bottom: 8px;
  width: 1px;
  background: linear-gradient(180deg, var(--accent-40), var(--border) 55%, transparent);
}
.tl-group {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.tl-dot {
  position: absolute;
  left: -20px;
  top: 5px;
  width: 9px;
  height: 9px;
  border-radius: 50%;
  background: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-14);
}
.tl-head {
  display: flex;
  align-items: baseline;
  gap: 8px;
  flex-wrap: wrap;
}
.tl-h {
  font-size: 14px;
  font-weight: 700;
  color: var(--text-1);
}
.tl-range,
.tl-dur {
  font-size: 11.5px;
  color: var(--text-3);
}
.tl-inst {
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 6px;
  background: var(--w-08);
  color: var(--text-2);
}
.tl-stats {
  margin-left: auto;
  font-size: 11.5px;
  color: var(--text-3);
}
.shots {
  display: grid;
  gap: 12px;
  grid-template-columns: repeat(auto-fill, minmax(232px, 1fr));
}
/* 紧凑密度：卡片更小、间距更紧 */
.stream.compact {
  grid-template-columns: repeat(auto-fill, minmax(168px, 1fr));
  gap: 9px;
}
.timeline.compact .shots {
  grid-template-columns: repeat(auto-fill, minmax(168px, 1fr));
  gap: 9px;
}

.day-div {
  grid-column: 1 / -1;
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 12px;
  font-weight: 700;
  color: var(--text-3);
  margin-top: 4px;
}
.day-div::after {
  content: "";
  flex: 1;
  height: 1px;
  background: var(--border);
}

.shot {
  display: flex;
  flex-direction: column;
  gap: 7px;
  min-width: 0;
}
.shot-pic {
  position: relative;
  border-radius: 12px;
  overflow: hidden;
  background: var(--w-06);
  aspect-ratio: 16 / 9;
  cursor: zoom-in;
  transition: transform 0.3s cubic-bezier(0.22, 1, 0.36, 1), box-shadow 0.3s ease;
}
.shot-pic:hover {
  transform: translateY(-3px);
  box-shadow: 0 12px 28px var(--k-25);
}
.shot-pic img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
  transition: transform 0.5s cubic-bezier(0.22, 1, 0.36, 1);
}
.shot-pic:hover img {
  transform: scale(1.06);
}
.shot-skel {
  width: 100%;
  height: 100%;
  background: linear-gradient(90deg, var(--w-04) 0%, var(--w-12) 50%, var(--w-04) 100%);
  background-size: 220% 100%;
  animation: shimmer 1.5s linear infinite;
}
@keyframes shimmer {
  from {
    background-position: 120% 0;
  }
  to {
    background-position: -120% 0;
  }
}
/* --w-* 是白色叠加色，浅色主题下等于隐形：占位面与骨架改用黑色低透明度 */
:root.light .shot-pic,
:root.light .hero {
  background: rgba(0, 0, 0, 0.05);
}
:root.light .shot-skel {
  background: linear-gradient(
    90deg,
    rgba(0, 0, 0, 0.04) 0%,
    rgba(0, 0, 0, 0.1) 50%,
    rgba(0, 0, 0, 0.04) 100%
  );
  background-size: 220% 100%;
}
:root.light .tl-inst {
  background: rgba(0, 0, 0, 0.05);
}
.shot-badge {
  position: absolute;
  left: 8px;
  top: 8px;
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 999px;
  color: #fff;
  background: rgba(0, 0, 0, 0.45);
}
.shot-badge.red {
  background: rgba(229, 83, 75, 0.92);
}
.shot-badge.gold {
  background: rgba(232, 154, 75, 0.95);
}
.shot-badge.teal {
  background: rgba(78, 201, 160, 0.92);
}
.shot-badge.purple {
  background: rgba(127, 119, 221, 0.92);
}
.shot-new {
  position: absolute;
  left: 8px;
  bottom: 8px;
  font-size: 10.5px;
  padding: 1px 7px;
  border-radius: 999px;
  background: var(--accent);
  color: #fff;
}
.shot-local {
  position: absolute;
  right: 8px;
  bottom: 8px;
  font-size: 10.5px;
  padding: 2px 7px;
  border-radius: 6px;
  background: rgba(0, 0, 0, 0.5);
  color: #e8e9ee;
}
.shot-del {
  position: absolute;
  right: 6px;
  top: 6px;
  width: 26px;
  height: 26px;
  border-radius: 8px;
  border: 0;
  background: rgba(0, 0, 0, 0.45);
  color: #fff;
  display: grid;
  place-items: center;
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.2s ease, background 0.2s ease;
}
.shot-pic:hover .shot-del {
  opacity: 1;
}
.shot-del:hover {
  background: var(--danger-50);
}
.shot-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 11.5px;
  color: var(--text-3);
}
.shot-inst {
  margin-left: auto;
  max-width: 58%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
/* 新到的截图：外扩呼吸光 + "新"标 */
.shot.fresh .shot-pic {
  animation: freshPulse 1.6s ease-out 4;
}
@keyframes freshPulse {
  0% {
    box-shadow: 0 0 0 0 var(--accent-45);
  }
  100% {
    box-shadow: 0 0 0 13px transparent;
  }
}

/* 滚动揭示：滚进视口才淡入上浮（标记是 data-in 属性，见 script 里的说明） */
[data-reveal] {
  opacity: 0;
  transform: translateY(16px);
}
[data-reveal][data-in] {
  opacity: 1;
  transform: none;
  transition: opacity 0.55s cubic-bezier(0.22, 1, 0.36, 1) var(--d, 0ms),
    transform 0.55s cubic-bezier(0.22, 1, 0.36, 1) var(--d, 0ms);
}
/* 分组整体揭示后，组内卡片依次跟上（错落上限 12 张，否则大场次要等好几秒） */
.tl-group[data-in] .shot {
  animation: cardIn 0.5s cubic-bezier(0.22, 1, 0.36, 1) both;
  animation-delay: calc(min(var(--i, 0), 12) * 45ms);
}
@keyframes cardIn {
  from {
    opacity: 0;
    transform: translateY(12px);
  }
  to {
    opacity: 1;
    transform: none;
  }
}

/* 回到最新 */
.to-top {
  position: fixed;
  right: 26px;
  bottom: 24px;
  z-index: 30;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  padding: 8px 14px;
  border-radius: 999px;
  border: 1px solid var(--border);
  background: var(--panel);
  color: var(--text-2);
  cursor: pointer;
}
.to-top:hover {
  color: var(--text-1);
  border-color: var(--accent-35);
}
.pop-enter-active,
.pop-leave-active {
  transition: opacity 0.22s ease, transform 0.22s cubic-bezier(0.22, 1, 0.36, 1);
}
.pop-enter-from,
.pop-leave-to {
  opacity: 0;
  transform: translateY(8px);
}

/* ---------------- 控制台弹窗 ---------------- */
.ctl-repo {
  margin-bottom: 10px;
  font-size: 11px;
  color: var(--text-3);
  word-break: break-all;
}
.ctl-body {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.ctl-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}

/* ---------------- 控制台里的设置行 ---------------- */
.row-line {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  font-size: 13px;
  padding: 5px 0;
}
.row-label {
  color: var(--text-2);
}
.inline-ctrl {
  display: flex;
  align-items: center;
  gap: 8px;
}
.sub-title {
  font-size: 12px;
  font-weight: 700;
  color: var(--text-3);
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 12px;
}
.triggers {
  display: flex;
  flex-wrap: wrap;
  gap: 8px 18px;
}
.tg {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--text-2);
  cursor: default;
}
.custom-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.custom-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 8px;
  border-radius: 8px;
  background: var(--w-04);
}
.c-info {
  flex: 1;
  min-width: 0;
}
.c-name {
  font-size: 12px;
}
.c-pattern {
  font-size: 11px;
  color: var(--text-3);
  word-break: break-all;
}
.c-cool {
  font-size: 11px;
  color: var(--text-3);
}
.hint {
  font-size: 12px;
  color: var(--text-3);
}

/* ---------------- 沉浸式看图 ---------------- */
.lb {
  position: fixed;
  inset: 0;
  z-index: 60;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 18px;
  padding: 42px 24px 56px;
  background: rgba(8, 9, 12, 0.9);
  backdrop-filter: blur(12px);
  -webkit-backdrop-filter: blur(12px);
}
.lb-enter-active,
.lb-leave-active {
  transition: opacity 0.26s ease;
}
.lb-enter-from,
.lb-leave-to {
  opacity: 0;
}
.lb-stage {
  position: relative;
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  align-items: center;
  justify-content: center;
}
.lb-img {
  max-width: 100%;
  max-height: calc(100vh - 150px);
  border-radius: 12px;
  box-shadow: 0 24px 64px var(--k-50);
  animation: lbIn 0.36s cubic-bezier(0.22, 1, 0.36, 1);
}
@keyframes lbIn {
  from {
    opacity: 0;
    transform: scale(0.965);
  }
  to {
    opacity: 1;
    transform: none;
  }
}
.lb-loading {
  font-size: 20px;
  color: rgba(255, 255, 255, 0.4);
}
.lb-nav,
.lb-x {
  border: 1px solid rgba(255, 255, 255, 0.14);
  background: rgba(255, 255, 255, 0.08);
  color: #fff;
  display: grid;
  place-items: center;
  cursor: pointer;
  transition: background 0.2s ease;
}
.lb-nav:hover,
.lb-x:hover {
  background: rgba(255, 255, 255, 0.18);
}
.lb-nav {
  position: absolute;
  top: 50%;
  transform: translateY(-50%);
  width: 44px;
  height: 44px;
  border-radius: 50%;
  font-size: 18px;
}
.lb-nav.prev {
  left: -6px;
}
.lb-nav.next {
  right: -6px;
}
.lb-x {
  position: absolute;
  right: 20px;
  top: 16px;
  width: 34px;
  height: 34px;
  border-radius: 10px;
}
.lb-side {
  width: 320px;
  flex-shrink: 0;
  max-height: calc(100vh - 150px);
  overflow: auto;
  padding: 14px 16px;
  border-radius: 14px;
  border: 1px solid rgba(255, 255, 255, 0.1);
  background: rgba(255, 255, 255, 0.06);
  display: flex;
  flex-direction: column;
  gap: 9px;
  font-size: 12px;
  color: #e8e9ee;
}
.lbs-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.lbs-label {
  width: 46px;
  flex-shrink: 0;
  color: #9a9daa;
}
.lbs-badge {
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.14);
}
.lbs-badge.red {
  background: rgba(229, 83, 75, 0.9);
}
.lbs-badge.gold {
  background: rgba(232, 154, 75, 0.92);
  color: #2b1a06;
}
.lbs-badge.teal {
  background: rgba(78, 201, 160, 0.9);
  color: #06251b;
}
.lbs-badge.purple {
  background: rgba(127, 119, 221, 0.9);
}
.lbs-title {
  margin-top: 4px;
  font-size: 12px;
  font-weight: 700;
  color: #9a9daa;
}
.lbs-log {
  margin: 0;
  max-height: 300px;
  overflow: auto;
  padding: 10px 12px;
  border-radius: 10px;
  border: 1px solid rgba(255, 255, 255, 0.07);
  background: rgba(0, 0, 0, 0.42);
  font-family: "Cascadia Code", Consolas, "Courier New", monospace;
  font-size: 11px;
  line-height: 1.6;
  color: #b9e6c8;
  white-space: pre-wrap;
  word-break: break-all;
}
.lbs-nolog {
  margin: 0;
  font-size: 11.5px;
  color: #9a9daa;
}
.lb-foot {
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  height: 44px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 0 24px;
  font-size: 11.5px;
  color: #9a9daa;
}

/* ---------------- 添加规则弹窗 ---------------- */
.form {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.form label {
  font-size: 12px;
  color: var(--text-3);
}
.mono :deep(input) {
  font-family: "Cascadia Code", Consolas, monospace;
  font-size: 12px;
}
.dlg-foot {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
</style>
