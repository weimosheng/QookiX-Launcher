<script setup lang="ts">
import { computed, h, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { fmtMem, fmtSize, fmtTime } from "../utils/format";
import { useRouter } from "vue-router";
import { NButton, NModal, NSelect, NSlider, NTooltip, useMessage, useDialog } from "naive-ui";
import { openUrl } from "@tauri-apps/plugin-opener";
import { open } from "@tauri-apps/plugin-dialog";
import { convertFileSrc } from "@tauri-apps/api/core";
import { relaunch } from "@tauri-apps/plugin-process";
import { useMemoryInfo } from "../composables/useMemoryInfo";
import { useSettingsStore } from "../stores/settings";
import { useInstancesStore } from "../stores/instances";
import { api } from "../api";
import { useSlidingIndicator } from "../composables/useSlidingIndicator";
import { useOnboarding } from "../composables/useOnboarding";
import {
  IconCpu,
  IconDownload,
  IconExternal,
  IconFile,
  IconGithub,
  IconGlobe,
  IconHardDrive,
  IconHeart,
  IconImage,
  IconList,
  IconUsers,
  IconRefresh,
  IconSearch,
  IconShield,
  IconSliders,
  IconTrash,
  IconBookOpen,
} from "../components/icons";
import { peekUpdate, downloadUpdate, updateReady, updateReadyVersion } from "../updater";
import type { JavaInfo, MirrorPreset, StorageStats } from "../types";
import devWeimoshengUrl from "../assets/dev-weimosheng.jpg";
import devZhayiUrl from "../assets/dev-zhayi.jpg";
import AboutShowcase from "../components/AboutShowcase.vue";
import DiagnosticsDialog from "../components/DiagnosticsDialog.vue";
import CloudSyncDialog from "../components/CloudSyncDialog.vue";
import ColorPicker from "../components/ColorPicker.vue";
import { IconCloud } from "../components/icons";
import { error as devError } from "../utils/logger";
import { useI18n } from "vue-i18n";
import { SUPPORTED_LOCALES } from "../i18n";

const settings = useSettingsStore();
const instances = useInstancesStore();
const message = useMessage();
const dialog = useDialog();
const router = useRouter();
const onboarding = useOnboarding();
const { t } = useI18n();

const checking = ref(false);
const showDiag = ref(false);
const cloudOpen = ref(false);
const updateVersion = ref<string | null>(null);

async function checkUpdate() {
  if (checking.value) return;
  checking.value = true;
  updateVersion.value = null;
  try {
    const update = await peekUpdate(true);
    if (!update) {
      message.success(t("settings.update.upToDate"));
      return;
    }
    updateVersion.value = update.version;
    // 该版本已经下载好、只等重启：不必再弹一次下载确认框
    if (updateReady.value && updateReadyVersion.value === update.version) {
      message.info(t("settings.update.downloadedRestart", { version: update.version }));
      return;
    }
    let dlg: { destroy: () => void } | null = null;
    const close = () => { dlg?.destroy(); dlg = null; };
    dlg = dialog.warning({
      title: t("settings.update.newVersionFound"),
      content: t("settings.update.newVersionContent", { version: update.version }),
      action: () =>
        h("div", { style: "display:flex; gap:8px; justify-content:flex-end;" }, [
          h(NButton, { size: "small", ghost: true, onClick: close }, () => t("settings.update.later")),
          h(
            NButton,
            {
              size: "small",
              type: "primary",
              disabled: updateReady.value,
              onClick: () => { close(); void doInstall(); },
            },
            { default: () => (updateReady.value ? t("settings.update.downloadedPending") : t("settings.update.downloadAndInstall")) },
          ),
        ]),
    });
  } catch {
    // 国内镜像不可用时给出明确提示，而不是误报「已是最新版本」
    const isBucket = settings.settings?.update_source !== "github";
    message.error(
      isBucket
        ? t("settings.update.mirrorError")
        : t("settings.update.checkFailed")
    );
  } finally {
    checking.value = false;
  }
}

/** 恢复被「忽略此版本」关掉的启动更新提醒。 */
async function restoreDismissed() {
  try {
    await settings.patch({ dismissed_update_version: null });
    message.success(t("settings.update.restored"));
  } catch {
    message.error(t("settings.update.restoreFailed"));
  }
}

async function doInstall() {
  // Jump to the Download Center so the user can watch the progress live.
  router.push("/downloads");
  try {
    const downloaded = await downloadUpdate();
    if (!downloaded) return;
    // 只下载不安装：安装与重启由标题栏「重启以更新」按钮触发。
    message.success(t("settings.update.installed"));
  } catch (err) {
    const detail = err instanceof Error ? err.message : String(err);
    message.error(detail || t("settings.update.installFailed"));
    devError("[updater] install error:", err);
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
const colorPickerShow = ref(false);
function onCustomColorPick(hex: string) {
  settings.patch({ theme_color: hex });
}
const isCustomThemeColor = computed(() => {
  const cur = (settings.settings?.theme_color ?? "").toLowerCase();
  return cur !== "" && !themeColorPresets.some((c) => c.toLowerCase() === cur);
});

// 关闭行为 seg 滑动高亮（每次询问 / 最小化到后台 / 退出程序）
const closeSegRef = ref<HTMLElement | null>(null);
const closeBehaviors = ["ask", "minimize", "quit"] as const;
const { indicatorStyle: closeSegStyle, refresh: refreshCloseSeg } = useSlidingIndicator(
  closeSegRef,
  () => Array.from(closeSegRef.value?.querySelectorAll<HTMLElement>(".seg button") ?? []),
  () => Math.max(0, closeBehaviors.indexOf((settings.settings?.close_behavior ?? "ask") as (typeof closeBehaviors)[number])),
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
// 文案跟着语言切换，所以用 computed（普通数组只在 setup 时算一次，切语言不会更新）
const proxyModes = computed(() => [
  { id: "system", label: t("settings.proxyMode.system") },
  { id: "direct", label: t("settings.proxyMode.direct") },
  { id: "custom", label: t("settings.proxyMode.custom") },
]);
const { indicatorStyle: proxyModeSegStyle, refresh: refreshProxyModeSeg } = useSlidingIndicator(
  proxyModeSegRef,
  () => Array.from(proxyModeSegRef.value?.querySelectorAll<HTMLElement>(".seg button") ?? []),
  () => Math.max(0, proxyModes.value.findIndex((m) => m.id === settings.settings?.proxy_mode)),
  { axis: "horizontal" }
);
watch(() => settings.settings?.proxy_mode, () => nextTick(() => refreshProxyModeSeg()));

// 内存模式 seg 滑动高亮（自动配置 / 手动配置）
const memModeSegRef = ref<HTMLElement | null>(null);
const { indicatorStyle: memModeSegStyle, refresh: refreshMemModeSeg } = useSlidingIndicator(
  memModeSegRef,
  () => Array.from(memModeSegRef.value?.querySelectorAll<HTMLElement>(".seg button") ?? []),
  () => (settings.settings?.memory_mode === "auto" ? 0 : 1),
  { axis: "horizontal" }
);
watch(() => settings.settings?.memory_mode, () => nextTick(() => refreshMemModeSeg()));

async function selectMemoryMode(mode: string) {
  if (settings.settings?.memory_mode === mode) return;
  try {
    await settings.patch({ memory_mode: mode });
  } catch (e) {
    message.error(String(e));
  }
}

// 语言 seg 滑动高亮（简体中文 / English）
const langSegRef = ref<HTMLElement | null>(null);
const { indicatorStyle: langSegStyle, refresh: refreshLangSeg } = useSlidingIndicator(
  langSegRef,
  () => Array.from(langSegRef.value?.querySelectorAll<HTMLElement>(".seg button") ?? []),
  () => Math.max(0, SUPPORTED_LOCALES.findIndex((l) => l.value === settings.settings?.language)),
  { axis: "horizontal" }
);
watch(() => settings.settings?.language, () => nextTick(() => refreshLangSeg()));

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
    // 自定义模式必须填地址，否则后端会退化为直连而误报成功
    if (proxy_mode === "custom" && !(proxy ?? "").trim()) {
      message.warning(t("settings.proxyTest.fillAddress"));
      return;
    }
    const res = await api.testProxy(
      proxy_mode,
      proxy_mode === "custom" ? proxy : null
    );
    message.success(t("settings.proxyTest.success", { ms: res.ms }));
  } catch (e) {
    message.error(t("settings.proxyTest.failed", { error: e }));
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
/**
 * 刷新当前页的滑动指示器。
 * 面板切换有过渡动画（out-in），新面板在 enter 结束后才完成布局，
 * 因此必须在动画结束后测量，否则矩形为 0、指示器位置错乱。
 */
function refreshCurrentPaneIndicators() {
  const val = tab.value;
  if (val === "appearance") {
    refreshThemeSeg();
    refreshLangSeg();
  }
  if (val === "java") refreshMemModeSeg();
  if (val === "about") refreshUpdateSourceSeg();
  // 「下载代理」seg 位于「内容服务」页，不是「下载」页
  if (val === "content") refreshProxyModeSeg();
}
watch(tab, () => {
  nextTick(refreshCurrentPaneIndicators);
});

const tabs = computed(() => [
  { key: "general", label: t("settings.tab.general"), icon: IconSliders },
  { key: "appearance", label: t("settings.tab.appearance"), icon: IconImage },
  { key: "java", label: t("settings.tab.java"), icon: IconCpu },
  { key: "download", label: t("settings.tab.download"), icon: IconDownload },
  { key: "content", label: t("settings.tab.content"), icon: IconGlobe },
  { key: "storage", label: t("settings.tab.storage"), icon: IconHardDrive },
  { key: "about", label: t("settings.tab.about"), icon: IconFile },
]);

const searchQuery = ref("");

type SettingItem = { id: string; cardId: string; tab: string; label: string; cardTitle: string };
const settingItemsDef: { id: string; cardId: string; tab: string; labelKey: string; cardTitleKey: string }[] = [
  { id: "g-beh-close", cardId: "general-behavior", tab: "general", labelKey: "settings.general.closeWindow", cardTitleKey: "settings.general.behavior" },
  { id: "g-beh-update", cardId: "general-behavior", tab: "general", labelKey: "settings.general.autoUpdate", cardTitleKey: "settings.general.behavior" },
  { id: "g-datadir", cardId: "general-datadir", tab: "general", labelKey: "settings.general.dataDir", cardTitleKey: "settings.general.dataDir" },
  { id: "a-theme", cardId: "appearance-theme", tab: "appearance", labelKey: "settings.theme.label", cardTitleKey: "settings.theme.label" },
  { id: "a-theme-color", cardId: "appearance-theme", tab: "appearance", labelKey: "settings.appearance.themeColor", cardTitleKey: "settings.theme.label" },
  { id: "a-lang", cardId: "appearance-language", tab: "appearance", labelKey: "settings.language.label", cardTitleKey: "settings.language.label" },
  { id: "a-iface-hero", cardId: "appearance-interface", tab: "appearance", labelKey: "settings.appearance.homeHero", cardTitleKey: "settings.appearance.interface" },
  { id: "a-iface-collapse", cardId: "appearance-interface", tab: "appearance", labelKey: "settings.appearance.sidebarCollapse", cardTitleKey: "settings.appearance.interface" },
  { id: "a-iface-news", cardId: "appearance-interface", tab: "appearance", labelKey: "settings.appearance.sidebarNews", cardTitleKey: "settings.appearance.interface" },
  { id: "a-bg", cardId: "appearance-background", tab: "appearance", labelKey: "settings.appearance.background", cardTitleKey: "settings.appearance.background" },
  { id: "a-bg-blur", cardId: "appearance-background", tab: "appearance", labelKey: "settings.appearance.backgroundBlur", cardTitleKey: "settings.appearance.background" },
  { id: "a-bg-dim", cardId: "appearance-background", tab: "appearance", labelKey: "settings.appearance.backgroundDim", cardTitleKey: "settings.appearance.background" },
  { id: "a-glass", cardId: "appearance-glass", tab: "appearance", labelKey: "settings.appearance.glassStrength", cardTitleKey: "settings.appearance.glassCard" },
  { id: "j-runtime", cardId: "java-runtime", tab: "java", labelKey: "settings.java.runtime", cardTitleKey: "settings.java.runtime" },
  { id: "j-memory", cardId: "java-memory", tab: "java", labelKey: "settings.java.memory", cardTitleKey: "settings.java.memory" },
  { id: "j-jvmargs", cardId: "java-jvmargs", tab: "java", labelKey: "settings.java.jvmArgs", cardTitleKey: "settings.java.jvmArgs" },
  { id: "j-gameargs", cardId: "java-gameargs", tab: "java", labelKey: "settings.java.gameArgs", cardTitleKey: "settings.java.gameArgs" },
  { id: "d-threads", cardId: "download-parallel", tab: "download", labelKey: "settings.download.threadsLabel", cardTitleKey: "settings.download.parallel" },
  { id: "d-chunk", cardId: "download-parallel", tab: "download", labelKey: "settings.download.chunkThreadsLabel", cardTitleKey: "settings.download.parallel" },
  { id: "d-mirror", cardId: "download-mirror", tab: "download", labelKey: "settings.download.mirror", cardTitleKey: "settings.download.mirror" },
  { id: "d-center", cardId: "download-center", tab: "download", labelKey: "settings.download.center", cardTitleKey: "settings.download.center" },
  { id: "c-curseforge", cardId: "content-curseforge", tab: "content", labelKey: "settings.content.curseforgeKey", cardTitleKey: "settings.content.curseforgeKey" },
  { id: "c-proxy", cardId: "content-proxy", tab: "content", labelKey: "settings.content.proxy", cardTitleKey: "settings.content.proxy" },
  { id: "c-translate", cardId: "content-translate", tab: "content", labelKey: "settings.content.translate", cardTitleKey: "settings.content.translate" },
  { id: "c-autobody", cardId: "content-translate", tab: "content", labelKey: "settings.content.autoBody", cardTitleKey: "settings.content.translate" },
  { id: "s-cloud", cardId: "storage-cloud", tab: "storage", labelKey: "settings.storage.cloudSave", cardTitleKey: "settings.storage.cloudSave" },
  { id: "s-stats", cardId: "storage-stats", tab: "storage", labelKey: "settings.storage.stats", cardTitleKey: "settings.storage.stats" },
  { id: "ab-overview", cardId: "about-showcase", tab: "about", labelKey: "settings.about.overview", cardTitleKey: "settings.about.overview" },
  { id: "ab-devs", cardId: "about-devs", tab: "about", labelKey: "settings.about.developers", cardTitleKey: "settings.about.developers" },
  { id: "ab-diag", cardId: "about-update", tab: "about", labelKey: "settings.about.diagReport", cardTitleKey: "settings.about.updateCard" },
  { id: "ab-update", cardId: "about-update", tab: "about", labelKey: "settings.about.checkUpdate", cardTitleKey: "settings.about.updateCard" },
  { id: "ab-upsrc", cardId: "about-update", tab: "about", labelKey: "settings.about.updateSource", cardTitleKey: "settings.about.updateCard" },
  { id: "ab-license", cardId: "about-license", tab: "about", labelKey: "settings.about.license", cardTitleKey: "settings.about.license" },
  { id: "ab-onboarding", cardId: "about-onboarding", tab: "about", labelKey: "settings.about.onboarding", cardTitleKey: "settings.about.onboarding" },
  { id: "ab-deps", cardId: "about-deps", tab: "about", labelKey: "settings.about.depsTitle", cardTitleKey: "settings.about.depsTitle" },
];

const allSettingItems = computed<SettingItem[]>(() =>
  settingItemsDef.map((d) => ({
    id: d.id,
    cardId: d.cardId,
    tab: d.tab,
    label: t(d.labelKey),
    cardTitle: t(d.cardTitleKey),
  })),
);

const settingGroups = computed(() => {
  const q = searchQuery.value.trim().toLowerCase();
  return tabs.value.map((tb) => {
    const all = allSettingItems.value.filter((it) => it.tab === tb.key);
    const items = q
      ? all.filter(
          (it) =>
            it.label.toLowerCase().includes(q) ||
            it.cardTitle.toLowerCase().includes(q),
        )
      : all;
    return { key: tb.key, label: tb.label, icon: tb.icon, items };
  });
});

function highlightSegments(text: string, q: string): { text: string; hit: boolean }[] {
  if (!q) return [{ text, hit: false }];
  const idx = text.toLowerCase().indexOf(q.toLowerCase());
  if (idx < 0) return [{ text, hit: false }];
  return [
    { text: text.slice(0, idx), hit: false },
    { text: text.slice(idx, idx + q.length), hit: true },
    { text: text.slice(idx + q.length), hit: false },
  ].filter((s) => s.text.length > 0);
}

const hasResults = computed(() => settingGroups.value.some((g) => g.items.length));

const activeItemId = ref("");
const pendingScrollId = ref("");
const skipPaneTransition = ref(false);

function scrollToSetting(id: string) {
  const el = document.getElementById(id);
  if (!el) return;
  const scroller = document.getElementById("app-content");
  if (scroller) {
    const sr = scroller.getBoundingClientRect();
    const tr = el.getBoundingClientRect();
    const offset =
      tr.top - sr.top + scroller.scrollTop - (scroller.clientHeight - tr.height) / 2;
    scroller.scrollTo({ top: Math.max(0, offset), behavior: "smooth" });
  } else {
    el.scrollIntoView({ behavior: "smooth", block: "center" });
  }
  el.classList.add("setting-flash");
  setTimeout(() => el.classList.remove("setting-flash"), 1200);
}
function selectSetting(item: SettingItem) {
  activeItemId.value = item.id;
  if (tab.value !== item.tab) {
    skipPaneTransition.value = true;
    tab.value = item.tab;
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        skipPaneTransition.value = false;
        refreshCurrentPaneIndicators();
        scrollToSetting(item.cardId);
      });
    });
  } else {
    scrollToSetting(item.cardId);
  }
}
function onPaneAfterEnter() {
  refreshCurrentPaneIndicators();
  if (pendingScrollId.value) {
    const id = pendingScrollId.value;
    pendingScrollId.value = "";
    nextTick(() => scrollToSetting(id));
  }
}

const aboutDeps: Record<
  "frontend" | "rust" | "thirdparty",
  { name: string; version: string; license: string; url: string; licenseUrl: string }[]
> = {
  frontend: [
    { name: "Vue", version: "3.5", license: "MIT", url: "https://vuejs.org", licenseUrl: "https://github.com/vuejs/core/blob/main/LICENSE" },
    { name: "Vue Router", version: "4.4", license: "MIT", url: "https://router.vuejs.org", licenseUrl: "https://github.com/vuejs/router/blob/main/LICENSE" },
    { name: "Naive UI", version: "2.45", license: "MIT", url: "https://www.naiveui.com", licenseUrl: "https://github.com/tusen-design/naive-ui/blob/main/LICENSE" },
    { name: "Pinia", version: "2.2", license: "MIT", url: "https://pinia.vuejs.org", licenseUrl: "https://github.com/vuejs/pinia/blob/v2/LICENSE" },
    { name: "Tauri API", version: "2", license: "MIT/Apache-2.0", url: "https://tauri.app", licenseUrl: "https://github.com/tauri-apps/tauri/blob/dev/LICENSE" },
    { name: "driver.js", version: "1.8", license: "MIT", url: "https://driverjs.com", licenseUrl: "https://github.com/driverjs/driver.js/blob/main/LICENSE" },
    { name: "marked", version: "18.0", license: "MIT", url: "https://marked.js.org", licenseUrl: "https://github.com/markedjs/marked/blob/master/LICENSE.md" },
    { name: "DOMPurify", version: "3.4", license: "Apache-2.0/MPL-2.0", url: "https://github.com/cure53/DOMPurify", licenseUrl: "https://github.com/cure53/DOMPurify/blob/main/LICENSE" },
    { name: "skinview3d", version: "3.4", license: "MIT", url: "https://github.com/bs-community/skinview3d", licenseUrl: "https://github.com/bs-community/skinview3d/blob/master/LICENSE" },
  ],
  rust: [
    { name: "Tauri", version: "2", license: "MIT/Apache-2.0", url: "https://tauri.app", licenseUrl: "https://github.com/tauri-apps/tauri/blob/dev/LICENSE" },
    { name: "Tokio", version: "1", license: "MIT", url: "https://tokio.rs", licenseUrl: "https://github.com/tokio-rs/tokio/blob/master/LICENSE" },
    { name: "reqwest", version: "0.12", license: "MIT/Apache-2.0", url: "https://github.com/seanmonstar/reqwest", licenseUrl: "https://github.com/seanmonstar/reqwest/blob/master/LICENSE" },
    { name: "serde", version: "1", license: "MIT/Apache-2.0", url: "https://serde.rs", licenseUrl: "https://github.com/serde-rs/serde/blob/master/LICENSE" },
    { name: "fastnbt", version: "2", license: "MIT", url: "https://github.com/owengage/fastnbt", licenseUrl: "https://github.com/owengage/fastnbt/blob/main/LICENSE" },
    { name: "cc", version: "1.0", license: "MIT/Apache-2.0", url: "https://github.com/rust-lang/cc-rs", licenseUrl: "https://github.com/rust-lang/cc-rs/blob/main/LICENSE" },
  ],
  thirdparty: [
    { name: "Terracotta", version: "", license: "AGPL-3.0-or-later", url: "https://github.com/burningtnt/Terracotta", licenseUrl: "https://github.com/burningtnt/Terracotta/blob/master/LICENSE" },
    { name: "Feather Icons", version: "", license: "MIT", url: "https://feathericons.com", licenseUrl: "https://github.com/feathericons/feather/blob/main/LICENSE" },
    { name: "cubiomes", version: "", license: "MIT", url: "https://github.com/Cubitect/cubiomes", licenseUrl: "https://github.com/Cubitect/cubiomes/blob/master/LICENSE" },
  ],
};

// ---- 内容翻译 ----
const translateOptions = computed(() => [
  { label: t("settings.content.translateDefault"), value: "default" },
  { label: t("settings.content.translateCustom"), value: "custom" },
  { label: t("settings.content.translateBaidu"), value: "baidu_web" },
]);
const testingTranslate = ref(false);
async function testTranslate() {
  const s = settings.settings;
  if (!s) return;
  if (!s.translate_api_base || !s.translate_api_key || !s.translate_api_model) {
    message.warning(t("settings.content.translateMissingFields"));
    return;
  }
  testingTranslate.value = true;
  try {
    await api.testTranslateApi(s.translate_api_base, s.translate_api_key, s.translate_api_model);
    message.success(t("settings.content.translateOk"));
  } catch (e) {
    message.error(String(e));
  } finally {
    testingTranslate.value = false;
  }
}

// ---- 内容翻译缓存 ----
const clearingCacheService = ref<"default" | "custom" | null>(null);
async function clearTranslations(service: "default" | "custom") {
  if (clearingCacheService.value) return;
  clearingCacheService.value = service;
  const label = service === "custom" ? t("settings.content.customApiLabel") : t("settings.content.builtinLabel");
  try {
    const freed = await api.clearTranslationCache(service);
    message.success(
      freed > 0 ? t("settings.content.cacheClearedFreed", { label, size: fmtSize(freed) }) : t("settings.content.cacheCleared", { label })
    );
  } catch (e) {
    message.error(String(e));
  } finally {
    clearingCacheService.value = null;
  }
}

const { memTotal, memUsed, memAvailable, startPolling, stopPolling } = useMemoryInfo();

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

async function openPath() {
  try {
    await api.revealDataDir();
  } catch (e) {
    message.error(t("settings.general.openFailed") + String(e));
  }
}

async function pickBackground() {
  try {
    const picked = await open({
      multiple: false,
      filters: [{ name: t("settings.imageFilter"), extensions: ["png", "jpg", "jpeg", "gif", "webp", "bmp"] }],
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
    const dir = await open({ directory: true, title: t("settings.migrate.pickTitle") });
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
    message.error(t("settings.migrate.relaunchFailed", { error: String(e) }));
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
    message.error(t("settings.storage.loadFailed", { error: String(e) }));
  } finally {
    loadingStats.value = false;
  }
}

async function refreshStats() {
  loadingStats.value = true;
  try {
    stats.value = await api.refreshStorageStats();
    message.success(t("settings.storage.refreshed"));
  } catch (e) {
    message.error(t("settings.storage.refreshFailed", { error: String(e) }));
  } finally {
    loadingStats.value = false;
  }
}

function confirmClear() {
  dialog.warning({
    title: t("settings.storage.clearTitle"),
    content: t("settings.storage.clearContent"),
    positiveText: t("settings.storage.clearCache"),
    negativeText: t("common.cancel"),
    onPositiveClick: async () => {
      clearing.value = true;
      try {
        const res = await api.clearCache();
        message.success(t("settings.storage.clearSuccess", { size: fmtSize(res.freed) }));
        await refreshStats();
      } catch (e) {
        message.error(t("settings.storage.clearFailed", { error: String(e) }));
      } finally {
        clearing.value = false;
      }
    },
  });
}

onMounted(() => {
  settings.load();
  loadMirrors();
  // cached scan: no full rescan if another view already fetched recently
  settings.loadJava().then((c) => (javaCandidates.value = c));
  startPolling();
  loadStats();
});
onUnmounted(() => {
  stopPolling();
  if (saveTimer) clearTimeout(saveTimer);
});
</script>

<template>
  <div v-if="settings.settings" class="settings-view">
    <div class="settings-side">
      <div class="nav-search-card glass">
        <IconSearch class="nav-search-icon" />
        <input
          v-model="searchQuery"
          type="text"
          class="nav-search-input"
          :placeholder="t('settings.search.placeholder')"
        />
      </div>
      <aside id="settings-nav" class="settings-nav">
        <nav class="nav-list">
          <template v-if="searchQuery.trim()">
            <div v-if="!hasResults" class="nav-empty">{{ t('settings.search.noResult') }}</div>
            <template v-for="g in settingGroups" :key="g.key">
              <div v-if="g.items.length" class="nav-group">
                <div class="nav-group-head">
                  <component :is="g.icon" class="nav-group-icon" />
                  <span>{{ g.label }}</span>
                </div>
                <button
                  v-for="item in g.items"
                  :key="item.id"
                  class="nav-result"
                  :class="{ active: activeItemId === item.id }"
                  @click="selectSetting(item)"
                >
                  <span class="nav-result-title">
                    <template v-for="(seg, i) in highlightSegments(item.label, searchQuery.trim())" :key="i">
                      <mark v-if="seg.hit" class="hl">{{ seg.text }}</mark>
                      <template v-else>{{ seg.text }}</template>
                    </template>
                  </span>
                  <span class="nav-result-sub">
                    <template v-for="(seg, i) in highlightSegments(item.cardTitle, searchQuery.trim())" :key="i">
                      <mark v-if="seg.hit" class="hl">{{ seg.text }}</mark>
                      <template v-else>{{ seg.text }}</template>
                    </template>
                  </span>
                </button>
              </div>
            </template>
          </template>
          <template v-else>
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
          </template>
        </nav>
      </aside>
    </div>

    <Transition :name="skipPaneTransition ? '' : 'settings-pane'" mode="out-in" @after-enter="onPaneAfterEnter">
    <div :key="tab" class="settings-body">
      <!-- 常规 -->
      <div v-show="tab === 'general'" class="settings-pane">
        <div class="grid">
          <div class="card glass" id="general-behavior">
            <h3>{{ t("settings.general.behavior") }}</h3>
            <div class="choice-row">
              <span>{{ t("settings.general.closeWindow") }}</span>
              <div ref="closeSegRef" class="seg">
                <div class="indicator" :style="closeSegStyle"></div>
                <button
                  :class="{ active: settings.settings.close_behavior === 'ask' }"
                  @click="settings.patch({ close_behavior: 'ask' })"
                >
                  {{ t("settings.general.closeAsk") }}
                </button>
                <button
                  :class="{ active: settings.settings.close_behavior === 'minimize' }"
                  @click="settings.patch({ close_behavior: 'minimize' })"
                >
                  {{ t("settings.general.closeMinimize") }}
                </button>
                <button
                  :class="{ active: settings.settings.close_behavior === 'quit' }"
                  @click="settings.patch({ close_behavior: 'quit' })"
                >
                  {{ t("settings.general.closeQuit") }}
                </button>
              </div>
            </div>
            <div class="choice-row">
              <div class="choice-info">
                <span class="choice-label">{{ t("settings.general.autoUpdate") }}</span>
                <p class="choice-hint">{{ t("settings.general.autoUpdateHint") }}</p>
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

          <div class="card glass" id="general-datadir">
            <h3>{{ t("settings.general.dataDir") }}</h3>
            <div class="dir-row">
              <code class="mono dir">{{ settings.settings.data_dir }}</code>
              <button class="mini-btn" @click="openPath()">{{ t("settings.general.open") }}</button>
              <button class="mini-btn" @click="pickDataDir">{{ t("settings.general.change") }}</button>
            </div>
            <p class="hint">{{ t("settings.general.dataDirHint") }}</p>
          </div>
        </div>
      </div>

      <!-- 外观 -->
      <div v-show="tab === 'appearance'" class="settings-pane">
        <div class="card glass" id="appearance-theme">
          <h3>{{ t("settings.theme.label") }}</h3>
          <div class="choice-row">
            <span>{{ t("settings.theme.label") }}</span>
            <div ref="themeSegRef" class="seg">
              <div class="indicator" :style="themeSegStyle"></div>
              <button
                :class="{ active: settings.settings.theme === 'dark' }"
                @click="settings.patch({ theme: 'dark' })"
              >
                {{ t("settings.theme.dark") }}
              </button>
              <button
                :class="{ active: settings.settings.theme === 'light' }"
                @click="settings.patch({ theme: 'light' })"
              >
                {{ t("settings.theme.light") }}
              </button>
            </div>
          </div>
          <div class="appearance-divider"></div>
          <div class="choice-row">
            <span>{{ t("settings.appearance.themeColor") }}</span>
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
              <button
                type="button"
                class="color-custom"
                :class="{ active: isCustomThemeColor }"
                :title="t('settings.appearance.customColor')"
                @click="colorPickerShow = true"
              >
                <span class="color-custom-ring" :style="{ background: settings.settings.theme_color }"></span>
              </button>
              <ColorPicker
                :show="colorPickerShow"
                :color="settings.settings.theme_color"
                :presets="themeColorPresets"
                @update:show="colorPickerShow = $event"
                @update:color="onCustomColorPick"
              />
            </div>
          </div>
        </div>
        <div class="card glass" id="appearance-language">
          <h3>{{ t("settings.language.label") }}</h3>
          <div class="choice-row">
            <div class="choice-info">
              <span class="choice-label">{{ t("settings.language.label") }}</span>
              <p class="choice-hint">{{ t("settings.language.desc") }}</p>
            </div>
            <div ref="langSegRef" class="seg lang-seg">
              <div class="indicator" :style="langSegStyle"></div>
              <button
                v-for="l in SUPPORTED_LOCALES"
                :key="l.value"
                :class="{ active: settings.settings.language === l.value }"
                @click="settings.patch({ language: l.value })"
              >
                {{ l.label }}
              </button>
            </div>
          </div>
        </div>
        <div class="card glass" id="appearance-interface">
          <h3>{{ t("settings.appearance.interface") }}</h3>
          <div class="choice-row">
            <div class="choice-info">
              <span class="choice-label">{{ t("settings.appearance.homeHero") }}</span>
              <p class="choice-hint">{{ t("settings.appearance.homeHeroHint") }}</p>
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
              <span class="choice-label">{{ t("settings.appearance.sidebarCollapse") }}</span>
              <p class="choice-hint">{{ t("settings.appearance.sidebarCollapseHint") }}</p>
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
          <div class="choice-row">
            <div class="choice-info">
              <span class="choice-label">{{ t("settings.appearance.sidebarNews") }}</span>
              <p class="choice-hint">{{ t("settings.appearance.sidebarNewsHint") }}</p>
            </div>
            <button
              class="toggle"
              :class="{ on: settings.settings.show_news ?? true }"
              role="switch"
              :aria-checked="settings.settings.show_news ?? true"
              @click="settings.patch({ show_news: !(settings.settings.show_news ?? true) })"
            >
              <span class="knob"></span>
            </button>
          </div>
        </div>
        <div class="card glass" id="appearance-background">
          <h3>{{ t("settings.appearance.background") }}</h3>
          <div v-if="settings.settings.background_image" class="bg-preview">
            <img :src="bgPreviewUrl" :alt="t('settings.appearance.backgroundPreview')" />
          </div>
          <div class="choice-row">
            <span>{{ t("settings.appearance.background") }}</span>
            <div class="bg-actions">
              <button class="mini-btn" @click="pickBackground">{{ t("settings.appearance.selectImage") }}</button>
              <button
                v-if="settings.settings.background_image"
                class="mini-btn"
                @click="settings.patch({ background_image: null })"
              >
                {{ t("settings.appearance.clear") }}
              </button>
            </div>
          </div>
          <div v-if="settings.settings.background_image" class="tune-block">
            <div class="tune-row">
              <label>{{ t("settings.appearance.backgroundBlur") }}</label>
              <NSlider
                v-model:value="settings.settings.background_blur"
                class="tune-slider"
                :min="0"
                :max="50"
                :step="1"
                :tooltip="false"
              />
              <span class="tune-val">{{ settings.settings.background_blur }} px</span>
            </div>
            <div class="tune-row">
              <label>{{ t("settings.appearance.backgroundDim") }}</label>
              <NSlider
                v-model:value="settings.settings.background_dim"
                class="tune-slider"
                :min="0"
                :max="100"
                :step="5"
                :tooltip="false"
              />
              <span class="tune-val">{{ settings.settings.background_dim }}%</span>
            </div>
          </div>
        </div>
        <div class="card glass" id="appearance-glass">
          <h3>{{ t("settings.appearance.glassCard") }}</h3>
          <div class="tune-row">
            <label>{{ t("settings.appearance.glassStrength") }}</label>
            <NSlider
              v-model:value="settings.settings.glass_blur"
              class="tune-slider"
              :min="0"
              :max="30"
              :step="1"
              :tooltip="false"
            />
            <span class="tune-val">{{ settings.settings.glass_blur }} px</span>
          </div>
          <p class="hint">{{ t("settings.appearance.glassHint") }}</p>
        </div>
      </div>

      <!-- Java -->
      <div v-show="tab === 'java'" class="settings-pane">
        <div class="grid">
          <div class="card glass" id="java-runtime">
            <h3><IconCpu /> {{ t("settings.java.runtime") }}</h3>
            <div class="java-toolbar">
              <button class="mini-btn" :disabled="detecting" @click="detect">
                <IconSearch /> {{ detecting ? t("settings.java.detecting") : t("settings.java.detect") }}
              </button>
              <span class="hint-inline">{{ t("settings.java.detectHint") }}</span>
            </div>
            <div v-if="javaCandidates.length" class="java-list">
              <div v-for="j in javaCandidates" :key="j.path" class="java-item">
                <span class="java-name">Java {{ j.major }} ({{ j.version }})</span>
                <span class="java-path">{{ j.path }}</span>
              </div>
            </div>
            <p v-else-if="!detecting" class="hint">{{ t("settings.java.notFound") }}</p>
            <p class="hint">{{ t("settings.java.perInstance") }}</p>
          </div>

          <div class="card glass" id="java-memory">
            <h3>{{ t("settings.java.memory") }}</h3>
            <div class="mem-mode-row">
              <div ref="memModeSegRef" class="seg">
                <div class="indicator" :style="memModeSegStyle"></div>
                <button
                  :class="{ active: settings.settings.memory_mode === 'auto' }"
                  @click="selectMemoryMode('auto')"
                >
                  {{ t("settings.java.memAuto") }}
                </button>
                <button
                  :class="{ active: settings.settings.memory_mode !== 'auto' }"
                  @click="selectMemoryMode('custom')"
                >
                  {{ t("settings.java.memManual") }}
                </button>
              </div>
            </div>
            <div v-if="settings.settings.memory_mode !== 'auto'" class="mem-row">
              <div>
                <label>{{ t("settings.java.maxMemory") }}</label>
                <NSlider
                  v-model:value="settings.settings.max_memory_mb"
                  :min="1024"
                  :max="16384"
                  :step="256"
                  :tooltip="false"
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
                <span><i class="dot used"></i>{{ t("settings.java.memUsed", { used: fmtMem(memUsed), percent: usedPercent }) }}</span>
                <span><i class="dot alloc"></i>{{ t("settings.java.memAlloc", { alloc: fmtMem(effectiveMemory), percent: allocPercent }) }}</span>
                <span><i class="dot total"></i>{{ t("settings.java.memTotal", { total: fmtMem(memTotal), available: fmtMem(memAvailable) }) }}</span>
              </div>
            </div>
          </div>

          <div class="card glass" id="java-jvmargs">
            <h3>{{ t("settings.java.jvmArgs") }}</h3>
            <textarea
              v-model="settings.settings.jvm_args"
              class="text-input mono"
              rows="3"
              :placeholder="t('settings.java.jvmArgsPlaceholder')"
            />
          </div>

          <div class="card glass" id="java-gameargs">
            <h3>{{ t("settings.java.gameArgs") }}</h3>
            <input
              v-model="settings.settings.game_args"
              class="text-input mono"
              :placeholder="t('settings.java.gameArgsPlaceholder')"
            />
          </div>
        </div>
      </div>

      <!-- 下载 -->
      <div v-show="tab === 'download'" class="settings-pane">
        <div class="grid">
          <div class="card glass" id="download-parallel">
            <h3>{{ t("settings.download.parallel") }}</h3>
            <div class="row-label">
              <span class="row-label-text">{{ t("settings.download.threads", { count: settings.settings.download_threads }) }}</span>
              <NSlider
                v-model:value="settings.settings.download_threads"
                :min="1"
                :max="32"
                :step="1"
                :tooltip="false"
              />
            </div>
            <p class="hint">{{ t("settings.download.threadsHint") }}</p>
            <div class="row-label" style="margin-top: 16px;">
              <span class="row-label-text">{{ t("settings.download.chunkThreads", { count: settings.settings.download_chunk_threads }) }}</span>
              <NSlider
                v-model:value="settings.settings.download_chunk_threads"
                :min="1"
                :max="16"
                :step="1"
                :tooltip="false"
              />
            </div>
            <p class="hint">{{ t("settings.download.chunkThreadsHint") }}</p>
          </div>
          <div class="card glass" id="download-mirror">
            <h3><IconGlobe /> {{ t("settings.download.mirror") }}</h3>
            <div class="mirror-list">
              <button
                v-for="m in mirrors"
                :key="m.id"
                type="button"
                class="mirror-item"
                :class="{ active: settings.settings.mirror === m.id }"
                @click="selectMirror(m.id)"
              >
                <span class="mirror-main">
                  <span class="mirror-name">{{ m.label }}</span>
                  <span class="mirror-base">{{ m.base || t("settings.download.mirrorDirect") }}</span>
                </span>
                <span class="mirror-side">
                  <span
                    v-if="mirrorLatency[m.id] !== undefined"
                    class="mirror-ms"
                    :class="{ bad: mirrorLatency[m.id] === null }"
                  >
                    {{ mirrorLatency[m.id] === null ? t("settings.download.mirrorUnavailable") : `${mirrorLatency[m.id]} ms` }}
                  </span>
                  <span
                    class="mirror-btn"
                    :class="{ disabled: testingMirror === m.id }"
                    @click.stop="testMirror(m.id, m.base)"
                  >
                    {{ testingMirror === m.id ? t("settings.download.testing") : t("settings.download.test") }}
                  </span>
                </span>
              </button>
              <div
                class="mirror-custom"
                :class="{ active: settings.settings.mirror === 'custom' }"
                role="button"
                tabindex="0"
                @click="selectMirror('custom')"
                @keydown.enter.prevent="selectMirror('custom')"
                @keydown.space.prevent="selectMirror('custom')"
              >
                <span class="mirror-custom-head">{{ t("settings.download.customMirror") }}</span>
                <input
                  v-model="settings.settings.mirror_custom"
                  class="text-input mono"
                  placeholder="https://your-mirror.example.com"
                  @input="onCustomMirrorInput"
                />
                <span
                  class="mirror-btn"
                  :class="{ disabled: testingMirror === 'custom' || !settings.settings.mirror_custom }"
                  @click="testMirror('custom', settings.settings.mirror_custom)"
                >
                  {{ testingMirror === 'custom' ? t("settings.download.testing") : t("settings.download.test") }}
                </span>
              </div>
            </div>
            <p class="hint">
              {{ t("settings.download.mirrorHintLead") }}
              <b>{{ t("settings.download.mirrorHintBold") }}</b>{{ t("settings.download.mirrorHintTail") }}
            </p>
          </div>
          <div class="card glass" id="download-center">
            <h3>{{ t("settings.download.center") }}</h3>
            <p class="hint">{{ t("settings.download.centerHint") }}</p>
          </div>
        </div>
      </div>

      <!-- 内容服务 -->
      <div v-show="tab === 'content'" class="settings-pane">
        <div class="grid">
          <div class="card glass" id="content-curseforge">
            <h3>{{ t("settings.content.curseforgeKey") }}</h3>
            <input
              v-model="settings.settings.curseforge_api_key"
              class="text-input mono"
              :placeholder="t('settings.content.curseforgePlaceholder')"
            />
            <p class="hint">{{ t("settings.content.curseforgeHint") }}</p>
          </div>
          <div class="card glass" id="content-proxy">
            <h3>{{ t("settings.content.proxy") }}</h3>
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
                class="mirror-btn proxy-test-btn"
                :class="{ disabled: testingProxy }"
                @click="testProxy"
              >
                {{ testingProxy ? t("settings.download.testing") : t("settings.content.proxyTest") }}
              </button>
            </div>
            <input
              v-if="settings.settings.proxy_mode === 'custom'"
              v-model="settings.settings.proxy"
              class="text-input mono"
              :placeholder="t('settings.content.proxyPlaceholder')"
              @input="onCustomProxyInput"
            />
            <p class="hint">
              {{
                settings.settings.proxy_mode === "system"
                  ? t("settings.content.proxySystem")
                  : settings.settings.proxy_mode === "direct"
                    ? t("settings.content.proxyDirect")
                    : t("settings.content.proxyCustom")
              }}
            </p>
          </div>
          <div class="card glass" id="content-translate">
            <h3>{{ t("settings.content.translate") }}</h3>
            <n-select
              v-model:value="settings.settings.translate_provider"
              :options="translateOptions"
              size="small"
              class="tb-select"
              @update:value="settings.save()"
            />
            <template v-if="settings.settings.translate_provider === 'custom'">
              <input
                v-model="settings.settings.translate_api_base"
                class="text-input mono"
                style="margin-top: 10px"
                :placeholder="t('settings.content.translateApiPlaceholder')"
                @change="settings.save()"
              />
              <input
                v-model="settings.settings.translate_api_key"
                class="text-input mono"
                type="password"
                style="margin-top: 10px"
                :placeholder="t('settings.content.translateKeyPlaceholder')"
                @change="settings.save()"
              />
              <div class="proxy-row" style="margin-top: 10px">
                <input
                  v-model="settings.settings.translate_api_model"
                  class="text-input mono"
                  :placeholder="t('settings.content.translateModelPlaceholder')"
                  @change="settings.save()"
                />
                <button
                  class="mirror-btn proxy-test-btn"
                  :class="{ disabled: testingTranslate }"
                  @click="testTranslate"
                >
                  {{ testingTranslate ? t("settings.download.testing") : t("settings.content.proxyTest") }}
                </button>
              </div>
              <p class="hint">{{ t("settings.content.translateCustomHint") }}</p>
            </template>
            <p v-else-if="settings.settings.translate_provider === 'baidu_web'" class="hint">
              {{ t("settings.content.translateBaiduHint") }}
            </p>
            <p v-else class="hint">{{ t("settings.content.translateDefaultHint") }}</p>
            <div class="proxy-row" style="margin-top: 12px">
              <button
                class="mirror-btn proxy-test-btn"
                :class="{ disabled: clearingCacheService !== null }"
                @click="clearTranslations('default')"
              >
                {{ clearingCacheService === "default" ? t("settings.content.clearing") : t("settings.content.clearDefaultCache") }}
              </button>
              <button
                class="mirror-btn proxy-test-btn"
                :class="{ disabled: clearingCacheService !== null }"
                @click="clearTranslations('custom')"
              >
                {{ clearingCacheService === "custom" ? t("settings.content.clearing") : t("settings.content.clearCustomCache") }}
              </button>
            </div>
            <p class="hint">{{ t("settings.content.cacheHint") }}</p>
            <div class="switch-row">
              <span>{{ t("settings.content.autoBody") }}</span>
              <button
                class="toggle"
                :class="{ on: settings.settings.body_translate_auto }"
                role="switch"
                :aria-checked="settings.settings.body_translate_auto"
                @click="settings.patch({ body_translate_auto: !settings.settings.body_translate_auto })"
              >
                <span class="knob"></span>
              </button>
            </div>
            <p class="hint">
              {{ t("settings.content.autoBodyHint") }}
            </p>
          </div>
        </div>
      </div>

      <!-- 存储 -->
      <div v-show="tab === 'storage'" class="settings-pane">
        <div class="grid storage-grid">
          <div class="card glass storage-card" id="storage-cloud">
            <div class="storage-header">
              <h3><IconCloud /> {{ t("settings.storage.cloudSave") }}</h3>
              <div class="storage-actions">
                <span class="hint-inline">{{ t("settings.storage.cloudHint") }}</span>
                <button class="mini-btn" @click="cloudOpen = true">
                  <IconCloud class="btn-icon" />
                  {{ t("settings.storage.browseCloud") }}
                </button>
              </div>
            </div>
          </div>

          <div class="card glass storage-card" id="storage-stats">
            <div class="storage-header">
              <h3>{{ t("settings.storage.stats") }}</h3>
              <div class="storage-actions">
                <span class="hint-inline">
                  <template v-if="stats">{{ t("settings.storage.updatedAt", { label: stats.cached ? t("settings.storage.lastUpdate") : t("settings.storage.updated"), time: fmtTime(stats.updated_at) }) }}</template>
                  <template v-else>{{ t("settings.storage.notScanned") }}</template>
                </span>
                <button class="mini-btn" :disabled="loadingStats" @click="refreshStats">
                  <IconRefresh class="btn-icon" />
                  {{ loadingStats ? t("settings.storage.scanning") : t("settings.storage.refresh") }}
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
                  <span class="donut-label">{{ t("settings.storage.totalUsed") }}</span>
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
                {{ t("settings.storage.perInstance") }}
                <span class="hint-inline">{{ t("settings.storage.count", { count: stats.instances.length }) }}</span>
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
                {{ t("settings.storage.perServer") }}
                <span class="hint-inline">{{ t("settings.storage.count", { count: stats.servers.length }) }}</span>
              </h4>
              <ul class="instance-storage-list">
                <li v-for="srv in stats.servers" :key="srv.id">
                  <span class="instance-name" :title="srv.name">{{ srv.name }}</span>
                  <span class="legend-size">{{ fmtSize(srv.size) }}</span>
                  <span class="legend-pct">{{ pct(srv.size) }}%</span>
                </li>
              </ul>
            </div>
            <p v-else-if="!stats?.instances.length" class="hint">{{ stats ? t("settings.storage.noData") : t("settings.storage.loading") }}</p>

            <div class="storage-footer">
              <button class="mini-btn danger" :disabled="clearing" @click="confirmClear">
                <IconTrash class="btn-icon" />
                {{ clearing ? t("settings.storage.clearing") : t("settings.storage.clearCache") }}
              </button>
              <span class="hint">{{ t("settings.storage.clearHint") }}</span>
            </div>
          </div>
        </div>
      </div>

      <!-- 关于 -->
      <div v-show="tab === 'about'" class="settings-pane">
        <div class="card glass about-showcase" id="about-showcase">
          <AboutShowcase />
          <div class="about-hero-title">
            <span class="about-name about-hero-name">QookiX Launcher</span>
            <span class="about-ver">v0.9.0</span>
          </div>
          <p class="about-hero-slogan">{{ t("settings.about.slogan") }}</p>
        </div>
        <div class="grid about-grid">
          <div class="card glass about-card" id="about-devs">
            <div class="about-devs-title">{{ t("settings.about.developers") }}</div>
            <div class="dev-list">
              <div class="dev-line">
                <img class="dev-avatar" :src="devWeimoshengUrl" alt="维墨笙" />
                <div class="dev-meta">
                  <div class="dev-head">
                    <span class="dev-name">维墨笙</span>
                    <n-tooltip trigger="hover" placement="top">
                      <template #trigger>
                        <button class="dev-github-btn" @click="openUrl('https://github.com/weimosheng')">
                          <IconGithub />
                        </button>
                      </template>
                      {{ t("settings.about.githubHome") }}
                    </n-tooltip>
                  </div>
                  <span class="dev-role">{{ t("settings.about.devRole") }}</span>
                </div>
              </div>
              <div class="dev-line">
                <img class="dev-avatar" :src="devZhayiUrl" alt="ZhaYi" />
                <div class="dev-meta">
                  <div class="dev-head">
                    <span class="dev-name">ZhaYi</span>
                    <n-tooltip trigger="hover" placement="top">
                      <template #trigger>
                        <button class="dev-github-btn" @click="openUrl('https://github.com/ZhaYi-Miao')">
                          <IconGithub />
                        </button>
                      </template>
                      {{ t("settings.about.githubHome") }}
                    </n-tooltip>
                  </div>
                  <span class="dev-role">{{ t("settings.about.collaborator") }}</span>
                </div>
              </div>
            </div>
          </div>
          <div class="card glass about-update-card" id="about-update">
            <div class="about-update">
              <button class="mini-btn" @click="showDiag = true">{{ t("settings.about.diagReport") }}</button>
              <button class="mini-btn primary" :disabled="checking" @click="checkUpdate">
                {{ checking ? t("settings.about.checking") : t("settings.about.checkUpdate") }}
              </button>
              <span v-if="updateVersion" class="hint-inline">
                {{ t("settings.about.newVersionFound", { version: updateVersion }) }}
              </span>
              <button
                v-if="settings.settings?.dismissed_update_version"
                class="mini-btn"
                @click="restoreDismissed"
              >
                {{ t("settings.about.restoreUpdate", { version: settings.settings.dismissed_update_version }) }}
              </button>
            </div>
            <div class="update-source">
              <div class="choice-info">
                <span class="choice-label">{{ t("settings.about.updateSource") }}</span>
                <p class="choice-hint">{{ t("settings.about.updateSourceHint") }}</p>
              </div>
              <div ref="updateSourceSegRef" class="seg">
                <div class="indicator" :style="updateSourceSegStyle"></div>
                <button
                  :class="{ active: settings.settings.update_source !== 'github' }"
                  @click="settings.patch({ update_source: 'bucket' })"
                >
                  {{ t("settings.about.mirrorCN") }}
                </button>
                <button
                  :class="{ active: settings.settings.update_source === 'github' }"
                  @click="settings.patch({ update_source: 'github' })"
                >
                  {{ t("settings.about.githubOfficial") }}
                </button>
              </div>
            </div>
          </div>
          <div class="about-links-row">
            <button class="about-link" @click="openUrl('https://qookix.swkj1.cn/')">
              <span class="link-left"><IconGlobe /> {{ t("settings.about.website") }}</span>
              <span class="link-arrow">→</span>
            </button>
            <button class="about-link" @click="openUrl('https://github.com/weimosheng/QookiX-Launcher')">
              <span class="link-left"><IconGithub /> {{ t("settings.about.githubRepo") }}</span>
              <span class="link-arrow">→</span>
            </button>
            <button class="about-link" @click="openUrl('https://github.com/weimosheng/QookiX-Launcher/issues')">
              <span class="link-left"><IconExternal /> {{ t("settings.about.feedback") }}</span>
              <span class="link-arrow">→</span>
            </button>
            <button class="about-link" @click="openUrl('https://github.com/weimosheng/QookiX-Launcher/releases')">
              <span class="link-left"><IconList /> {{ t("settings.about.changelog") }}</span>
              <span class="link-arrow">→</span>
            </button>
            <button class="about-link" @click="openUrl('https://qm.qq.com/q/91keQnJ8dy')">
              <span class="link-left"><IconUsers /> {{ t("settings.about.qqGroup") }}</span>
              <span class="link-arrow">→</span>
            </button>
            <button class="about-link" @click="openUrl('https://afdian.com/a/qookix')">
              <span class="link-left"><IconHeart /> {{ t("settings.about.sponsor") }}</span>
              <span class="link-arrow">→</span>
            </button>
          </div>
          <div class="card glass about-license-card" id="about-license">
            <h3>{{ t("settings.about.license") }}</h3>
            <p class="license-text">
              {{ t("settings.about.licenseLead") }}
              <span class="license-accent">{{ t("settings.about.licenseAccent") }}</span>
              {{ t("settings.about.licenseTail") }}
            </p>
            <p class="legal-privacy-note">{{ t("settings.about.legalPrivacyNote") }}</p>
            <button class="about-link" @click="openUrl('https://github.com/weimosheng/QookiX-Launcher/blob/main/LICENSE')">
              <span class="link-left"><IconFile /> {{ t("settings.about.viewLicense") }}</span>
              <span class="link-arrow">→</span>
            </button>
            <button class="about-link" @click="openUrl('https://docs.qookix.cn/agreement.html')">
              <span class="link-left"><IconShield /> {{ t("settings.about.agreement") }}</span>
              <span class="link-arrow">→</span>
            </button>
          </div>
          <div class="card glass about-replay-card" id="about-onboarding">
            <div class="about-replay-info">
              <div class="about-replay-title">{{ t("settings.about.onboarding") }}</div>
              <p class="about-replay-desc">{{ t("settings.about.onboardingDesc") }}</p>
            </div>
            <button class="mini-btn primary" @click="onboarding.open">
              <IconBookOpen /> {{ t("settings.about.replayOnboarding") }}
            </button>
          </div>
          <div class="card glass about-deps-card" id="about-deps">
            <h3>{{ t("settings.about.depsTitle") }}</h3>
            <p class="license-text">{{ t("settings.about.depsIntro") }}</p>
            <div class="deps-groups">
              <div class="deps-group">
                <div class="deps-group-title">{{ t("settings.about.depsFrontend") }}</div>
                <div v-for="d in aboutDeps.frontend" :key="d.name" class="about-dep-row">
                  <div class="dep-info">
                    <span class="dep-name">{{ d.name }}<span class="dep-ver" v-if="d.version">v{{ d.version }}</span></span>
                    <span class="dep-license">{{ d.license }}</span>
                  </div>
                  <div class="dep-links">
                    <button class="dep-link" @click="openUrl(d.url)">{{ t("settings.about.source") }}</button>
                    <button class="dep-link" @click="openUrl(d.licenseUrl)">{{ t("settings.about.licenseLabel") }}</button>
                  </div>
                </div>
              </div>
              <div class="deps-group">
                <div class="deps-group-title">Rust</div>
                <div v-for="d in aboutDeps.rust" :key="d.name" class="about-dep-row">
                  <div class="dep-info">
                    <span class="dep-name">{{ d.name }}<span class="dep-ver" v-if="d.version">v{{ d.version }}</span></span>
                    <span class="dep-license">{{ d.license }}</span>
                  </div>
                  <div class="dep-links">
                    <button class="dep-link" @click="openUrl(d.url)">{{ t("settings.about.source") }}</button>
                    <button class="dep-link" @click="openUrl(d.licenseUrl)">{{ t("settings.about.licenseLabel") }}</button>
                  </div>
                </div>
              </div>
              <div class="deps-group">
                <div class="deps-group-title">{{ t("settings.about.depsThirdparty") }}</div>
                <div v-for="d in aboutDeps.thirdparty" :key="d.name" class="about-dep-row">
                  <div class="dep-info">
                    <span class="dep-name">{{ d.name }}<span class="dep-ver" v-if="d.version">v{{ d.version }}</span></span>
                    <span class="dep-license">{{ d.license }}</span>
                  </div>
                  <div class="dep-links">
                    <button class="dep-link" @click="openUrl(d.url)">{{ t("settings.about.source") }}</button>
                    <button class="dep-link" @click="openUrl(d.licenseUrl)">{{ t("settings.about.licenseLabel") }}</button>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
    </Transition>

    <DiagnosticsDialog v-model:show="showDiag" />

    <!-- 云存档浏览（浏览模式：不绑定实例，恢复时在弹窗内选择实例） -->
    <CloudSyncDialog
      v-model:show="cloudOpen"
      instance-id=""
      world=""
      game-version=""
      @restored="instances.load(true)"
    />

    <n-modal v-model:show="migrateModal" preset="card" :title="t('settings.migrate.modalTitle')" class="migrate-modal">
      <div v-if="migratePhase === 'select'" class="migrate-body">
        <p class="migrate-label">{{ t("settings.migrate.newDir") }}</p>
        <code class="mono dir">{{ pendingNewDir }}</code>
        <p class="migrate-label">{{ t("settings.migrate.mode") }}</p>
        <div class="migrate-modes">
          <button
            type="button"
            class="migrate-mode"
            :class="{ active: migrateMode === 'move' }"
            @click="migrateMode = 'move'"
          >
            <span class="mm-title">{{ t("settings.migrate.moveTitle") }}</span>
            <span class="mm-desc">{{ t("settings.migrate.moveDesc") }}</span>
          </button>
          <button
            type="button"
            class="migrate-mode"
            :class="{ active: migrateMode === 'copy' }"
            @click="migrateMode = 'copy'"
          >
            <span class="mm-title">{{ t("settings.migrate.copyTitle") }}</span>
            <span class="mm-desc">{{ t("settings.migrate.copyDesc") }}</span>
          </button>
          <button
            type="button"
            class="migrate-mode"
            :class="{ active: migrateMode === 'pointer' }"
            @click="migrateMode = 'pointer'"
          >
            <span class="mm-title">{{ t("settings.migrate.pointerTitle") }}</span>
            <span class="mm-desc">{{ t("settings.migrate.pointerDesc") }}</span>
          </button>
        </div>
        <p class="migrate-warn">{{ t("settings.migrate.warn") }}</p>
        <div class="migrate-actions">
          <button class="mini-btn" @click="migrateModal = false">{{ t("common.cancel") }}</button>
          <button class="mini-btn primary" :disabled="migrating" @click="confirmMigrate">
            {{ migrating ? t("settings.migrate.migrating") : t("settings.migrate.start") }}
          </button>
        </div>
      </div>
      <div v-else class="migrate-body">
        <p class="migrate-ok">{{ t("settings.migrate.done") }}</p>
        <code class="mono dir">{{ pendingNewDir }}</code>
        <p class="migrate-warn">{{ t("settings.migrate.restartWarn") }}</p>
        <div class="migrate-actions">
          <button class="mini-btn" @click="migrateModal = false">{{ t("settings.migrate.later") }}</button>
          <button class="mini-btn primary" @click="relaunchNow">{{ t("settings.migrate.now") }}</button>
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
.settings-side {
  flex-shrink: 0;
  width: 200px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  position: sticky;
  top: 0;
  align-self: flex-start;
}
.nav-search-card {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 8px 11px;
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 12px;
  backdrop-filter: blur(var(--glass-blur, 8px));
  -webkit-backdrop-filter: blur(var(--glass-blur, 8px));
  transition: border-color 0.12s;
}
.nav-search-card:focus-within {
  border-color: var(--accent);
}
.nav-search-icon {
  width: 14px;
  height: 14px;
  flex-shrink: 0;
  opacity: 0.6;
  color: var(--text-2);
}
.nav-search-input {
  width: 100%;
  border: none;
  background: transparent;
  outline: none;
  color: var(--text-1);
  font-size: 13px;
  font-family: inherit;
}
.nav-search-input::placeholder {
  color: var(--text-3);
}
.settings-nav {
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 14px;
  backdrop-filter: blur(var(--glass-blur, 8px));
  -webkit-backdrop-filter: blur(var(--glass-blur, 8px));
  padding: 8px 6px;
  max-height: calc(100vh - 48px);
  overflow-y: auto;
}
.nav-list {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.nav-group {
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.nav-group-head {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 9px 10px 4px;
  color: var(--text-3);
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.02em;
}
.nav-group-icon {
  width: 13px;
  height: 13px;
  flex-shrink: 0;
  opacity: 0.7;
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
.nav-result {
  display: flex;
  flex-direction: column;
  gap: 2px;
  text-align: left;
  border: none;
  background: transparent;
  color: var(--text-2);
  padding: 7px 10px;
  border-radius: 8px;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  font-family: inherit;
  transition: background 0.12s, color 0.12s;
}
.nav-result-title {
  color: var(--text-1);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.nav-result-sub {
  font-size: 11px;
  font-weight: 400;
  color: var(--text-3);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.nav-result:hover {
  background: var(--panel-hover);
}
.nav-result.current {
  box-shadow: inset 3px 0 0 0 var(--accent);
}
.nav-result.active {
  background: var(--accent-soft);
}
.hl {
  background: var(--accent-soft);
  color: var(--accent);
  border-radius: 3px;
  padding: 0 2px;
}
.nav-empty {
  padding: 14px 10px;
  color: var(--text-3);
  font-size: 12px;
  text-align: center;
}
.setting-flash {
  animation: setting-flash 1.2s ease-out;
}
@keyframes setting-flash {
  0% {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  100% {
    outline: 2px solid transparent;
    outline-offset: 2px;
  }
}
.settings-body {
  flex: 1;
  min-width: 0;
}
/* 子标签页切换动画 */
.settings-pane-enter-active {
  transition: opacity 0.22s ease, transform 0.26s cubic-bezier(0.22, 1, 0.36, 1);
}
.settings-pane-leave-active {
  transition: opacity 0.13s ease, transform 0.13s ease-in;
}
.settings-pane-enter-from {
  opacity: 0;
  transform: translateY(10px);
}
.settings-pane-leave-to {
  opacity: 0;
  transform: translateY(-6px);
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
  grid-template-columns: repeat(auto-fill, minmax(340px, 1fr));
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
  background: var(--w-06);
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
  background: var(--w-04);
  color: var(--text-2);
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  white-space: nowrap;
}
.mini-btn:hover {
  background: var(--w-08);
}
.mini-btn:disabled {
  opacity: 0.5;
}
.mini-btn.danger {
  border-color: var(--danger-35);
  color: #e5534b;
  background: var(--danger-08);
}
.mini-btn.danger:hover {
  background: var(--danger-16);
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
  background: var(--w-03);
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
.mem-gauge {
  margin-top: 14px;
}
.mem-gauge-track {
  position: relative;
  height: 10px;
  border-radius: 6px;
  background: var(--w-08);
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
.row-label {
  display: block;
  font-size: 13px;
  color: var(--text-2);
  margin-bottom: 10px;
}
.row-label-text {
  display: block;
  margin-bottom: 4px;
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
  background: var(--w-05);
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
  width: 22px;
  height: 22px;
  border-radius: 50%;
  cursor: pointer;
  padding: 0;
  position: relative;
  background: conic-gradient(from 0deg, #f00, #ff0, #0f0, #0ff, #00f, #f0f, #f00);
  border: none;
  transition: transform 0.12s ease, box-shadow 0.12s ease;
}
.color-custom:hover {
  transform: scale(1.12);
}
.color-custom.active {
  box-shadow: 0 0 0 2px var(--accent-soft);
}
.color-custom-ring {
  position: absolute;
  inset: 3px;
  border-radius: 50%;
  box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.18);
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
  background: var(--w-05);
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
.migrate-mode {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: 10px;
  cursor: pointer;
  font-family: inherit;
  font-size: 13px;
  line-height: inherit;
  text-align: left;
  color: var(--text-2);
  background: var(--w-03);
  transition: border-color 0.15s, background 0.15s;
}
.migrate-mode:hover {
  border-color: var(--accent-05);
}
.migrate-mode.active {
  border-color: var(--accent);
  background: var(--accent-soft);
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
}
.about-license-card {
  grid-column: 1 / -1;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.about-license-card .about-link {
  margin-bottom: 0;
}
.about-license-card h3 {
  margin: 0;
  font-size: 15px;
  font-weight: 700;
  color: var(--text-1);
}
.license-text {
  margin: 0;
  font-size: 13px;
  line-height: 1.6;
  color: var(--text-3);
}
.legal-privacy-note {
  margin: 0;
  padding: 10px 12px;
  font-size: 12.5px;
  line-height: 1.65;
  color: var(--text-2);
  background: var(--accent-soft);
  border-left: 3px solid var(--accent);
  border-radius: 6px;
}
.license-accent {
  color: var(--accent);
  font-weight: 600;
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
/* 顶部展示卡：画布与卡片保持留白，桌布做斜向动画 + 曲奇咬口 */
.about-showcase {
  padding: 16px;
}
.about-showcase .about-show-wrap {
  height: 200px;
  border-radius: 10px;
  overflow: hidden;
  border: 1px solid var(--border);
}
.about-hero-title {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 12px;
  margin-top: 14px;
}
.about-hero-name {
  font-size: 20px;
}
.about-hero-slogan {
  text-align: center;
  margin: 6px 0 2px;
  font-size: 13px;
  color: var(--text-2);
  letter-spacing: 0.2px;
}
.about-replay-card {
  grid-column: 1 / -1;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 18px 20px;
}
.about-replay-title {
  font-size: 14px;
  font-weight: 700;
  color: var(--text-1);
}
.about-replay-desc {
  margin: 4px 0 0;
  font-size: 12px;
  color: var(--text-3);
  line-height: 1.5;
}
.about-deps-card {
  grid-column: 1 / -1;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.about-deps-card h3 {
  margin: 0;
  font-size: 15px;
  font-weight: 700;
  color: var(--text-1);
}
.deps-groups {
  display: grid;
  grid-template-columns: 1fr;
  gap: 16px;
  margin-top: 4px;
}
.deps-group {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.deps-group-title {
  font-size: 11px;
  font-weight: 600;
  color: var(--text-3);
  text-transform: uppercase;
  letter-spacing: 0.5px;
  margin-bottom: 2px;
}
.about-dep-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 7px 10px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--panel);
  -webkit-backdrop-filter: blur(var(--glass-blur, 8px));
  backdrop-filter: blur(var(--glass-blur, 8px));
  transition: border-color 0.15s;
  flex-wrap: wrap;
}
.about-dep-row:hover {
  border-color: var(--accent);
}
.dep-info {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  flex: 1;
}
.about-dep-row .dep-name {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-1);
  display: flex;
  align-items: baseline;
  gap: 5px;
  white-space: nowrap;
}
.dep-ver {
  font-size: 11px;
  color: var(--text-3);
  font-weight: 400;
}
.dep-license {
  font-size: 10px;
  color: var(--accent);
  background: var(--accent-soft);
  padding: 2px 6px;
  border-radius: 5px;
  font-weight: 500;
  white-space: nowrap;
}
.dep-links {
  display: flex;
  gap: 6px;
  flex-shrink: 0;
}
.dep-link {
  font-size: 11px;
  font-family: inherit;
  padding: 3px 8px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: transparent;
  color: var(--text-3);
  cursor: pointer;
  transition: all 0.15s;
  white-space: nowrap;
}
.dep-link:hover {
  border-color: var(--accent);
  color: var(--accent);
  background: var(--accent-soft);
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
.dev-role {
  font-size: 12px;
  color: var(--text-3);
  line-height: 1.3;
}
/* 开发者区块：一人一行，不套卡片 */
.about-devs-title {
  font-size: 12px;
  font-weight: 700;
  color: var(--text-3);
  margin: 0 0 10px;
  letter-spacing: 0.3px;
}
.dev-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
  margin-bottom: 18px;
}
.dev-line {
  display: flex;
  align-items: center;
  gap: 12px;
}
.dev-meta {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.dev-head {
  display: flex;
  align-items: center;
  gap: 8px;
}
.dev-avatar {
  width: 40px;
  height: 40px;
  border-radius: 50%;
  object-fit: cover;
  border: 2px solid var(--accent-35);
  flex-shrink: 0;
}
.dev-name {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-1);
}
.dev-github-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  padding: 0;
  border: 1px solid var(--border);
  border-radius: 7px;
  background: transparent;
  color: var(--text-3);
  font-size: 13px;
  cursor: pointer;
  transition: color 0.14s, border-color 0.14s, background 0.14s;
}
.dev-github-btn:hover {
  color: var(--accent);
  border-color: var(--accent-35);
  background: var(--accent-soft);
}
.about-links-row {
  grid-column: 1 / -1;
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 12px;
}
.about-links-row .about-link {
  justify-content: flex-start;
  padding: 18px 16px;
  min-height: 56px;
  font-size: 14px;
}
.about-link {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: 9px;
  /* 与 .card.glass 一致的磨砂背景，避免和卡片质感不一致 */
  background: var(--panel);
  -webkit-backdrop-filter: blur(var(--glass-blur, 8px));
  backdrop-filter: blur(var(--glass-blur, 8px));
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
.link-left {
  display: flex;
  align-items: center;
  gap: 8px;
}
.link-left :deep(svg) {
  font-size: 15px;
  color: var(--accent);
  flex-shrink: 0;
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
.tune-row .tune-slider {
  flex: 1;
  min-width: 0;
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
  background: var(--w-08);
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
  background: var(--w-03);
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
/* 下载代理「测试连接」：跟随主题色 */
.proxy-test-btn {
  color: var(--accent);
  border-color: var(--accent-35);
  background: var(--accent-soft);
  font-weight: 600;
  transition: color 0.15s, border-color 0.15s, background 0.15s;
}
.proxy-test-btn:hover:not(.disabled) {
  background: var(--accent);
  border-color: var(--accent);
  color: #1a1208;
}
.proxy-row {
  display: flex;
  align-items: center;
  gap: 10px;
}
.switch-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-top: 12px;
  font-size: 13px;
  color: var(--text-2);
}
.proxy-row .seg {
  flex: 1;
  min-width: 0;
}
.proxy-row + .text-input {
  margin-top: 8px;
}
.mirror-custom {
  flex-wrap: wrap;
}
.mirror-custom-head {
  flex-shrink: 0;
  border: none;
  background: transparent;
  padding: 0;
  font-family: inherit;
  font-size: 13px;
  line-height: inherit;
  font-weight: 600;
  color: var(--text-1);
  cursor: pointer;
  transition: color 0.15s;
}
.mirror-custom-head:hover {
  color: var(--accent);
}
.mirror-custom .text-input {
  flex: 1;
  min-width: 150px;
}
</style>

