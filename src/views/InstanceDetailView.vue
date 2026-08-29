<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch, computed, nextTick } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useInstancesStore } from "../stores/instances";
import { useAccountsStore } from "../stores/accounts";
import { useSettingsStore } from "../stores/settings";
import { usePinsStore } from "../stores/pins";
import { useMessage, NModal, NSelect, NButton } from "naive-ui";
import { supportsQuickPlay } from "../version";
import { api } from "../api";
import { open } from "@tauri-apps/plugin-dialog";
import { convertFileSrc } from "@tauri-apps/api/core";
import LogViewer from "../components/LogViewer.vue";
import AppIcon from "../components/AppIcon.vue";
import IconPickerDialog from "../components/IconPickerDialog.vue";
import { useSlidingIndicator } from "../composables/useSlidingIndicator";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { ContentItem, ProjectVersion, ServerEntry, ServerStatus, UpdateInfo } from "../types";

function sourceLabel(s: string) {
  return s === "modrinth" ? "Modrinth" : s === "curseforge" ? "CurseForge" : s === "modpack" ? "整合包" : "手动";
}
import {
  IconBox,
  IconCamera,
  IconCheck,
  IconClose,
  IconDownload,
  IconExternal,
  IconFile,
  IconFolder,
  IconImage,
  IconLayers,
  IconPlay,
  IconPlus,
  IconRefresh,
  IconRepeat,
  IconSearch,
  IconTrash,
  IconGlobe,
  IconMapPin,
} from "../components/icons";
const route = useRoute();
const router = useRouter();
const instances = useInstancesStore();
const accounts = useAccountsStore();
const message = useMessage();
const pins = usePinsStore();

const confirmState = ref<{ title: string; content: string; positiveText: string; onOk: () => void | Promise<void> } | null>(null);
function confirm(opts: { title: string; content: string; positiveText: string; onOk: () => void | Promise<void> }) {
  confirmState.value = opts;
}
const confirmLoading = ref(false);
async function handleConfirm() {
  if (!confirmState.value) return;
  confirmLoading.value = true;
  try {
    await confirmState.value.onOk();
    confirmState.value = null;
  } finally {
    confirmLoading.value = false;
  }
}

// 世界（单人游戏 + 多人游戏）相关状态
const worldSub = ref<"sp" | "mp">("sp");
const servers = ref<ServerEntry[]>([]);
const serverStatus = ref<Record<string, ServerStatus>>({});
const pinging = ref<Set<string>>(new Set());
const loadingServers = ref(false);

async function loadServers() {
  loadingServers.value = true;
  try {
    servers.value = await api.listServers(instanceId);
  } catch (e) {
    servers.value = [];
    message.error(String(e));
  } finally {
    loadingServers.value = false;
  }
  await pingMissing();
}

// 只对还没有状态记录的服务器测延迟，避免重复 ping 已有数据
async function pingMissing() {
  const todo = servers.value.filter((s) => !serverStatus.value[s.address]);
  await Promise.all(todo.map((s) => pingOne(s.address)));
}

async function pingOne(address: string) {
  const next = new Set(pinging.value);
  next.add(address);
  pinging.value = next;
  try {
    const st = await api.pingServer(address);
    serverStatus.value = { ...serverStatus.value, [address]: st };
  } catch (e) {
    serverStatus.value = {
      ...serverStatus.value,
      [address]: {
        online: false,
        address,
        name: null,
        version: null,
        players_online: null,
        players_max: null,
        motd: null,
        favicon: null,
        latency_ms: null,
        error: String(e),
      },
    };
  } finally {
    const n2 = new Set(pinging.value);
    n2.delete(address);
    pinging.value = n2;
  }
}

function selectWorldSub(s: "sp" | "mp") {
  worldSub.value = s;
  if (s === "mp" && !loadingServers.value) {
    loadServers();
  }
}

function serverIcon(entry: ServerEntry, st?: ServerStatus): string | undefined {
  if (st && !st.online) return undefined;
  if (st?.favicon) return st.favicon;
  if (entry.icon) return "data:image/png;base64," + entry.icon;
  return undefined;
}

function latencyInfo(latency: number | null | undefined): { count: number; tier: string } {
  if (latency == null) return { count: 0, tier: "off" };
  if (latency <= 50) return { count: 5, tier: "good" };
  if (latency <= 100) return { count: 4, tier: "good" };
  if (latency <= 200) return { count: 3, tier: "mid" };
  if (latency <= 300) return { count: 2, tier: "bad" };
  return { count: 1, tier: "bad" };
}

const instanceId = route.params.id as string;
const tab = ref<string>(
  (Array.isArray(route.query.tab) ? route.query.tab[0] : route.query.tab) ?? "mods"
);

const instance = computed(() => instances.get(instanceId));

const contentItems = ref<ContentItem[]>([]);
const iconErrors = ref(new Set<string>());
const updates = ref<Record<string, UpdateInfo>>({});
const checkingUpdates = ref(false);
const loadingContent = ref(false);

const folders = ref<Record<string, boolean>>({});
const fileItems = ref<
  { name: string; size: number; modified: number; isDir: boolean; path: string; icon: string | null }[]
>([]);
const loadingFiles = ref(false);
const previewImg = ref("");
const showPreview = ref(false);
const launchingWorld = ref("");

const CONTENT_TABS = ["mods", "shaders", "resourcepacks"];
const KIND_BY_TAB: Record<string, string> = {
  mods: "mod",
  shaders: "shader",
  resourcepacks: "resourcepack",
};
function kindOf(t: string) {
  return KIND_BY_TAB[t] ?? t;
}

const ALL_TABS = [
  { key: "mods", label: "模组", icon: IconBox, folder: "mods" },
  { key: "shaders", label: "光影", icon: IconLayers, folder: "shaderpacks" },
  { key: "resourcepacks", label: "材质包", icon: IconImage, folder: "resourcepacks" },
  { key: "screenshots", label: "截图", icon: IconCamera, folder: "screenshots" },
  { key: "saves", label: "世界", icon: IconFolder, folder: "saves" },
  { key: "logs", label: "日志", icon: IconFile },
  { key: "settings", label: "设置", icon: IconExternal },
];

// folder-backed tabs are only shown when the corresponding folder exists;
// vanilla instances have no mods and no shaders, so hide those tabs entirely.
const tabs = computed(() =>
  ALL_TABS.filter((t) => {
    if (
      (t.key === "mods" || t.key === "shaders") &&
      instance.value?.loader === "vanilla"
    )
      return false;
    return !t.folder || folders.value[t.folder] || t.key === tab.value;
  })
);

// 支持通过 URL query 切换 tab（如崩溃弹窗「查看日志」跳转）
watch(
  () => route.query.tab,
  (v) => {
    const next = Array.isArray(v) ? v[0] : v;
    if (typeof next === "string" && ALL_TABS.some((t) => t.key === next)) {
      tab.value = next;
    }
  }
);

// If the active tab is hidden (e.g. "mods" on vanilla), switch to first visible
watch(tabs, (ts) => {
  if (!ts.some((t) => t.key === tab.value) && ts.length > 0) {
    tab.value = ts[0].key;
  }
});

// Sliding active-highlight indicator for the tab bar
const tabsBox = ref<HTMLElement | null>(null);
const { indicatorStyle: tabIndicatorStyle, refresh: refreshTabIndicator } = useSlidingIndicator(
  tabsBox,
  () => Array.from(tabsBox.value?.querySelectorAll<HTMLElement>(".tab") ?? []),
  () => tabs.value.findIndex((t) => t.key === tab.value),
  { axis: "horizontal" }
);
watch(
  () => [tab.value, tabs.value.map((t) => t.key).join(",")],
  () => nextTick(() => refreshTabIndicator())
);

function loaderLabel() {
  const i = instance.value;
  if (!i) return "";
  return i.loader === "vanilla" ? "原版" : i.loader.charAt(0).toUpperCase() + i.loader.slice(1);
}

async function loadFolders() {
  try {
    const r = await api.listInstanceFolders(instanceId);
    folders.value = Object.fromEntries(r.folders.map((f) => [f.name, f.exists]));
  } catch {
    /* ignore */
  }
}

let loadSeq = 0;
async function loadContent() {
  if (!instance.value) return;
  const seq = ++loadSeq;
  loadingContent.value = true;
  try {
    const res = await api.listContent(instanceId, kindOf(tab.value));
    if (seq !== loadSeq) return;
    contentItems.value = res.items;
    api.identifyContent(instanceId, kindOf(tab.value)).catch(() => {});
  } catch (e) {
    if (seq !== loadSeq) return;
    message.error(String(e));
  } finally {
    if (seq === loadSeq) loadingContent.value = false;
  }
}

async function loadFiles() {
  const sub = tab.value === "screenshots" ? "screenshots" : "saves";
  const seq = ++loadSeq;
  loadingFiles.value = true;
  try {
    const r = await api.listInstanceFiles(instanceId, sub);
    if (seq !== loadSeq) return;
    fileItems.value = r.files;
  } catch (e) {
    if (seq !== loadSeq) return;
    message.error(String(e));
  } finally {
    if (seq === loadSeq) loadingFiles.value = false;
  }
}

function fmtSize(n: number) {
  if (n >= 1024 * 1024 * 1024) return (n / 1024 / 1024 / 1024).toFixed(2) + " GB";
  if (n >= 1024 * 1024) return (n / 1024 / 1024).toFixed(1) + " MB";
  if (n >= 1024) return (n / 1024).toFixed(1) + " KB";
  return n + " B";
}

function fmtDate(sec: number) {
  if (!sec) return "";
  return new Date(sec * 1000).toLocaleString();
}

function assetUrl(p: string) {
  return convertFileSrc(p);
}

function iconUrl(icon: string | null): string | null {
  if (!icon) return null;
  if (icon.startsWith("http://") || icon.startsWith("https://") || icon.startsWith("data:")) return icon;
  return convertFileSrc(icon);
}

async function launchWorld(name: string) {
  const i = instance.value;
  if (!i) return;
  if (!accounts.accounts.length) {
    message.warning("请先添加账号（正版或离线）");
    accounts.showManager = true;
    return;
  }
  launchingWorld.value = name;
  if (!supportsQuickPlay(i.mc_version)) {
    message.info(`此实例是 ${i.mc_version}，不支持命令行直达存档，将启动游戏后手动进入存档`);
  }
  try {
    await instances.launch(i.id, name);
    message.success(`正在进入世界「${name}」`);
  } catch (e) {
    message.error(String(e));
  } finally {
    launchingWorld.value = "";
  }
}

const launchingServer = ref<string>("");
async function launchServer(entry: ServerEntry) {
  const i = instance.value;
  if (!i) return;
  if (!accounts.accounts.length) {
    message.warning("请先添加账号（正版或离线）");
    accounts.showManager = true;
    return;
  }
  launchingServer.value = entry.address;
  try {
    await instances.launch(i.id, undefined, entry.address);
    message.success(`正在加入服务器「${entry.name || entry.address}」`);
  } catch (e) {
    message.error(String(e));
  } finally {
    launchingServer.value = "";
  }
}

// —— 固定到首页 ——
function worldPinId(name: string) {
  return pins.makeId("world", instanceId, name);
}
function serverPinId(address: string) {
  return pins.makeId("server", instanceId, address);
}
function toggleWorldPin(w: { name: string; icon: string | null }) {
  const i = instance.value;
  if (!i) return;
  pins.toggle({
    id: worldPinId(w.name),
    type: "world",
    instanceId,
    instanceName: i.name,
    instanceIcon: i.icon,
    mcVersion: i.mc_version,
    loader: i.loader,
    name: w.name,
    world: w.name,
    icon: w.icon,
  });
}
function toggleServerPin(entry: ServerEntry) {
  const i = instance.value;
  if (!i) return;
  pins.toggle({
    id: serverPinId(entry.address),
    type: "server",
    instanceId,
    instanceName: i.name,
    instanceIcon: i.icon,
    mcVersion: i.mc_version,
    loader: i.loader,
    name: entry.name || entry.address,
    address: entry.address,
    icon: entry.icon,
  });
}

// 把 MC 的 §x 颜色码解析为可渲染的片段，用于彩色显示服务器 MOTD
const MC_COLORS: Record<string, string> = {
  "0": "#000000", "1": "#0000aa", "2": "#00aa00", "3": "#00aaaa",
  "4": "#aa0000", "5": "#aa00aa", "6": "#ffaa00", "7": "#aaaaaa",
  "8": "#555555", "9": "#5555ff", "a": "#55ff55", "b": "#55ffff",
  "c": "#ff5555", "d": "#ff55ff", "e": "#ffff55", "f": "#ffffff",
};
function parseMotd(raw?: string | null): Array<{ text: string; color: string | null; bold: boolean; italic: boolean; underline: boolean; strike: boolean }> {
  if (!raw) return [];
  const out: Array<{ text: string; color: string | null; bold: boolean; italic: boolean; underline: boolean; strike: boolean }> = [];
  let style = { color: null as string | null, bold: false, italic: false, underline: false, strike: false };
  let buf = "";
  const flush = () => { if (buf) { out.push({ text: buf, ...style }); buf = ""; } };
  for (let i = 0; i < raw.length; i++) {
    const ch = raw[i];
    if (ch === "§" && i + 1 < raw.length) {
      const code = raw[i + 1].toLowerCase();
      i++;
      flush();
      if (code === "r") {
        style = { color: null, bold: false, italic: false, underline: false, strike: false };
      } else if (code === "l") {
        style.bold = true;
      } else if (code === "m") {
        style.strike = true;
      } else if (code === "n") {
        style.underline = true;
      } else if (code === "o") {
        style.italic = true;
      } else if (code === "k") {
        // 乱码（obfuscated）：无法还原，保留原文本
      } else if (MC_COLORS[code]) {
        style = { color: MC_COLORS[code], bold: false, italic: false, underline: false, strike: false };
      } else {
        buf += "§" + code;
      }
      continue;
    }
    buf += ch;
  }
  flush();
  return out;
}

async function checkUpdates() {
  checkingUpdates.value = true;
  try {
    const kind = kindOf(tab.value);
    const list = await api.checkUpdates(instanceId, kind);
    const map: Record<string, UpdateInfo> = {};
    for (const u of list) map[u.filename] = u;
    updates.value = map;
    if (!list.length) message.success("所有内容都是最新版本");
    else message.info(`发现 ${list.length} 个可更新内容`);
  } catch (e) {
    message.error(String(e));
  } finally {
    checkingUpdates.value = false;
  }
}

async function applyUpdate(u: UpdateInfo) {
  try {
    const kind = kindOf(tab.value);
    await api.applyUpdate(instanceId, kind, u.filename, u.provider, u.projectId, u.latestVersionId);
    message.success("已加入下载队列：" + (u.projectTitle ?? u.filename));
    const next = { ...updates.value };
    delete next[u.filename];
    updates.value = next;
  } catch (e) {
    message.error(String(e));
  }
}

function removeContent(item: ContentItem) {
  confirm({
    title: "移除内容",
    content: `确定要移除「${item.record.filename}」吗？`,
    positiveText: "移除",
    onOk: async () => {
      try {
        const kind = kindOf(tab.value);
        await api.uninstallContent(instanceId, kind, item.record.filename);
        message.success("已移除");
        await loadContent();
      } catch (e) {
        message.error(String(e));
      }
    },
  });
}

async function importLocal() {
  const kind: string = kindOf(tab.value);
  const filter = kind === "mod" ? [{ name: "JAR 文件", extensions: ["jar"] }] : [{ name: "ZIP 文件", extensions: ["zip"] }];
  const file = await open({ multiple: false, filters: filter });
  if (!file) return;
  try {
    await api.importLocalFile(instanceId, kind, file as string);
    message.success("已导入");
    await loadContent();
  } catch (e) {
    message.error(String(e));
  }
}

async function toggleContent(item: ContentItem) {
  try {
    const kind = kindOf(tab.value);
    await api.toggleContentEnabled(instanceId, kind, item.record.filename, !item.record.enabled);
    item.record.enabled = !item.record.enabled;
    message.success(item.record.enabled ? "已启用" : "已禁用");
  } catch (e) {
    message.error(String(e));
  }
}

// ---- 切换版本 ----
const switchState = ref<{
  show: boolean;
  loading: boolean;
  item: ContentItem | null;
  versions: ProjectVersion[];
  selected: string | null;
  provider: string;
  projectId: string;
}>({
  show: false,
  loading: false,
  item: null,
  versions: [],
  selected: null,
  provider: "",
  projectId: "",
});

// 点击遮罩关闭（document 委托兜底，naive-ui mask 机制在此环境不可靠）
const switchCardRef = ref<HTMLElement | null>(null);
const confirmCardRef = ref<HTMLElement | null>(null);
const previewCardRef = ref<HTMLElement | null>(null);
function onDocMouseDown(e: MouseEvent) {
  const t = e.target as Element | null;
  if (!t) return;
  if (t.closest(".v-binder-follower-container, .n-base-select-menu, .n-popover, .n-dropdown")) return;
  if (switchState.value.show && switchCardRef.value && !switchCardRef.value.contains(t)) {
    switchState.value.show = false;
    return;
  }
  if (confirmState.value && confirmCardRef.value && !confirmCardRef.value.contains(t)) {
    confirmState.value = null;
    return;
  }
  if (showPreview.value && previewCardRef.value && !previewCardRef.value.contains(t)) {
    showPreview.value = false;
  }
}
let unlistenUpdate: UnlistenFn | null = null;
let unlistenIdentify: UnlistenFn | null = null;
onMounted(async () => {
  document.addEventListener("mousedown", onDocMouseDown);
  try {
    unlistenUpdate = await listen<{ filename: string; ok: boolean; error?: string }>(
      "content://update-finished",
      (ev) => {
        const p = ev.payload;
        if (p.ok) {
          message.success((updates.value[p.filename]?.projectTitle ?? p.filename) + " 已更新");
        } else {
          message.error("更新失败 " + p.filename + (p.error ? "：" + p.error : ""));
        }
        loadContent();
      }
    );
  } catch {
    /* 事件监听不可用不影响主流程 */
  }
  try {
    unlistenIdentify = await listen<{
      instanceId: string; kind: string; filename: string;
      source: string; projectId: string; versionId: string;
      slug: string | null; name: string | null; description: string | null;
      icon: string | null; authors: string[] | null;
    }>("content::identified", (ev) => {
      const p = ev.payload;
      if (p.instanceId !== instanceId) return;
      const idx = contentItems.value.findIndex((it) => it.record.filename === p.filename);
      if (idx < 0) return;
      const rec = contentItems.value[idx].record;
      rec.source = p.source;
      rec.project_id = p.projectId;
      rec.version_id = p.versionId;
      rec.slug = p.slug;
      if (p.name) rec.name = p.name;
      if (p.description) rec.description = p.description;
      if (p.icon) rec.icon = p.icon;
      if (p.authors) rec.authors = p.authors;
    });
  } catch {
    /* ignore */
  }
});
onBeforeUnmount(() => {
  document.removeEventListener("mousedown", onDocMouseDown);
  unlistenUpdate?.();
  unlistenUpdate = null;
  unlistenIdentify?.();
  unlistenIdentify = null;
  if (memTimer) clearInterval(memTimer);
  if (saveTimer) clearTimeout(saveTimer);
});

function fmtIsoDate(s: string) {
  return s ? s.slice(0, 10) : "";
}

function modSearchTerm(item: ContentItem): string {
  const rec = item.record;
  // 优先用 slug 检索：内容中心按 slug 精确匹配，命中率最高
  if (rec.slug) return rec.slug as string;
  // 其次用项目标题（远程模组）
  if (rec.name && rec.name !== rec.filename) return rec.name;
  // 本地文件回退到文件名
  const base = rec.filename.replace(/\.(jar|zip|litemod|disabled)$/i, "");
  const loaders = ["fabric", "forge", "neoforge", "quilt", "rift", "optifine", "vanilla"];
  const parts = base
    .split(/[-_]/)
    .filter((p) => p && !loaders.includes(p.toLowerCase()) && !/\d/.test(p));
  return parts.length ? parts.join("-") : base;
}

/** 构建搜索跳转参数：带上来源 provider，让内容中心直接定位到对应平台 */
function buildModSearchQuery(item: ContentItem) {
  const q = modSearchTerm(item);
  const source = item.record.source;
  const provider = source === "modrinth" || source === "curseforge" ? source : null;
  return provider ? { q, provider } : { q };
}

async function openSwitchVersion(item: ContentItem) {
  const src = item.record.source;
  if (src !== "modrinth" && src !== "curseforge") {
    message.info("手动导入的内容无法切换版本");
    return;
  }
  const pid = item.record.project_id;
  if (!pid) {
    message.info("缺少项目信息，无法切换版本");
    return;
  }
  const inst = instance.value;
  const mc = inst?.mc_version ?? "";
  const ld = inst && inst.loader !== "vanilla" ? inst.loader : "";
  switchState.value = {
    show: true,
    loading: true,
    item,
    versions: [],
    selected: null,
    provider: src,
    projectId: pid,
  };
  try {
    const res = await api.projectVersions(src, pid, mc, ld);
    switchState.value.versions = res.versions;
    const cur = res.versions.find((v) => v.id === item.record.version_id);
    switchState.value.selected = cur?.id ?? res.versions[0]?.id ?? null;
  } catch (e) {
    message.error(String(e));
  } finally {
    switchState.value.loading = false;
  }
}

async function doSwitchVersion() {
  const s = switchState.value;
  if (!s.item || !s.selected) {
    message.warning("请选择一个版本");
    return;
  }
  if (s.selected === s.item.record.version_id) {
    message.info("已选择当前安装的版本");
    switchState.value.show = false;
    return;
  }
  try {
    const kind = kindOf(tab.value);
    await api.applyUpdate(instanceId, kind, s.item.record.filename, s.provider, s.projectId, s.selected);
    message.success("已将切换版本任务添加到下载队列");
    switchState.value.show = false;
    await loadContent();
  } catch (e) {
    message.error(String(e));
  }
}

async function launch() {
  const i = instance.value;
  if (!i) return;
  if (!accounts.accounts.length) {
    message.warning("请先添加账号（正版或离线）");
    accounts.showManager = true;
    return;
  }
  try {
    await instances.launch(i.id);
    message.success("游戏已启动，可在「日志」查看输出");
  } catch (e) {
    message.error(String(e));
  }
}

async function openFolder(sub?: string) {
  try {
    await api.openInstanceFolder(instanceId, sub);
  } catch (e) {
    message.error(String(e));
  }
}

function openTabFolder() {
  const t = ALL_TABS.find((x) => x.key === tab.value);
  openFolder(t?.folder);
}

function removeInstance() {
  const isSymlink = instance.value?.is_symlink;
  confirm({
    title: "删除实例",
    content: isSymlink
      ? `此实例通过符号链接导入，删除只会移除启动器中的链接，原始目录${instance.value?.source_path ? `（${instance.value.source_path}）` : ""}的文件会完整保留。确定删除该实例吗？`
      : "删除实例将移除其游戏目录与全部内容，此操作不可恢复。",
    positiveText: "删除",
    onOk: async () => {
      try {
        await instances.remove(instanceId);
        message.success("实例已删除");
        router.push("/instances");
      } catch (e) {
        message.error(String(e));
      }
    },
  });
}

// ---------------- settings tab ----------------
const javaCandidates = ref<{ path: string; version: string; major: number; vendor: string; arch: string }[]>([]);
const requiredJava = ref<number | null>(null);
const needDownload = ref(false);
const downloadingJava = ref(false);
const autoSelecting = ref(false);
const showIconPicker = ref(false);
const edit = ref({
  icon: "",
  max_memory_mb: 4096,
  memory_mode: "global" as "global" | "auto" | "custom",
  jvm_args: "",
  game_args: "",
  java_path: "",
  account_id: "",
  resolution_w: "",
  resolution_h: "",
});

const memTotal = ref(0);
const memUsed = ref(0);
const memAvailable = ref(0);
let memTimer: ReturnType<typeof setInterval> | null = null;

/** Format MB into a readable "GB / MB" string. */
function fmtMem(mb: number): string {
  if (!mb || mb <= 0) return "0 MB";
  if (mb >= 1024) return (mb / 1024).toFixed(mb % 1024 === 0 ? 0 : 1) + " GB";
  return Math.round(mb) + " MB";
}

// The custom slider max is capped at the currently available (free) memory,
// so the game allocation can never exceed the remaining space (fallback 16 GB).
const sliderMax = computed(() => {
  if (!memAvailable.value) return 16384;
  return Math.max(1024, memAvailable.value);
});

const settingsStore = useSettingsStore();
const globalMemoryMode = computed(() => settingsStore.settings?.memory_mode ?? "custom");
const globalMemory = computed(() => {
  if (globalMemoryMode.value === "auto") return autoMemory.value;
  return settingsStore.settings?.max_memory_mb ?? 4096;
});

const autoMemory = computed(() => {
  const modCount = instance.value?.mods?.length ?? 0;
  // Base: 40% of available (min 2048 MB), +512 MB per 100 mods (cap +4 GB)
  let rec = Math.max(2048, Math.floor(memAvailable.value * 40 / 100)) + Math.min(4096, Math.floor(modCount * 512 / 100));
  // Cap at 75% of available memory, leave room for OS
  const cap = Math.max(512, Math.floor(memAvailable.value * 3 / 4));
  rec = Math.min(rec, cap, 8192);
  return Math.max(rec, 512);
});

const effectiveMemory = computed(() => {
  if (edit.value.memory_mode === "auto") return autoMemory.value;
  if (edit.value.memory_mode === "global") return globalMemory.value;
  return edit.value.max_memory_mb;
});

const usedPercent = computed(() => {
  if (!memTotal.value) return 0;
  return Math.min(100, Math.round((memUsed.value / memTotal.value) * 100));
});
const allocPercent = computed(() => {
  if (!memTotal.value) return 0;
  return Math.min(100, Math.round((effectiveMemory.value / memTotal.value) * 100));
});
// The allocated segment sits right after the used segment so both colors are always visible.
const allocStart = computed(() => usedPercent.value);
const allocWidth = computed(() =>
  Math.max(0, Math.min(allocPercent.value, 100 - usedPercent.value))
);

async function loadMemoryInfo() {
  try {
    const res = await api.autoDetectMemory();
    memTotal.value = res.total_mb;
    memUsed.value = res.used_mb;
    memAvailable.value = res.available_mb ?? Math.max(0, res.total_mb - res.used_mb);
  } catch {
    /* ignore, fall back to defaults */
  }
}

let skipNextEditSync = false;
watch(
  () => instance.value,
  (i) => {
    if (!i) return;
    if (skipNextEditSync) {
      skipNextEditSync = false;
      return;
    }
    edit.value = {
      icon: i.icon ?? "",
      max_memory_mb: i.max_memory_mb ?? 4096,
      memory_mode: (i.memory_mode as "global" | "auto" | "custom") ?? "global",
      jvm_args: i.jvm_args ?? "",
      game_args: i.game_args ?? "",
      java_path: i.java_path ?? "",
      account_id: i.account_id ?? "",
      resolution_w: i.resolution?.[0]?.toString() ?? "",
      resolution_h: i.resolution?.[1]?.toString() ?? "",
    };
  },
  { immediate: true }
);

async function detectJava() {
  try {
    const [cands, rec] = await Promise.all([
      useSettingsStore().loadJava(),
      api.recommendJava(instanceId),
    ]);
    javaCandidates.value = cands;
    requiredJava.value = rec.required;
    needDownload.value = rec.needDownload;
  } catch (e) {
    message.error(String(e));
  }
}

/** Auto-pick a suitable Java for this instance (downloads it if missing). */
async function autoSelectJava() {
  autoSelecting.value = true;
  try {
    const rec = await api.recommendJava(instanceId);
    requiredJava.value = rec.required;
    if (rec.java && rec.java.major >= rec.required) {
      edit.value.java_path = rec.java.path;
      message.success(`已选择 Java ${rec.java.version}`);
    } else if (rec.needDownload) {
      await downloadJava(rec.required);
    } else {
      message.info("未找到合适的 Java");
    }
  } catch (e) {
    message.error(String(e));
  } finally {
    autoSelecting.value = false;
  }
}

async function downloadJava(major: number) {
  downloadingJava.value = true;
  try {
    const info = await api.downloadJava(major);
    message.success(`Java ${major} 已下载（${info.version}）`);
    javaCandidates.value = await useSettingsStore().loadJava(true);
    edit.value.java_path = info.path;
    await detectJava();
  } catch (e) {
    message.error(String(e));
  } finally {
    downloadingJava.value = false;
  }
}

async function pickJava() {
  const file = await open({
    multiple: false,
    filters: [{ name: "Java 可执行文件", extensions: ["exe"] }],
    directory: false,
  });
  if (file) edit.value.java_path = file as string;
}

async function saveSettings() {
  try {
    const mem =
      edit.value.memory_mode === "custom"
        ? Math.min(edit.value.max_memory_mb, sliderMax.value)
        : 0;
    skipNextEditSync = true;
    await instances.patch({
      id: instanceId,
      icon: edit.value.icon,
      max_memory_mb: mem,
      memory_mode: edit.value.memory_mode,
      jvm_args: edit.value.jvm_args,
      game_args: edit.value.game_args,
      java_path: edit.value.java_path,
      account_id: edit.value.account_id,
      resolution:
        edit.value.resolution_w && edit.value.resolution_h
          ? [Number(edit.value.resolution_w), Number(edit.value.resolution_h)]
          : null,
    });
  } catch (e) {
    message.error(String(e));
  }
}

let saveTimer: ReturnType<typeof setTimeout> | null = null;
let skipFirstSave = true;
watch(
  edit,
  () => {
    if (skipFirstSave) {
      skipFirstSave = false;
      return;
    }
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(saveSettings, 500);
  },
  { deep: true }
);

watch(
  () => tab.value,
  (t) => {
    if (t === "logs") {
      // nothing to load
    } else if (t === "settings") {
      detectJava();
      loadMemoryInfo();
      memTimer = setInterval(loadMemoryInfo, 10000);
    } else if (t === "screenshots" || t === "saves") {
      loadFiles();
      if (t === "saves" && !loadingServers.value) loadServers();
    } else {
      loadContent();
      updates.value = {};
    }
    if (t !== "settings" && memTimer) {
      clearInterval(memTimer);
      memTimer = null;
    }
  },
  { immediate: true }
);

watch(
  () => instance.value?.installed,
  () => {
    loadFolders();
    if (tab.value === "screenshots" || tab.value === "saves") {
      loadFiles();
      if (tab.value === "saves" && !loadingServers.value) loadServers();
    } else loadContent();
  },
  { immediate: true }
);

watch(
  () => instances.instances,
  () => loadContent(),
  { deep: true }
);
</script>

<template>
  <div v-if="instance" class="detail">
    <div class="d-head glass">
      <div class="d-icon"><AppIcon :name="instance.icon" /></div>
      <div class="d-info">
        <h1>{{ instance.name }}</h1>
        <div class="d-meta">
          <span class="badge">{{ loaderLabel() }}</span>
          <span class="mc">{{ instance.mc_version }}</span>
          <span v-if="instance.loader_version" class="lv">{{ instance.loader_version }}</span>
        </div>
      </div>
      <div class="d-actions">
        <button class="btn primary" @click="launch">
          <IconPlay />
          启动游戏
        </button>
        <button class="btn ghost" title="打开游戏目录" @click="openFolder()">
          <IconFolder />
        </button>
        <button class="btn danger" title="删除实例" @click="removeInstance">
          <IconTrash />
        </button>
      </div>
    </div>

    <div v-if="instance.is_symlink" class="symlink-notice glass">
      <IconExternal />
      <span>当前实例通过符号链接方式导入，对 mods / 存档等文件所做的更改会直接影响原始目录<template v-if="instance.source_path">（来源：{{ instance.source_path }}）</template>。下载的 mod 也会保存到原始目录。</span>
    </div>

    <div ref="tabsBox" class="tabs glass">
      <div class="indicator" :style="tabIndicatorStyle"></div>
      <button
        v-for="t in tabs"
        :key="t.key"
        class="tab"
        :class="{ active: tab === t.key }"
        @click="tab = t.key"
      >
        <component :is="t.icon" />
        {{ t.label }}
      </button>
      <div class="tab-right">
        <template v-if="CONTENT_TABS.includes(tab)">
          <button class="mini-btn" :disabled="checkingUpdates" @click="checkUpdates">
            <IconRefresh /> 检查更新
            <span v-if="Object.keys(updates).length" class="upd-n">{{ Object.keys(updates).length }}</span>
          </button>
          <button class="mini-btn" @click="importLocal"><IconPlus /> 导入本地</button>
        </template>
        <template v-if="tab !== 'logs' && tab !== 'settings'">
          <button class="mini-btn" title="打开对应文件夹" @click="openTabFolder">
            <IconFolder /> 打开文件夹
          </button>
        </template>
      </div>
    </div>

    <div class="tab-body">
      <!-- mods / resourcepacks / shaders -->
      <template v-if="tab === 'mods' || tab === 'resourcepacks' || tab === 'shaders'">
        <div v-if="!loadingContent && !contentItems.length" class="empty glass">
          <p>这里还是空的</p>
          <div class="empty-actions">
            <button class="btn ghost" @click="importLocal"><IconPlus /> 导入本地文件</button>
            <button class="btn ghost" @click="router.push('/browse')">从内容中心安装</button>
          </div>
        </div>
        <div v-else class="content-list glass">
          <div v-for="item in contentItems" :key="item.record.filename" class="c-row">
            <div class="c-icon">
              <img
                v-if="item.record.icon && !iconErrors.has(item.record.filename)"
                :src="iconUrl(item.record.icon) ?? ''"
                class="c-thumb"
                alt=""
                loading="lazy"
                @error="iconErrors.add(item.record.filename)"
              />
              <IconFile v-else-if="item.record.filename.endsWith('.jar')" />
              <IconImage v-else />
            </div>
            <div class="c-info">
              <div class="c-name text-ellipsis">
                {{ item.record.cn_name ?? item.record.name ?? item.record.filename }}
                <span v-if="item.record.cn_name && item.record.name" class="c-en">{{ item.record.name }}</span>
              </div>
              <div v-if="(item.record.name && item.record.name !== item.record.filename) || item.record.cn_name" class="c-file text-ellipsis">{{ item.record.filename }}</div>
              <div class="c-meta">
                <span v-if="item.record.source !== 'manual'" class="src" :class="item.record.source">{{ sourceLabel(item.record.source) }}</span>
                <span v-if="item.record.version" class="ver">{{ item.record.version }}</span>
                <span v-if="item.record.authors && item.record.authors.length" class="author">作者：{{ item.record.authors.join("、") }}</span>
                <span v-if="!item.exists" class="missing">文件缺失</span>
              </div>
            </div>
            <div class="c-actions">
              <button
                v-if="updates[item.record.filename]"
                class="icon-btn ok"
                :title="`更新到 ${updates[item.record.filename].latestVersion}`"
                @click="applyUpdate(updates[item.record.filename])"
              >
                <IconDownload />
              </button>
              <button
                v-if="(item.record.source === 'modrinth' || item.record.source === 'curseforge') && item.record.project_id"
                class="icon-btn"
                title="切换版本"
                @click="openSwitchVersion(item)"
              >
                <IconRepeat />
              </button>
              <button
                class="icon-btn"
                title="在内容中心搜索"
                @click="router.push({ name: 'browse', query: buildModSearchQuery(item) })"
              >
                <IconSearch />
              </button>
              <button
                v-if="item.record.enabled"
                class="icon-btn warn"
                title="禁用"
                @click="toggleContent(item)"
              >
                <IconClose />
              </button>
              <button
                v-else
                class="icon-btn ok"
                title="启用"
                @click="toggleContent(item)"
              >
                <IconCheck />
              </button>
              <button class="icon-btn danger" title="移除" @click="removeContent(item)">
                <IconTrash />
              </button>
            </div>
          </div>
        </div>
      </template>

      <!-- screenshots -->
      <template v-if="tab === 'screenshots'">
        <div v-if="loadingFiles" class="center">加载中…</div>
        <div v-else-if="!fileItems.length" class="empty glass">
          <p>还没有截图</p>
          <button class="btn ghost" @click="openTabFolder"><IconFolder /> 打开截图文件夹</button>
        </div>
        <div v-else class="shot-grid">
          <div
            v-for="f in fileItems.filter((x) => !x.isDir)"
            :key="f.name"
            class="shot-card glass clickable"
            @click="previewImg = assetUrl(f.path); showPreview = true"
          >
            <img :src="assetUrl(f.path)" class="shot-img" alt="" loading="lazy" />
            <div class="shot-info">
              <div class="shot-name text-ellipsis">{{ f.name }}</div>
              <div class="shot-meta">{{ fmtSize(f.size) }} · {{ fmtDate(f.modified) }}</div>
            </div>
          </div>
        </div>
      </template>

      <!-- 世界：单人游戏 / 多人游戏 -->
      <template v-if="tab === 'saves'">
        <div class="world-sub">
          <button class="seg" :class="{ active: worldSub === 'sp' }" @click="selectWorldSub('sp')">
            <IconFolder /> 单人游戏
          </button>
          <button class="seg" :class="{ active: worldSub === 'mp' }" @click="selectWorldSub('mp')">
            <IconGlobe /> 多人游戏
          </button>
        </div>

        <!-- 单人游戏：本地世界存档 -->
        <template v-if="worldSub === 'sp'">
          <div v-if="loadingFiles" class="center">加载中…</div>
          <div v-else-if="!fileItems.length" class="empty glass">
            <p>还没有世界存档</p>
            <button class="btn ghost" @click="openTabFolder"><IconFolder /> 打开存档文件夹</button>
          </div>
          <div v-else class="content-list glass">
            <div v-for="f in fileItems.filter((x) => x.isDir)" :key="f.name" class="world-row">
              <img v-if="f.icon" :src="assetUrl(f.icon)" class="world-icon" alt="" />
              <div v-else class="world-icon ph"><IconFolder /></div>
              <div class="c-info">
                <div class="c-name text-ellipsis">{{ f.name }}</div>
                <div class="c-meta">
                  <span class="ver">世界存档</span>
                  <span v-if="f.modified" class="ver">{{ fmtDate(f.modified) }}</span>
                </div>
              </div>
              <div class="c-actions">
                <button
                  class="mini-btn pin"
                  :class="{ active: pins.isPinned(worldPinId(f.name)) }"
                  :title="pins.isPinned(worldPinId(f.name)) ? '取消固定' : '固定到首页'"
                  @click="toggleWorldPin(f)"
                >
                  <IconMapPin />
                </button>
                <button
                  class="mini-btn play"
                  :disabled="!!launchingWorld"
                  @click="launchWorld(f.name)"
                >
                  <IconPlay /> {{ launchingWorld === f.name ? "启动中…" : "启动" }}
                </button>
              </div>
            </div>
            <div v-if="!fileItems.some((x) => x.isDir)" class="center">这个实例还没有世界存档</div>
          </div>
        </template>

        <!-- 多人游戏：服务器列表 -->
        <template v-else>
          <div v-if="loadingServers" class="center">加载中…</div>
          <div v-else-if="!servers.length" class="empty glass">
            <p>还没有添加服务器</p>
            <p class="hint">在游戏内“多人游戏”中添加一个服务器，它会出现在这里</p>
          </div>
          <div v-else class="content-list glass">
            <div v-for="s in servers" :key="s.address" class="server-row">
              <img v-if="serverIcon(s, serverStatus[s.address])" :src="serverIcon(s, serverStatus[s.address])!" class="server-icon" alt="" />
              <div class="c-info">
                <div class="c-name text-ellipsis">{{ serverStatus[s.address]?.name || s.name }}</div>
                <div class="server-meta">
                  <span class="latency" :class="latencyInfo(serverStatus[s.address]?.latency_ms).tier">
                    <span class="bars">
                      <i v-for="n in 5" :key="n" :class="{ on: n <= latencyInfo(serverStatus[s.address]?.latency_ms).count }"></i>
                    </span>
                    <span v-if="serverStatus[s.address]?.latency_ms != null">{{ serverStatus[s.address]?.latency_ms }} ms</span>
                    <span v-else-if="serverStatus[s.address] && !serverStatus[s.address]?.online">离线</span>
                    <span v-else>…</span>
                  </span>
                  <span v-if="serverStatus[s.address]?.players_online != null" class="players">
                    {{ serverStatus[s.address]?.players_online }}<template v-if="serverStatus[s.address]?.players_max != null">/{{ serverStatus[s.address]?.players_max }}</template> 人在线
                  </span>
                  <span v-else-if="serverStatus[s.address]?.error" class="err" :title="serverStatus[s.address]?.error ?? undefined">无法连接</span>
                </div>
                <div v-if="serverStatus[s.address]?.motd" class="server-motd">
                  <span
                    v-for="(seg, si) in parseMotd(serverStatus[s.address]?.motd)"
                    :key="si"
                    :style="{
                      color: seg.color || undefined,
                      fontWeight: seg.bold ? 700 : undefined,
                      fontStyle: seg.italic ? 'italic' : undefined,
                      textDecoration: (seg.underline ? 'underline ' : '') + (seg.strike ? 'line-through' : '') || undefined,
                    }"
                    >{{ seg.text }}</span
                  >
                </div>
              </div>
              <div class="c-actions">
                <button
                  class="mini-btn pin"
                  :class="{ active: pins.isPinned(serverPinId(s.address)) }"
                  :title="pins.isPinned(serverPinId(s.address)) ? '取消固定' : '固定到首页'"
                  @click="toggleServerPin(s)"
                >
                  <IconMapPin />
                </button>
                <button
                  class="mini-btn play"
                  :disabled="!!launchingServer || pinging.has(s.address)"
                  :title="launchingServer === s.address ? '启动中…' : '启动并加入此服务器'"
                  @click="launchServer(s)"
                >
                  <IconPlay /> {{ launchingServer === s.address ? "启动中" : "启动" }}
                </button>
                <button class="mini-btn" :disabled="pinging.has(s.address) || !!launchingServer" @click="pingOne(s.address)">
                  <IconRefresh /> {{ pinging.has(s.address) ? "测试中" : "刷新" }}
                </button>
              </div>
            </div>
          </div>
        </template>
      </template>

      <!-- logs -->
      <template v-if="tab === 'logs'">
        <LogViewer :instance-id="instanceId" />
      </template>

      <!-- settings -->
      <template v-if="tab === 'settings'">
        <div class="settings-grid">
          <div class="set-card glass">
            <h4>Java 运行时</h4>
            <div class="java-req">
              <span class="req-label">该游戏需要</span>
              <span class="req-val">Java {{ requiredJava ?? "?" }}+</span>
              <button class="mini-btn" :disabled="autoSelecting" @click="autoSelectJava">
                自动选择
              </button>
              <button
                v-if="needDownload && requiredJava"
                class="mini-btn accent"
                :disabled="downloadingJava"
                @click="downloadJava(requiredJava)"
              >
                <IconDownload />
                {{ downloadingJava ? "下载中…" : `下载 Java ${requiredJava}` }}
              </button>
            </div>
            <div class="java-row">
              <input v-model="edit.java_path" class="text-input mono" placeholder="留空则自动选择合适版本" />
              <button class="mini-btn" @click="pickJava">浏览…</button>
              <button class="mini-btn" @click="detectJava">刷新列表</button>
            </div>
            <div v-if="javaCandidates.length" class="java-list">
              <button
                v-for="j in javaCandidates.slice(0, 10)"
                :key="j.path"
                class="java-item"
                :class="{ active: edit.java_path === j.path }"
                @click="edit.java_path = j.path"
              >
                <span class="java-name">Java {{ j.major }} ({{ j.version }})</span>
                <span class="java-path">{{ j.path }}</span>
              </button>
            </div>
            <p class="hint">留空时启动器会自动挑选合适版本；没有合适版本会先自动下载。</p>
          </div>

          <div class="set-card glass">
            <h4>内存分配</h4>
            <div class="mem-modes">
              <label
                class="mem-mode"
                :class="{ active: edit.memory_mode === 'global' }"
              >
                <input v-model="edit.memory_mode" type="radio" value="global" />
                根据全局配置
              </label>
              <label
                class="mem-mode"
                :class="{ active: edit.memory_mode === 'auto' }"
              >
                <input v-model="edit.memory_mode" type="radio" value="auto" />
                自动配置
              </label>
              <label
                class="mem-mode"
                :class="{ active: edit.memory_mode === 'custom' }"
              >
                <input v-model="edit.memory_mode" type="radio" value="custom" />
                自定义
              </label>
            </div>

            <template v-if="edit.memory_mode === 'custom'">
              <input
                v-model.number="edit.max_memory_mb"
                type="range"
                min="1024"
                :max="sliderMax"
                step="256"
                class="range"
              />
              <div class="range-labels"><span>1 GB</span><span>{{ fmtMem(sliderMax) }}</span></div>
              <div class="mem-current">{{ edit.max_memory_mb }} MB</div>
            </template>

            <div v-else class="mem-current">
              {{ effectiveMemory }} MB
              <span v-if="edit.memory_mode === 'global' && globalMemoryMode === 'auto'" class="mem-mode-note">（全局自动配置）</span>
              <span v-else-if="edit.memory_mode === 'global'" class="mem-mode-note">（全局手动配置）</span>
              <span v-else-if="edit.memory_mode === 'auto'" class="mem-mode-note">（自动配置）</span>
            </div>

            <div class="mem-gauge">
              <div class="mem-gauge-track">
                <div class="mem-gauge-used" :style="{ width: usedPercent + '%' }"></div>
                <div
                  class="mem-gauge-alloc"
                  :style="{ left: allocStart + '%', width: allocWidth + '%' }"
                ></div>
              </div>
              <div class="mem-gauge-labels">
                <span><i class="dot used"></i>已使用 {{ fmtMem(memUsed) }}（{{ usedPercent }}%）</span>
                <span><i class="dot alloc"></i>游戏分配 {{ fmtMem(effectiveMemory) }}（{{ allocPercent }}%）</span>
                <span><i class="dot total"></i>总内存 {{ fmtMem(memTotal) }} / 可用 {{ fmtMem(memAvailable) }}</span>
              </div>
            </div>
          </div>

          <div class="set-card glass">
            <h4>JVM 参数（额外）</h4>
            <textarea v-model="edit.jvm_args" class="text-input mono" rows="3" placeholder="例如：-XX:+UseG1GC -Dfile.encoding=UTF-8" />
          </div>

          <div class="set-card glass">
            <h4>游戏参数（额外）</h4>
            <input v-model="edit.game_args" class="text-input mono" placeholder="例如：--fullscreen" />
          </div>

          <div class="set-card glass">
            <h4>账号</h4>
            <n-select
              v-model:value="edit.account_id"
              :options="[
                { label: `跟随全局当前账号（${accounts.current?.username ?? '未选择'}）`, value: '' },
                ...accounts.accounts.map((a) => ({
                  label: `${a.username}（${a.type === 'microsoft' ? '正版' : '离线'}）`,
                  value: a.uuid,
                })),
              ]"
            />
          </div>

          <div class="set-card glass">
            <h4>游戏窗口分辨率（可选）</h4>
            <div class="res-row">
              <input v-model="edit.resolution_w" class="text-input" placeholder="宽，如 1920" />
              <span>×</span>
              <input v-model="edit.resolution_h" class="text-input" placeholder="高，如 1080" />
            </div>
          </div>

          <div class="set-card glass">
            <h4>实例图标</h4>
            <div class="icon-pick">
              <div class="icon-preview">
                <AppIcon :name="edit.icon" />
              </div>
              <button class="btn" @click="showIconPicker = true">选择图标</button>
            </div>
          </div>

        </div>
      </template>
    </div>

    <!-- confirm dialog -->
    <n-modal
      :show="confirmState !== null"
      preset="card"
      :title="confirmState?.title ?? ''"
      style="width: 420px; max-width: 92vw"
      :mask-closable="true"
      :close-on-esc="true"
      @update:show="(v: boolean) => { if (!v) confirmState = null; }"
      @mask-click="confirmState = null"
    >
      <div v-if="confirmState" ref="confirmCardRef" style="display: flex; flex-direction: column; gap: 16px;">
        <div style="font-size: 14px; color: var(--text-2); line-height: 1.6;">{{ confirmState.content }}</div>
        <div style="display: flex; justify-content: flex-end; gap: 10px;">
          <n-button @click="confirmState = null">取消</n-button>
          <n-button type="error" :loading="confirmLoading" @click="handleConfirm">{{ confirmState.positiveText }}</n-button>
        </div>
      </div>
    </n-modal>

    <!-- screenshot preview -->
    <n-modal
      v-model:show="showPreview"
      preset="card"
      title="截图预览"
      style="width: min(860px, 92vw)"
      :mask-closable="true"
      :close-on-esc="true"
      @mask-click="showPreview = false"
    >
      <img ref="previewCardRef" :src="previewImg" class="preview-img" alt="" />
    </n-modal>

    <!-- switch version -->
    <n-modal
      v-model:show="switchState.show"
      preset="card"
      :title="`切换版本：${switchState.item?.record.name ?? switchState.item?.record.filename ?? ''}`"
      style="width: 520px; max-width: 94vw"
      :mask-closable="true"
      :close-on-esc="true"
      @mask-click="switchState.show = false"
    >
      <div ref="switchCardRef" class="sv-body">
        <div v-if="switchState.loading" class="center">加载中…</div>
        <div v-else-if="!switchState.versions.length" class="center">没有可用的版本</div>
        <div v-else class="sv-list">
          <button
            v-for="v in switchState.versions"
            :key="v.id"
            class="sv-item"
            :class="{ active: switchState.selected === v.id }"
            @click="switchState.selected = v.id"
          >
            <span class="sv-num">{{ v.version_number ?? v.name }}</span>
            <span v-if="v.date_published" class="sv-date">{{ fmtIsoDate(v.date_published) }}</span>
          </button>
        </div>
        <div class="sv-actions">
          <n-button size="small" @click="switchState.show = false">取消</n-button>
          <n-button
            size="small"
            type="primary"
            :disabled="!switchState.selected || switchState.loading"
            @click="doSwitchVersion"
          >
            切换
          </n-button>
        </div>
      </div>
    </n-modal>

    <IconPickerDialog
      v-model:show="showIconPicker"
      :value="edit.icon"
      :instance-id="instanceId"
      @save="edit.icon = $event"
    />
  </div>
  <div v-else class="center">实例不存在或已删除</div>
</template>

<style scoped>
.detail {
  max-width: 1080px;
  margin: 0 auto;
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.d-head {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 18px 20px;
}
.symlink-notice {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 20px;
  margin-top: 12px;
  font-size: 13px;
  color: #e8a33d;
  background: rgba(232, 163, 61, 0.1);
  border-radius: 12px;
}
.symlink-notice svg {
  flex-shrink: 0;
}
.d-icon {
  width: 56px;
  height: 56px;
  border-radius: 14px;
  overflow: hidden;
  background: transparent;
  position: relative;
  font-size: 26px;
  color: var(--accent);
  flex-shrink: 0;
  box-sizing: border-box;
}
.d-icon :deep(.app-icon) {
  position: absolute;
  inset: 0;
}
.d-info {
  flex: 1;
  min-width: 0;
}
.d-info h1 {
  margin: 0 0 6px;
  font-size: 21px;
}
.d-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
}
.badge {
  background: rgba(232, 154, 75, 0.16);
  color: var(--accent);
  border-radius: 6px;
  padding: 1px 8px;
  font-weight: 600;
}
.mc {
  color: var(--text-2);
  font-weight: 600;
}
.lv {
  color: var(--text-3);
}
.st {
  padding: 2px 8px;
  border-radius: 7px;
  font-weight: 600;
}
.st.ok {
  color: #4ec9a0;
  background: rgba(78, 201, 160, 0.12);
}
.st.warn {
  color: #e0a030;
  background: rgba(224, 160, 48, 0.12);
}
.d-actions {
  display: flex;
  gap: 8px;
}
.btn {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  border: none;
  border-radius: 10px;
  padding: 9px 16px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  transition: all 0.14s;
}
.btn.primary {
  background: linear-gradient(135deg, var(--accent), var(--accent-deep));
  color: #1a1208;
}
.btn.primary:hover:not(:disabled) {
  filter: brightness(1.08);
}
.btn.ghost {
  background: rgba(255, 255, 255, 0.06);
  color: var(--text-1);
  border: 1px solid var(--border);
}
.btn.ghost:hover {
  background: rgba(255, 255, 255, 0.1);
}
.btn.danger {
  background: rgba(255, 255, 255, 0.06);
  color: var(--text-1);
  border: 1px solid var(--border);
}
.btn.danger:hover {
  color: #e5534b;
  border-color: rgba(229, 83, 75, 0.5);
}
.btn:disabled {
  opacity: 0.5;
  cursor: default;
}
.not-installed {
  padding: 18px 20px;
  display: flex;
  justify-content: space-between;
  align-items: center;
  border-color: rgba(224, 160, 48, 0.35);
}
.not-installed h3 {
  margin: 0 0 4px;
  color: #e0a030;
}
.not-installed p {
  margin: 0;
  color: var(--text-2);
  font-size: 13px;
}
.tabs {
  position: relative;
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 6px;
}
.indicator {
  position: absolute;
  top: 6px;
  height: calc(100% - 12px);
  border-radius: 9px;
  background: var(--accent-soft);
  transition:
    left 0.28s cubic-bezier(0.22, 1, 0.36, 1),
    width 0.28s cubic-bezier(0.22, 1, 0.36, 1),
    opacity 0.18s;
  pointer-events: none;
}
.tab {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 8px 14px;
  border-radius: 9px;
  border: none;
  background: transparent;
  color: var(--text-2);
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  transition: all 0.12s;
}
.tab:hover {
  background: rgba(255, 255, 255, 0.05);
}
.tab.active {
  color: var(--accent);
}
.tab-right {
  margin-left: auto;
  display: flex;
  gap: 8px;
}
.mini-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 7px 12px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-2);
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  transition: all 0.12s;
}
.mini-btn:hover {
  background: rgba(255, 255, 255, 0.08);
  color: var(--text-1);
}
.mini-btn.accent {
  color: var(--accent);
  border-color: rgba(232, 154, 75, 0.4);
}
.upd-n {
  background: var(--accent);
  color: #1a1208;
  border-radius: 10px;
  font-size: 11px;
  padding: 0 6px;
}
.updates {
  padding: 12px 16px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  border-color: rgba(232, 154, 75, 0.3);
}
.upd-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.upd-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.ver-change {
  font-size: 12px;
  color: var(--text-3);
  display: inline-flex;
  align-items: center;
  gap: 4px;
}
.content-list {
  padding: 6px 12px;
}
.c-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 8px;
  border-bottom: 1px solid var(--border);
}
.c-row:last-child {
  border-bottom: none;
}
.c-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}
.c-icon {
  width: 34px;
  height: 34px;
  border-radius: 9px;
  background: rgba(255, 255, 255, 0.05);
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  font-size: 16px;
  flex-shrink: 0;
  overflow: hidden;
}
.c-thumb {
  width: 100%;
  height: 100%;
  object-fit: cover;
  image-rendering: pixelated;
}
.c-info {
  flex: 1;
  min-width: 0;
}
.c-name {
  font-size: 13px;
  font-weight: 600;
  margin-bottom: 3px;
}
.c-en {
  font-size: 11px;
  font-weight: 400;
  color: var(--text-3);
  margin-left: 6px;
}
.c-file {
  font-size: 11px;
  color: var(--text-3);
  margin-bottom: 3px;
}
.c-meta {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 11px;
}
.src {
  padding: 1px 7px;
  border-radius: 6px;
  font-weight: 600;
}
.src.modrinth {
  background: rgba(90, 162, 240, 0.15);
  color: #7cb8f5;
}
.src.curseforge {
  background: rgba(240, 101, 67, 0.15);
  color: #f08a67;
}
.src.manual {
  background: rgba(255, 255, 255, 0.08);
  color: var(--text-3);
}
.src.modpack {
  background: rgba(150, 181, 225, 0.18);
  color: #96b5e1;
}
.ver {
  color: var(--text-3);
}
.author {
  color: var(--text-3);
  opacity: 0.85;
}
.missing {
  color: #e5534b;
  font-weight: 600;
}
.icon-btn {
  width: 30px;
  height: 30px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: transparent;
  color: var(--text-2);
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  font-size: 13px;
  transition: all 0.12s;
}
.icon-btn.danger:hover {
  color: #e5534b;
  border-color: rgba(229, 83, 75, 0.5);
}
.icon-btn.warn {
  color: #e5534b;
  border-color: rgba(229, 83, 75, 0.35);
}
.icon-btn.warn:hover {
  background: rgba(229, 83, 75, 0.15);
  border-color: #e5534b;
}
.icon-btn.ok {
  color: #4ec9a0;
  border-color: rgba(78, 201, 160, 0.35);
}
.icon-btn.ok:hover {
  background: rgba(78, 201, 160, 0.15);
  border-color: #4ec9a0;
}
.center {
  padding: 60px;
  text-align: center;
  color: var(--text-3);
}
.shot-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 12px;
}
.shot-card {
  overflow: hidden;
  padding: 0;
}
.shot-img {
  width: 100%;
  aspect-ratio: 16 / 9;
  object-fit: cover;
  display: block;
  background: rgba(0, 0, 0, 0.3);
}
.shot-info {
  padding: 8px 10px;
}
.shot-name {
  font-size: 12px;
  font-weight: 600;
  margin-bottom: 3px;
}
.shot-meta {
  font-size: 11px;
  color: var(--text-3);
}
.preview-img {
  width: 100%;
  max-height: 70vh;
  object-fit: contain;
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.4);
}
.sv-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.sv-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 320px;
  overflow-y: auto;
}
.sv-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  border: 1px solid var(--border);
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-2);
  border-radius: 9px;
  padding: 8px 14px;
  font-size: 13px;
  cursor: pointer;
  font-family: inherit;
  transition: all 0.12s;
}
.sv-item:hover {
  background: rgba(255, 255, 255, 0.08);
}
.sv-item.active {
  border-color: var(--accent);
  color: var(--accent);
  background: var(--accent-soft);
}
.sv-num {
  font-weight: 600;
}
.sv-date {
  font-size: 11px;
  color: var(--text-3);
}
.sv-actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}
.world-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 8px;
  border-bottom: 1px solid var(--border);
}
.world-row:last-child {
  border-bottom: none;
}
.world-icon {
  width: 44px;
  height: 44px;
  border-radius: 10px;
  image-rendering: pixelated;
  background: rgba(255, 255, 255, 0.05);
  flex-shrink: 0;
}
.world-icon.ph {
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  font-size: 18px;
}
.world-sub {
  display: flex;
  gap: 8px;
  margin-bottom: 14px;
}
.seg {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 7px 14px;
  border-radius: 999px;
  border: 1px solid var(--border);
  background: var(--surface-2);
  color: var(--text-2);
  font-size: 13px;
  cursor: pointer;
  transition: all 0.15s;
}
.seg:hover {
  color: var(--text-1);
  border-color: var(--accent);
}
.seg.active {
  background: var(--accent-soft);
  color: var(--accent);
  border-color: rgba(232, 154, 75, 0.5);
}
.mp-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 10px;
}
.mp-hint {
  font-size: 13px;
  color: var(--text-3);
}
.server-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 8px;
  border-bottom: 1px solid var(--border);
}
.server-row:last-child {
  border-bottom: none;
}
.server-icon {
  width: 44px;
  height: 44px;
  border-radius: 10px;
  image-rendering: pixelated;
  background: rgba(255, 255, 255, 0.05);
  flex-shrink: 0;
}
.server-icon.ph {
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-3);
  font-size: 18px;
}
.server-meta {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 2px;
  font-size: 12px;
  flex-wrap: wrap;
}
.latency {
  display: inline-flex;
  align-items: center;
  gap: 5px;
}
.latency .bars {
  display: inline-flex;
  align-items: flex-end;
  gap: 2px;
  height: 12px;
}
.latency .bars i {
  width: 3px;
  background: rgba(255, 255, 255, 0.18);
  border-radius: 1px;
  opacity: 0.35;
}
.latency .bars i:nth-child(1) { height: 4px; }
.latency .bars i:nth-child(2) { height: 6px; }
.latency .bars i:nth-child(3) { height: 8px; }
.latency .bars i:nth-child(4) { height: 10px; }
.latency .bars i:nth-child(5) { height: 12px; }
.latency .bars i.on { opacity: 1; }
.latency.good .bars i.on { background: #57c257; }
.latency.good span:not(.bars) { color: #57c257; }
.latency.mid .bars i.on { background: #e0c000; }
.latency.mid span:not(.bars) { color: #e0c000; }
.latency.bad .bars i.on { background: #e0533d; }
.latency.bad span:not(.bars) { color: #e0533d; }
.latency.off { color: rgba(255, 255, 255, 0.4); }
.players { color: var(--text-2); }
.err { color: #e0533d; }
.server-motd {
  font-size: 12px;
  color: var(--text-3);
  margin-top: 2px;
  max-width: 540px;
  white-space: pre-wrap;
  word-break: break-word;
  line-height: 1.5;
}
.hint {
  font-size: 12px;
  color: var(--text-3);
  margin-top: 4px;
}
.mini-btn.play {
  color: var(--accent);
  border-color: rgba(232, 154, 75, 0.4);
  background: var(--accent-soft);
}
.mini-btn.play:disabled {
  opacity: 0.5;
  cursor: default;
}
.mini-btn.pin {
  padding: 7px 9px;
}
.mini-btn.pin svg {
  width: 14px;
  height: 14px;
}
.mini-btn.pin.active {
  color: var(--accent);
  border-color: rgba(232, 154, 75, 0.4);
  background: var(--accent-soft);
}
.empty {
  padding: 40px;
  text-align: center;
  color: var(--text-3);
  display: flex;
  flex-direction: column;
  gap: 14px;
  align-items: center;
}
.empty-actions {
  display: flex;
  gap: 10px;
}
.settings-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 14px;
}
.set-card {
  padding: 16px;
}
.set-card h4 {
  margin: 0 0 12px;
  font-size: 14px;
}
.java-req {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 10px;
  font-size: 13px;
}
.req-label {
  color: var(--text-3);
}
.req-val {
  color: var(--accent);
  font-weight: 700;
}
.mini-btn.accent {
  color: var(--accent);
  border-color: rgba(232, 154, 75, 0.4);
}
.java-row {
  display: flex;
  gap: 8px;
}
.text-input {
  flex: 1;
  min-width: 0;
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid var(--border);
  border-radius: 9px;
  color: var(--text-1);
  padding: 8px 12px;
  font-size: 13px;
  font-family: inherit;
  outline: none;
  transition: border-color 0.12s;
}
.text-input:focus {
  border-color: rgba(232, 154, 75, 0.5);
}
textarea.text-input {
  resize: vertical;
  width: 100%;
}
.java-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-top: 10px;
  max-height: 200px;
  overflow-y: auto;
}
.java-item {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 7px 12px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: transparent;
  color: var(--text-2);
  cursor: pointer;
  font-family: inherit;
  text-align: left;
}
.java-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-1);
}
.java-path {
  font-size: 11px;
  color: var(--text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.java-item.active {
  border-color: rgba(232, 154, 75, 0.5);
  background: var(--accent-soft);
  color: var(--accent);
}
.range {
  width: 100%;
  accent-color: var(--accent);
}
.range-labels {
  display: flex;
  justify-content: space-between;
  font-size: 11px;
  color: var(--text-3);
}
.mem-modes {
  display: flex;
  gap: 8px;
  margin-bottom: 10px;
  flex-wrap: wrap;
}
.mem-mode {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: rgba(255, 255, 255, 0.04);
  font-size: 13px;
  cursor: pointer;
  color: var(--text-2);
  transition: all 0.12s;
}
.mem-mode:hover {
  background: rgba(255, 255, 255, 0.08);
}
.mem-mode.active {
  border-color: var(--accent);
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 12%, transparent);
}
.mem-mode input {
  accent-color: var(--accent);
}
.mem-current {
  font-size: 14px;
  font-weight: 600;
  color: var(--accent);
  margin-top: 6px;
}
.mem-mode-note {
  font-size: 12px;
  font-weight: 400;
  color: var(--text-3);
}
.mem-gauge {
  margin-top: 14px;
}
.mem-gauge-track {
  position: relative;
  height: 10px;
  border-radius: 6px;
  background: rgba(255, 255, 255, 0.08);
  overflow: hidden;
}
.mem-gauge-used,
.mem-gauge-alloc {
  position: absolute;
  top: 0;
  bottom: 0;
  height: 100%;
  /* 两端圆角由外层 track 的 overflow:hidden 统一裁剪，
     两段之间保持直角无缝衔接，铺满整个轨道 */
  transition: width 0.2s, left 0.2s;
}
.mem-gauge-used {
  left: 0;
  background: linear-gradient(90deg, #5a8ef0, #8ab4ff);
}
.mem-gauge-alloc {
  background: linear-gradient(90deg, #e89a4b, #f2c079);
}
.mem-gauge-labels {
  display: flex;
  justify-content: space-between;
  flex-wrap: wrap;
  gap: 4px;
  font-size: 11px;
  color: var(--text-3);
  margin-top: 6px;
}
.mem-gauge-labels span {
  display: inline-flex;
  align-items: center;
  gap: 5px;
}
.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  display: inline-block;
}
.dot.used {
  background: #8ec4ff;
}
.dot.alloc {
  background: #e89a4b;
}
.dot.total {
  background: #9aa4b2;
}
.res-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.icon-pick {
  display: flex;
  align-items: center;
  gap: 10px;
}
.icon-preview {
  width: 38px;
  height: 38px;
  border-radius: 10px;
  overflow: hidden;
  background: transparent;
  position: relative;
  flex-shrink: 0;
  box-sizing: border-box;
}
.icon-preview :deep(.app-icon) {
  position: absolute;
  inset: 0;
  font-size: 18px;
}
.set-actions {
  grid-column: 1 / -1;
  display: flex;
  justify-content: flex-end;
}
</style>
