<!--
  种子地图

  · 生物群系 / 结构 / 出生点计算：cubiomes（MIT，Copyright © 2020 Cubitect）
    —— 经 src-tauri/src/cubiomes.rs + src-tauri/src/cubiomes_bridge.c 调用。
  · 瓦片式地图（拖动 / 缩放 / 分块加载）的交互设计参考 Axolotl Launcher
    （https://github.com/Mystic-Stars/Axolotl，GPL-3.0-only，Copyright © Mystic-Stars）；
    配色、渲染与任务调度为本启动器自行实现。
  · 上述第三方组件的版权与许可声明见仓库根目录 THIRD_PARTY_NOTICES.md。
-->
<script setup lang="ts">
import { ref, computed, reactive, watch, nextTick, onMounted, onUnmounted, h } from "vue";
import {
  NInput,
  NSelect,
  NInputNumber,
  NPopover,
  NModal,
  NCheckbox,
  useMessage,
} from "naive-ui";
import type { SelectOption } from "naive-ui";
import { api } from "../api";
import {
  IconZap,
  IconMapPin,
  IconClock,
  IconFolder,
  IconRefresh,
  IconLayers,
  IconHome,
  IconMaximize,
  IconMinimize,
} from "../components/icons";
import { useInstancesStore } from "../stores/instances";

const message = useMessage();
const instances = useInstancesStore();

// —— 种子 ——
const seedText = ref("0");
// 默认取当前正式版。
const mc = ref("26.2");
// cubiomes 建模到的最后一个版本是 1.21.4（Winter Drop，见 vendor/cubiomes/biomes.h），
// 之后的 1.21.5~1.21.11 与 2026 起改用新版本号（年.版本.补丁）的 26.x 地表群系/结构
// 模型都没变，所以这里全部列出来只是为了跟游戏版本对得上，实际用同一个模型生成。
const MC_NEW = ["26.2", "26.1.2", "26.1.1", "26.1"];
const MC_121 = ["1.21.11", "1.21.10", "1.21.9", "1.21.8", "1.21.7", "1.21.6", "1.21.5", "1.21.4", "1.21.3", "1.21.1", "1.21"];
const MC_LEGACY = ["1.20", "1.19.4", "1.19.2", "1.18", "1.17", "1.16", "1.16.1", "1.15", "1.14", "1.13", "1.12", "1.11", "1.10", "1.9", "1.8", "1.7"];
const MC_CLASSIC: [string, string][] = [
  ["1.6", "1.6"], ["1.5", "1.5"], ["1.4", "1.4"], ["1.3", "1.3"], ["1.2", "1.2"],
  ["1.1", "1.1"], ["1.0", "1.0"], ["b1.8", "Beta 1.8"], ["b1.7", "Beta 1.7"],
];
// naive-ui 的分组项用 `option.name || option.key` 当虚拟列表的 key，不给就会全部
// 落到 "key-required" → 重复 key 会让下拉滚动时出现重复条目，所以每组都需唯一 key。
const mcOptions = [
  {
    key: "mc-26",
    type: "group",
    label: "26.x（2026 起）",
    children: MC_NEW.map((v) => ({ label: v, value: v })),
  },
  {
    key: "mc-121",
    type: "group",
    label: "1.21.x",
    children: MC_121.map((v) => ({ label: v, value: v })),
  },
  {
    key: "mc-120",
    type: "group",
    label: "1.20 ~ 1.7",
    children: MC_LEGACY.map((v) => ({ label: v, value: v })),
  },
  {
    key: "mc-classic",
    type: "group",
    label: "1.6 及更早",
    children: MC_CLASSIC.map(([value, label]) => ({ label, value })),
  },
];
// 版本选择旁的解释（悬停可见）：说明新版本号复用同一个世界生成模型
const MC_MODEL_NOTE = "26.x 与 1.21.4 之后的地表群系/结构沿用同一套生成模型（cubiomes 只建模到 1.21.4）；26.2 新增的地下「硫磺洞穴」群系未收录。";
/** 选择器里全部可选版本（与 mcOptions 一致），用于把存档版本名映射到选项 */
const MC_ALL = [...MC_NEW, ...MC_121, ...MC_LEGACY, ...MC_CLASSIC.map(([v]) => v)];

/**
 * 存档里的版本名 → 选择器里的版本。
 * 精确命中就用它；否则退一步用同一条版本线（如 1.20.1 → 1.20、26.1.3 → 26.1，
 * 同一版本的补丁号世界生成模型一致）；对不上就返回 null，不乱猜。
 */
function matchMcVersion(name: string | null): string | null {
  if (!name) return null;
  if (MC_ALL.includes(name)) return name;
  const fams = MC_ALL.filter((v) => name.startsWith(`${v}.`)).sort((a, b) => b.length - a.length);
  return fams[0] ?? null;
}
/** java = 普通世界类型；large = 大型生物群系（cubiomes LARGE_BIOMES） */
const worldType = ref<"java" | "large">("java");
const worldTypeOptions = [
  { label: "Java 版", value: "java" },
  { label: "大型生物群系", value: "large" },
];
const largeBiomes = computed(() => worldType.value === "large");
const dim = ref(0);
const dimOptions = [
  { label: "主世界", value: 0 },
  { label: "下界", value: -1 },
  { label: "末地", value: 1 },
];

function randomSeedValue(): number {
  // 48 位随机种子（落在 Number 安全整数范围内，可正可负）
  const v = Math.floor(Math.random() * 0x1000000000000) - 0x800000000000;
  return v;
}
function randomSeed() {
  seedText.value = String(randomSeedValue());
  applySeed();
}

// —— 种子历史 ——
type SeedHistoryItem = { seed: string; mc: string; worldType: string; at: number };
const HISTORY_KEY = "qookix.seedmap.history";
const history = ref<SeedHistoryItem[]>([]);

function persistHistory() {
  try {
    localStorage.setItem(HISTORY_KEY, JSON.stringify(history.value));
  } catch {
    /* 忽略存储失败 */
  }
}
function loadHistory() {
  try {
    const raw = localStorage.getItem(HISTORY_KEY);
    const arr = raw ? JSON.parse(raw) : [];
    history.value = Array.isArray(arr) ? arr.slice(0, 30) : [];
  } catch {
    history.value = [];
  }
}
function pushHistory() {
  const t = seedText.value.trim();
  if (!t || seedStr.value === null) return;
  const item: SeedHistoryItem = { seed: t, mc: mc.value, worldType: worldType.value, at: Date.now() };
  history.value = [
    item,
    ...history.value.filter((h) => !(h.seed === item.seed && h.mc === item.mc && h.worldType === item.worldType)),
  ].slice(0, 30);
  persistHistory();
}
function useHistory(h: SeedHistoryItem) {
  seedText.value = h.seed;
  if (h.mc && h.mc !== mc.value) mc.value = h.mc;
  if (h.worldType === "large" || h.worldType === "java") worldType.value = h.worldType;
  applySeed();
}
function clearHistory() {
  history.value = [];
  persistHistory();
}

/**
 * 种子文本 → 规范化后的十进制串（去掉 + 号与前导 0）；不是合法 64 位整数就返回 null。
 * 用字符串而不是 number：Minecraft 种子是 64 位整数，存档里读出来的种子大多超出
 * JS 安全整数范围（2^53），转成 number 会被静默舍入成另一个世界。
 */
const seedStr = computed<string | null>(() => {
  const raw = seedText.value.trim().replace(/^\+/, "");
  if (!/^-?\d+$/.test(raw)) return null;
  try {
    const v = BigInt(raw);
    if (v < -9223372036854775808n || v > 9223372036854775807n) return null;
    return v.toString();
  } catch {
    return null;
  }
});

// —— 生物群系元数据：中文名 + 配色 ——
// 配色按「贴近原版、但更鲜明」的方向调过：
//   · 同一族（森林 / 针叶林 / 雪地 / 恶地……）共用色相，变体（丘陵、高原、山地）
//     只调明度 → 地图上能看出连成片的区域，而不是一堆互不相干的色块；
//   · 陆地色偏亮、饱和度略高，配合 0.5~1.4 的地貌阴影既有立体感又不会糊成一片；
//     整体压暗会丢细节，所以深色只留给深暗之域 / 虚空这类本该很暗的群系；
//   · 水域基准色不必太暗：深海 / 深水层会按 in-water 深度自动压暗（见 isWater / depthOf）。
const BIOMES: Record<number, [string, string]> = {
  // 海洋 / 河流
  0: ["海洋", "#4a86c8"], 10: ["冰冻海洋", "#6f96c9"], 24: ["深海", "#33629e"],
  44: ["暖水海洋", "#3cc0d2"], 45: ["温水海洋", "#41a8cc"], 46: ["冷水海洋", "#4674bd"],
  47: ["深层暖水海洋", "#2fa8c0"], 48: ["深层温水海洋", "#3395b8"], 49: ["深层冷水海洋", "#3a63a8"],
  50: ["深层冰冻海洋", "#5c85bd"], 7: ["河流", "#4d95d6"], 11: ["冰冻河流", "#8fbede"],
  // 平原 / 丘陵 / 山地
  1: ["平原", "#8fc45e"], 129: ["向日葵平原", "#a6d374"], 3: ["风袭丘陵", "#8b8d80"],
  131: ["砂砾山地", "#9a9c8d"], 34: ["风袭森林", "#7e9c6a"], 162: ["风袭砂砾丘陵", "#a3a494"],
  20: ["山地边缘", "#8f9488"], 180: ["尖峭山峰", "#d7e0e6"], 181: ["冰封山峰", "#c4d2e2"],
  182: ["裸岩山峰", "#9aa3a8"], 179: ["积雪山坡", "#e2eaf0"], 177: ["草甸", "#79c05a"],
  178: ["雪林", "#6f9a8a"],
  // 森林 / 针叶林
  4: ["森林", "#6bb04e"], 132: ["繁花森林", "#82c95f"], 18: ["疏林丘陵", "#5d9c42"],
  27: ["桦木森林", "#95c46b"], 28: ["桦木森林丘陵", "#86b65c"], 155: ["原始桦木森林", "#a3d17b"],
  156: ["高桦木丘陵", "#93c26a"], 29: ["黑森林", "#418335"], 157: ["黑森林丘陵", "#39752e"],
  5: ["针叶林", "#6f9f7c"], 133: ["针叶林山地", "#5f8b6c"], 19: ["针叶林丘陵", "#638f70"],
  30: ["积雪针叶林", "#7fa595"], 31: ["积雪针叶林丘陵", "#739a8b"], 158: ["积雪针叶林山地", "#688f80"],
  32: ["原始松木针叶林", "#5c8a63"], 160: ["原始云杉针叶林", "#6f9673"], 33: ["巨型针叶林丘陵", "#4f7c58"],
  161: ["原始云杉针叶林丘陵", "#628a68"],
  // 沼泽
  6: ["沼泽", "#6a7a45"], 134: ["沼泽丘陵", "#74854e"], 184: ["红树林沼泽", "#3f9179"],
  // 冰雪 / 沙滩
  12: ["雪原", "#eef3f8"], 13: ["雪山", "#dfe7ef"], 140: ["冰刺平原", "#d6ecf4"],
  16: ["沙滩", "#efe0ab"], 25: ["石岸", "#a8a89a"], 26: ["积雪沙滩", "#f2ece0"],
  // 丛林
  21: ["丛林", "#46a334"], 149: ["丛林变种", "#3f9a2f"], 22: ["丛林丘陵", "#3d9130"],
  23: ["稀疏丛林", "#5cb03d"], 151: ["丛林边缘变种", "#52a83a"], 168: ["竹林", "#86a52e"],
  169: ["竹林丘陵", "#7a982a"],
  // 热带草原
  35: ["热带草原", "#c6b657"], 163: ["破碎热带草原", "#d7c963"], 36: ["热带草原高原", "#b4a44b"],
  164: ["破碎热带草原高原", "#c5b556"],
  // 恶地 / 沙漠
  37: ["恶地", "#b0603c"], 165: ["被风蚀的恶地", "#c46a3a"], 38: ["疏林恶地", "#a8804f"],
  166: ["疏林恶地变种", "#99734a"], 39: ["恶地高原", "#a4583a"], 167: ["恶地高原变种", "#975033"],
  2: ["沙漠", "#e6d59a"], 130: ["沙漠湖泊", "#eedfa8"], 17: ["沙漠丘陵", "#dbc98a"],
  // 洞穴 / 蘑菇岛 / 樱花 / 苍白
  174: ["滴水石洞穴", "#7d6a55"], 175: ["繁茂洞穴", "#4f7f43"], 183: ["深暗之域", "#1e2733"],
  14: ["蘑菇岛", "#b98ac2"], 15: ["蘑菇岛岸边", "#a87bae"], 185: ["樱花树林", "#e79cc4"],
  186: ["苍白之园", "#97a1ad"],
  // 下界
  8: ["下界荒地", "#6d3a33"], 170: ["灵魂沙峡谷", "#54505f"], 171: ["绯红森林", "#8f3038"],
  172: ["诡异森林", "#2f8b84"], 173: ["玄武岩三角洲", "#56575f"],
  // 末地
  9: ["末地", "#a99ce0"], 40: ["末地小型岛屿", "#8a7cc6"], 41: ["末地内陆", "#c0b5ee"],
  42: ["末地高地", "#d6cdf6"], 43: ["末地荒岛", "#7c6fb6"],
  // 其它
  127: ["虚空", "#101018"],
};

// 用数组按下标（id + 1）做查表，比 Map 快很多：瓦片生成是逐像素调用的热路径
const biomeRgbLut: ([number, number, number] | undefined)[] = [];
const biomeDepthLut: (number | undefined)[] = [];

function hexRgb(hex: string): [number, number, number] {
  const v = parseInt(hex.slice(1), 16);
  return [(v >> 16) & 255, (v >> 8) & 255, v & 255];
}

function biomeRgb(id: number): [number, number, number] {
  const idx = id + 1;
  let c = biomeRgbLut[idx];
  if (!c) {
    const hex = BIOMES[id]?.[1];
    c = hex ? hexRgb(hex) : hslToRgb((id * 37) % 360, 45, 55);
    biomeRgbLut[idx] = c;
  }
  return c;
}

function biomeName(id: number): string {
  return BIOMES[id]?.[0] ?? `未知群系(${id})`;
}

/** 群系代表色（HEX），用于悬停信息里的色点 */
function biomeHex(id: number): string {
  const hex = BIOMES[id]?.[1];
  if (hex) return hex;
  const [r, g, b] = biomeRgb(id);
  return `rgb(${r},${g},${b})`;
}

/** 群系基准高度（cubiomes getBiomeDepthAndScale），用于地貌阴影 */
function depthOf(id: number): number {
  const v = biomeDepthLut[id + 1];
  return v === undefined ? 0.1 : v;
}

/** 水域判定：基准高度足够低视为海洋 / 河流 */
function isWater(id: number): boolean {
  const d = depthOf(id);
  return d <= -0.35 || id === 7 || id === 11;
}

function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  s /= 100; l /= 100;
  const k = (n: number) => (n + h / 30) % 12;
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) => l - a * Math.max(-1, Math.min(k(n) - 3, 9 - k(n), 1));
  return [Math.round(f(0) * 255), Math.round(f(8) * 255), Math.round(f(4) * 255)];
}

// —— 地图（瓦片式，可拖动/缩放） ——
const TILE = 256;
const SEED_MAP_SCALES = [1, 4, 16, 64] as const;
const MIN_ZOOM = -2;
const MAX_ZOOM = 3;

const center = reactive({ x: 0, z: 0 });
const zoom = ref(0);
const scale = computed(() => 4 ** zoom.value);
const tileScale = computed(
  () => SEED_MAP_SCALES[Math.min(Math.max(Math.floor(zoom.value), 0), SEED_MAP_SCALES.length - 1)],
);
const scaleLabel = computed(() =>
  scale.value >= 1
    ? `1px ≈ ${scale.value >= 10 ? Math.round(scale.value) : scale.value.toFixed(1)} 方块`
    : `${Math.round(1 / scale.value)}px ≈ 1 方块`,
);

const canvasRef = ref<HTMLCanvasElement | null>(null);
const viewport = reactive({ w: 0, h: 0 });
type TileEntry = {
  canvas: HTMLCanvasElement;
  ids: Uint8Array;
  /** 高亮遮罩（选中的群系留亮、其余压暗）及其对应的选择签名 */
  hlKey: string;
  hl: HTMLCanvasElement | null;
  /** 所属缩放级别与瓦片坐标（供缓存淘汰时算距离） */
  s: number;
  tx: number;
  tz: number;
};
const tileCache = new Map<string, TileEntry>();
/** 数据源版本：种子 / 版本 / 维度 / 版本类型变化时递增，旧请求的结果直接丢弃 */
let tileGen = 0;
/** 正在请求中的瓦片键，避免同一块被重复请求 */
const inflight = new Set<string>();
/** 尚在请求中的瓦片数，用于显示轻量加载提示（不使用转圈动画） */
const pendingTiles = ref(0);
/** 当前视野需要的瓦片键，用于计算加载进度 */
const visibleKeys = ref<string[]>([]);
/** 瓦片写入缓存 / 清空时递增，用于让进度与图例等派生值重新计算 */
const tileTick = ref(0);
const mapWrapRef = ref<HTMLElement | null>(null);
/** 地图内左侧图层面板：画边缘标尺时用它避开被挡住的几格 */
const sideRef = ref<HTMLElement | null>(null);
const isFullscreen = ref(false);
let refreshTimer: ReturnType<typeof setTimeout> | undefined;
let redrawFrame: number | undefined;
let resizeObserver: ResizeObserver | undefined;
const hoverInfo = ref<{ bx: number; bz: number; name: string; color: string } | null>(null);
/** 鼠标下面有没有结构图钉（用于把光标变成手型、优先显示结构名） */
const hoverStruct = ref<{ type: string; x: number; z: number } | null>(null);
/** 悬停提示跟随鼠标（左下角留给点选信息卡） */
const hoverPos = reactive({ x: 0, y: 0 });
const hoverStyle = computed(() => ({
  left: `${Math.min(hoverPos.x + 14, Math.max(8, viewport.w - 266))}px`,
  top: `${Math.max(6, hoverPos.y - 42)}px`,
}));
// —— 结构叠加层 ——
const structures = ref<{ type: string; x: number; z: number }[]>([]);
const structLoading = ref(false);
let structTimer: ReturnType<typeof setTimeout> | undefined;

function scheduleStructures(delay = 260) {
  if (structTimer) clearTimeout(structTimer);
  if (!showStructures.value || seedStr.value === null || !viewport.w) {
    if (structures.value.length) {
      structures.value = [];
      redraw();
    }
    return;
  }
  structTimer = setTimeout(refreshStructures, delay);
}

async function refreshStructures() {
  // 出生点是虚拟类型：由出生点接口提供，不参与结构查询（也只在主世界有意义）
  const wantSpawn = structTypes.value.includes("spawn") && dim.value === 0;
  const types = structTypes.value.filter((t) => t !== "spawn");
  const spawn = wantSpawn ? await ensureSpawn() : null;
  const base: { type: string; x: number; z: number }[] = spawn
    ? [{ type: "spawn", x: spawn.x, z: spawn.z }]
    : [];
  if (seedStr.value === null || !types.length) {
    structures.value = base;
    redraw();
    return;
  }
  const half = (Math.max(viewport.w, viewport.h) * scale.value) / 2;
  const radius = Math.ceil(half / 16) + 8;
  if (radius > 512) {
    // 缩得太远时不查结构（会非常慢），但出生点依然保留
    structures.value = base;
    redraw();
    return;
  }
  structLoading.value = true;
  try {
    const res = await api.toolboxQueryStructures(
      seedStr.value,
      mc.value,
      Math.round(center.x),
      Math.round(center.z),
      radius,
      types,
    );
    structures.value = [...base, ...res];
    redraw();
  } catch {
    structures.value = base;
  } finally {
    structLoading.value = false;
  }
}

// —— 世界出生点（按 种子/版本/版本类型 缓存，避免重复计算） ——
const spawnLoading = ref(false);
let spawnKey = "";
let spawnPromise: Promise<{ x: number; z: number } | null> | null = null;

function ensureSpawn(): Promise<{ x: number; z: number } | null> {
  if (seedStr.value === null) return Promise.resolve(null);
  const key = `${seedStr.value}|${mc.value}|${worldType.value}`;
  if (spawnPromise && spawnKey === key) return spawnPromise;
  spawnKey = key;
  spawnPromise = api
    .toolboxWorldSpawn(seedStr.value, mc.value, largeBiomes.value)
    .catch((e) => {
      message.error(`计算出生点失败：${e}`);
      spawnPromise = null;
      return null;
    });
  return spawnPromise;
}

/** 定位到世界出生点（主世界） */
async function goSpawn() {
  if (seedStr.value === null) {
    message.warning("种子必须是 64 位整数");
    return;
  }
  if (dim.value !== 0) {
    message.warning("出生点只存在于主世界");
    return;
  }
  spawnLoading.value = true;
  try {
    const p = await ensureSpawn();
    if (!p) return;
    center.x = clampWorld(p.x);
    center.z = clampWorld(p.z);
    gotoX.value = p.x;
    gotoZ.value = p.z;
    pin.value = { x: p.x, z: p.z };
    picked.value = null;
    scheduleRefresh(0);
    message.success(`出生点 (${p.x}, ${p.z})`);
  } finally {
    spawnLoading.value = false;
  }
}

// —— 图层开关 ——
const showRelief = ref(true);
const showHoverName = ref(true);
const showStructures = ref(false);
/** 叠加区块网格 + 边缘区块坐标标尺 */
const showChunks = ref(false);
const pin = ref<{ x: number; z: number } | null>(null);

// —— 高亮地形（生物群系） ——
// 选中的群系保持明亮、其余压暗，用来快速找特定地形（蘑菇岛、樱花树林、恶地……）。
const highlightBiomes = ref<number[]>([]);
const highlightSet = computed(() => new Set(highlightBiomes.value));
/** 选择签名：变了就要重建瓦片的高亮遮罩 */
const highlightSig = computed(() =>
  highlightBiomes.value.length ? [...highlightBiomes.value].sort((a, b) => a - b).join(",") : "",
);
const isHighlighted = (id: number | null): boolean => id !== null && highlightSet.value.has(id);

/** 高亮只在当前维度有意义：这个维度一个都没选就不压暗（否则整张图变暗很迷惑） */
const NETHER_IDS = [8, 170, 171, 172, 173];
const END_IDS = [9, 40, 41, 42, 43];
function dimHasBiome(d: number, id: number): boolean {
  const nether = NETHER_IDS.includes(id);
  const end = END_IDS.includes(id);
  if (d === -1) return nether;
  if (d === 1) return end;
  return !nether && !end;
}
const highlightActive = computed(() => highlightBiomes.value.some((id) => dimHasBiome(dim.value, id)));
const highlightPaintSig = computed(() => (highlightActive.value ? highlightSig.value : ""));

/** 「高亮地形」下拉选项：按维度分组，带颜色点 */
type BiomeOption = { label: string; value: number; color: string };
const biomeSelectOptions = computed(() => {
  const overworld: BiomeOption[] = [];
  const nether: BiomeOption[] = [];
  const end: BiomeOption[] = [];
  for (const [idStr, [name, color]] of Object.entries(BIOMES)) {
    const id = Number(idStr);
    const list = NETHER_IDS.includes(id) ? nether : END_IDS.includes(id) ? end : overworld;
    list.push({ label: name, value: id, color });
  }
  return [
    { type: "group" as const, key: "hl-ow", label: "主世界", children: overworld },
    { type: "group" as const, key: "hl-nether", label: "下界", children: nether },
    { type: "group" as const, key: "hl-end", label: "末地", children: end },
  ].filter((g) => g.children.length);
});
function clearHighlight() {
  highlightBiomes.value = [];
}
/** 高亮下拉里带一个颜色点，便于对照地图。
 *  注意：`render-label` 的第一个参数就是选项本身（分组头也会走这个回调，它没有 color）。 */
function renderBiomeLabel(raw: SelectOption) {
  const opt = raw as { label?: string; color?: string };
  return h("span", { style: { display: "inline-flex", alignItems: "center", gap: "6px" } }, [
    opt?.color
      ? h("i", {
          style: {
            width: "9px",
            height: "9px",
            borderRadius: "2px",
            flex: "none",
            background: opt.color,
            boxShadow: "inset 0 0 0 1px rgba(0, 0, 0, 0.35)",
          },
        })
      : null,
    h("span", { style: { overflow: "hidden", textOverflow: "ellipsis" } }, opt?.label ?? ""),
  ]);
}
/** 把点选的群系加进高亮列表 */
function highlightPickedBiome() {
  const p = picked.value;
  if (!p || p.biomeId === null || p.biomeId < 0 || highlightSet.value.has(p.biomeId)) return;
  highlightBiomes.value = [...highlightBiomes.value, p.biomeId];
}

/** 地图上点选的东西（群系点位或结构）：就锚在这点上 */
type Picked = {
  /** biome = 普通点位；struct = 结构图钉 */
  kind: "biome" | "struct";
  x: number;
  z: number;
  name: string;
  color: string;
  chunkX: number;
  chunkZ: number;
  /** 群系点才有（结构为 null），用于「高亮该群系」 */
  biomeId: number | null;
};
const picked = ref<Picked | null>(null);
/** 卡片尺寸（用于把卡片夹在地图范围内） */
const CARD_W = 232;
const CARD_H = 96;
/** 卡片跟着地图上的那个点走（拖动、缩放都会重算） */
const pickedStyle = computed(() => {
  const p = picked.value;
  if (!p) return {};
  const s = worldToScreen(p.x, p.z);
  const left = Math.min(Math.max(8, s.x + 16), Math.max(8, viewport.w - CARD_W - 8));
  const top = Math.min(Math.max(8, s.y - 22), Math.max(8, viewport.h - CARD_H - 8));
  return { left: `${Math.round(left)}px`, top: `${Math.round(top)}px` };
});

// —— 结构类型（地图上以图钉标注） ——
/** "spawn" 是虚拟类型：由出生点接口提供，不走结构查询 */
/** 面板里「常用」按钮恢复的组合 */
const STRUCT_PRESET = ["village", "monument", "mansion", "ancient_city", "trial_chambers"];
const structTypes = ref<string[]>([...STRUCT_PRESET]);
const structOptions = [
  { label: "出生点", value: "spawn" },
  { label: "村庄", value: "village" }, { label: "沙漠神庙", value: "desert_pyramid" },
  { label: "丛林神庙", value: "jungle_temple" }, { label: "沼泽小屋", value: "swamp_hut" },
  { label: "雪屋", value: "igloo" }, { label: "海底废墟", value: "ocean_ruin" },
  { label: "沉船", value: "shipwreck" }, { label: "海底神殿", value: "monument" },
  { label: "林地府邸", value: "mansion" }, { label: "掠夺者前哨站", value: "outpost" },
  { label: "废弃传送门", value: "ruined_portal" }, { label: "远古城市", value: "ancient_city" },
  { label: "埋藏宝藏", value: "treasure" }, { label: "矿井", value: "mineshaft" },
  { label: "紫水晶洞", value: "geode" }, { label: "下界要塞", value: "fortress" },
  { label: "堡垒遗迹", value: "bastion" }, { label: "末地城", value: "end_city" },
  { label: "踪迹遗迹", value: "trail_ruins" }, { label: "试炼密室", value: "trial_chambers" },
];
const structLabel = (v: string) => structOptions.find((o) => o.value === v)?.label ?? v;
const STRUCT_COLORS: Record<string, string> = {
  spawn: "#7ad08a",
  village: "#f0c060", desert_pyramid: "#e0b070", jungle_temple: "#7fc060",
  swamp_hut: "#8fa070", igloo: "#cfe8ff", ocean_ruin: "#6fb0c0", shipwreck: "#c0a070",
  monument: "#5fd0d0", mansion: "#c080d0", outpost: "#a06040", ruined_portal: "#b070e0",
  ancient_city: "#4a5f80", treasure: "#ffd76e", mineshaft: "#a08060", geode: "#9f7fe0",
  fortress: "#d05050", bastion: "#707080", end_city: "#d0c0ff", trail_ruins: "#d0a060",
  trial_chambers: "#80e0c0",
};
const structColor = (v: string) => STRUCT_COLORS[v] ?? "#e89a4b";

function tileKey(s: number, tx: number, tz: number) {
  return `${s}:${tx}:${tz}`;
}

function clampWorld(v: number) {
  return Math.min(Math.max(v, -30_000_000), 30_000_000);
}

function worldToScreen(x: number, z: number) {
  const halfW = (viewport.w * scale.value) / 2;
  const halfH = (viewport.h * scale.value) / 2;
  return { x: (x - (center.x - halfW)) / scale.value, y: (z - (center.z - halfH)) / scale.value };
}

function screenToWorld(sx: number, sy: number) {
  const halfW = (viewport.w * scale.value) / 2;
  const halfH = (viewport.h * scale.value) / 2;
  return {
    x: Math.round(center.x - halfW + sx * scale.value),
    z: Math.round(center.z - halfH + sy * scale.value),
  };
}

function redraw() {
  if (redrawFrame !== undefined) return;
  redrawFrame = requestAnimationFrame(() => {
    redrawFrame = undefined;
    drawMap();
  });
}

function drawMap() {
  const canvas = canvasRef.value;
  if (!canvas || !viewport.w || !viewport.h) return;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  ctx.clearRect(0, 0, viewport.w, viewport.h);
  ctx.fillStyle = "#1b1e24";
  ctx.fillRect(0, 0, viewport.w, viewport.h);
  ctx.fillStyle = "#23262d";
  for (let y = 0; y < viewport.h; y += 32) {
    for (let x = 0; x < viewport.w; x += 32) {
      if (((x / 32) + (y / 32)) % 2 === 0) ctx.fillRect(x, y, 32, 32);
    }
  }
  const halfW = (viewport.w * scale.value) / 2;
  const halfH = (viewport.h * scale.value) / 2;
  const b = {
    minX: center.x - halfW,
    minZ: center.z - halfH,
    maxX: center.x + halfW,
    maxZ: center.z + halfH,
  };
  const s = tileScale.value;
  const span = TILE * s;
  const displaySize = span / scale.value;
  const minTx = Math.floor(b.minX / span) - 1;
  const minTz = Math.floor(b.minZ / span) - 1;
  const maxTx = Math.floor(b.maxX / span) + 1;
  const maxTz = Math.floor(b.maxZ / span) + 1;
  ctx.imageSmoothingEnabled = false;
  for (let tx = minTx; tx <= maxTx; tx++) {
    for (let tz = minTz; tz <= maxTz; tz++) {
      const tile = tileCache.get(tileKey(s, tx, tz));
      const p = worldToScreen(tx * span, tz * span);
      if (!tile) {
        // 当前缩放的瓦片还没到：先用其它缩放级别已缓存的瓦片顶上，
        // 避免缩放瞬间整片地图变空白（看起来像“区块消失了”）。
        drawPreview(ctx, tx, tz, s, p.x, p.y, displaySize);
        continue;
      }
      ctx.drawImage(tile.canvas, p.x, p.y, displaySize, displaySize);
      const overlay = highlightOverlay(tile);
      if (overlay) ctx.drawImage(overlay, p.x, p.y, displaySize, displaySize);
    }
  }
  if (showStructures.value && structures.value.length) drawStructures(ctx);
  if (showChunks.value) drawChunkGrid(ctx);
  drawPin(ctx);
  const c = worldToScreen(center.x, center.z);
  ctx.strokeStyle = "rgba(150,181,225,0.85)";
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.moveTo(c.x - 8, c.y);
  ctx.lineTo(c.x + 8, c.y);
  ctx.moveTo(c.x, c.y - 8);
  ctx.lineTo(c.x, c.y + 8);
  ctx.stroke();
}

/**
 * 当前缩放的瓦片尚未加载时，用相邻缩放级别已缓存的瓦片先画个预览：
 * 先试更细一级（4×4 个瓦片，更清晰），再试更粗一级（取对应 1/16 区域）。
 */
function drawPreview(
  ctx: CanvasRenderingContext2D,
  tx: number,
  tz: number,
  s: number,
  px: number,
  py: number,
  size: number,
) {
  const sf = s / 4;
  if (sf >= 1) {
    const sub = size / 4;
    let drawn = false;
    for (let j = 0; j < 4; j++) {
      for (let i = 0; i < 4; i++) {
        const fine = tileCache.get(tileKey(sf, tx * 4 + i, tz * 4 + j));
        if (!fine) continue;
        ctx.drawImage(fine.canvas, px + i * sub, py + j * sub, sub, sub);
        drawn = true;
      }
    }
    if (drawn) return;
  }
  const coarse = tileCache.get(tileKey(s * 4, Math.floor(tx / 4), Math.floor(tz / 4)));
  if (coarse) {
    const q = TILE / 4;
    const sx = (tx - Math.floor(tx / 4) * 4) * q;
    const sz = (tz - Math.floor(tz / 4) * 4) * q;
    ctx.drawImage(coarse.canvas, sx, sz, q, q, px, py, size, size);
  }
}

/** 按当前高亮选择，惰性生成瓦片的高亮遮罩（选中的群系不动、其余压暗） */
function highlightOverlay(tile: TileEntry): HTMLCanvasElement | null {
  const sig = highlightPaintSig.value;
  if (!sig) return null;
  if (tile.hlKey !== sig) {
    tile.hlKey = sig;
    tile.hl = buildHighlightCanvas(tile.ids);
  }
  return tile.hl;
}

/** 高亮遮罩按半分辨率生成（2×2 块判一次）：边界柔和、内存和构建开销都小一半 */
const HL_BLOCK = 2;
function buildHighlightCanvas(ids: Uint8Array): HTMLCanvasElement | null {
  const set = highlightSet.value;
  if (!set.size) return null;
  const n = TILE / HL_BLOCK;
  const cv = document.createElement("canvas");
  cv.width = n;
  cv.height = n;
  const ctx = cv.getContext("2d");
  if (!ctx) return null;
  const img = ctx.createImageData(n, n);
  const data = img.data;
  // 未选中的压暗，选中的叠一层提亮，对比拉开才看得出地形
  const DIM = [6, 8, 12, 150];
  const LIFT = [255, 255, 255, 54];
  for (let y = 0; y < n; y++) {
    for (let x = 0; x < n; x++) {
      let on = false;
      for (let dy = 0; dy < HL_BLOCK && !on; dy++) {
        const row = (y * HL_BLOCK + dy) * TILE + x * HL_BLOCK;
        for (let dx = 0; dx < HL_BLOCK; dx++) {
          if (set.has(ids[row + dx])) {
            on = true;
            break;
          }
        }
      }
      const c = on ? LIFT : DIM;
      const o = (y * n + x) * 4;
      data[o] = c[0];
      data[o + 1] = c[1];
      data[o + 2] = c[2];
      data[o + 3] = c[3];
    }
  }
  ctx.putImageData(img, 0, 0);
  return cv;
}

// —— 区块网格与边缘区块坐标（图层「显示区块」） ——
const CHUNK = 16;

/** 找到 >= v 的最小 2 的幂（至少 1）：网格与标尺步长按缩放自适应 */
function pow2AtLeast(v: number) {
  let s = 1;
  while (s < v) s *= 2;
  return s;
}

/**
 * 区块网格 + 边缘区块坐标标尺：
 * 每 16 方块一条细线，上边缘标区块 X、左边缘标区块 Z。
 * 线条与文字的步长按缩放自适应（都是 2 的幂，正好和区域边界对齐）；
 * 被左侧图层面板挡住的那几格不画，免得糊在面板底下。
 */
function drawChunkGrid(ctx: CanvasRenderingContext2D) {
  const sw = viewport.w;
  const sh = viewport.h;
  const sc = scale.value; // 1 屏幕像素 = sc 个方块
  const pxc = CHUNK / sc; // 一个区块在屏幕上的边长
  if (!(pxc > 0)) return;
  const halfW = (sw * sc) / 2;
  const halfH = (sh * sc) / 2;
  const minX = center.x - halfW;
  const minZ = center.z - halfH;
  const c0 = Math.floor(minX / CHUNK);
  const c1 = Math.ceil((center.x + halfW) / CHUNK);
  const r0 = Math.floor(minZ / CHUNK);
  const r1 = Math.ceil((center.z + halfH) / CHUNK);
  // 线至少隔 7px 一条、文字至少隔 40px 一个
  const lineStep = pow2AtLeast(7 / pxc);
  const labelStep = pow2AtLeast(40 / pxc);
  const screenX = (c: number) => Math.round((c * CHUNK - minX) / sc) + 0.5;
  const screenZ = (r: number) => Math.round((r * CHUNK - minZ) / sc) + 0.5;

  ctx.save();
  ctx.lineWidth = 1;
  ctx.strokeStyle = "rgba(255,255,255,0.17)";
  ctx.beginPath();
  for (let c = Math.ceil(c0 / lineStep) * lineStep; c <= c1; c += lineStep) {
    const x = screenX(c);
    ctx.moveTo(x, 0);
    ctx.lineTo(x, sh);
  }
  for (let r = Math.ceil(r0 / lineStep) * lineStep; r <= r1; r += lineStep) {
    const y = screenZ(r);
    ctx.moveTo(0, y);
    ctx.lineTo(sw, y);
  }
  ctx.stroke();

  // 标尺：上边缘 = 区块 X（居中在该区块列上），左边缘 = 区块 Z
  const BAND = 13;
  const wrap = mapWrapRef.value?.getBoundingClientRect();
  const side = sideRef.value?.getBoundingClientRect();
  const panel =
    wrap && side
      ? { x: side.left - wrap.left, y: side.top - wrap.top, w: side.width, h: side.height }
      : null;
  const hidden = (x: number, y: number, w: number, h: number) =>
    !!panel && x < panel.x + panel.w && x + w > panel.x && y < panel.y + panel.h && y + h > panel.y;
  ctx.font = "10px ui-monospace, SFMono-Regular, Consolas, monospace";
  ctx.textAlign = "left";
  ctx.textBaseline = "middle";
  const chip = (text: string, cx: number, y: number, align: "center" | "left") => {
    const w = ctx.measureText(text).width + 8;
    const x = Math.round(align === "center" ? cx - w / 2 : cx);
    if (x + w < 1 || x > sw - 1 || hidden(x, y, w, BAND)) return;
    ctx.fillStyle = "rgba(10,12,16,0.62)";
    ctx.fillRect(x, y, w, BAND);
    ctx.fillStyle = "rgba(238,241,247,0.94)";
    ctx.fillText(text, x + 4, y + BAND / 2 + 0.5);
  };
  for (let c = Math.ceil(c0 / labelStep) * labelStep; c <= c1; c += labelStep) {
    chip(String(c), ((c + 0.5) * CHUNK - minX) / sc, 1, "center");
  }
  for (let r = Math.ceil(r0 / labelStep) * labelStep; r <= r1; r += labelStep) {
    chip(String(r), 2, ((r + 0.5) * CHUNK - minZ) / sc - BAND / 2, "left");
  }
  ctx.restore();
}

function drawStructures(ctx: CanvasRenderingContext2D) {
  const withLabel = scale.value <= 8;
  ctx.font = "12px system-ui, -apple-system, sans-serif";
  for (const h of structures.value) {
    const p = worldToScreen(h.x, h.z);
    if (p.x < -30 || p.y < -30 || p.x > viewport.w + 30 || p.y > viewport.h + 30) continue;
    ctx.beginPath();
    ctx.arc(p.x, p.y, 6, 0, Math.PI * 2);
    ctx.fillStyle = structColor(h.type);
    ctx.fill();
    ctx.lineWidth = 2;
    ctx.strokeStyle = "rgba(10,12,16,0.72)";
    ctx.stroke();
    if (withLabel) {
      const label = structLabel(h.type);
      const w = ctx.measureText(label).width;
      ctx.fillStyle = "rgba(12,14,18,0.72)";
      ctx.fillRect(p.x + 10, p.y - 10, w + 12, 19);
      ctx.fillStyle = "#f2f3f7";
      ctx.fillText(label, p.x + 16, p.y + 4);
    }
  }
}

function drawPin(ctx: CanvasRenderingContext2D) {
  const target = pin.value;
  if (!target) return;
  const p = worldToScreen(target.x, target.z);
  if (p.x < -40 || p.y < -40 || p.x > viewport.w + 40 || p.y > viewport.h + 40) return;
  ctx.strokeStyle = "#ff6b6b";
  ctx.lineWidth = 2;
  ctx.beginPath();
  ctx.arc(p.x, p.y, 7, 0, Math.PI * 2);
  ctx.moveTo(p.x - 13, p.y);
  ctx.lineTo(p.x - 8, p.y);
  ctx.moveTo(p.x + 8, p.y);
  ctx.lineTo(p.x + 13, p.y);
  ctx.moveTo(p.x, p.y - 13);
  ctx.lineTo(p.x, p.y - 8);
  ctx.moveTo(p.x, p.y + 8);
  ctx.lineTo(p.x, p.y + 13);
  ctx.stroke();
}

// —— 地貌阴影 ——
// 亮度字节由后端按固定的 1:4（4 方块）世界栅格算好，因此：
// 1) 地貌不会随缩放级别改变（缩小只是同一地形采样更稀疏）；
// 2) 逐像素求导不在主线程做，滚动/缩放不会卡。
const SHADE_MIN = 0.5;
const SHADE_MAX = 1.4;

/** 瓦片字节流头部长度（4 个 u32：width/height/shadeSize/reserved） */
const TILE_HEADER = 16;

/** 解析后端返回的瓦片字节流，裁掉 padding 得到本瓦片自己的 256x256 群系网格 */
function parseTile(buf: ArrayBuffer, crop: number) {
  const view = new DataView(buf);
  const w = view.getUint32(0, true);
  const h = view.getUint32(4, true);
  const shadeSize = view.getUint32(8, true);
  const biomes = new Uint8Array(buf, TILE_HEADER, w * h);
  const ids = new Uint8Array(TILE * TILE);
  for (let y = 0; y < TILE; y++) {
    const from = (y + crop) * w + crop;
    ids.set(biomes.subarray(from, from + TILE), y * TILE);
  }
  const shade = shadeSize
    ? new Uint8Array(buf, TILE_HEADER + w * h, shadeSize * shadeSize)
    : null;
  return { ids, shade, shadeSize };
}

/** 双线性采样亮度网格（g×g → TILE×TILE 像素） */
function shadeAt(data: Uint8Array, g: number, x: number, y: number): number {
  const step = TILE / g;
  const fx = x / step - 0.5;
  const fy = y / step - 0.5;
  const x0 = fx <= 0 ? 0 : Math.min(g - 1, Math.floor(fx));
  const y0 = fy <= 0 ? 0 : Math.min(g - 1, Math.floor(fy));
  const x1 = Math.min(g - 1, x0 + 1);
  const y1 = Math.min(g - 1, y0 + 1);
  const tx = Math.max(0, Math.min(1, fx - x0));
  const ty = Math.max(0, Math.min(1, fy - y0));
  const a = data[y0 * g + x0] * (1 - tx) + data[y0 * g + x1] * tx;
  const b = data[y1 * g + x0] * (1 - tx) + data[y1 * g + x1] * tx;
  return a * (1 - ty) + b * ty;
}

function clamp255(v: number): number {
  return v < 0 ? 0 : v > 255 ? 255 : v;
}

function buildTileCanvas(ids: Uint8Array, shade: Uint8Array | null, g: number): HTMLCanvasElement {
  const tile = document.createElement("canvas");
  tile.width = TILE;
  tile.height = TILE;
  const ctx = tile.getContext("2d");
  if (!ctx) return tile;
  const img = ctx.createImageData(TILE, TILE);
  const data = img.data;
  const sh = showRelief.value && shade && g > 1 ? shade : null;
  const range = SHADE_MAX - SHADE_MIN;
  for (let i = 0; i < ids.length; i++) {
    const id = ids[i];
    const [r, g2, b2] = biomeRgb(id);
    let f = 1;
    if (sh) {
      const x = i % TILE;
      const y = (i / TILE) | 0;
      if (isWater(id)) {
        // 水域：用基准高度表现水深（越深越暗），不叠山体阴影
        const t = Math.max(0, Math.min(1, (-depthOf(id) - 0.3) / 1.6));
        f = 1 - 0.42 * t;
      } else {
        f = SHADE_MIN + (shadeAt(sh, g, x, y) / 255) * range;
      }
    }
    const o = i * 4;
    data[o] = clamp255(r * f);
    data[o + 1] = clamp255(g2 * f);
    data[o + 2] = clamp255(b2 * f);
    data[o + 3] = 255;
  }
  ctx.putImageData(img, 0, 0);
  return tile;
}

function scheduleRefresh(delay = 80) {
  if (!viewport.w || !viewport.h) return;
  redraw();
  if (refreshTimer) clearTimeout(refreshTimer);
  refreshTimer = setTimeout(requestTiles, delay);
  scheduleStructures();
}

/** 瓦片是否仍与视口相交（含一格余量）；用于丢弃已经滑出视野的待请求瓦片 */
function tileInView(tx: number, tz: number, span: number) {
  const halfW = (viewport.w * scale.value) / 2;
  const halfH = (viewport.h * scale.value) / 2;
  const x0 = center.x - halfW - span;
  const x1 = center.x + halfW + span;
  const z0 = center.z - halfH - span;
  const z1 = center.z + halfH + span;
  return tx * span < x1 && (tx + 1) * span > x0 && tz * span < z1 && (tz + 1) * span > z0;
}

function syncPending() {
  pendingTiles.value = inflight.size;
}

/**
 * 缓存淘汰：只淘汰「不在视野内」的瓦片（按距离由远到近），
 * 并释放视野外瓦片的高亮遮罩内存。
 * 视野内的瓦片永不淘汰 —— 否则刚加载好的区块会凭空消失，
 * 而且不会自动重新请求（只能靠再次拖动/缩放才补回来）。
 */
function trimCache(allow: number) {
  const cap = Math.max(160, Math.min(420, Math.round(allow * 1.5)));
  const span = TILE * tileScale.value;
  const victims: { key: string; d: number }[] = [];
  for (const [key, t] of tileCache) {
    if (inflight.has(key)) continue;
    const own = t.s === tileScale.value;
    if (own && tileInView(t.tx, t.tz, span)) continue;
    // 其它缩放级别的瓦片只是备用（缩放回退时还能当预览），优先级最低
    if (t.hl) {
      t.hl = null;
      t.hlKey = "";
    }
    const cx = t.tx * TILE * t.s + (TILE * t.s) / 2;
    const cz = t.tz * TILE * t.s + (TILE * t.s) / 2;
    victims.push({ key, d: Math.hypot(cx - center.x, cz - center.z) + (own ? 0 : 1e9) });
  }
  if (tileCache.size <= cap) return;
  victims.sort((a, b) => b.d - a.d);
  for (const v of victims) {
    if (tileCache.size <= cap) break;
    tileCache.delete(v.key);
  }
}

async function requestTiles() {
  refreshTimer = undefined;
  if (seedStr.value === null) return;
  const seedKey = seedStr.value;
  const gen = tileGen;
  const s = tileScale.value;
  const span = TILE * s;
  // 高程样本固定跨 4 方块的整数倍；缩小时按 2 倍降采样（阴影快 4 倍）
  const k = s >= 4 ? 1 : 4;
  const kstep = k * (s >= 4 ? 2 : 1);
  // 多留一圈采样作为 padding：让阴影梯度在相邻瓦片接缝处完全连续
  const reqSize = TILE + 2 * kstep;
  const halfW = (viewport.w * scale.value) / 2;
  const halfH = (viewport.h * scale.value) / 2;
  const b = {
    minX: center.x - halfW,
    minZ: center.z - halfH,
    maxX: center.x + halfW,
    maxZ: center.z + halfH,
  };
  const minTx = Math.floor(b.minX / span) - 1;
  const minTz = Math.floor(b.minZ / span) - 1;
  const maxTx = Math.floor(b.maxX / span) + 1;
  const maxTz = Math.floor(b.maxZ / span) + 1;
  const jobs: { tx: number; tz: number; key: string }[] = [];
  const visible: string[] = [];
  for (let tx = minTx; tx <= maxTx; tx++) {
    for (let tz = minTz; tz <= maxTz; tz++) {
      const key = tileKey(s, tx, tz);
      visible.push(key);
      // 已缓存或已在请求中的瓦片不再重复请求
      if (tileCache.has(key) || inflight.has(key)) continue;
      jobs.push({ tx, tz, key });
    }
  }
  visibleKeys.value = visible;
  tileTick.value++;
  if (!jobs.length) return;
  const ctxTileX = center.x / span;
  const ctxTileZ = center.z / span;
  jobs.sort(
    (a, b2) =>
      Math.hypot(a.tx - ctxTileX, a.tz - ctxTileZ) - Math.hypot(b2.tx - ctxTileX, b2.tz - ctxTileZ),
  );
  const CONCURRENCY = Math.max(4, Math.min(8, navigator.hardwareConcurrency || 6));
  let idx = 0;
  let failed = 0;
  async function run() {
    while (idx < jobs.length) {
      // 数据源变了、或缩放级别已经切换：本批任务作废，避免占着并发拖慢新视图
      if (gen !== tileGen || tileScale.value !== s) return;
      const job = jobs[idx++];
      // 地图被拖动后，已经滑出视野的瓦片等下次刷新再按需加载
      if (!tileInView(job.tx, job.tz, span)) continue;
      if (tileCache.has(job.key)) continue;
      inflight.add(job.key);
      syncPending();
      try {
        const ccx = job.tx * span + 128 * s;
        const ccz = job.tz * span + 128 * s;
        const buf = await api.toolboxQueryBiomeMap(
          seedKey,
          mc.value,
          dim.value,
          ccx,
          ccz,
          reqSize,
          s,
          largeBiomes.value,
          showRelief.value,
        );
        // 只有数据源与缩放级别都没变时才写入缓存；否则丢弃并等新一轮加载
        if (gen !== tileGen || tileScale.value !== s) return;
        // 山体阴影由后端按固定 1:4 世界栅格算好，这里只做采样
        const { ids, shade, shadeSize } = parseTile(buf, kstep);
        tileCache.set(job.key, {
          canvas: buildTileCanvas(ids, shade, shadeSize),
          ids,
          hlKey: "",
          hl: null,
          s,
          tx: job.tx,
          tz: job.tz,
        });
        trimCache((maxTx - minTx + 1) * (maxTz - minTz + 1));
        redraw();
      } catch {
        // 单瓦片失败不影响其它瓦片，稍后自动重试一次
        failed++;
      } finally {
        inflight.delete(job.key);
        syncPending();
        tileTick.value++;
      }
    }
  }
  await Promise.all(Array.from({ length: Math.min(CONCURRENCY, jobs.length) }, run));
  // 有失败瓦片且期间没有新的刷新请求时，延迟重试
  if (failed && gen === tileGen && !refreshTimer) {
    refreshTimer = setTimeout(requestTiles, 1500);
  }
}

function clearTiles() {
  tileCache.clear();
  inflight.clear();
  syncPending();
  visibleKeys.value = [];
  tileTick.value++;
  // 数据换了，点选卡片上的群系信息就不准了
  picked.value = null;
  // 递增数据源版本，让在途请求的结果失效，避免把旧种子的瓦片写进缓存
  tileGen++;
}

function resizeCanvas() {
  const canvas = canvasRef.value;
  if (!canvas) return;
  // 量容器（canvas 是绝对定位铺满容器，容器尺寸才是基准）
  const rect = (mapWrapRef.value ?? canvas).getBoundingClientRect();
  viewport.w = Math.max(1, Math.round(rect.width));
  viewport.h = Math.max(1, Math.round(rect.height));
  const ratio = window.devicePixelRatio || 1;
  canvas.width = Math.round(viewport.w * ratio);
  canvas.height = Math.round(viewport.h * ratio);
  canvas.getContext("2d")?.setTransform(ratio, 0, 0, ratio, 0, 0);
  redraw();
  scheduleRefresh();
}

// —— 指针拖动 ——
const pointer = reactive({
  id: -1,
  startX: 0,
  startY: 0,
  centerX: 0,
  centerZ: 0,
  moved: false,
});
let panFrame: number | undefined;
let pendingPan: { x: number; z: number } | undefined;

function onPointerDown(e: PointerEvent) {
  const canvas = canvasRef.value;
  // 左键 / 右键一视同仁：按住拖动平移、单击（未拖动）选点。
  // 中键、侧键不参与，避免误操作。
  if (!canvas || (e.button !== 0 && e.button !== 2)) return;
  canvas.setPointerCapture(e.pointerId);
  pointer.id = e.pointerId;
  pointer.startX = e.offsetX;
  pointer.startY = e.offsetY;
  pointer.centerX = center.x;
  pointer.centerZ = center.z;
  pointer.moved = false;
}

function onPointerMove(e: PointerEvent) {
  if (pointer.id !== e.pointerId) return;
  const dx = e.offsetX - pointer.startX;
  const dy = e.offsetY - pointer.startY;
  if (Math.abs(dx) > 3 || Math.abs(dy) > 3) pointer.moved = true;
  if (!pointer.moved) return;
  pendingPan = {
    x: clampWorld(pointer.centerX - dx * scale.value),
    z: clampWorld(pointer.centerZ - dy * scale.value),
  };
  if (panFrame === undefined) panFrame = requestAnimationFrame(flushPan);
}

function flushPan() {
  panFrame = undefined;
  if (!pendingPan) return;
  center.x = pendingPan.x;
  center.z = pendingPan.z;
  pendingPan = undefined;
  redraw();
}

function onPointerUp(e: PointerEvent) {
  if (pointer.id !== e.pointerId) return;
  flushPan();
  const moved = pointer.moved;
  pointer.id = -1;
  canvasRef.value?.releasePointerCapture(e.pointerId);
  if (moved) scheduleRefresh(40);
  else selectAt(e.offsetX, e.offsetY);
}

function onPointerLeave() {
  hoverInfo.value = null;
  hoverStruct.value = null;
}

/** 命中测试：返回屏幕坐标处的群系 id（未加载返回 null） */
function pickAt(sx: number, sy: number): number | null {
  const p = screenToWorld(sx, sy);
  const s = tileScale.value;
  const span = TILE * s;
  const tx = Math.floor(p.x / span);
  const tz = Math.floor(p.z / span);
  const tile = tileCache.get(tileKey(s, tx, tz));
  if (!tile) return null;
  const lx = Math.floor((p.x - tx * span) / s);
  const lz = Math.floor((p.z - tz * span) / s);
  if (lx < 0 || lx >= TILE || lz < 0 || lz >= TILE) return null;
  const raw = tile.ids[lz * TILE + lx];
  return raw === 255 ? -1 : raw;
}

function selectAt(sx: number, sy: number) {
  // 先看是不是点在结构图钉上（图钉很小，优先命中）
  const st = structAt(sx, sy);
  if (st) {
    gotoX.value = st.x;
    gotoZ.value = st.z;
    pin.value = { x: st.x, z: st.z };
    picked.value = describeStruct(st);
    redraw();
    return;
  }
  const p = screenToWorld(sx, sy);
  gotoX.value = p.x;
  gotoZ.value = p.z;
  pin.value = { x: p.x, z: p.z };
  picked.value = describePoint(p.x, p.z, pickAt(sx, sy));
  redraw();
}

/** 命中测试：屏幕坐标处是否有结构图钉（后画的在上层，命中半径放宽到 10px） */
function structAt(sx: number, sy: number): { type: string; x: number; z: number } | null {
  if (!showStructures.value || !structures.value.length) return null;
  for (let i = structures.value.length - 1; i >= 0; i--) {
    const s = structures.value[i];
    const p = worldToScreen(s.x, s.z);
    if (Math.hypot(p.x - sx, p.y - sy) <= 10) return s;
  }
  return null;
}

/** 把坐标 + 群系信息整理成卡片数据（群系未加载 / 无群系时也能给出坐标） */
function describePoint(x: number, z: number, id: number | null): Picked {
  const missing = id === null || id < 0;
  return {
    kind: "biome",
    x,
    z,
    name: id === null ? "（瓦片未加载）" : id < 0 ? "（此处无群系）" : biomeName(id),
    color: missing ? "transparent" : biomeHex(id),
    chunkX: Math.floor(x / 16),
    chunkZ: Math.floor(z / 16),
    biomeId: id,
  };
}

/** 结构图钉的卡片数据 */
function describeStruct(s: { type: string; x: number; z: number }): Picked {
  return {
    kind: "struct",
    x: s.x,
    z: s.z,
    name: structLabel(s.type),
    color: structColor(s.type),
    chunkX: Math.floor(s.x / 16),
    chunkZ: Math.floor(s.z / 16),
    biomeId: null,
  };
}

async function copyText(text: string, ok: string) {
  try {
    await navigator.clipboard.writeText(text);
    message.success(ok);
  } catch {
    message.error("复制失败");
  }
}
function copyPickedCoords() {
  const p = picked.value;
  if (p) void copyText(`${p.x} ${p.z}`, "已复制坐标");
}
function copyPickedChunk() {
  const p = picked.value;
  if (p) void copyText(`${p.chunkX} ${p.chunkZ}`, "已复制区块坐标");
}
/** 生成 /tp 指令：y 用后端估算的地表高度（避免丢 ~ 后落在石头里），取不到才退回 ~ */
async function tpCommand(x: number, z: number): Promise<string> {
  const seed = seedStr.value;
  if (seed === null) return `/tp @s ${x} ~ ${z}`;
  try {
    const y = await api.toolboxSurfaceHeight(seed, mc.value, dim.value, x, z);
    return `/tp @s ${x} ${y} ${z}`;
  } catch {
    return `/tp @s ${x} ~ ${z}`;
  }
}
async function copyPickedTp() {
  const p = picked.value;
  if (!p) return;
  void copyText(await tpCommand(p.x, p.z), "已复制 TP 指令");
}
/** 把地图移到点选的结构 / 点位上 */
function goToPicked() {
  const p = picked.value;
  if (!p) return;
  center.x = clampWorld(p.x);
  center.z = clampWorld(p.z);
  pin.value = { x: p.x, z: p.z };
  scheduleRefresh(0);
}
/** 复制工具栏里 X / Z 输入框对应的 TP 指令 */
async function copyGotoTp() {
  const x = Math.round(Number(gotoX.value) || 0);
  const z = Math.round(Number(gotoZ.value) || 0);
  void copyText(await tpCommand(x, z), "已复制 TP 指令");
}

function onMapHover(e: MouseEvent) {
  if (pointer.id !== -1) return;
  hoverPos.x = e.offsetX;
  hoverPos.y = e.offsetY;
  // 结构优先：图钉小，悬停时直接报结构名（光标提示不受「悬停显示信息」影响）
  const st = structAt(e.offsetX, e.offsetY);
  hoverStruct.value = st;
  if (!showHoverName.value) {
    if (hoverInfo.value) hoverInfo.value = null;
    return;
  }
  if (st) {
    hoverInfo.value = { bx: st.x, bz: st.z, name: structLabel(st.type), color: structColor(st.type) };
    return;
  }
  const p = screenToWorld(e.offsetX, e.offsetY);
  const id = pickAt(e.offsetX, e.offsetY);
  if (id === null) {
    hoverInfo.value = null;
    return;
  }
  hoverInfo.value = {
    bx: p.x,
    bz: p.z,
    name: biomeName(id),
    color: biomeHex(id),
  };
}

// —— 缩放 ——
function applyZoomAt(sx: number, sy: number, delta: number) {
  const target = Math.min(Math.max(zoom.value + delta, MIN_ZOOM), MAX_ZOOM);
  if (Math.abs(target - zoom.value) < 0.0001) return;
  const ax = center.x + (sx - viewport.w / 2) * scale.value;
  const az = center.z + (sy - viewport.h / 2) * scale.value;
  zoom.value = target;
  center.x = clampWorld(ax - (sx - viewport.w / 2) * scale.value);
  center.z = clampWorld(az - (sy - viewport.h / 2) * scale.value);
  redraw();
  scheduleRefresh();
}

function onWheel(e: WheelEvent) {
  e.preventDefault();
  // 向上滚动（deltaY < 0）→ 放大（scale 变小）
  applyZoomAt(e.offsetX, e.offsetY, e.deltaY / 600);
}

function onDblClick(e: MouseEvent) {
  e.preventDefault();
  applyZoomAt(e.offsetX, e.offsetY, -0.5);
}

function zoomBy(delta: number) {
  applyZoomAt(viewport.w / 2, viewport.h / 2, delta);
}

function refreshMap() {
  clearTiles();
  scheduleRefresh(0);
}

// —— 地图全屏 ——
function onFullscreenChange() {
  isFullscreen.value = !!document.fullscreenElement;
}

async function toggleFullscreen() {
  try {
    if (document.fullscreenElement) await document.exitFullscreen();
    else await mapWrapRef.value?.requestFullscreen();
  } catch (e) {
    message.error(`切换全屏失败：${e}`);
  }
}

// —— 加载进度（当前视野需要的瓦片 / 已加载的瓦片） ——
const loadedTiles = computed(() => {
  void tileTick.value;
  let n = 0;
  for (const k of visibleKeys.value) if (tileCache.has(k)) n++;
  return n;
});
const totalTiles = computed(() => visibleKeys.value.length);
const tileProgress = computed(() => {
  const t = totalTiles.value;
  return t ? loadedTiles.value / t : 1;
});
/** 只在确实有请求在飞时显示，加载完自动隐藏 */
const showProgress = computed(() => pendingTiles.value > 0 && totalTiles.value > 0);

watch([seedText, mc, worldType, dim, showRelief], () => {
  hoverInfo.value = null;
  picked.value = null;
  clearTiles();
  scheduleRefresh(0);
});

watch([showStructures, structTypes], () => {
  scheduleStructures(0);
  redraw();
}, { deep: true });

watch(highlightPaintSig, () => redraw());
watch(showChunks, () => redraw());

// —— 前往坐标 ——
const gotoX = ref(0);
const gotoZ = ref(0);

function goTo() {
  center.x = clampWorld(gotoX.value);
  center.z = clampWorld(gotoZ.value);
  pin.value = null;
  picked.value = null;
  scheduleRefresh(0);
}

function applySeed() {
  if (seedStr.value === null) {
    message.warning("种子必须是 64 位整数（-9223372036854775808 ~ 9223372036854775807）");
    return;
  }
  pin.value = null;
  clearTiles();
  pushHistory();
  scheduleRefresh(0);
}

// —— 从实例存档导入种子 ——
const importShow = ref(false);
const importInstance = ref<string | null>(null);
const importWorld = ref<string | null>(null);
const importSeed = ref<string | null>(null);
/** 存档自己记的版本（level.dat 的 Data.Version.Name），用来在导入时自动切版本 */
const importVersion = ref<string | null>(null);
const worldLoading = ref(false);
const importWorlds = ref<string[]>([]);

const instanceOptions = computed(() => instances.instances.map((i) => ({ label: i.name, value: i.id })));
const worldOptions = computed(() => importWorlds.value.map((w) => ({ label: w, value: w })));

async function openSaveImport() {
  importShow.value = true;
  if (!instances.instances.length) {
    try {
      await instances.load();
    } catch {
      /* 忽略实例列表加载失败 */
    }
  }
  if (!importInstance.value && instances.instances.length) importInstance.value = instances.instances[0].id;
}

watch(importInstance, async (id) => {
  importWorlds.value = [];
  importWorld.value = null;
  importSeed.value = null;
  importVersion.value = null;
  if (!id) return;
  worldLoading.value = true;
  try {
    const res = await api.listInstanceFiles(id, "saves");
    importWorlds.value = (res.files ?? []).filter((f) => f.isDir).map((f) => f.name);
  } catch (e) {
    message.error(String(e));
  } finally {
    worldLoading.value = false;
  }
});

watch(importWorld, async (world) => {
  importSeed.value = null;
  importVersion.value = null;
  if (!world || !importInstance.value) return;
  try {
    const info = await api.toolboxReadWorldInfo(importInstance.value, world);
    if (info.seed === null) message.warning("该存档没有固定种子（随机种子世界）");
    importSeed.value = info.seed;
    importVersion.value = info.version;
  } catch (e) {
    message.error(String(e));
  }
});

function applyImportedSeed() {
  if (importSeed.value === null) return;
  seedText.value = importSeed.value;
  // 存档里记了版本就顺手切过去（只有选择器里存在的版本才切，避免乱猜）
  const want = matchMcVersion(importVersion.value);
  if (want && want !== mc.value) {
    mc.value = want;
    message.success(`已按存档切换到 ${want}`);
  }
  importShow.value = false;
  applySeed();
}

// —— 群系高度表（后端 getBiomeDepthAndScale），用于地貌阴影 ——
async function loadBiomeTable() {
  try {
    const rows = await api.toolboxBiomeTable();
    for (const r of rows) if (biomeDepthLut[r.id + 1] === undefined) biomeDepthLut[r.id + 1] = r.depth;
    clearTiles();
    scheduleRefresh(0);
  } catch {
    /* 高度表不可用时退化为纯色群系地图 */
  }
}

onMounted(async () => {
  await nextTick();
  loadHistory();
  seedText.value = String(randomSeedValue());
  document.addEventListener("fullscreenchange", onFullscreenChange);
  pushHistory();
  void loadBiomeTable();
  resizeCanvas();
  // 观察地图容器（而不是 canvas）：canvas 现在是 position:absolute 铺满容器，
  // 容器尺寸才是真正的「可视区」来源
  if (mapWrapRef.value) {
    resizeObserver = new ResizeObserver(resizeCanvas);
    resizeObserver.observe(mapWrapRef.value);
  }
  scheduleRefresh(0);
});

onUnmounted(() => {
  if (refreshTimer) clearTimeout(refreshTimer);
  if (structTimer) clearTimeout(structTimer);
  if (panFrame !== undefined) cancelAnimationFrame(panFrame);
  if (redrawFrame !== undefined) cancelAnimationFrame(redrawFrame);
  resizeObserver?.disconnect();
  document.removeEventListener("fullscreenchange", onFullscreenChange);
});

</script>

<template>
  <div id="seed-root" class="seed-view">
    <div class="glass param-bar">
      <div class="field seed-field">
        <label>种子</label>
        <NInput v-model:value="seedText" placeholder="输入世界种子（支持负数）" @keyup.enter="applySeed" />
      </div>
      <div class="seed-actions">
        <button class="mini-btn" title="随机一个种子" @click="randomSeed">
          <IconZap />随机
        </button>
        <NPopover trigger="click" placement="bottom-start" :width="300">
          <template #trigger>
            <button class="mini-btn" :disabled="!history.length">
              <IconClock />历史({{ history.length }})
            </button>
          </template>
          <div class="history-panel">
            <div class="history-head">
              <span>最近使用</span>
              <button class="link-btn" @click="clearHistory">清空</button>
            </div>
            <div v-if="!history.length" class="muted">暂无记录</div>
            <button v-for="(h, i) in history" :key="i" class="history-item" @click="useHistory(h)">
              <span class="hs-seed mono">{{ h.seed }}</span>
              <span class="hs-meta">{{ h.mc }} · {{ h.worldType === "large" ? "大型" : "Java" }}</span>
            </button>
          </div>
        </NPopover>
        <button class="mini-btn" title="从实例存档读取种子" @click="openSaveImport">
          <IconFolder />导入存档
        </button>
      </div>
      <div class="field" :title="MC_MODEL_NOTE">
        <label>MC 版本</label>
        <NSelect v-model:value="mc" :options="mcOptions" />
      </div>
      <div class="field">
        <label>版本类型</label>
        <NSelect v-model:value="worldType" :options="worldTypeOptions" />
      </div>
      <div class="field">
        <label>维度</label>
        <NSelect v-model:value="dim" :options="dimOptions" />
      </div>
    </div>

    <div class="glass map-toolbar">
      <div class="coord-field"><label>X</label><NInputNumber v-model:value="gotoX" size="small" :show-button="false" /></div>
      <div class="coord-field"><label>Z</label><NInputNumber v-model:value="gotoZ" size="small" :show-button="false" /></div>
      <button class="mini-btn primary" @click="goTo"><IconMapPin />前往</button>
      <button class="mini-btn" title="复制 /tp @s X Y Z（Y 为估算地表高度）" @click="copyGotoTp">TP</button>
      <div class="tb-sep" />
      <button
        class="mini-btn"
        :title="dim === 0 ? '定位世界出生点' : '出生点只存在于主世界'"
        :disabled="spawnLoading || dim !== 0"
        @click="goSpawn"
      >
        <IconHome />出生点
      </button>
      <button class="mini-btn" title="重新加载当前视野" @click="refreshMap"><IconRefresh />刷新</button>
      <span class="tb-scale muted tiny">{{ scaleLabel }}</span>
    </div>

    <div ref="mapWrapRef" class="map-wrap glass">
      <canvas
        ref="canvasRef"
        class="biome-canvas"
        :class="{ 'over-struct': !!hoverStruct }"
        @pointerdown="onPointerDown"
        @pointermove="onPointerMove"
        @pointerup="onPointerUp"
        @pointercancel="onPointerUp"
        @pointerleave="onPointerLeave"
        @mousemove="onMapHover"
        @wheel="onWheel"
        @dblclick="onDblClick"
        @contextmenu.prevent
      />

      <!-- 地图内左侧面板 -->
      <aside ref="sideRef" class="map-side">
        <h3 class="panel-title"><IconLayers /> 图层</h3>
        <label class="layer-row"><NCheckbox v-model:checked="showRelief" /> 地貌阴影</label>
        <label class="layer-row"><NCheckbox v-model:checked="showHoverName" /> 悬停显示信息</label>
        <label
          class="layer-row"
          title="叠加区块网格（16×16 方块），并在上边缘显示区块 X、左边缘显示区块 Z"
        >
          <NCheckbox v-model:checked="showChunks" /> 显示区块
        </label>

        <div class="layer-sep" />
        <label class="layer-row"><NCheckbox v-model:checked="showStructures" /> 结构</label>
        <NSelect
          v-model:value="structTypes"
          multiple
          filterable
          size="small"
          :options="structOptions"
          :disabled="!showStructures"
          :max-tag-count="1"
          placeholder="搜索或选择结构"
        />
        <div class="row-links">
          <button class="link-btn" @click="structTypes = [...STRUCT_PRESET]">常用</button>
          <button class="link-btn" @click="structTypes = []">清空</button>
        </div>
        <div v-if="showStructures" class="muted tiny">
          {{ structLoading ? "正在查找结构…" : `已标注 ${structures.length} 个结构` }}
        </div>

        <div class="layer-sep" />
        <div class="panel-sub">
          <span>高亮地形</span>
          <button v-if="highlightBiomes.length" class="link-btn" @click="clearHighlight">清除</button>
        </div>
        <NSelect
          v-model:value="highlightBiomes"
          multiple
          filterable
          size="small"
          :max-tag-count="1"
          :options="biomeSelectOptions"
          :render-label="renderBiomeLabel"
          placeholder="搜索群系名…"
        />
        <div class="muted tiny">
          <template v-if="highlightBiomes.length && !highlightActive">当前维度没有选中的群系</template>
          <template v-else>选中群系留亮、其余压暗；点地图也能一键高亮。</template>
        </div>
      </aside>

      <!-- 地图内右侧控制 -->
      <div class="map-ctrl">
        <button class="ctrl-btn" title="放大" @click="zoomBy(-0.5)">
          <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><line x1="12" y1="5" x2="12" y2="19" /><line x1="5" y1="12" x2="19" y2="12" /></svg>
        </button>
        <button class="ctrl-btn" title="缩小" @click="zoomBy(0.5)">
          <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><line x1="5" y1="12" x2="19" y2="12" /></svg>
        </button>
        <button class="ctrl-btn" :title="isFullscreen ? '退出全屏' : '全屏显示地图'" @click="toggleFullscreen">
          <IconMinimize v-if="isFullscreen" />
          <IconMaximize v-else />
        </button>
      </div>

      <!-- 中心坐标 -->
      <span class="map-coord mono">{{ Math.round(center.x) }}, {{ Math.round(center.z) }}</span>

      <!-- 跟随鼠标的悬停提示 -->
      <div v-if="hoverInfo" class="map-tooltip" :style="hoverStyle">
        <i class="biome-dot" :style="{ background: hoverInfo.color }" />
        <span class="biome-name">{{ hoverInfo.name }}</span>
        <span class="coord">(x {{ hoverInfo.bx }}, z {{ hoverInfo.bz }})</span>
      </div>

      <!-- 点选的点位信息卡：就贴在那个点上，跟着地图走 -->
      <div v-if="picked" class="map-info" :style="pickedStyle">
        <div class="mi-head">
          <i class="biome-dot" :style="{ background: picked.color }" />
          <span class="biome-name">{{ picked.name }}</span>
          <span v-if="picked.kind === 'struct'" class="mi-kind">结构</span>
          <button class="mi-close" title="关闭" @click="picked = null">×</button>
        </div>
        <div class="mi-line mono">x {{ picked.x }} · z {{ picked.z }}</div>
        <div class="mi-line muted">区块 ({{ picked.chunkX }}, {{ picked.chunkZ }})</div>
        <div class="mi-actions">
          <button class="mini-btn" title="复制坐标 x z" @click="copyPickedCoords">复制坐标</button>
          <button class="mini-btn" title="复制区块坐标" @click="copyPickedChunk">区块</button>
          <button class="mini-btn primary" title="复制 /tp @s x y z（y 为估算地表高度）" @click="copyPickedTp">TP</button>
          <button v-if="picked.kind === 'struct'" class="mini-btn" title="把地图移到这个位置" @click="goToPicked">
            前往
          </button>
          <button
            v-else
            class="mini-btn"
            title="高亮该群系（其余地形压暗）"
            :disabled="picked.biomeId === null || picked.biomeId < 0 || isHighlighted(picked.biomeId)"
            @click="highlightPickedBiome"
          >
            {{ isHighlighted(picked.biomeId) ? "已高亮" : "高亮" }}
          </button>
        </div>
      </div>

      <!-- 底部：瓦片加载进度 -->
      <div v-if="showProgress" class="map-progress">
        <div class="progress-head">
          <span>正在加载地图区块</span>
          <span class="mono">{{ loadedTiles }} / {{ totalTiles }}</span>
        </div>
        <div class="progress-track">
          <i :style="{ width: `${Math.round(tileProgress * 100)}%` }" />
        </div>
      </div>
    </div>

    <!-- 第三方组件署名（cubiomes，MIT）：与联机页对陶瓦联机的标注保持一致 -->
    <p class="seed-credit muted tiny">
      生物群系 / 结构计算由
      <a href="https://github.com/Cubitect/cubiomes" target="_blank" rel="noopener">cubiomes</a>
      （Copyright © 2020 Cubitect，MIT License）提供；结果为本地算法推算，仅供参考。
    </p>

    <NModal v-model:show="importShow" preset="card" title="从实例存档导入种子" style="max-width: 520px">
      <div class="import-body">
        <div class="field">
          <label>实例</label>
          <NSelect v-model:value="importInstance" :options="instanceOptions" filterable placeholder="选择实例" />
        </div>
        <div class="field">
          <label>存档</label>
          <NSelect v-model:value="importWorld" :options="worldOptions" :loading="worldLoading" placeholder="选择存档" />
        </div>
        <div class="muted small">
          <template v-if="importSeed !== null">
            已读取种子：<b class="mono">{{ importSeed }}</b>
            <template v-if="importVersion">
              （存档版本 <b>{{ importVersion }}</b
              ><template v-if="matchMcVersion(importVersion)">，导入后自动切到该版本</template>）
            </template>
          </template>
          <template v-else-if="importWorld">该存档未显式设置种子（随机种子世界）</template>
          <template v-else>选择一个存档后自动读取种子</template>
        </div>
      </div>
      <template #footer>
        <div class="import-footer">
          <button class="mini-btn" @click="importShow = false">取消</button>
          <button class="mini-btn primary" :disabled="importSeed === null" @click="applyImportedSeed">
            使用该种子
          </button>
        </div>
      </template>
    </NModal>
  </div>
</template>

<style scoped>
/* 高度必须用 height（而不是 min-height）+ min-height:0：
   用 min-height:100% 时页面的“最小高度”只是等于可用高度，一旦子项的最小高度
   （地图的 min-height + 参数栏 + 工具栏）相加超过它，整页就会比窗口高 —— 出现
   页面滚动条，地图被挤到窗口外。改成确定高度后，地图（flex:1）会吸收剩余的
   高度：窗口变矮时地图跟着变矮，不会把页面顶出去。 */
.seed-view { display: flex; flex-direction: column; gap: 16px; height: 100%; min-height: 0; overflow: hidden; }

/* —— 第三方组件署名 —— */
.seed-credit { flex: none; margin: -6px 0 0; text-align: center; }
.seed-credit a { color: var(--text-3); text-decoration: underline; }
.seed-credit a:hover { color: var(--accent); }

/* —— 通用小按钮（与设置页 / 联机页保持一致） —— */
.mini-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 7px 13px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: var(--w-04);
  color: var(--text-2);
  font-size: 12px;
  cursor: pointer;
  transition: background 0.15s, border-color 0.15s, color 0.15s;
}
.mini-btn:hover { background: var(--w-08); color: var(--text-1); }
.mini-btn:disabled { opacity: 0.5; cursor: default; }
.mini-btn.primary { border-color: var(--accent); color: var(--accent); }
.mini-btn.primary:hover { background: var(--accent-soft); }
.mini-btn svg { font-size: 14px; }
.link-btn { padding: 0; border: none; background: none; color: var(--text-3); font-size: 12px; cursor: pointer; }
.link-btn:hover { color: var(--accent); }

/* —— 参数栏 —— */
.param-bar { padding: 14px 16px; display: flex; gap: 10px; flex-wrap: wrap; align-items: flex-end; }
.param-bar .field { width: 128px; }
.seed-field { flex: 1 1 180px; min-width: 180px; }
.seed-actions { display: flex; align-items: center; gap: 6px; padding-bottom: 1px; }
.field { display: flex; flex-direction: column; gap: 6px; min-width: 0; }
.field label { font-size: 12px; color: var(--text-3); }
.mono { font-variant-numeric: tabular-nums; }

/* —— 历史记录 —— */
.history-panel { display: flex; flex-direction: column; gap: 2px; max-height: 320px; overflow-y: auto; }
.history-head { display: flex; align-items: center; justify-content: space-between; font-size: 12px; color: var(--text-3); padding-bottom: 6px; }
.history-item { display: flex; flex-direction: column; gap: 2px; text-align: left; padding: 6px 8px; border: none; border-radius: 6px; background: transparent; cursor: pointer; color: var(--text-1); font: inherit; }
.history-item:hover { background: var(--panel-hover); }
.hs-seed { font-size: 13px; font-variant-numeric: tabular-nums; }
.hs-meta { font-size: 11px; color: var(--text-3); }

/* —— 主体布局 —— */
.map-side .panel-title { margin: 0 0 2px; }

/* —— 工具栏 —— */
.map-toolbar { padding: 8px 12px; display: flex; align-items: center; gap: 7px; flex-wrap: wrap; row-gap: 7px; }
.tb-sep { width: 1px; height: 20px; background: var(--border); margin: 0 2px; }
.tb-scale { margin-left: auto; white-space: nowrap; }
.coord-field { display: flex; align-items: center; gap: 5px; }
.coord-field label { font-size: 12px; color: var(--text-3); }
.coord-field :deep(.n-input-number) { width: 110px; }
.panel-title { display: flex; align-items: center; gap: 8px; margin: 0 0 4px; font-size: 14px; font-weight: 600; color: var(--text-1); }
.panel-title svg { color: var(--accent); }
.layer-row { display: flex; align-items: center; gap: 8px; font-size: 13px; color: var(--text-2); cursor: pointer; }
.layer-sep { height: 1px; background: var(--border); margin: 2px 0; }
/* 小标题行：左侧文字 + 右侧「清除」 */
.panel-sub { display: flex; align-items: center; justify-content: space-between; font-size: 13px; color: var(--text-2); }
.row-links { display: flex; gap: 10px; font-size: 12px; }

/* —— 地图上的浮层 ——
   `.glass` 用的是 --panel（白 4% / 白 55% 半透明），在应用背景上没问题，
   但叠在彩色地图上会透得看不清。这里改用主题的实色面 --feat-solid，
   再掺一点透明度保留“玻璃”感：深色主题下是近黑，浅色主题下是浅灰，跟着主题走。 */
.map-side,
.map-progress,
.map-tooltip,
.map-info,
.map-coord,
.ctrl-btn {
  border: 1px solid var(--border);
  background: var(--feat-solid);
  background: color-mix(in srgb, var(--feat-solid) 88%, transparent);
  backdrop-filter: blur(calc(var(--glass-blur, 8px) + 2px));
  -webkit-backdrop-filter: blur(calc(var(--glass-blur, 8px) + 2px));
}

/* 地图内左侧面板 */
.map-side {
  position: absolute;
  top: 12px;
  left: 12px;
  width: 232px;
  max-height: calc(100% - 136px);
  overflow-y: auto;
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 9px;
  border-radius: 12px;
}
.map-side .layer-row { font-size: 12px; }

/* 点选的点位信息卡：锚在点击的那个点上（left/top 由 pickedStyle 给） */
.map-info {
  position: absolute;
  width: 232px;
  padding: 10px 12px 11px;
  display: flex;
  flex-direction: column;
  gap: 5px;
  border-radius: 10px;
}
.mi-head { display: flex; align-items: center; gap: 8px; }
.mi-head .biome-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.mi-kind { flex: none; font-size: 11px; color: var(--text-3); border: 1px solid var(--border); border-radius: 4px; padding: 0 4px; }
.mi-close { border: none; background: none; color: var(--text-3); font-size: 15px; line-height: 1; padding: 0 2px; cursor: pointer; }
.mi-close:hover { color: var(--text-1); }
.mi-line { font-size: 12px; color: var(--text-2); }
.mi-actions { display: flex; gap: 6px; margin-top: 3px; }
.mi-actions .mini-btn { padding: 5px 8px; white-space: nowrap; }

/* 地图内右侧控制 */
.map-ctrl {
  position: absolute;
  top: 12px;
  right: 12px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.ctrl-btn {
  width: 32px;
  height: 32px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 8px;
  color: var(--text-2);
  font-size: 16px;
  cursor: pointer;
  transition: background 0.15s, color 0.15s, border-color 0.15s;
}
.ctrl-btn:hover { background: var(--panel-hover); background: color-mix(in srgb, var(--feat-solid) 84%, var(--accent)); color: var(--text-1); border-color: var(--accent-35); }

/* 悬停群系面板 + 加载进度 */
.biome-dot { width: 10px; height: 10px; border-radius: 3px; flex: none; box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.35); }
.biome-name { font-size: 13px; color: var(--text-1); }
.map-progress {
  position: absolute;
  right: 12px;
  bottom: 12px;
  width: min(300px, calc(100% - 24px));
  padding: 8px 12px 10px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  border-radius: 10px;
}
.progress-head { display: flex; align-items: center; justify-content: space-between; font-size: 12px; color: var(--text-3); }
.progress-track { height: 5px; border-radius: 3px; background: var(--panel-hover); background: color-mix(in srgb, var(--text-3) 22%, var(--feat-solid)); overflow: hidden; }
.progress-track i { display: block; height: 100%; border-radius: 3px; background: var(--accent); transition: width 0.18s ease; }

/* —— 地图 —— */
/* canvas 必须用绝对定位填满：`.map-wrap` 的高度来自 flex，而 canvas 是
   替换元素，`height: 100%` 在这种「高度由 min-height/flex 决定」的父级里
   解析不出来，Chrome 会回退成按固有宽高比（canvas 属性尺寸）算高度 ——
   窗口拉高后地图就撑不满，下面留出一条空行。*/
.map-wrap { flex: 1 1 0; min-height: 140px; padding: 0; position: relative; overflow: hidden; }
.map-wrap:fullscreen { border-radius: 0; border: none; }
.biome-canvas { position: absolute; inset: 0; display: block; width: 100%; height: 100%; image-rendering: pixelated; cursor: grab; touch-action: none; }
/* 悬停在结构图钉上：可点的提示 */
.biome-canvas.over-struct { cursor: pointer; }
.biome-canvas:active { cursor: grabbing; }
.map-coord { position: absolute; top: 12px; right: 54px; padding: 3px 8px; border-radius: 7px; font-size: 12px; color: var(--text-2); pointer-events: none; }
.map-tooltip { position: absolute; display: flex; align-items: center; gap: 8px; padding: 7px 10px; border-radius: 10px; pointer-events: none; white-space: nowrap; }
.map-tooltip .coord { font-size: 12px; color: var(--text-3); font-variant-numeric: tabular-nums; }

/* —— 导入存档 —— */
.import-body { display: flex; flex-direction: column; gap: 14px; }
.import-footer { display: flex; justify-content: flex-end; gap: 10px; }
.muted { color: var(--text-3); font-size: 13px; }
.muted.tiny { font-size: 11px; line-height: 1.5; }
.muted.small { font-size: 12px; }
</style>
