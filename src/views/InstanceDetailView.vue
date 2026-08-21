<script setup lang="ts">
import { ref, watch, computed } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useInstancesStore } from "../stores/instances";
import { useAccountsStore } from "../stores/accounts";
import { useTasksStore } from "../stores/tasks";
import { useSettingsStore } from "../stores/settings";
import { useMessage, NModal, NSelect, NButton } from "naive-ui";
import { api } from "../api";
import { open } from "@tauri-apps/plugin-dialog";
import { convertFileSrc } from "@tauri-apps/api/core";
import LogViewer from "../components/LogViewer.vue";
import AppIcon from "../components/AppIcon.vue";
import IconPickerDialog from "../components/IconPickerDialog.vue";
import type { ContentItem, UpdateInfo } from "../types";

function sourceLabel(s: string) {
  return s === "modrinth" ? "Modrinth" : s === "curseforge" ? "CurseForge" : "手动";
}
import {
  IconBox,
  IconCamera,
  IconCheck,
  IconChevronRight,
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
  IconStop,
  IconTrash,
} from "../components/icons";
const route = useRoute();
const router = useRouter();
const instances = useInstancesStore();
const accounts = useAccountsStore();
const tasks = useTasksStore();
const message = useMessage();

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

const instanceId = route.params.id as string;
const tab = ref("mods");

const instance = computed(() => instances.get(instanceId));

const contentItems = ref<ContentItem[]>([]);
const updates = ref<UpdateInfo[]>([]);
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
  { key: "saves", label: "存档", icon: IconFolder, folder: "saves" },
  { key: "logs", label: "日志", icon: IconFile },
  { key: "settings", label: "设置", icon: IconExternal },
];

// folder-backed tabs are only shown when the corresponding folder exists
const tabs = computed(() =>
  ALL_TABS.filter((t) => !t.folder || folders.value[t.folder] || t.key === tab.value)
);

function loaderLabel() {
  const i = instance.value;
  if (!i) return "";
  return i.loader === "vanilla" ? "原版" : i.loader.toUpperCase();
}

async function loadFolders() {
  try {
    const r = await api.listInstanceFolders(instanceId);
    folders.value = Object.fromEntries(r.folders.map((f) => [f.name, f.exists]));
  } catch {
    /* ignore */
  }
}

async function loadContent() {
  if (!instance.value) return;
  loadingContent.value = true;
  try {
    const res = await api.listContent(instanceId, kindOf(tab.value));
    contentItems.value = res.items;
  } catch (e) {
    message.error(String(e));
  } finally {
    loadingContent.value = false;
  }
}

async function loadFiles() {
  const sub = tab.value === "screenshots" ? "screenshots" : "saves";
  loadingFiles.value = true;
  try {
    const r = await api.listInstanceFiles(instanceId, sub);
    fileItems.value = r.files;
  } catch (e) {
    message.error(String(e));
  } finally {
    loadingFiles.value = false;
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
  try {
    await instances.launch(i.id, name);
    message.success(`正在进入世界「${name}」`);
  } catch (e) {
    message.error(String(e));
  } finally {
    launchingWorld.value = "";
  }
}

async function checkUpdates() {
  checkingUpdates.value = true;
  try {
    const kind = kindOf(tab.value);
    updates.value = await api.checkUpdates(instanceId, kind);
    if (!updates.value.length) message.success("所有内容都是最新版本");
  } catch (e) {
    message.error(String(e));
  } finally {
    checkingUpdates.value = false;
  }
}

async function applyUpdate(u: UpdateInfo) {
  try {
    const kind = kindOf(tab.value);
    await api.applyUpdate(instanceId, kind, u.filename, "modrinth", u.projectId, u.latestVersionId);
    message.success("已更新 " + (u.projectTitle ?? u.filename));
    updates.value = [];
    await loadContent();
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
  confirm({
    title: "删除实例",
    content: "删除实例将移除其游戏目录与全部内容，此操作不可恢复。",
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
const autoMem = ref(0);

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

const globalMemory = computed(() => useSettingsStore().settings?.max_memory_mb ?? 4096);

const autoMemory = computed(() => {
  let rec = autoMem.value || Math.round(memTotal.value * 0.5);
  // 自动配置不超出当前可用（剩余）内存
  if (memAvailable.value > 0 && rec > memAvailable.value) rec = memAvailable.value;
  return rec;
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
    autoMem.value = res.max_mb;
  } catch {
    /* ignore, fall back to defaults */
  }
}

watch(
  () => instance.value,
  (i) => {
    if (!i) return;
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
    message.success("设置已保存");
  } catch (e) {
    message.error(String(e));
  }
}

watch(
  () => tab.value,
  (t) => {
    if (t === "logs") {
      // nothing to load
    } else if (t === "settings") {
      detectJava();
      loadMemoryInfo();
    } else if (t === "screenshots" || t === "saves") {
      loadFiles();
    } else {
      loadContent();
      updates.value = [];
    }
  },
  { immediate: true }
);

watch(
  () => instance.value?.installed,
  () => {
    loadFolders();
    if (tab.value === "screenshots" || tab.value === "saves") loadFiles();
    else loadContent();
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
        <button class="btn primary" :disabled="tasks.gameRunning" @click="launch">
          <IconStop v-if="tasks.gameRunning" />
          <IconPlay v-else />
          {{ tasks.gameRunning ? "运行中" : "启动游戏" }}
        </button>
        <button class="btn ghost" title="打开游戏目录" @click="openFolder()">
          <IconFolder />
        </button>
        <button class="btn danger" title="删除实例" @click="removeInstance">
          <IconTrash />
        </button>
      </div>
    </div>

    <div class="tabs glass">
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
            <span v-if="updates.length" class="upd-n">{{ updates.length }}</span>
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
        <div v-if="updates.length" class="updates glass">
          <div v-for="u in updates" :key="u.filename" class="upd-row">
            <div class="upd-info">
              <b>{{ u.projectTitle ?? u.filename }}</b>
              <span class="ver-change">
                {{ u.currentVersion ?? "未知" }} <IconChevronRight /> {{ u.latestVersion }}
              </span>
            </div>
            <button class="mini-btn accent" @click="applyUpdate(u)">更新</button>
          </div>
        </div>

        <div v-if="loadingContent" class="center">加载中…</div>
        <div v-else-if="!contentItems.length" class="empty glass">
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
                v-if="item.record.icon"
                :src="iconUrl(item.record.icon) ?? ''"
                class="c-thumb"
                alt=""
                loading="lazy"
              />
              <IconFile v-else-if="item.record.filename.endsWith('.jar')" />
              <IconImage v-else />
            </div>
            <div class="c-info">
              <div class="c-name text-ellipsis">{{ item.record.name ?? item.record.filename }}</div>
              <div class="c-meta">
                <span class="src" :class="item.record.source">{{ sourceLabel(item.record.source) }}</span>
                <span v-if="item.record.version" class="ver">{{ item.record.version }}</span>
                <span v-if="!item.exists" class="missing">文件缺失</span>
              </div>
            </div>
            <div class="c-actions">
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

      <!-- saves -->
      <template v-if="tab === 'saves'">
        <div v-if="loadingFiles" class="center">加载中…</div>
        <div v-else-if="!fileItems.length" class="empty glass">
          <p>还没有存档</p>
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
                class="mini-btn play"
                :disabled="!!launchingWorld || tasks.gameRunning"
                @click="launchWorld(f.name)"
              >
                <IconPlay /> {{ launchingWorld === f.name ? "启动中…" : "直接启动" }}
              </button>
            </div>
          </div>
          <div v-if="!fileItems.some((x) => x.isDir)" class="center">这个实例还没有世界存档</div>
        </div>
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
              <span v-if="edit.memory_mode === 'global'" class="mem-mode-note">（全局设置）</span>
              <span v-else-if="edit.memory_mode === 'auto'" class="mem-mode-note">（推荐 {{ autoMemory }} MB）</span>
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

          <div class="set-actions">
            <button class="btn primary" @click="saveSettings"><IconCheck /> 保存设置</button>
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
      :on-update:show="(v: boolean) => { if (!v) confirmState = null; }"
    >
      <div v-if="confirmState" style="display: flex; flex-direction: column; gap: 16px;">
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
    >
      <img :src="previewImg" class="preview-img" alt="" />
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
.d-icon {
  width: 56px;
  height: 56px;
  border-radius: 14px;
  background: linear-gradient(135deg, rgba(232, 154, 75, 0.3), rgba(232, 154, 75, 0.1));
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 26px;
  color: var(--accent);
  flex-shrink: 0;
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
  background: transparent;
  color: var(--text-3);
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
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 6px;
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
  background: var(--accent-soft);
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
.ver {
  color: var(--text-3);
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
.mini-btn.play {
  color: var(--accent);
  border-color: rgba(232, 154, 75, 0.4);
  background: var(--accent-soft);
}
.mini-btn.play:disabled {
  opacity: 0.5;
  cursor: default;
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
  background: linear-gradient(135deg, rgba(232, 154, 75, 0.28), rgba(232, 154, 75, 0.08));
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 18px;
  color: var(--accent);
  flex-shrink: 0;
}
.icon-preview :deep(.app-icon) {
  font-size: 18px;
}
.set-actions {
  grid-column: 1 / -1;
  display: flex;
  justify-content: flex-end;
}
</style>
