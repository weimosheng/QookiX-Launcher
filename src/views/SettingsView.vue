<script setup lang="ts">
import { computed, h, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { useRouter } from "vue-router";
import { NButton, NModal, useMessage, useDialog } from "naive-ui";
import { openUrl } from "@tauri-apps/plugin-opener";
import { open } from "@tauri-apps/plugin-dialog";
import { convertFileSrc } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { useSettingsStore } from "../stores/settings";
import { api } from "../api";
import { useSlidingIndicator } from "../composables/useSlidingIndicator";
import {
  IconCpu,
  IconDownload,
  IconFile,
  IconGlobe,
  IconHardDrive,
  IconImage,
  IconRefresh,
  IconSearch,
  IconSliders,
  IconTrash,
} from "../components/icons";
import { peekUpdate, downloadAndInstall, relaunchApp } from "../updater";
import type { JavaInfo, StorageStats } from "../types";
import logoUrl from "../assets/logo.png";

const settings = useSettingsStore();
const message = useMessage();
const dialog = useDialog();
const router = useRouter();

const checking = ref(false);
const updateVersion = ref<string | null>(null);

async function checkUpdate() {
  if (checking.value) return;
  checking.value = true;
  updateVersion.value = null;
  try {
    const update = await peekUpdate(true);
    if (!update) {
      message.success("已是最新版本");
      return;
    }
    updateVersion.value = update.version;
    let dlg: { destroy: () => void } | null = null;
    const close = () => { dlg?.destroy(); dlg = null; };
    dlg = dialog.warning({
      title: "发现新版本",
      content: `QookiX Launcher 有新版本 v${update.version}，是否下载并安装？`,
      action: () =>
        h("div", { style: "display:flex; gap:8px; justify-content:flex-end;" }, [
          h(NButton, { size: "small", ghost: true, onClick: close }, () => "以后再说"),
          h(
            NButton,
            { size: "small", quaternary: true, onClick: () => { close(); void doInstall(); } },
            { default: () => "下载并更新" },
          ),
          h(
            NButton,
            { size: "small", type: "primary", onClick: () => { close(); void doInstallAndRelaunch(); } },
            { default: () => "重启以更新" },
          ),
        ]),
    });
  } catch {
    // 国内镜像不可用时给出明确提示，而不是误报「已是最新版本」
    const isBucket = settings.settings?.update_source !== "github";
    message.error(
      isBucket
        ? "国内镜像暂时不可用，请检查网络，或切换到 GitHub 官方源后重试"
        : "检查更新失败，请稍后重试"
    );
  } finally {
    checking.value = false;
  }
}

/** 恢复被「忽略此版本」关掉的启动更新提醒。 */
async function restoreDismissed() {
  try {
    await settings.patch({ dismissed_update_version: null });
    message.success("已恢复更新提醒");
  } catch {
    message.error("操作失败，请稍后重试");
  }
}

async function doInstall() {
  // Jump to the Download Center so the user can watch the progress live.
  router.push("/downloads");
  try {
    const installed = await downloadAndInstall();
    if (!installed) return;
    dialog.success({
      title: "更新完成",
      content: "需要重启启动器才能生效，是否立即重启？",
      positiveText: "立即重启",
      negativeText: "稍后手动重启",
      onPositiveClick: () => relaunchApp(),
    });
  } catch (err) {
    const detail = err instanceof Error ? err.message : String(err);
    message.error(detail || "更新失败，请稍后重试或手动下载");
    console.error("[updater] install error:", err);
  }
}

/** 下载安装并自动重启，一步到位（「重启以更新」按钮）。 */
async function doInstallAndRelaunch() {
  router.push("/downloads");
  try {
    const installed = await downloadAndInstall();
    if (!installed) return;
    await relaunchApp();
  } catch (err) {
    const detail = err instanceof Error ? err.message : String(err);
    message.error(detail || "更新失败，请稍后重试或手动下载");
    console.error("[updater] install error:", err);
  }
}

// 主题 seg 滑动高亮
const themeSegRef = ref<HTMLElement | null>(null);
const { indicatorStyle: themeSegStyle, refresh: refreshThemeSeg } = useSlidingIndicator(
  themeSegRef,
  () => Array.from(themeSegRef.value?.querySelectorAll<HTMLElement>(".seg button") ?? []),
  () => (settings.settings?.theme === "light" ? 1 : 0),
  { axis: "horizontal" }
);
watch(() => settings.settings?.theme, () => nextTick(() => refreshThemeSeg()));

// 主题色预设
const themeColorPresets = [
  "#e89a4b",
  "#ff6b35",
  "#e5534b",
  "#ec4899",
  "#8b5cf6",
  "#5aa2f0",
  "#22d3ee",
  "#4ec9a0",
  "#f5c518",
];
function onThemeColorInput(e: Event) {
  const val = (e.target as HTMLInputElement).value;
  if (val) settings.patch({ theme_color: val });
}

// 关闭行为 seg 滑动高亮
const closeSegRef = ref<HTMLElement | null>(null);
const { indicatorStyle: closeSegStyle, refresh: refreshCloseSeg } = useSlidingIndicator(
  closeSegRef,
  () => Array.from(closeSegRef.value?.querySelectorAll<HTMLElement>(".seg button") ?? []),
  () => (settings.settings?.close_behavior === "quit" ? 1 : 0),
  { axis: "horizontal" }
);
watch(() => settings.settings?.close_behavior, () => nextTick(() => refreshCloseSeg()));

// 更新源 seg 滑动高亮（国内镜像 / GitHub 官方）
const updateSourceSegRef = ref<HTMLElement | null>(null);
const { indicatorStyle: updateSourceSegStyle, refresh: refreshUpdateSourceSeg } =
  useSlidingIndicator(
    updateSourceSegRef,
    () => Array.from(updateSourceSegRef.value?.querySelectorAll<HTMLElement>(".seg button") ?? []),
    () => (settings.settings?.update_source === "github" ? 1 : 0),
    { axis: "horizontal" }
  );
watch(() => settings.settings?.update_source, () => nextTick(() => refreshUpdateSourceSeg()));

// 下载代理 seg 滑动高亮（系统代理 / 直连 / 自定义）
const proxyModeSegRef = ref<HTMLElement | null>(null);
const proxyModes = [
  { id: "system", label: "系统代理" },
  { id: "direct", label: "直连" },
  { id: "custom", label: "自定义" },
];
const { indicatorStyle: proxyModeSegStyle, refresh: refreshProxyModeSeg } = useSlidingIndicator(
  proxyModeSegRef,
  () => Array.from(proxyModeSegRef.value?.querySelectorAll<HTMLElement>(".seg button") ?? []),
  () => Math.max(0, proxyModes.findIndex((m) => m.id === settings.settings?.proxy_mode)),
  { axis: "horizontal" }
);
watch(() => settings.settings?.proxy_mode, () => nextTick(() => refreshProxyModeSeg()));

async function selectProxyMode(id: string) {
  if (settings.settings?.proxy_mode === id) return;
  try {
    await settings.patch({ proxy_mode: id });
  } catch (e) {
    message.error(String(e));
  }
}

function onCustomProxyInput() {
  if (settings.settings && settings.settings.proxy_mode !== "custom") {
    settings.settings.proxy_mode = "custom";
  }
}

// 测试代理连接
const testingProxy = ref(false);
async function testProxy() {
  if (testingProxy.value || !settings.settings) return;
  testingProxy.value = true;
  try {
    const { proxy_mode, proxy } = settings.settings;
    const res = await api.testProxy(
      proxy_mode,
      proxy_mode === "custom" ? proxy : null
    );
    message.success(`连接成功 ${res.ms} ms`);
  } catch (e) {
    message.error(`连接失败: ${e}`);
  } finally {
    testingProxy.value = false;
  }
}

// 下载镜像源
const mirrors = ref<MirrorPreset[]>([]);
/** 每个镜像最近一次测速结果（毫秒）；null 表示不可用 */
const mirrorLatency = ref<Record<string, number | null>>({});
const testingMirror = ref("");

async function loadMirrors() {
  try {
    mirrors.value = await api.listMirrors();
  } catch {
    mirrors.value = [];
  }
}

async function selectMirror(id: string) {
  if (settings.settings?.mirror === id) return;
  try {
    await settings.patch({ mirror: id });
  } catch (e) {
    message.error(String(e));
  }
}

async function testMirror(id: string, base: string) {
  if (testingMirror.value) return;
  testingMirror.value = id;
  try {
    const res = await api.testMirror(base);
    mirrorLatency.value = { ...mirrorLatency.value, [id]: res.ms };
  } catch (e) {
    mirrorLatency.value = { ...mirrorLatency.value, [id]: null };
    message.error(String(e));
  } finally {
    testingMirror.value = "";
  }
}

function onCustomMirrorInput() {
  if (settings.settings && settings.settings.mirror !== "custom") {
    settings.settings.mirror = "custom";
  }
}

const javaCandidates = ref<JavaInfo[]>([]);
const detecting = ref(false);
let saveTimer: ReturnType<typeof setTimeout> | null = null;
const tab = ref("general");
// 更新源位于「关于」页，面板用 v-show 隐藏时矩形为 0，需在切换到该页时刷新指示器
watch(tab, (val) => {
  // 这些面板用 v-show 隐藏时矩形为 0，需在切换到对应页时刷新滑动指示器
  if (val === "appearance") nextTick(() => refreshThemeSeg());
  if (val === "about") nextTick(() => refreshUpdateSourceSeg());
  if (val === "download") nextTick(() => refreshProxyModeSeg());
});

const tabs = [
  { key: "general", label: "常规", icon: IconSliders },
  { key: "appearance", label: "外观", icon: IconImage },
  { key: "java", label: "Java", icon: IconCpu },
  { key: "download", label: "下载", icon: IconDownload },
  { key: "content", label: "内容服务", icon: IconGlobe },
  { key: "storage", label: "存储", icon: IconHardDrive },
  { key: "about", label: "关于", icon: IconFile },
];

const memTotal = ref(0);
const memUsed = ref(0);
const memAvailable = ref(0);
let memTimer: ReturnType<typeof setInterval> | null = null;

function fmtMem(mb: number): string {
  if (!mb || mb <= 0) return "0 MB";
  if (mb >= 1024) return (mb / 1024).toFixed(mb % 1024 === 0 ? 0 : 1) + " GB";
  return Math.round(mb) + " MB";
}

const effectiveMemory = computed(() => {
  if (settings.settings?.memory_mode === "auto") {
    // Base: 40% of available (min 2048 MB), cap at 75% of available
    const cap = Math.max(512, Math.floor(memAvailable.value * 3 / 4));
    return Math.max(512, Math.min(Math.max(2048, Math.floor(memAvailable.value * 40 / 100)), cap, 8192));
  }
  return settings.settings?.max_memory_mb ?? 4096;
});
const usedPercent = computed(() => {
  if (!memTotal.value) return 0;
  return Math.min(100, Math.round((memUsed.value / memTotal.value) * 100));
});
const allocPercent = computed(() => {
  if (!memTotal.value) return 0;
  return Math.min(100, Math.round((effectiveMemory.value / memTotal.value) * 100));
});
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
    /* ignore */
  }
}

async function detect() {
  detecting.value = true;
  try {
    await settings.loadJava(true);
    javaCandidates.value = settings.javaCandidates;
  } catch (e) {
    message.error(String(e));
  } finally {
    detecting.value = false;
  }
}

async function save() {
  try {
    skipNextSave = true;
    await settings.save();
  } catch (e) {
    message.error(String(e));
  }
}

let skipNextSave = true;
watch(
  () => settings.settings,
  () => {
    if (skipNextSave) {
      skipNextSave = false;
      return;
    }
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(save, 500);
  },
  { deep: true }
);

async function openPath(path: string) {
  try {
    await openUrl("file://" + path.replace(/\\/g, "/"));
  } catch {
    /* ignore */
  }
}

async function pickBackground() {
  try {
    const picked = await open({
      multiple: false,
      filters: [{ name: "图片", extensions: ["png", "jpg", "jpeg", "gif", "webp", "bmp"] }],
    });
    if (!picked || typeof picked !== "string") return;
    const path = await api.importBackgroundImage(picked);
    await settings.patch({ background_image: path });
  } catch (e) {
    message.error(String(e));
  }
}

const bgPreviewUrl = computed(() => {
  const p = settings.settings?.background_image;
  return p ? convertFileSrc(p) : "";
});

// 数据目录迁移
const migrating = ref(false);
const migrateModal = ref(false);
const migratePhase = ref<"select" | "done">("select");
const pendingNewDir = ref("");
const migrateMode = ref<"move" | "copy" | "pointer">("move");

async function pickDataDir() {
  try {
    const dir = await open({ directory: true, title: "选择新的数据目录" });
    if (!dir || typeof dir !== "string") return;
    pendingNewDir.value = dir;
    migrateMode.value = "move";
    migratePhase.value = "select";
    migrateModal.value = true;
  } catch {
    /* ignore */
  }
}

async function confirmMigrate() {
  if (!pendingNewDir.value) return;
  migrating.value = true;
  try {
    const res = await api.changeDataDir(pendingNewDir.value, migrateMode.value);
    if (settings.settings) settings.settings.data_dir = res.new_dir;
    migratePhase.value = "done";
  } catch (e) {
    message.error(String(e));
  } finally {
    migrating.value = false;
  }
}

async function relaunchNow() {
  migrateModal.value = false;
  try {
    await relaunch();
  } catch (e) {
    message.error("重启失败：" + String(e));
  }
}

// ---------------------------------------------------------------------------
// 存储统计
// ---------------------------------------------------------------------------
const stats = ref<StorageStats | null>(null);
const loadingStats = ref(false);
const clearing = ref(false);

const DONUT_COLORS: Record<string, string> = {
  instances: "var(--accent)",
  servers: "#f59e0b",
  libraries: "#5aa2f0",
  assets: "#4ec9a0",
  versions: "#8b5cf6",
  runtime: "#ec4899",
  logs: "#94a3b8",
  other: "#64748b",
  launcher: "#22d3ee",
};
const DONUT_R = 74;
const DONUT_C = 2 * Math.PI * DONUT_R;

function fmtSize(bytes: number): string {
  if (!bytes || bytes <= 0) return "0 B";
  if (bytes < 1024) return bytes + " B";
  const units = ["KB", "MB", "GB", "TB"];
  let v = bytes / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return (v >= 100 ? v.toFixed(0) : v.toFixed(1)) + " " + units[i];
}

function fmtTime(ts: number): string {
  if (!ts) return "—";
  const d = new Date(ts * 1000);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function pct(size: number): number {
  if (!stats.value || !stats.value.total) return 0;
  return Math.round((size / stats.value.total) * 1000) / 10;
}

const visibleCats = computed(() => (stats.value?.categories ?? []).filter((c) => c.size > 0));

/** 环形饼图各分段：start 偏移累积，返回 dash/offset 供 SVG stroke-dasharray 使用 */
const donutSegs = computed(() => {
  const total = stats.value?.total ?? 0;
  if (!total) return [];
  let acc = 0;
  return visibleCats.value.map((c) => {
    const frac = c.size / total;
    const dash = Math.max(0, frac * DONUT_C - 1.5);
    const seg = { key: c.key, dash, offset: -acc, color: DONUT_COLORS[c.key] ?? "#64748b" };
    acc += frac * DONUT_C;
    return seg;
  });
});

async function loadStats() {
  loadingStats.value = true;
  try {
    stats.value = await api.getStorageStats();
  } catch (e) {
    message.error("加载存储统计失败：" + String(e));
  } finally {
    loadingStats.value = false;
  }
}

async function refreshStats() {
  loadingStats.value = true;
  try {
    stats.value = await api.refreshStorageStats();
    message.success("已更新存储统计");
  } catch (e) {
    message.error("更新存储统计失败：" + String(e));
  } finally {
    loadingStats.value = false;
  }
}

function confirmClear() {
  dialog.warning({
    title: "清除缓存",
    content:
      "将清理 Java 下载临时文件、Java 检测缓存等可安全删除的缓存，不会影响任何实例、库、资源或版本文件。确定继续吗？",
    positiveText: "清除",
    negativeText: "取消",
    onPositiveClick: async () => {
      clearing.value = true;
      try {
        const res = await api.clearCache();
        message.success(`已清除缓存，释放 ${fmtSize(res.freed)}`);
        await refreshStats();
      } catch (e) {
        message.error("清除缓存失败：" + String(e));
      } finally {
        clearing.value = false;
      }
    },
  });
}

onMounted(() => {
  settings.load();
  // cached scan: no full rescan if another view already fetched recently
  settings.loadJava().then((c) => (javaCandidates.value = c));
  loadMemoryInfo();
  memTimer = setInterval(loadMemoryInfo, 10000);
  loadStats();
});
onUnmounted(() => {
  if (memTimer) clearInterval(memTimer);
  if (saveTimer) clearTimeout(saveTimer);
});
</script>

<template>
  <div v-if="settings.settings" class="settings-view">
    <aside class="settings-nav">
      <nav class="nav-list">
        <button
          v-for="t in tabs"
          :key="t.key"
          class="nav-item"
          :class="{ active: tab === t.key }"
          @click="tab = t.key"
        >
          <component :is="t.icon" class="nav-icon" />
          <span>{{ t.label }}</span>
        </button>
      </nav>
    </aside>

    <div class="settings-body">
      <!-- 常规 -->
      <div v-show="tab === 'general'" class="settings-pane">
        <div class="grid">
          <div class="card glass">
            <h3>行为</h3>
            <div class="choice-row">
              <span>关闭窗口时</span>
              <div ref="closeSegRef" class="seg">
                <div class="indicator" :style="closeSegStyle"></div>
                <button
                  :class="{ active: settings.settings.close_behavior === 'minimize' }"
                  @click="settings.patch({ close_behavior: 'minimize' })"
                >
                  最小化到后台
                </button>
                <button
                  :class="{ active: settings.settings.close_behavior === 'quit' }"
                  @click="settings.patch({ close_behavior: 'quit' })"
                >
                  退出程序
                </button>
              </div>
            </div>
            <div class="choice-row">
              <div class="choice-info">
                <span class="choice-label">自动更新</span>
                <p class="choice-hint">启动时检测到新版本自动下载安装，无需手动确认。</p>
              </div>
              <button
                class="toggle"
                :class="{ on: settings.settings.auto_update }"
                role="switch"
                :aria-checked="settings.settings.auto_update"
                @click="settings.patch({ auto_update: !settings.settings.auto_update })"
              >
                <span class="knob"></span>
              </button>
            </div>
          </div>

          <div class="card glass">
            <h3>数据目录</h3>
            <div class="dir-row">
              <code class="mono dir">{{ settings.settings.data_dir }}</code>
              <button class="mini-btn" @click="openPath(settings.settings.data_dir)">打开</button>
              <button class="mini-btn" @click="pickDataDir">更改</button>
            </div>
            <p class="hint">实例、游戏文件与下载缓存均存储在此目录。点击「更改」可迁移到其他位置。</p>
          </div>
        </div>
      </div>

      <!-- 外观 -->
      <div v-show="tab === 'appearance'" class="settings-pane">
        <div class="card glass">
          <h3>主题</h3>
          <div class="choice-row">
            <span>主题</span>
            <div ref="themeSegRef" class="seg">
              <div class="indicator" :style="themeSegStyle"></div>
              <button
                :class="{ active: settings.settings.theme === 'dark' }"
                @click="settings.patch({ theme: 'dark' })"
              >
                深色
              </button>
              <button
                :class="{ active: settings.settings.theme === 'light' }"
                @click="settings.patch({ theme: 'light' })"
              >
                浅色
              </button>
            </div>
          </div>
          <div class="appearance-divider"></div>
          <div class="choice-row">
            <span>主题色</span>
            <div class="theme-color-row">
              <button
                v-for="c in themeColorPresets"
                :key="c"
                type="button"
                class="color-swatch"
                :class="{ active: settings.settings.theme_color === c }"
                :style="{ background: c }"
                :title="c"
                @click="settings.patch({ theme_color: c })"
              ></button>
              <label class="color-custom" title="自定义颜色">
                <span class="color-custom-ring" :style="{ background: settings.settings.theme_color }"></span>
                <input type="color" :value="settings.settings.theme_color" @input="onThemeColorInput" />
              </label>
            </div>
          </div>
        </div>
        <div class="card glass">
          <h3>界面</h3>
          <div class="choice-row">
            <div class="choice-info">
              <span class="choice-label">首页主标题卡片</span>
              <p class="choice-hint">控制首页顶部的主标题卡片是否显示，关闭后首页更加简洁。</p>
            </div>
            <button
              class="toggle"
              :class="{ on: settings.settings.show_home_hero }"
              role="switch"
              :aria-checked="settings.settings.show_home_hero"
              @click="settings.patch({ show_home_hero: !settings.settings.show_home_hero })"
            >
              <span class="knob"></span>
            </button>
          </div>
          <div class="choice-row">
            <div class="choice-info">
              <span class="choice-label">侧边栏折叠按钮</span>
              <p class="choice-hint">控制侧边栏底部的展开/收缩按钮是否显示，关闭后可保持侧边栏固定。</p>
            </div>
            <button
              class="toggle"
              :class="{ on: settings.settings.show_sidebar_collapse_btn }"
              role="switch"
              :aria-checked="settings.settings.show_sidebar_collapse_btn"
              @click="settings.patch({ show_sidebar_collapse_btn: !settings.settings.show_sidebar_collapse_btn })"
            >
              <span class="knob"></span>
            </button>
          </div>
        </div>
        <div class="card glass">
          <h3>背景图片</h3>
          <div v-if="settings.settings.background_image" class="bg-preview">
            <img :src="bgPreviewUrl" alt="背景预览" />
          </div>
          <div class="choice-row">
            <span>背景图片</span>
            <div class="bg-actions">
              <button class="mini-btn" @click="pickBackground">选择图片</button>
              <button
                v-if="settings.settings.background_image"
                class="mini-btn"
                @click="settings.patch({ background_image: null })"
              >
                清除
              </button>
            </div>
          </div>
          <div v-if="settings.settings.background_image" class="tune-block">
            <div class="tune-row">
              <label>背景模糊</label>
              <input
                v-model.number="settings.settings.background_blur"
                type="range"
                min="0"
                max="50"
                step="1"
                class="range"
              />
              <span class="tune-val">{{ settings.settings.background_blur }} px</span>
            </div>
            <div class="tune-row">
              <label>背景遮罩</label>
              <input
                v-model.number="settings.settings.background_dim"
                type="range"
                min="0"
                max="100"
                step="5"
                class="range"
              />
              <span class="tune-val">{{ settings.settings.background_dim }}%</span>
            </div>
          </div>
        </div>
        <div class="card glass">
          <h3>磨砂卡片</h3>
          <div class="tune-row">
            <label>磨砂强度</label>
            <input
              v-model.number="settings.settings.glass_blur"
              type="range"
              min="0"
              max="30"
              step="1"
              class="range"
            />
            <span class="tune-val">{{ settings.settings.glass_blur }} px</span>
          </div>
          <p class="hint">调节卡片毛玻璃模糊半径，数值越大磨砂越强。</p>
        </div>
      </div>

      <!-- Java -->
      <div v-show="tab === 'java'" class="settings-pane">
        <div class="grid">
          <div class="card glass">
            <h3><IconCpu /> Java 运行时</h3>
            <div class="java-toolbar">
              <button class="mini-btn" :disabled="detecting" @click="detect">
                <IconSearch /> {{ detecting ? "查找中…" : "查找 Java" }}
              </button>
              <span class="hint-inline">自动扫描注册表、系统路径与常见安装目录</span>
            </div>
            <div v-if="javaCandidates.length" class="java-list">
              <div v-for="j in javaCandidates" :key="j.path" class="java-item">
                <span class="java-name">Java {{ j.major }} ({{ j.version }})</span>
                <span class="java-path">{{ j.path }}</span>
              </div>
            </div>
            <p v-else-if="!detecting" class="hint">未检测到 Java。可在实例设置中触发自动下载。</p>
            <p class="hint">Java 选择按实例独立设置：进入「游戏实例 → 实例 → 设置」，可为每个实例指定 Java 或自动下载适配版本。</p>
          </div>

          <div class="card glass">
            <h3>内存分配（默认值）</h3>
            <div class="mem-mode-row">
              <label class="radio-label" :class="{ active: settings.settings.memory_mode === 'auto' }">
                <input v-model="settings.settings.memory_mode" type="radio" value="auto" />
                自动配置
              </label>
              <label class="radio-label" :class="{ active: settings.settings.memory_mode !== 'auto' }">
                <input v-model="settings.settings.memory_mode" type="radio" value="custom" />
                手动配置
              </label>
            </div>
            <div v-if="settings.settings.memory_mode !== 'auto'" class="mem-row">
              <div>
                <label>最大内存</label>
                <input
                  v-model.number="settings.settings.max_memory_mb"
                  type="range"
                  min="1024"
                  max="16384"
                  step="256"
                  class="range"
                />
                <div class="mem-val">{{ settings.settings.max_memory_mb }} MB</div>
              </div>
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

          <div class="card glass">
            <h3>JVM 参数（额外，默认值）</h3>
            <textarea
              v-model="settings.settings.jvm_args"
              class="text-input mono"
              rows="3"
              placeholder="例如：-XX:+UseG1GC -XX:MaxGCPauseMillis=50"
            />
          </div>

          <div class="card glass">
            <h3>游戏参数（额外，默认值）</h3>
            <input
              v-model="settings.settings.game_args"
              class="text-input mono"
              placeholder="例如：--fullscreen"
            />
          </div>
        </div>
      </div>

      <!-- 下载 -->
      <div v-show="tab === 'download'" class="settings-pane">
        <div class="grid">
          <div class="card glass">
            <h3>并行下载</h3>
            <label class="row-label">
              同时下载文件数：{{ settings.settings.download_threads }}
              <input
                v-model.number="settings.settings.download_threads"
                type="range"
                min="1"
                max="32"
                step="1"
                class="range"
              />
            </label>
            <p class="hint">同时从服务器下载的文件数量。值越大并发越高，但对服务器压力也越大。</p>
            <label class="row-label" style="margin-top: 16px;">
              单文件分片线程数：{{ settings.settings.download_chunk_threads }}
              <input
                v-model.number="settings.settings.download_chunk_threads"
                type="range"
                min="1"
                max="16"
                step="1"
                class="range"
              />
            </label>
            <p class="hint">对单个大文件使用 HTTP Range 分片并行下载的线程数。仅对支持断点续传的服务器生效，小文件始终单线程。</p>
          </div>
          <div class="card glass">
            <h3>下载中心</h3>
            <p class="hint">所有安装与下载任务可在左侧「下载中心」实时查看进度、速度与剩余文件。</p>
          </div>
        </div>
      </div>

      <!-- 内容服务 -->
      <div v-show="tab === 'content'" class="settings-pane">
        <div class="grid">
          <div class="card glass">
            <h3>CurseForge API Key</h3>
            <input
              v-model="settings.settings.curseforge_api_key"
              class="text-input mono"
              placeholder="在 console.curseforge.com 免费申请"
            />
            <p class="hint">可选。不填使用默认key 可能会导致 CurseForge 内容中心不可用，Modrinth 不受影响。</p>
          </div>
          <div class="card glass">
            <h3>下载代理</h3>
            <div class="proxy-row">
              <div ref="proxyModeSegRef" class="seg">
                <div class="indicator" :style="proxyModeSegStyle"></div>
                <button
                  v-for="m in proxyModes"
                  :key="m.id"
                  :class="{ active: settings.settings.proxy_mode === m.id }"
                  @click="selectProxyMode(m.id)"
                >
                  {{ m.label }}
                </button>
              </div>
              <button
                class="mirror-btn"
                :class="{ disabled: testingProxy }"
                @click="testProxy"
              >
                {{ testingProxy ? "测试中…" : "测试连接" }}
              </button>
            </div>
            <input
              v-if="settings.settings.proxy_mode === 'custom'"
              v-model="settings.settings.proxy"
              class="text-input mono"
              placeholder="http://127.0.0.1:7890 或 socks5://127.0.0.1:1080"
              @input="onCustomProxyInput"
            />
            <p class="hint">
              {{
                settings.settings.proxy_mode === "system"
                  ? "使用系统网络代理设置（默认）。"
                  : settings.settings.proxy_mode === "direct"
                    ? "直连，不经过任何代理。"
                    : "自定义代理。用于绕过 CDN 下载失败（404/连接失败）。修改后需重启启动器生效。"
              }}
            </p>
          </div>
        </div>
      </div>

      <!-- 存储 -->
      <div v-show="tab === 'storage'" class="settings-pane">
        <div class="grid storage-grid">
          <div class="card glass storage-card">
            <div class="storage-header">
              <h3>存储统计</h3>
              <div class="storage-actions">
                <span class="hint-inline">
                  <template v-if="stats">{{ stats.cached ? "上次更新" : "已更新" }}：{{ fmtTime(stats.updated_at) }}</template>
                  <template v-else>尚未扫描</template>
                </span>
                <button class="mini-btn" :disabled="loadingStats" @click="refreshStats">
                  <IconRefresh class="btn-icon" />
                  {{ loadingStats ? "扫描中…" : "更新" }}
                </button>
              </div>
            </div>

            <div v-if="stats && stats.total > 0" class="storage-body">
              <div class="donut-wrap">
                <svg viewBox="0 0 200 200" class="donut">
                  <circle
                    v-for="seg in donutSegs"
                    :key="seg.key"
                    cx="100"
                    cy="100"
                    r="74"
                    fill="none"
                    :stroke="seg.color"
                    stroke-width="30"
                    :stroke-dasharray="`${seg.dash} ${DONUT_C - seg.dash}`"
                    :stroke-dashoffset="seg.offset"
                    transform="rotate(-90 100 100)"
                  />
                </svg>
                <div class="donut-center">
                  <span class="donut-total">{{ fmtSize(stats.total) }}</span>
                  <span class="donut-label">总占用</span>
                </div>
              </div>

              <ul class="storage-legend">
                <li v-for="cat in visibleCats" :key="cat.key">
                  <span class="legend-dot" :style="{ background: DONUT_COLORS[cat.key] ?? '#64748b' }"></span>
                  <span class="legend-name">{{ cat.label }}</span>
                  <span class="legend-size">{{ fmtSize(cat.size) }}</span>
                  <span class="legend-pct">{{ pct(cat.size) }}%</span>
                </li>
              </ul>
            </div>

            <div v-if="stats && stats.instances.length" class="instance-storage">
              <h4 class="instance-storage-title">
                每个实例
                <span class="hint-inline">{{ stats.instances.length }} 个</span>
              </h4>
              <ul class="instance-storage-list">
                <li v-for="inst in stats.instances" :key="inst.id">
                  <span class="instance-name" :title="inst.name">{{ inst.name }}</span>
                  <span class="legend-size">{{ fmtSize(inst.size) }}</span>
                  <span class="legend-pct">{{ pct(inst.size) }}%</span>
                </li>
              </ul>
            </div>

            <div v-if="stats && stats.servers.length" class="instance-storage">
              <h4 class="instance-storage-title">
                每个服务器
                <span class="hint-inline">{{ stats.servers.length }} 个</span>
              </h4>
              <ul class="instance-storage-list">
                <li v-for="srv in stats.servers" :key="srv.id">
                  <span class="instance-name" :title="srv.name">{{ srv.name }}</span>
                  <span class="legend-size">{{ fmtSize(srv.size) }}</span>
                  <span class="legend-pct">{{ pct(srv.size) }}%</span>
                </li>
              </ul>
            </div>
            <p v-else-if="!stats?.instances.length" class="hint">{{ stats ? "暂无可统计的数据" : "正在加载存储统计…" }}</p>

            <div class="storage-footer">
              <button class="mini-btn danger" :disabled="clearing" @click="confirmClear">
                <IconTrash class="btn-icon" />
                {{ clearing ? "清理中…" : "清除缓存" }}
              </button>
              <span class="hint">清理 Java 下载临时文件、Java 检测缓存等可安全删除的缓存，不会影响实例、库、资源或版本文件。</span>
            </div>
          </div>
        </div>
      </div>

      <!-- 关于 -->
      <div v-show="tab === 'about'" class="settings-pane">
        <div class="grid about-grid">
          <div class="card glass about-card">
            <div class="about-logo">
              <img class="about-logo-img" :src="logoUrl" alt="QookiX" />
              <span class="about-name">QookiX Launcher</span>
              <span class="about-ver">v0.4.5</span>
            </div>
            <p class="about-desc">现代化、简洁、无广告的 Minecraft 启动器</p>
            <div class="about-features">
              <div class="feature-item"><span class="feature-dot"></span>Modrinth / CurseForge 内容中心</div>
              <div class="feature-item"><span class="feature-dot"></span>多线程高速下载</div>
              <div class="feature-item"><span class="feature-dot"></span>Java 自动检测与下载</div>
              <div class="feature-item"><span class="feature-dot"></span>微软正版与离线登录</div>
              <div class="feature-item"><span class="feature-dot"></span>离线皮肤支持</div>
            </div>
            <div class="about-update">
              <button class="mini-btn primary" :disabled="checking" @click="checkUpdate">
                {{ checking ? "检查中…" : "检查更新" }}
              </button>
              <span v-if="updateVersion" class="hint-inline">
                发现新版本 v{{ updateVersion }}
              </span>
              <button
                v-if="settings.settings?.dismissed_update_version"
                class="mini-btn"
                @click="restoreDismissed"
              >
                恢复 v{{ settings.settings.dismissed_update_version }} 更新提醒
              </button>
            </div>
            <div class="update-source">
              <div class="choice-info">
                <span class="choice-label">更新源</span>
                <p class="choice-hint">选择从哪个渠道下载启动器更新。默认使用国内镜像（更快），也可切换到 GitHub 官方源（最新），切换后点「检查更新」立即生效。</p>
              </div>
              <div ref="updateSourceSegRef" class="seg">
                <div class="indicator" :style="updateSourceSegStyle"></div>
                <button
                  :class="{ active: settings.settings.update_source !== 'github' }"
                  @click="settings.patch({ update_source: 'bucket' })"
                >
                  国内镜像
                </button>
                <button
                  :class="{ active: settings.settings.update_source === 'github' }"
                  @click="settings.patch({ update_source: 'github' })"
                >
                  GitHub 官方
                </button>
              </div>
            </div>
          </div>
          <div class="card glass about-links">
            <h3>链接</h3>
            <button class="about-link" @click="openUrl('https://qookix.swkj1.cn/')">
              <span>官方网站</span>
              <span class="link-arrow">→</span>
            </button>
            <button class="about-link" @click="openUrl('https://github.com/weimosheng/QookiX-Launcher')">
              <span>GitHub 仓库</span>
              <span class="link-arrow">→</span>
            </button>
            <button class="about-link" @click="openUrl('https://github.com/weimosheng/QookiX-Launcher/issues')">
              <span>问题反馈</span>
              <span class="link-arrow">→</span>
            </button>
            <button class="about-link" @click="openUrl('https://github.com/weimosheng/QookiX-Launcher/releases')">
              <span>更新日志</span>
              <span class="link-arrow">→</span>
            </button>

          </div>
        </div>
      </div>
    </div>

    <n-modal v-model:show="migrateModal" preset="card" title="更改数据目录" class="migrate-modal">
      <div v-if="migratePhase === 'select'" class="migrate-body">
        <p class="migrate-label">新数据目录：</p>
        <code class="mono dir">{{ pendingNewDir }}</code>
        <p class="migrate-label">迁移方式：</p>
        <div class="migrate-modes">
          <label :class="{ active: migrateMode === 'move' }">
            <input type="radio" value="move" v-model="migrateMode" />
            <span class="mm-title">移动数据</span>
            <span class="mm-desc">把所有数据移动到新目录（推荐，同盘瞬间完成）</span>
          </label>
          <label :class="{ active: migrateMode === 'copy' }">
            <input type="radio" value="copy" v-model="migrateMode" />
            <span class="mm-title">复制数据</span>
            <span class="mm-desc">复制到新目录，保留旧目录作为备份</span>
          </label>
          <label :class="{ active: migrateMode === 'pointer' }">
            <input type="radio" value="pointer" v-model="migrateMode" />
            <span class="mm-title">仅切换目录</span>
            <span class="mm-desc">不迁移数据，仅指向新目录（需自行处理数据）</span>
          </label>
        </div>
        <p class="migrate-warn">更改后需要重启应用才能完全生效。</p>
        <div class="migrate-actions">
          <button class="mini-btn" @click="migrateModal = false">取消</button>
          <button class="mini-btn primary" :disabled="migrating" @click="confirmMigrate">
            {{ migrating ? "迁移中…" : "开始迁移" }}
          </button>
        </div>
      </div>
      <div v-else class="migrate-body">
        <p class="migrate-ok">数据目录已更改：</p>
        <code class="mono dir">{{ pendingNewDir }}</code>
        <p class="migrate-warn">需要重启应用以完全生效。</p>
        <div class="migrate-actions">
          <button class="mini-btn" @click="migrateModal = false">稍后重启</button>
          <button class="mini-btn primary" @click="relaunchNow">立即重启</button>
        </div>
      </div>
    </n-modal>
  </div>
</template>

<style scoped>
.settings-view {
  display: flex;
  gap: 18px;
  align-items: flex-start;
}
.settings-nav {
  flex-shrink: 0;
  width: 188px;
  position: sticky;
  top: 0;
  padding: 16px 14px;
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 14px;
  backdrop-filter: blur(var(--glass-blur, 8px));
  -webkit-backdrop-filter: blur(var(--glass-blur, 8px));
}
.nav-list {
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.nav-item {
  display: flex;
  align-items: center;
  gap: 9px;
  text-align: left;
  border: none;
  background: transparent;
  color: var(--text-2);
  padding: 9px 12px;
  border-radius: 9px;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  font-family: inherit;
  transition: background 0.12s, color 0.12s;
}
.nav-icon {
  width: 15px;
  height: 15px;
  flex-shrink: 0;
  opacity: 0.85;
}
.nav-item:hover {
  background: var(--panel-hover);
  color: var(--text-1);
}
.nav-item.active {
  background: var(--accent-soft);
  color: var(--accent);
  font-weight: 600;
}
.nav-item.active .nav-icon {
  opacity: 1;
}
.settings-body {
  flex: 1;
  min-width: 0;
}
.settings-pane {
  display: flex;
  flex-direction: column;
  gap: 18px;
}
.btn {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  border: none;
  border-radius: 10px;
  padding: 10px 18px;
  font-size: 14px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  transition: all 0.15s;
}
.btn.primary {
  background: linear-gradient(135deg, var(--accent), var(--accent-deep));
  color: #1a1208;
}
.btn.primary:hover:not(:disabled) {
  filter: brightness(1.08);
}
.btn:disabled {
  opacity: 0.6;
}
.settings-pane > .grid {
  margin-top: 0;
}
.grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 16px;
  margin-top: 14px;
}
.card {
  padding: 18px;
}
.card h3 {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0 0 14px;
  font-size: 14px;
}
.card h3 svg {
  color: var(--accent);
}
.java-row {
  display: flex;
  gap: 8px;
}
.text-input {
  flex: 1;
  width: 100%;
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
  border-color: var(--accent-05);
}
textarea.text-input {
  resize: vertical;
  width: 100%;
}
.mini-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 7px 13px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-2);
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  white-space: nowrap;
}
.mini-btn:hover {
  background: rgba(255, 255, 255, 0.08);
}
.mini-btn:disabled {
  opacity: 0.5;
}
.mini-btn.danger {
  border-color: rgba(229, 83, 75, 0.35);
  color: #e5534b;
  background: rgba(229, 83, 75, 0.08);
}
.mini-btn.danger:hover {
  background: rgba(229, 83, 75, 0.16);
}
.btn-icon {
  width: 14px;
  height: 14px;
}
.storage-grid {
  grid-template-columns: 1fr;
}
.storage-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 14px;
}
.storage-header h3 {
  margin: 0;
}
.storage-actions {
  display: flex;
  align-items: center;
  gap: 12px;
}
.storage-footer {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-top: 18px;
  padding-top: 16px;
  border-top: 1px solid var(--border);
  flex-wrap: wrap;
}
.storage-footer .hint {
  margin: 0;
  flex: 1;
  min-width: 200px;
}
.storage-body {
  display: flex;
  align-items: center;
  gap: 26px;
}
.donut-wrap {
  position: relative;
  flex: none;
  width: 190px;
  height: 190px;
}
.donut {
  width: 100%;
  height: 100%;
}
.donut-center {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  pointer-events: none;
}
.donut-total {
  font-size: 20px;
  font-weight: 700;
  color: var(--text-1);
  line-height: 1.2;
}
.donut-label {
  font-size: 12px;
  color: var(--text-3);
  margin-top: 2px;
}
.storage-legend {
  list-style: none;
  margin: 0;
  padding: 0;
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 9px;
  min-width: 0;
}
.storage-legend li {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
}
.legend-dot {
  flex: none;
  width: 10px;
  height: 10px;
  border-radius: 50%;
}
.legend-name {
  color: var(--text-2);
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.legend-size {
  color: var(--text-1);
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}
.legend-pct {
  color: var(--text-3);
  width: 48px;
  text-align: right;
  font-variant-numeric: tabular-nums;
}
.instance-storage {
  margin-top: 18px;
  border-top: 1px solid var(--border);
  padding-top: 12px;
}
.instance-storage-title {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0 0 10px;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-2);
}
.instance-storage-list {
  list-style: none;
  margin: 0;
  padding: 0;
  max-height: 220px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.instance-storage-list li {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 13px;
}
.instance-name {
  color: var(--text-1);
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.java-toolbar {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 12px;
}
.hint-inline {
  font-size: 12px;
  color: var(--text-3);
}
.java-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 260px;
  overflow-y: auto;
}
.java-item {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 7px 12px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: rgba(255, 255, 255, 0.03);
  color: var(--text-2);
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
.hint {
  font-size: 12px;
  color: var(--text-3);
  margin: 10px 0 0;
}
.mem-row {
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.mem-row label {
  display: block;
  font-size: 13px;
  color: var(--text-2);
  margin-bottom: 8px;
}
.mem-val {
  font-size: 13px;
  color: var(--accent);
  font-weight: 600;
  margin-top: 4px;
}
.mem-mode-row {
  display: flex;
  gap: 16px;
  margin-bottom: 14px;
}
.radio-label {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  color: var(--text-2);
  cursor: pointer;
  user-select: none;
}
.radio-label input {
  accent-color: var(--accent);
  cursor: pointer;
}
.radio-label.active {
  color: var(--accent);
  font-weight: 600;
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
.range {
  width: 100%;
  accent-color: var(--accent);
}
.row-label {
  display: block;
  font-size: 13px;
  color: var(--text-2);
  margin-bottom: 10px;
}
.choice-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 16px;
  margin-bottom: 14px;
  font-size: 13px;
  color: var(--text-2);
}
.choice-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.choice-label {
  color: var(--text-1);
  font-weight: 500;
}
.choice-hint {
  font-size: 12px;
  color: var(--text-3);
  margin: 0;
  line-height: 1.5;
}
.seg {
  position: relative;
  display: flex;
  background: rgba(255, 255, 255, 0.05);
  border-radius: 9px;
  padding: 3px;
}
.seg .indicator {
  position: absolute;
  top: 3px;
  bottom: 3px;
  border-radius: 7px;
  background: var(--accent-soft);
  pointer-events: none;
}
.seg button {
  border: none;
  background: transparent;
  color: var(--text-3);
  padding: 6px 13px;
  border-radius: 7px;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
}
.seg button.active {
  color: var(--accent);
}
.theme-color-row {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}
.color-swatch {
  width: 22px;
  height: 22px;
  border-radius: 50%;
  border: 2px solid transparent;
  background: transparent;
  cursor: pointer;
  padding: 0;
  position: relative;
  transition: transform 0.12s ease, border-color 0.12s ease;
}
.color-swatch:hover {
  transform: scale(1.12);
}
.color-swatch.active {
  border-color: var(--text-1);
  box-shadow: 0 0 0 2px var(--accent-soft);
}
.color-custom {
  position: relative;
  width: 22px;
  height: 22px;
  border-radius: 50%;
  cursor: pointer;
  overflow: hidden;
  box-shadow: inset 0 0 0 1px var(--border);
}
.color-custom-ring {
  position: absolute;
  inset: 0;
  border-radius: 50%;
}
.color-custom input[type="color"] {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  opacity: 0;
  cursor: pointer;
  border: none;
  padding: 0;
}
.dir-row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.dir {
  flex: 1;
  font-size: 12px;
  color: var(--text-2);
  background: rgba(255, 255, 255, 0.05);
  padding: 8px 10px;
  border-radius: 8px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.migrate-body {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.migrate-label {
  font-size: 13px;
  color: var(--text-2);
  margin: 10px 0 2px;
}
.migrate-modes {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 4px;
}
.migrate-modes label {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: 10px;
  cursor: pointer;
  font-size: 13px;
  color: var(--text-2);
  background: rgba(255, 255, 255, 0.03);
}
.migrate-modes label.active {
  border-color: var(--accent);
  background: var(--accent-soft);
}
.migrate-modes input[type="radio"] {
  accent-color: var(--accent);
  margin: 0;
}
.mm-title {
  font-weight: 600;
  color: var(--text-1);
  flex: 1;
}
.mm-desc {
  width: 100%;
  font-size: 11px;
  color: var(--text-3);
  margin-left: 22px;
}
.migrate-warn {
  font-size: 12px;
  color: #e89a4b;
  margin-top: 10px;
}
.migrate-ok {
  font-size: 13px;
  color: var(--text-2);
  margin-bottom: 4px;
}
.migrate-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 14px;
}
.mini-btn.primary {
  border-color: var(--accent);
  color: var(--accent);
}
.mini-btn.primary:hover {
  background: var(--accent-soft);
}
.about-update {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-top: 14px;
}
.update-source {
  display: flex;
  flex-direction: column;
  gap: 10px;
  margin-top: 18px;
  padding-top: 18px;
  border-top: 1px solid var(--border);
}
.update-source .seg {
  width: 100%;
  max-width: 280px;
  align-self: flex-start;
}
.update-source .seg button {
  flex: 1;
}
.about-grid {
  grid-template-columns: 1fr 1fr;
}
.about-card {
  display: flex;
  flex-direction: column;
}
.about-logo {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 4px;
}
.about-logo-img {
  width: 52px;
  height: 52px;
  border-radius: 14px;
  flex-shrink: 0;
}
.about-name {
  font-size: 18px;
  font-weight: 700;
  color: var(--text-1);
}
.about-ver {
  font-size: 13px;
  font-weight: 600;
  color: var(--accent);
  background: var(--accent-soft);
  padding: 2px 8px;
  border-radius: 6px;
}
.about-desc {
  font-size: 13px;
  color: var(--text-3);
  margin: 0 0 16px;
}
.about-features {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-bottom: 4px;
}
.feature-item {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  color: var(--text-2);
}
.feature-dot {
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--accent);
  flex-shrink: 0;
}
.about-links {
  display: flex;
  flex-direction: column;
}
.about-link {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: 9px;
  background: rgba(255, 255, 255, 0.03);
  color: var(--text-2);
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  font-family: inherit;
  margin-bottom: 8px;
  transition: all 0.15s;
}
.about-link:hover {
  border-color: var(--accent);
  background: var(--accent-soft);
  color: var(--text-1);
}
.link-arrow {
  color: var(--text-3);
  font-size: 14px;
  transition: transform 0.15s;
}
.about-link:hover .link-arrow {
  transform: translateX(3px);
  color: var(--accent);
}
.appearance-divider {
  height: 1px;
  background: var(--border);
  margin: 4px 0 14px;
}
.bg-preview {
  margin-bottom: 12px;
  border-radius: 10px;
  overflow: hidden;
  border: 1px solid var(--border);
}
.bg-preview img {
  display: block;
  width: 100%;
  height: 92px;
  object-fit: cover;
}
.bg-actions {
  display: flex;
  gap: 8px;
}
.tune-block {
  display: flex;
  flex-direction: column;
  gap: 12px;
  margin-bottom: 14px;
}
.tune-row {
  display: flex;
  align-items: center;
  gap: 12px;
  font-size: 13px;
  color: var(--text-2);
}
.tune-row label {
  flex-shrink: 0;
  width: 96px;
}
.tune-row .range {
  flex: 1;
}
.tune-val {
  flex-shrink: 0;
  width: 46px;
  text-align: right;
  font-size: 12px;
  color: var(--text-3);
  font-variant-numeric: tabular-nums;
}
.toggle {
  position: relative;
  width: 42px;
  height: 24px;
  border-radius: 999px;
  border: 1px solid var(--border);
  background: rgba(255, 255, 255, 0.08);
  cursor: pointer;
  transition: background 0.18s, border-color 0.18s;
  flex-shrink: 0;
}
.toggle .knob {
  position: absolute;
  top: 3px;
  left: 3px;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: var(--text-3);
  transition: transform 0.18s, background 0.18s;
}
.toggle.on {
  background: var(--accent);
  border-color: var(--accent);
}
.toggle.on .knob {
  transform: translateX(18px);
  background: #fff;
}
.mirror-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.mirror-item,
.mirror-custom {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.03);
  color: var(--text-2);
  cursor: pointer;
  font-family: inherit;
  font-size: 13px;
  text-align: left;
  transition: border-color 0.15s, background 0.15s;
}
.mirror-item:hover,
.mirror-custom:hover {
  border-color: var(--accent-05);
}
.mirror-item.active,
.mirror-custom.active {
  border-color: var(--accent);
  background: var(--accent-soft);
}
.mirror-main {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.mirror-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-1);
}
.mirror-base {
  font-size: 11px;
  color: var(--text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.mirror-side {
  display: flex;
  align-items: center;
  gap: 10px;
  flex-shrink: 0;
}
.mirror-ms {
  font-size: 12px;
  color: var(--accent);
  font-variant-numeric: tabular-nums;
}
.mirror-ms.bad {
  color: #e5534b;
}
.mirror-btn {
  font-size: 12px;
  color: var(--text-2);
  padding: 4px 10px;
  border-radius: 7px;
  border: 1px solid var(--border);
  flex-shrink: 0;
  transition: color 0.15s, border-color 0.15s;
}
.mirror-btn:hover {
  color: var(--accent);
  border-color: var(--accent);
}
.mirror-btn.disabled {
  opacity: 0.5;
  pointer-events: none;
}
.proxy-row {
  display: flex;
  align-items: center;
  gap: 10px;
}
.proxy-row .seg {
  flex: 1;
  min-width: 0;
}
.mirror-custom {
  flex-wrap: wrap;
}
.mirror-custom-head {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-1);
  cursor: pointer;
  flex-shrink: 0;
}
.mirror-custom-head input {
  accent-color: var(--accent);
  margin: 0;
}
.mirror-custom .text-input {
  flex: 1;
  min-width: 150px;
}
</style>
