<script setup lang="ts">
/**
 * 实例详情 · 按键绑定 tab（标准 108 键可视化键盘）。
 * 三条绑定路径：点键帽 → 面板设置；点动作 → 监听态（按物理键或点键帽）；
 * 物理键直按。改动攒批统一保存，后端只写提交的动作。
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { listen } from "@tauri-apps/api/event";
import { useI18n } from "vue-i18n";
import { NSelect, useMessage } from "naive-ui";
import { api } from "../../api";
import { UNBOUND, codeToMc, mcKeyLabel } from "../../utils/minecraftKeys";
import {
  actionOwner,
  keybindGroupOf,
  keybindSort,
  useKeybindLabels,
} from "../../composables/useKeybindLabels";

const props = defineProps<{ instanceId: string }>();
const { t } = useI18n();
const message = useMessage();
const { actionLabel: dictActionLabel } = useKeybindLabels();

/** 动作中文名：优先游戏语言文件里的词条（与游戏内一致），没有的回落内置字典 */
const mcLabels = ref<Record<string, string>>({});
/** 模组 id（小写）集合，用于判断动作属于哪个模组 */
const modIds = ref<Set<string>>(new Set());
/** 模组 id（小写）→ 显示名 */
const modNames = ref<Map<string, string>>(new Map());
/** 模组中文名（走内容中心同一套翻译服务异步补，失败保持原名） */
const zhNames = ref<Map<string, string>>(new Map());
let zhRequested = false;
/** 卡片标题：中文名优先，其次 jar 里的显示名，最后 modid */
function modTitle(id: string): string {
  return zhNames.value.get(id) ?? modNames.value.get(id) ?? id;
}
/** 卡片副标题：有中文名时把原名也带上，方便对照 */
function modSubtitle(id: string): string {
  const orig = modNames.value.get(id) ?? id;
  const zh = zhNames.value.get(id);
  return zh && zh !== orig ? `${orig} · ${id}` : id;
}
function actionLabel(action: string): string {
  return mcLabels.value[`key.${action}`] ?? dictActionLabel(action);
}

const loading = ref(true);
/** 扫描 jar（读中文名 + 模组列表）进行中：模组多时要几十秒，期间显示进度条而不是半成品列表 */
const scanning = ref(false);
/** 真正把进度条显示出来：命中缓存时请求很快返回，不要闪一下 */
const showScan = ref(false);
const scanDone = ref(0);
const scanTotal = ref(0);
const scanPercent = computed(() =>
  scanTotal.value > 0 ? Math.min(100, Math.round((scanDone.value / scanTotal.value) * 100)) : null,
);
let unlistenScan: (() => void) | null = null;
const exists = ref(false);
/** options.txt 里的当前值：动作 → 按键码 */
const saved = ref<Record<string, string>>({});
/** 未保存的修改：动作 → 按键码 */
const pending = ref<Record<string, string>>({});
/** 监听中的动作（点动作列表后，按物理键或点键盘键帽都能绑定） */
const listening = ref<string | null>(null);
/** 键盘上被点选的键（MC 按键码） */
const selectedKey = ref<string | null>(null);
/** 面板里正在挑动作 */
const picking = ref(false);
const pickingSearch = ref("");
const saving = ref(false);
const search = ref("");

async function load() {
  loading.value = true;
  try {
    const res = await api.optionsListKeybinds(props.instanceId);
    exists.value = res.exists;
    const map: Record<string, string> = {};
    for (const b of res.binds) map[b.action] = b.key;
    saved.value = map;
    pending.value = {};
    // 中文名 + 模组列表单独异步补（要扫 jar，别拖慢按键列表显示）
    zhRequested = false;
    zhNames.value = new Map();
    scanning.value = true;
    showScan.value = false;
    scanDone.value = 0;
    scanTotal.value = 0;
    // 后端命中文件指纹缓存时是毫秒级返回，延迟一点再显示进度条，避免闪一下
    const scanTimer = window.setTimeout(() => {
      if (scanning.value) showScan.value = true;
    }, 250);
    api
      .keybindActionLabels(props.instanceId)
      .then((r) => {
        mcLabels.value = r.labels;
        modIds.value = new Set(r.mods.map((m) => m.id.toLowerCase()));
        modNames.value = new Map(r.mods.map((m) => [m.id.toLowerCase(), m.name]));
        void ensureZhNames();
      })
      .catch(() => {
        /* 读不到就回落到内置字典 */
      })
      .finally(() => {
        scanning.value = false;
        showScan.value = false;
        window.clearTimeout(scanTimer);
      });
  } catch (e) {
    message.error(String(e));
  } finally {
    loading.value = false;
  }
}
onMounted(async () => {
  // 先挂监听再 load，避免错过第一批扫描进度事件
  unlistenScan = await listen<{ done: number; total: number }>("keybind://scan", (e) => {
    scanDone.value = e.payload.done;
    scanTotal.value = e.payload.total;
  });
  load();
});
onBeforeUnmount(() => {
  unlistenScan?.();
  unlistenScan = null;
});

const allActions = computed(() => {
  const set = new Set([...Object.keys(saved.value), ...Object.keys(pending.value)]);
  return [...set].sort(keybindSort);
});

function keyOf(action: string): string {
  const k = pending.value[action] ?? saved.value[action];
  // options.txt 里未绑定的动作值是空字符串，必须归一到 UNBOUND，
  // 否则 98 个未绑定动作会被当成"共用一个键"而全部标红冲突。
  return !k || k === UNBOUND ? UNBOUND : k;
}

// ---- 真实 108 键布局 ----
// 1u = 44px（键帽 40px 正方形 + 4px 键距），F 区与主键区间隔 16px，块间距 22px
const U = 44;
const GAP = 4;
const KEY_SIZE = U - GAP; // 40
const F_ROW_GAP = 16;
const BLOCK_GAP = 22;
const PAD = 14; // 键盘外壳内边距
const MAIN_W = 15 * U - GAP; // 656（主键区 15u）
const NAV_W = 3 * U - GAP; // 128（导航区 3u）
const NUM_W = 4 * U - GAP; // 172（小键盘 4u）

interface BoardKey {
  code: string;
  label: string;
  /** 宽度（相对单位） */
  w: number;
  /** 鼠标键直接给 MC 按键码（不走 KeyboardEvent.code 映射） */
  mc?: string;
  /** 网格跨行/跨列（数字小键盘用） */
  rs?: number;
  cs?: number;
  /** 纯装饰键（多媒体键，游戏内无法绑定） */
  decor?: boolean;
  /** 文字居中（F 键、导航区、小键盘按参考图居中；数字/字母在左上角） */
  center?: boolean;
}
/** 空位：按 u 指定宽度（F 区分组间隔、Esc 与 F1 间隔） */
interface Spacer {
  spacer: number;
}
type Cell = BoardKey | Spacer;
function k(code: string, label: string, w = 1, extra?: Partial<BoardKey>): BoardKey {
  return { code, label, w, ...extra };
}
const sp = (u: number): Spacer => ({ spacer: u });
const isSpacer = (c: Cell): c is Spacer => "spacer" in c;
const K = (n: number) => k(`F${n}`, `F${n}`, 1, { center: true });

/** F 区（含 Esc）：1 + 1 + 4 + 0.5 + 4 + 0.5 + 4 = 15u，与主键区等宽 */
const F_ROW: Cell[] = [
  k("Escape", "Esc"),
  sp(1),
  K(1), K(2), K(3), K(4),
  sp(0.5),
  K(5), K(6), K(7), K(8),
  sp(0.5),
  K(9), K(10), K(11), K(12),
];

/** 主键区：5 行 × 15u（F 区单独一行，见 F_ROW） */
const MAIN_ROWS: Cell[][] = [
  [
    k("Backquote", "`"),
    ...[1, 2, 3, 4, 5, 6, 7, 8, 9, 0].map((d) => k(`Digit${d}`, `${d}`)),
    k("Minus", "-"),
    k("Equal", "="),
    k("Backspace", "⌫", 2),
  ],
  [
    k("Tab", "Tab", 1.5),
    ..."QWERTYUIOP".split("").map((c) => k(`Key${c}`, c)),
    k("BracketLeft", "["),
    k("BracketRight", "]"),
    k("Backslash", "\\", 1.5),
  ],
  [
    k("CapsLock", "Caps", 1.75),
    ..."ASDFGHJKL".split("").map((c) => k(`Key${c}`, c)),
    k("Semicolon", ";"),
    k("Quote", "'"),
    k("Enter", "Enter", 2.25),
  ],
  [
    k("ShiftLeft", "Shift", 2.25),
    ..."ZXCVBNM".split("").map((c) => k(`Key${c}`, c)),
    k("Comma", ","),
    k("Period", "."),
    k("Slash", "/"),
    k("ShiftRight", "Shift", 2.75),
  ],
  [
    k("ControlLeft", "Ctrl", 1.25),
    k("MetaLeft", "Win", 1.25),
    k("AltLeft", "Alt", 1.25),
    k("Space", "Space", 6.25),
    k("AltRight", "Alt", 1.25),
    k("MetaRight", "Win", 1.25),
    k("ContextMenu", "Menu", 1.25),
    k("ControlRight", "Ctrl", 1.25),
  ],
];

/** 导航区：3 列 × 6 行（PrtSc 行与 F 区同高、Ins 行与数字行同高、
 *  Del 行与 Tab 行同高、空一行、↑ 与 Shift 行同高、←↓→ 与底行同高） */
const NAV_ROWS: Cell[][] = [
  [k("PrintScreen", "PrtSc", 1, { center: true }), k("ScrollLock", "ScrLk", 1, { center: true }), k("Pause", "Pause", 1, { center: true })],
  [k("Insert", "Ins", 1, { center: true }), k("Home", "Home", 1, { center: true }), k("PageUp", "PgUp", 1, { center: true })],
  [k("Delete", "Del", 1, { center: true }), k("End", "End", 1, { center: true }), k("PageDown", "PgDn", 1, { center: true })],
  [sp(1), sp(1), sp(1)],
  [sp(1), k("ArrowUp", "↑", 1, { center: true }), sp(1)],
  [k("ArrowLeft", "←", 1, { center: true }), k("ArrowDown", "↓", 1, { center: true }), k("ArrowRight", "→", 1, { center: true })],
];

/** 数字小键盘：4 列网格（NumLock 行与数字行对齐，+ 与 Enter 跨两行，0 跨两列） */
const NUMPAD: BoardKey[] = [
  k("NumLock", "Num", 1, { center: true }),
  k("NumpadDivide", "÷", 1, { center: true }),
  k("NumpadMultiply", "×", 1, { center: true }),
  k("NumpadSubtract", "−", 1, { center: true }),
  k("Numpad7", "7", 1, { center: true }),
  k("Numpad8", "8", 1, { center: true }),
  k("Numpad9", "9", 1, { center: true }),
  k("NumpadAdd", "+", 1, { rs: 2, center: true }),
  k("Numpad4", "4", 1, { center: true }),
  k("Numpad5", "5", 1, { center: true }),
  k("Numpad6", "6", 1, { center: true }),
  k("Numpad1", "1", 1, { center: true }),
  k("Numpad2", "2", 1, { center: true }),
  k("Numpad3", "3", 1, { center: true }),
  k("NumpadEnter", "Enter", 1, { rs: 2, center: true }),
  k("Numpad0", "0", 1, { cs: 2, center: true }),
  k("NumpadDecimal", ".", 1, { center: true }),
];

/** 鼠标键（攻击 / 使用默认就在鼠标上） */
const MOUSE_KEYS: BoardKey[] = [
  k("mouse.left", "左键", 1, { mc: "key.mouse.left" }),
  k("mouse.middle", "中键", 1, { mc: "key.mouse.middle" }),
  k("mouse.right", "右键", 1, { mc: "key.mouse.right" }),
  k("mouse.4", "侧4", 1, { mc: "key.mouse.4" }),
  k("mouse.5", "侧5", 1, { mc: "key.mouse.5" }),
  k("mouse.6", "侧6", 1, { mc: "key.mouse.6" }),
];

/** 键帽对应的 MC 按键码 */
function mcOf(key: BoardKey): string | null {
  return key.mc ?? codeToMc(key.code);
}

// ---- 设计稿尺寸：整块键盘按固定像素绘制，再整体等比缩放（不横竖各自拉伸） ----
/** 主键区总高：6 行键帽 + 5 个行间距（含 F 行下额外留白）= 272 */
const MAIN_H = KEY_SIZE + GAP + (F_ROW_GAP - GAP) + (5 * KEY_SIZE + 4 * GAP); // 272
/** 键帽区内容尺寸（不含外壳内边距） */
const BOARD_W = MAIN_W + BLOCK_GAP + NAV_W + BLOCK_GAP + NUM_W; // 1000
const BOARD_H = MAIN_H; // 272
/** 含外壳的整块尺寸（用于缩放与占位高度） */
const BOARD_WIDTH = BOARD_W + PAD * 2;
const BOARD_HEIGHT = BOARD_H + PAD * 2;
/** 数字行顶部（相对内容顶）：F 行 + F 行下边距 + 行距 —— 小键盘靠它对齐 */
const NUM_OFFSET = KEY_SIZE + (F_ROW_GAP - GAP) + GAP; // 56

const boardWrap = ref<HTMLElement | null>(null);
const boardScale = ref(1);
let boardRo: ResizeObserver | null = null;

// 键盘容器在 v-if（loading）分支里，挂载时还不存在——
// 观察器必须等 ref 出现后再挂，否则 scale 永远停在初始值、键盘吃不满宽度
watch(boardWrap, (el) => {
  boardRo?.disconnect();
  boardRo = null;
  if (!el) return;
  // 先同步量一次，避免首帧用默认比例
  boardScale.value = Math.min(1.6, el.clientWidth / BOARD_WIDTH);
  // 容器宽度变化时整体等比缩放（保持键帽是标准比例，不单独拉宽/压扁）
  boardRo = new ResizeObserver((entries) => {
    const w = entries[0]?.contentRect.width ?? 0;
    // 上限给足，宽屏下让键盘吃满可用宽度（不然右侧会空一大块）
    if (w > 0) boardScale.value = Math.min(1.6, w / BOARD_WIDTH);
  });
  boardRo.observe(el);
});
onBeforeUnmount(() => boardRo?.disconnect());

/** 同一个按键被几个动作使用（未绑定不计入冲突） */
const keyUseCount = computed(() => {
  const count = new Map<string, number>();
  for (const action of allActions.value) {
    const key = keyOf(action);
    if (key !== UNBOUND) count.set(key, (count.get(key) ?? 0) + 1);
  }
  return count;
});

/** 按键码 → 绑定在它上面的动作 */
const actionsByKey = computed(() => {
  const map = new Map<string, string[]>();
  for (const action of allActions.value) {
    const key = keyOf(action);
    if (key === UNBOUND) continue;
    const list = map.get(key) ?? [];
    list.push(action);
    map.set(key, list);
  }
  return map;
});

/** 键帽只用颜色表达状态：灰=未绑定、主题色=已绑定、红=冲突；悬停提示写清绑了什么 */
function keyTooltip(key: BoardKey): string {
  if (key.decor) return t("keybind.mediaHint");
  const mc = mcOf(key);
  const title = t("keybind.keyTitle", { key: mcKeyLabel(mc ?? key.code) });
  const list = mc ? actionsByKey.value.get(mc) : undefined;
  if (!list?.length) return title;
  return `${title}\n${list.map((a) => actionLabel(a)).join("\n")}`;
}

function boardKeyClass(key: BoardKey) {
  if (key.decor) return { decor: true, wide: true };
  const mc = mcOf(key);
  const wide = key.w > 1.25 || !!key.center;
  if (!mc) return { wide };
  const bound = actionsByKey.value.get(mc);
  return {
    wide,
    bound: !!bound?.length,
    conflict: (keyUseCount.value.get(mc) ?? 0) > 1,
    selected: selectedKey.value === mc,
  };
}

function numGridStyle(key: BoardKey) {
  const style: Record<string, string> = {};
  if (key.rs) style.gridRow = `span ${key.rs}`;
  if (key.cs) style.gridColumn = `span ${key.cs}`;
  return style;
}

/** 键帽宽度按 1u 精确计算（标准键 1u = 40px 正方形，长键按比例） */
function keyStyle(key: BoardKey) {
  return { width: `${key.w * U - GAP}px`, height: `${KEY_SIZE}px` };
}
function spacerStyle(c: Spacer) {
  // 必须给高度：导航区有一整行是纯 spacer（占位对齐 Caps 行），
  // 不给高度时该行会塌陷成 0，导致后面的方向键整体上移一行、与底排错开。
  return { width: `${c.spacer * U - GAP}px`, height: `${KEY_SIZE}px` };
}

// ---- 动作列表（按模组分卡片）----
interface Row {
  action: string;
  key: string;
  label: string;
  group: string;
  /** 归属：vanilla / mod:<id> / other */
  owner: string;
  conflict: boolean;
}
const rows = computed<Row[]>(() => {
  const q = search.value.trim().toLowerCase();
  return allActions.value
    .map((action) => {
      const key = keyOf(action);
      return {
        action,
        key,
        label: actionLabel(action),
        group: keybindGroupOf(action),
        owner: actionOwner(action, modIds.value),
        conflict: key !== UNBOUND && (keyUseCount.value.get(key) ?? 0) > 1,
      };
    })
    .filter(
      (r) =>
        !q ||
        r.label.toLowerCase().includes(q) ||
        r.action.toLowerCase().includes(q) ||
        mcKeyLabel(r.key).toLowerCase().includes(q)
    );
});

/** 模组卡片：原版排第一，模组按名称，无法归属的垫底 */
interface ModCard {
  /** vanilla / mod:<id> / other */
  key: string;
  title: string;
  subtitle: string;
  rows: Row[];
}
const cards = computed<ModCard[]>(() => {
  const out = new Map<string, ModCard>();
  for (const r of rows.value) {
    let card = out.get(r.owner);
    if (!card) {
      let title = "Minecraft 原版";
      let subtitle = "vanilla";
      if (r.owner.startsWith("mod:")) {
        const id = r.owner.slice(4);
        title = modTitle(id);
        subtitle = modSubtitle(id);
      } else if (r.owner === "other") {
        title = "未归类按键";
        subtitle = "无法判断所属模组（可能是模组未按规范命名）";
      }
      card = { key: r.owner, title, subtitle, rows: [] };
      out.set(r.owner, card);
    }
    card.rows.push(r);
  }
  const order = (k: string) => (k === "vanilla" ? 0 : k === "other" ? 2 : 1);
  return [...out.values()].sort(
    (a, b) => order(a.key) - order(b.key) || a.title.localeCompare(b.title),
  );
});

/** 模组筛选：下拉可选择，也可直接输入过滤（只显示有按键的模组） */
const selectedMod = ref<string | null>(null);
const modOptions = computed(() =>
  cards.value.map((c) => ({
    label: `${c.title}（${c.rows.length}）`,
    value: c.key,
  })),
);
const visibleCards = computed(() => {
  if (!selectedMod.value) return cards.value;
  return cards.value.filter((c) => c.key === selectedMod.value);
});

// ---- 卡片瀑布流：按顺序放进"当前最矮的列"，卡片高低不齐也不会留空 ----
const stackEl = ref<HTMLElement | null>(null);
const stackWidth = ref(0);
let stackRo: ResizeObserver | null = null;
/** 卡片最小宽度与列间距（列数按容器实际宽度算，窗口任意大小都能自适应） */
const CARD_MIN_W = 300;
const CARD_GAP = 12;
/** 单行动作的高度（.row 的内边距 + 两行文字），用来估算卡片高度 */
const ROW_H = 49;
const columnCount = computed(() => {
  const w = stackWidth.value;
  if (!w) return 1;
  return Math.max(1, Math.floor((w + CARD_GAP) / (CARD_MIN_W + CARD_GAP)));
});
function cardHeight(c: ModCard): number {
  return 46 + c.rows.length * ROW_H + 12;
}
const cardColumns = computed<ModCard[][]>(() => {
  const n = columnCount.value;
  const cols: ModCard[][] = Array.from({ length: n }, () => []);
  const heights = new Array<number>(n).fill(0);
  for (const c of visibleCards.value) {
    let idx = 0;
    for (let i = 1; i < n; i++) {
      if (heights[i] < heights[idx]) idx = i;
    }
    cols[idx].push(c);
    heights[idx] += cardHeight(c) + CARD_GAP;
  }
  return cols;
});
// 卡片容器在 v-if（loading / exists 判定）里，挂载时还不存在——
// 不能在 onMounted 里 observe（会直接跳过，列数永远算成 1），要用 watch 挂观察器
watch(stackEl, (el, _old, onCleanup) => {
  stackRo?.disconnect();
  stackRo = null;
  if (!el) return;
  // 先同步量一次，避免首帧只渲染成一列
  stackWidth.value = el.clientWidth;
  stackRo = new ResizeObserver((entries) => {
    stackWidth.value = entries[0]?.contentRect.width ?? 0;
  });
  stackRo.observe(el);
  onCleanup(() => {
    stackRo?.disconnect();
    stackRo = null;
  });
});
/** 下拉里输入时：模组名和 modid 都能匹配 */
function filterMod(pattern: string, option: { value?: unknown }) {
  const q = pattern.trim().toLowerCase();
  if (!q) return true;
  const card = cards.value.find((c) => c.key === option.value);
  if (!card) return false;
  return card.title.toLowerCase().includes(q) || card.subtitle.toLowerCase().includes(q);
}

/** 列表里实际出现的模组（只翻这些，避免把 300+ 个模组名全翻一遍） */
const usedModIds = computed(() => {
  const s = new Set<string>();
  for (const a of allActions.value) {
    const owner = actionOwner(a, modIds.value);
    if (owner.startsWith("mod:")) s.add(owner.slice(4));
  }
  return [...s].sort();
});

/** 请求模组中文名（每个实例只请求一次；失败静默保持原名） */
async function ensureZhNames() {
  if (zhRequested || !modNames.value.size) return;
  const idByName = new Map<string, string>();
  const names: string[] = [];
  for (const id of usedModIds.value) {
    const n = modNames.value.get(id);
    if (n && !idByName.has(n)) {
      idByName.set(n, id);
      names.push(n);
    }
  }
  if (!names.length) return;
  zhRequested = true;
  try {
    const r = await api.translateModNames(names);
    if (r.unsupported) return;
    const m = new Map(zhNames.value);
    for (const [rawKey, zh] of Object.entries(r.translations ?? {})) {
      // 模型偶尔把列表符号/空白带进键名，这里宽容匹配
      const key = rawKey.trim();
      const id = idByName.get(key) ?? idByName.get(key.replace(/^[-*•\s]+/, ""));
      if (id && zh && zh !== key) m.set(id, zh);
    }
    zhNames.value = m;
  } catch {
    /* 翻译服务不可用时保持英文名 */
  }
}
watch(usedModIds, () => void ensureZhNames());

const conflictCount = computed(() => rows.value.filter((r) => r.conflict).length);
const dirty = computed(() => Object.keys(pending.value).length);

// ---- 交互：点键盘 / 点动作 / 按物理键 三条路都能绑定 ----
/** 监听期间吞掉随后的 click，防止松开鼠标那一下又把自己点进监听态 */
let clickGuard = 0;

function onBoardKey(key: BoardKey) {
  if (Date.now() < clickGuard) return;
  const mc = mcOf(key);
  if (!mc) return;
  if (listening.value) {
    assign(listening.value, mc);
    clickGuard = Date.now() + 350;
    return;
  }
  if (selectedKey.value === mc) {
    selectedKey.value = null;
    return;
  }
  selectedKey.value = mc;
  picking.value = false;
  pickingSearch.value = "";
}

/** 点动作行：进入监听态（再点一次取消） */
function toggleListen(action: string) {
  if (Date.now() < clickGuard) return;
  listening.value = listening.value === action ? null : action;
  if (!listening.value) return;
  // 从物理键盘捕获
  window.addEventListener("keydown", onKeydown, true);
  window.addEventListener("mousedown", onMousedown, true);
  window.addEventListener("contextmenu", preventEvent, true);
}

function stopListen() {
  listening.value = null;
  window.removeEventListener("keydown", onKeydown, true);
  window.removeEventListener("mousedown", onMousedown, true);
  window.removeEventListener("contextmenu", preventEvent, true);
}

function preventEvent(e: Event) {
  e.preventDefault();
  e.stopPropagation();
}

function onKeydown(e: KeyboardEvent) {
  preventEvent(e);
  const action = listening.value;
  if (!action) return stopListen();
  if (e.key === "Escape") return stopListen();
  if (e.code === "Delete" || e.code === "Backspace") return assign(action, UNBOUND);
  const mc = codeToMc(e.code);
  if (mc) {
    assign(action, mc);
    clickGuard = Date.now() + 350;
  }
  // 识别不了的键（多媒体键等）保持监听
}

function onMousedown(e: MouseEvent) {
  preventEvent(e);
  // 点在键盘上时不能停：交给 click 走 onBoardKey 完成绑定；点在别处 = 取消
  const el = e.target as HTMLElement | null;
  if (!el?.closest(".kb-board")) stopListen();
}

function assign(action: string, key: string) {
  pending.value = { ...pending.value, [action]: key };
  stopListen();
}
onBeforeUnmount(stopListen);

// ---- 选中键的绑定面板 ----
const selectedBindings = computed(() => (selectedKey.value ? actionsByKey.value.get(selectedKey.value) ?? [] : []));

/** 选中键上的绑定按模组分组（面板里要能看出每个动作属于哪个模组） */
const selectedGroups = computed(() => {
  const map = new Map<string, { title: string; actions: string[] }>();
  for (const action of selectedBindings.value) {
    const owner = actionOwner(action, modIds.value);
    const id = owner.startsWith("mod:") ? owner.slice(4) : owner;
    const title =
      owner === "vanilla"
        ? "Minecraft 原版"
        : owner === "other"
          ? "未归类按键"
          : modTitle(id);
    const key = `${owner}|${title}`;
    const g = map.get(key) ?? { title, actions: [] };
    g.actions.push(action);
    map.set(key, g);
  }
  return [...map.values()].sort((a, b) => a.title.localeCompare(b.title));
});

/** 面板里可挑的动作（按搜索过滤；已绑在此键的排前面并标记） */
const pickable = computed(() => {
  const q = pickingSearch.value.trim().toLowerCase();
  return allActions.value
    .map((action) => ({ action, label: actionLabel(action), here: keyOf(action) === selectedKey.value }))
    .filter((r) => !q || r.label.toLowerCase().includes(q) || r.action.toLowerCase().includes(q))
    .sort((a, b) => Number(b.here) - Number(a.here) || a.label.localeCompare(b.label));
});

function bindToSelected(action: string) {
  if (!selectedKey.value) return;
  assign(action, selectedKey.value);
  picking.value = false;
}

function unbindAction(action: string) {
  pending.value = { ...pending.value, [action]: UNBOUND };
}

/** 清除这个键上的全部绑定 */
function clearSelectedKey() {
  for (const action of selectedBindings.value) unbindAction(action);
}

async function save() {
  if (!dirty.value || saving.value) return;
  saving.value = true;
  try {
    await api.optionsSetKeybinds(props.instanceId, { ...pending.value });
    message.success(t("keybind.saved"));
    await load();
  } catch (e) {
    message.error(String(e));
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <div class="keybind-tab">
    <div v-if="loading" class="empty glass">{{ t("keybind.loading") }}</div>

    <div v-else-if="!exists" class="empty glass">
      <p>{{ t("keybind.missingFile") }}</p>
      <p class="sub">{{ t("keybind.missingFileHint") }}</p>
    </div>

    <template v-else>
      <div class="bar glass">
        <span v-if="conflictCount" class="conflict-badge" :title="t('keybind.conflictHint')">
          {{ t("keybind.conflicts", { n: conflictCount }) }}
        </span>
        <span class="spacer" />
        <span v-if="dirty" class="dirty">{{ t("keybind.dirty", { n: dirty }) }}</span>
        <button v-if="dirty" class="mini-btn" @click="pending = {}">{{ t("keybind.discard") }}</button>
        <button class="mini-btn primary" :disabled="!dirty || saving" @click="save">
          {{ saving ? t("keybind.saving") : t("keybind.save") }}
        </button>
      </div>

      <div v-if="listening" class="listening-tip glass">
        {{ t("keybind.listeningTip", { action: actionLabel(listening) }) }}
        <span class="sub">{{ t("keybind.pressEsc") }}</span>
      </div>

      <!-- 键盘 + 键位面板：键盘占左侧自适应区，面板贴右侧填空 -->
      <div class="board-row">
        <div class="board-col">
      <!-- 标准 108 键（固定设计尺寸 + 等比缩放） -->
      <div ref="boardWrap" class="board-wrap">
        <!-- 占位尺寸 = 缩放后的视觉尺寸：键盘本体宽 1000px 是全尺寸设计稿，
             只用 transform 缩放时布局占位仍是 1000px（且默认从中心缩放），
             窗口变窄时会溢出压到右侧面板上 -->
        <div
          class="board-scale"
          :style="{
            width: BOARD_WIDTH * boardScale + 'px',
            height: BOARD_HEIGHT * boardScale + 'px',
          }"
        >
          <div
            class="kb-board glass"
            :class="{ targeting: !!listening }"
            :style="{
              width: BOARD_W + 'px',
              height: BOARD_H + 'px',
              padding: PAD + 'px',
              transform: `scale(${boardScale})`,
              transformOrigin: 'top left',
            }"
          >
            <!-- 主体：主键区 | 导航区 | 小键盘 -->
            <div class="kb-body" :style="{ gap: BLOCK_GAP + 'px' }">
              <!-- 主键区（F 区一行 + 5 行） -->
              <div class="block main" :style="{ width: MAIN_W + 'px' }">
                <div class="kb-row" :style="{ gap: GAP + 'px', marginBottom: F_ROW_GAP - GAP + 'px' }">
                  <template v-for="(c, ci) in F_ROW" :key="ci">
                    <div v-if="isSpacer(c)" class="kb-spacer" :style="spacerStyle(c)" />
                    <button
                      v-else
                      class="kb-key"
                      :style="keyStyle(c)"
                      :class="boardKeyClass(c)"
                      :title="keyTooltip(c)"
                      @click="onBoardKey(c)"
                    >
                      <span class="cap">{{ c.label }}</span>
                    </button>
                  </template>
                </div>
                <div v-for="(row, ri) in MAIN_ROWS" :key="ri" class="kb-row" :style="{ gap: GAP + 'px' }">
                  <button
                    v-for="(c, ci) in row"
                    :key="ci"
                    class="kb-key"
                    :style="keyStyle(c as BoardKey)"
                    :class="boardKeyClass(c as BoardKey)"
                    :title="keyTooltip(c as BoardKey)"
                    @click="onBoardKey(c as BoardKey)"
                  >
                    <span class="cap">{{ (c as BoardKey).label }}</span>
                  </button>
                </div>
              </div>

              <!-- 导航区（3 列 × 6 行，逐行与主键区对齐：
                   PrtSc↔F 区、Ins↔数字行、Del↔Tab 行、空↔Caps 行、↑↔Shift 行、←↓→↔Ctrl 行） -->
              <div class="block nav" :style="{ width: NAV_W + 'px' }">
                <div
                  v-for="(row, ri) in NAV_ROWS"
                  :key="ri"
                  class="kb-row"
                  :style="{
                    gap: GAP + 'px',
                    marginTop: ri === 1 ? F_ROW_GAP - GAP + 'px' : '0px',
                  }"
                >
                  <template v-for="(c, ci) in row" :key="ci">
                    <div v-if="isSpacer(c)" class="kb-spacer" :style="spacerStyle(c)" />
                    <button
                      v-else
                      class="kb-key"
                      :style="keyStyle(c)"
                      :class="boardKeyClass(c)"
                      :title="keyTooltip(c)"
                      @click="onBoardKey(c)"
                    >
                      <span class="cap">{{ c.label }}</span>
                    </button>
                  </template>
                </div>
              </div>

              <!-- 数字小键盘（顶部与数字行对齐） -->
              <div class="block num" :style="{ width: NUM_W + 'px', marginTop: NUM_OFFSET + 'px' }">
                <div class="num-grid" :style="{ gap: GAP + 'px' }">
                  <button
                    v-for="c in NUMPAD"
                    :key="c.code"
                    class="kb-key"
                    :style="{ ...numGridStyle(c), height: c.rs ? `${c.rs * U - GAP}px` : `${KEY_SIZE}px` }"
                    :class="boardKeyClass(c)"
                    :title="keyTooltip(c)"
                    @click="onBoardKey(c)"
                  >
                    <span class="cap">{{ c.label }}</span>
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- 鼠标键 + 提示：宽度跟随键盘的视觉宽度，左右边缘与键盘对齐 -->
      <div class="board-sub" :style="{ width: BOARD_WIDTH * boardScale + 'px' }">
        <div class="mouse-board" :class="{ targeting: !!listening }">
          <span class="mouse-label">🖱 鼠标</span>
          <button
            v-for="c in MOUSE_KEYS"
            :key="c.code"
            class="kb-key mouse-key"
            :class="boardKeyClass(c)"
            :title="keyTooltip(c)"
            @click="onBoardKey(c)"
          >
            <span class="cap">{{ c.label }}</span>
          </button>
        </div>

        <div class="board-hint">{{ t("keybind.boardHint") }}</div>
      </div>
        </div>

        <!-- 选中键面板（键盘右侧，把原先的空白利用起来） -->
        <div class="panel-col">
          <div v-if="selectedKey" class="key-panel glass">
            <div class="kp-head">
              <span class="kp-title">{{ t("keybind.keyTitle", { key: mcKeyLabel(selectedKey) }) }}</span>
              <button class="kp-close" @click="selectedKey = null">✕</button>
            </div>

            <template v-if="!picking">
              <div v-if="!selectedBindings.length" class="kp-none">{{ t("keybind.none") }}</div>
              <!-- 绑定按模组分组，一眼看出动作来自哪个模组 -->
              <div v-else class="kp-groups">
                <div v-for="g in selectedGroups" :key="g.title" class="kp-group">
                  <span class="kp-group-title">{{ g.title }}</span>
                  <div class="kp-binds">
                    <span v-for="a in g.actions" :key="a" class="kp-chip">
                      {{ actionLabel(a) }}
                      <button class="kp-x" :title="t('keybind.unbind')" @click="unbindAction(a)">✕</button>
                    </span>
                  </div>
                </div>
              </div>
              <div class="kp-actions">
                <button class="mini-btn primary" @click="picking = true; pickingSearch = ''">
                  {{ t("keybind.bindAction") }}
                </button>
                <button v-if="selectedBindings.length" class="mini-btn" @click="clearSelectedKey">
                  {{ t("keybind.clearKey") }}
                </button>
              </div>
            </template>

            <template v-else>
              <p class="kp-pick-hint">{{ t("keybind.pickHint", { key: mcKeyLabel(selectedKey) }) }}</p>
              <input v-model="pickingSearch" class="text-input" :placeholder="t('keybind.search')" />
              <div class="kp-list">
                <button
                  v-for="r in pickable"
                  :key="r.action"
                  class="kp-item"
                  :class="{ here: r.here }"
                  @click="bindToSelected(r.action)"
                >
                  {{ r.label }}<span v-if="r.here" class="kp-here">{{ t("keybind.here") }}</span>
                </button>
                <p v-if="!pickable.length" class="kp-none">{{ t("keybind.noResult") }}</p>
              </div>
            </template>
          </div>
          <div v-else class="panel-hint glass">{{ t("keybind.panelHint") }}</div>
        </div>
      </div>

      <!-- 动作列表：每个模组一张卡片，可筛选模组名（整宽多列，往下堆） -->
      <div class="list-col">
          <div class="list-filters glass">
            <input
              v-model="search"
              class="text-input"
              :placeholder="t('keybind.search')"
            />
            <NSelect
              v-model:value="selectedMod"
              class="mod-select"
              :options="modOptions"
              :filter="filterMod"
              :menu-props="{ class: 'qk-select-menu' }"
              filterable
              clearable
              size="small"
              placeholder="按模组筛选（可输入）"
            />
          </div>
          <!-- 扫描 jar 期间：不渲染卡片（否则会出现"没分组 / 没排序"的半成品）。
               命中缓存时很快返回，此时只留一个占位，不闪进度条。 -->
          <template v-if="scanning">
            <div v-if="showScan" class="scan-box glass">
              <div class="scan-head">
                <span>正在读取模组与按键中文名…</span>
                <span v-if="scanPercent != null" class="scan-pct">{{ scanPercent }}%</span>
              </div>
              <div class="scan-track">
                <div
                  v-if="scanPercent != null"
                  class="scan-fill"
                  :style="{ width: scanPercent + '%' }"
                ></div>
                <div v-else class="scan-fill indet"></div>
              </div>
              <div v-if="scanTotal" class="scan-sub">
                已扫描 {{ scanDone }} / {{ scanTotal }} 个文件（模组多时需要一点时间）
              </div>
              <div v-else class="scan-sub">正在列出模组文件…</div>
            </div>
            <div v-else class="scan-holder"></div>
          </template>

          <div v-else ref="stackEl" class="card-stack">
            <div v-for="(col, ci) in cardColumns" :key="ci" class="card-col">
              <div v-for="c in col" :key="c.key" class="mod-card glass">
                <div class="mod-card-head">
                  <span class="mod-card-title">{{ c.title }}</span>
                  <span class="mod-card-count">{{ c.rows.length }}</span>
                </div>
                <div class="mod-card-sub">{{ c.subtitle }}</div>
                <div class="mod-card-rows">
                  <div
                    v-for="r in c.rows"
                    :key="r.action"
                    class="row"
                    :class="{ listening: listening === r.action, conflict: r.conflict }"
                    @click="toggleListen(r.action)"
                  >
                    <div class="names">
                      <span class="label">{{ r.label }}</span>
                      <span class="action">{{ r.action }}</span>
                    </div>
                    <span class="row-key" :class="{ unbound: r.key === UNBOUND }">
                      {{ listening === r.action ? t("keybind.listening") : mcKeyLabel(r.key) }}
                    </span>
                  </div>
                </div>
              </div>
            </div>
            <div v-if="!visibleCards.length" class="kp-none pad">没有匹配的按键</div>
          </div>
        </div>
    </template>
  </div>
</template>

<style scoped>
.keybind-tab {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.empty {
  padding: 48px 20px;
  text-align: center;
  border-radius: 12px;
  color: var(--text-2);
  font-size: 14px;
}
.empty .sub {
  margin-top: 6px;
  font-size: 12px;
  color: var(--text-3);
}
.bar {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  border-radius: 12px;
}
.spacer {
  flex: 1;
}
.conflict-badge {
  font-size: 12px;
  color: #e5534b;
  cursor: help;
}
.dirty {
  font-size: 12px;
  color: var(--accent);
}
.mini-btn {
  padding: 7px 13px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: transparent;
  color: var(--text-2);
  font-size: 12px;
  cursor: pointer;
  white-space: nowrap;
}
.mini-btn:disabled {
  opacity: 0.5;
  cursor: default;
}
.mini-btn.primary {
  color: var(--accent);
  border-color: var(--accent-35);
  background: var(--accent-soft);
  font-weight: 600;
}
.listening-tip {
  padding: 10px 14px;
  border-radius: 12px;
  font-size: 13px;
  color: var(--accent);
}
.listening-tip .sub {
  margin-left: 10px;
  font-size: 12px;
  color: var(--text-3);
}

/* 键盘：固定设计尺寸（1u = 44px，键帽 40px 正方形），容器变窄时整体等比缩放 */
.board-wrap {
  width: 100%;
}
.kb-board {
  display: flex;
  flex-direction: column;
  border-radius: 14px;
  /* 键盘底座：比键帽更深，营造外壳包围感 */
  background: rgba(0, 0, 0, 0.22);
  border: 1px solid var(--border);
  box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.06), 0 2px 10px rgba(0, 0, 0, 0.18);
  transform-origin: top left;
  box-sizing: content-box;
}
.kb-body {
  display: flex;
  align-items: flex-start;
}
.block {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.kb-row {
  display: flex;
  width: 100%;
}
.kb-spacer {
  flex-shrink: 0;
}
.kb-key {
  flex-shrink: 0;
  box-sizing: border-box;
  border-radius: 6px;
  /* 键帽质感：四边同色描边 + 下方投影当"厚度"。
     早前用 border-bottom 画厚度，未绑定的键没有强调色边框，
     黑色底边在深色底座上等于看不见，导致键帽像少了下边缘。 */
  border: 1px solid var(--border);
  box-shadow: 0 2px 0 rgba(0, 0, 0, 0.45);
  background: linear-gradient(180deg, var(--w-08), var(--w-04));
  color: var(--text-1);
  cursor: pointer;
  display: flex;
  /* 普通键文字在左上角（真实键帽的印字位置），长键居中 */
  align-items: flex-start;
  justify-content: flex-start;
  padding: 3px 4px;
  overflow: hidden;
  transition: border-color 0.12s, background 0.12s, color 0.12s, transform 0.06s,
    box-shadow 0.06s;
}
/* 长键（≥1.5u）文字居中，与真实键盘一致 */
.kb-key.wide {
  align-items: center;
  justify-content: center;
}
.kb-key .cap {
  /* 中文动作名比英文长，字号略小以便在 40px 键帽里多显示几个字 */
  font-size: 9px;
  line-height: 1.05;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.kb-key:hover {
  border-color: var(--accent);
}
.kb-key:active {
  transform: translateY(1px);
  box-shadow: 0 1px 0 rgba(0, 0, 0, 0.45);
}
/* 状态只用颜色：灰=未绑定、主题色=已绑定、红=冲突 */
.kb-key.bound {
  background: linear-gradient(180deg, var(--accent-35), var(--accent-soft));
  border-color: var(--accent);
}
.kb-key.selected {
  border-color: var(--accent);
  /* 保留键帽厚度，内描边表示"选中这根键" */
  box-shadow: inset 0 0 0 1.5px var(--accent), 0 2px 0 rgba(0, 0, 0, 0.45);
}
.kb-key.conflict {
  background: rgba(229, 83, 75, 0.28);
  border-color: #e5534b;
  color: #ffb4ad;
}
/* 多媒体键：纯装饰，游戏内无法绑定 */
.kb-key.decor {
  cursor: default;
  opacity: 0.85;
}
.kb-key.decor .cap {
  font-size: 15px;
}
.kb-key.decor:hover {
  border-color: var(--border);
  transform: none;
}
.kb-board.targeting .kb-key {
  border-style: dashed;
  border-color: var(--accent-35);
}
.kb-board.targeting .kb-key:hover {
  border-style: solid;
  border-color: var(--accent);
}
.num-grid {
  display: grid;
  grid-template-columns: repeat(4, 40px);
  grid-auto-rows: 40px;
}
.mouse-board {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 10px 14px;
  border-radius: 14px;
  background: rgba(0, 0, 0, 0.22);
  border: 1px solid var(--border);
}
.mouse-label {
  font-size: 12px;
  color: var(--text-3);
  margin-right: 4px;
  flex-shrink: 0;
}
.board-sub {
  display: flex;
  flex-direction: column;
  gap: 8px;
  /* 与键盘左对齐（宽度本来就等于键盘的视觉宽度） */
  margin: 0;
  max-width: 100%;
}
.mouse-board .mouse-key {
  /* 均分整行宽度：鼠标键总宽与键盘一致，不再出现"比键盘长/短" */
  flex: 1 1 0;
  min-width: 0;
  height: 34px;
  align-items: center;
  justify-content: center;
}
.board-hint {
  font-size: 11px;
  color: var(--text-3);
  margin: -4px 2px 0;
}

/* 上排：键盘（自适应缩放）+ 键位面板（贴右，利用宽屏留白） */
.board-row {
  display: flex;
  align-items: stretch;
  gap: 14px;
}
.board-col {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
  /* 缩放值滞后一帧时也不让键盘压到右侧面板上 */
  overflow: hidden;
}
.panel-col {
  /* 宽度随窗口自适应：窄屏 280、宽屏最多 340 */
  flex: 0 0 clamp(280px, 26%, 340px);
  max-width: 340px;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
/* 窄窗口：面板落到键盘下方 */
@media (max-width: 1100px) {
  .board-row {
    flex-direction: column;
  }
  .panel-col {
    flex: 1 1 auto;
    max-width: none;
    width: 100%;
  }
}
.panel-hint {
  padding: 18px 16px;
  border-radius: 12px;
  font-size: 12px;
  line-height: 1.7;
  color: var(--text-3);
  /* 未选中键时把面板撑到与键盘同高，右侧不至于空一大块 */
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  text-align: center;
}

.key-panel {
  padding: 12px;
  border-radius: 12px;
  /* 选中键时面板也撑到与键盘同高，避免右侧下方留一大块空白 */
  flex: 1;
}
.kp-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 8px;
}
.kp-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-1);
}
.kp-close {
  border: none;
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
  font-size: 12px;
  padding: 2px 4px;
}
.kp-close:hover {
  color: var(--text-1);
}
.kp-groups {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-bottom: 10px;
}
.kp-group {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.kp-group-title {
  font-size: 11px;
  font-weight: 600;
  color: var(--text-3);
}
.kp-binds {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.kp-chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 4px 8px;
  border-radius: 7px;
  background: var(--accent-soft);
  border: 1px solid var(--accent-35);
  color: var(--accent);
  font-size: 12px;
}
.kp-x {
  border: none;
  background: transparent;
  color: inherit;
  cursor: pointer;
  font-size: 10px;
  padding: 0;
  opacity: 0.7;
}
.kp-x:hover {
  opacity: 1;
}
.kp-none {
  font-size: 12px;
  color: var(--text-3);
}
.kp-actions {
  display: flex;
  gap: 8px;
}
.kp-pick-hint {
  font-size: 12px;
  color: var(--text-2);
  margin: 0 0 8px;
}
.kp-list {
  margin-top: 8px;
  max-height: 220px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.kp-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 7px 10px;
  border: none;
  border-radius: 7px;
  background: transparent;
  color: var(--text-1);
  font-size: 13px;
  text-align: left;
  cursor: pointer;
}
.kp-item:hover {
  background: var(--accent-soft);
  color: var(--accent);
}
.kp-item.here {
  color: var(--text-3);
  cursor: default;
}
.kp-here {
  font-size: 11px;
  color: var(--accent);
}

/* 动作列表：模组卡片堆叠（页面自然变长，不做内部滚动） */
.list-col {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-width: 0;
}
.list-filters {
  display: flex;
  gap: 8px;
  padding: 8px;
  border-radius: 12px;
  /* 整宽下输入框拉太长很怪，限制一行的最大宽度 */
  max-width: 720px;
}
.list-filters .text-input {
  flex: 1;
  min-width: 0;
  height: 34px;
  box-sizing: border-box;
}
.list-filters .mod-select {
  flex: 1;
  min-width: 0;
}
/* naive-ui Select 外观对齐上面的自定义输入框（同一背景/边框/圆角/高度/字号） */
.list-filters .mod-select :deep(.n-base-selection) {
  --n-height: 34px;
  --n-font-size: 13px;
  --n-border-radius: 9px;
  --n-color: var(--w-06);
  --n-color-active: var(--w-06);
  --n-border: 1px solid var(--border);
  --n-border-hover: 1px solid var(--accent-05);
  --n-border-active: 1px solid var(--accent-05);
  --n-border-focus: 1px solid var(--accent-05);
  --n-box-shadow-active: none;
  --n-box-shadow-focus: none;
  --n-text-color: var(--text-1);
  --n-placeholder-color: var(--text-3);
  --n-arrow-color: var(--text-3);
  --n-arrow-color-active: var(--text-2);
  --n-arrow-size: 14px;
}
/* 扫描进度条（读取模组与中文名期间） */
.scan-holder {
  min-height: 12px;
}
.scan-box {
  padding: 16px;
  border-radius: 12px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.scan-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-size: 12px;
  color: var(--text-1);
}
.scan-pct {
  font-weight: 700;
  color: var(--accent);
}
.scan-track {
  height: 6px;
  border-radius: 3px;
  background: var(--w-08);
  overflow: hidden;
}
.scan-fill {
  height: 100%;
  border-radius: 3px;
  background: var(--accent);
  transition: width 0.2s ease;
}
.scan-fill.indet {
  width: 40%;
  animation: scan-indet 1.1s ease-in-out infinite;
}
@keyframes scan-indet {
  0% {
    margin-left: -40%;
  }
  100% {
    margin-left: 100%;
  }
}
.scan-sub {
  font-size: 11px;
  color: var(--text-3);
}

/* 瀑布流：每列是一个 flex 纵向容器，卡片按"最矮列优先"分配（见 cardColumns），
   高度不齐也不会像 grid 那样在行内留大片空白 */
.card-stack {
  display: flex;
  align-items: flex-start;
  gap: 12px;
}
.card-col {
  flex: 1 1 0;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.mod-card {
  border-radius: 12px;
  padding: 10px 12px 12px;
}
.mod-card-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.mod-card-title {
  font-size: 13px;
  font-weight: 700;
  color: var(--text-1);
}
.mod-card-count {
  font-size: 11px;
  color: var(--accent);
  background: var(--accent-soft);
  border-radius: 999px;
  padding: 1px 8px;
  flex-shrink: 0;
}
.mod-card-sub {
  font-size: 10px;
  color: var(--text-3);
  font-family: ui-monospace, Consolas, monospace;
  margin-bottom: 4px;
}
.mod-card-rows {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.pad {
  padding: 16px;
  text-align: center;
}
.text-input {
  background: var(--w-06);
  border: 1px solid var(--border);
  border-radius: 9px;
  color: var(--text-1);
  padding: 8px 12px;
  font-size: 13px;
  font-family: inherit;
  outline: none;
}
.text-input:focus {
  border-color: var(--accent-05);
}
.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 8px 10px;
  border-radius: 8px;
  cursor: pointer;
  border: 1px solid transparent;
  transition: background 0.12s;
}
.row:hover {
  background: var(--w-06);
}
.row.listening {
  border-color: var(--accent);
  background: var(--accent-soft);
}
.row.listening .label,
.row.listening .row-key {
  color: var(--accent);
}
.row.conflict .label,
.row.conflict .row-key {
  color: #e5534b;
}
.names {
  display: flex;
  flex-direction: column;
  gap: 1px;
  min-width: 0;
  flex: 1;
}
.label {
  font-size: 13px;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.action {
  font-size: 10px;
  color: var(--text-3);
  font-family: ui-monospace, Consolas, monospace;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.row-key {
  flex-shrink: 0;
  font-size: 12px;
  color: var(--text-2);
}
.row-key.unbound {
  color: var(--text-3);
  opacity: 0.7;
}

.text-input {
  background: var(--w-06);
  border: 1px solid var(--border);
  border-radius: 9px;
  color: var(--text-1);
  padding: 8px 12px;
  font-size: 13px;
  font-family: inherit;
  outline: none;
  min-width: 0;
}
.text-input:focus {
  border-color: var(--accent-05);
}
</style>
