<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { fmtBytes } from "../utils/format";
import { CORE_COLORS, CORE_LABELS } from "../utils/cores";
import { isAprilFools } from "../utils/versions";
import { useRouter } from "vue-router";
import { NModal, NInput, NProgress, useMessage } from "naive-ui";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api } from "../api";
import { useAccountsStore } from "../stores/accounts";
import { useServersStore } from "../stores/servers";
import type { ServerCore, TerracottaDownloadProgress, TerracottaInfo } from "../types";
import {
  IconBox,
  IconCheck,
  IconClose,
  IconCopy,
  IconDoorOpen,
  IconDownload,
  IconExternal,
  IconPlay,
  IconPlus,
  IconRefresh,
  IconServer,
  IconStop,
  IconTrash,
  IconUsers,
} from "../components/icons";

const router = useRouter();
const accounts = useAccountsStore();
const servers = useServersStore();
const message = useMessage();

const tab = ref<"servers" | "rooms">("servers");

// ---- 版本清单 ----
const versions = ref<{ id: string; type: string; releaseTime: string }[]>([]);
const versionCat = ref<string>("release");

const filteredVersions = computed(() =>
  versions.value.filter((v) => {
    if (versionCat.value === "release") return v.type === "release" || v.type.startsWith("old_");
    if (versionCat.value === "april") return isAprilFools(v);
    return v.type === "snapshot" && !isAprilFools(v);
  }),
);

// ---- 服务器核心选项 ----
const CORES: { value: ServerCore; label: string; desc: string }[] = [
  { value: "vanilla", label: "Vanilla", desc: "官方原版核心" },
  { value: "paper", label: "Paper", desc: "高性能，推荐" },
  { value: "spigot", label: "Spigot", desc: "经典插件核心" },
  { value: "purpur", label: "Purpur", desc: "Paper 下游优化" },
  { value: "forge", label: "Forge", desc: "模组服务端" },
  { value: "fabric", label: "Fabric", desc: "轻量模组服务端" },
];

// ---- 创建对话框（仅名称 / 核心 / 版本）----
type ServerDialog = {
  name: string;
  core: ServerCore;
  mc_version: string;
};

const dialog = ref<ServerDialog | null>(null);
const saving = ref(false);

// ---- 核心下载进度 ----
const installing = ref<{ name: string; phase: string; done: number; total: number } | null>(null);

const installPct = computed(() => {
  const ins = installing.value;
  if (!ins || ins.total <= 0) return 0;
  return Math.min(100, Math.round((ins.done / ins.total) * 100));
});
const installHasBytes = computed(() => {
  const ins = installing.value;
  return !!ins && ins.total > 1;
});
function openCreate() {
  versionCat.value = "release";
  dialog.value = { name: "", core: "paper", mc_version: "" };
}

const canSave = computed(() => {
  const d = dialog.value;
  return !!d && !!d.core && !!d.mc_version;
});

async function saveDialog() {
  const d = dialog.value;
  if (!d) return;
  if (!d.mc_version) return message.warning("请选择游戏版本");
  saving.value = true;
  try {
    const s = await servers.create(d.name, d.core, d.mc_version);
    dialog.value = null;
    saving.value = false;

    installing.value = { name: s.name, phase: "正在准备核心…", done: 0, total: 0 };
    const un = await listen<{ serverId: string; phase: string; done: number; total: number }>(
      "server://install-progress",
      (ev) => {
        if (installing.value) {
          installing.value = {
            ...installing.value,
            phase: ev.payload.phase,
            done: ev.payload.done ?? 0,
            total: ev.payload.total ?? 0,
          };
        }
      },
    );
    try {
      await servers.installCore(s.id);
      message.success(`服务器「${s.name}」核心已就绪`);
    } catch (e) {
      message.error(String(e));
    }
    un();
    installing.value = null;
    router.push(`/multiplayer/${s.id}`);
  } catch (e) {
    message.error(String(e));
    saving.value = false;
    installing.value = null;
  }
}

// ---- 删除确认 ----
const confirmState = ref<{ id: string; name: string } | null>(null);
function confirmDelete(id: string, name: string) {
  confirmState.value = { id, name };
}
async function doDelete() {
  const c = confirmState.value;
  if (!c) return;
  try {
    await servers.remove(c.id);
    message.success(`已删除「${c.name}」`);
  } catch (e) {
    message.error(String(e));
  }
  confirmState.value = null;
}

// ---- 启动 / 停止 ----
async function toggleRun(id: string, running: boolean) {
  try {
    if (running) {
      await servers.stop(id);
      message.success("已停止服务器");
    } else {
      await servers.start(id);
      message.success("服务器已启动");
    }
  } catch (e) {
    message.error(String(e));
  }
}

// ---- 陶瓦联机（联机房间）----
const tc = ref<TerracottaInfo | null>(null);
const tcLoading = ref(false);
const tcBusy = ref(false);
const tcRoomCode = ref("");
// 创建房间的玩家名，默认读取当前账号名
const tcPlayerName = ref("");
const tcRoom = ref<Record<string, unknown> | null>(null);
const tcJoined = ref(false);
const tcDownloading = ref(false);
const tcDownloadProgress = ref<TerracottaDownloadProgress | null>(null);
let tcDlUnlisten: (() => void) | null = null;
let tcPollTimer: number | null = null;

const tcStateName = computed<string>(() => String(tcRoom.value?.state ?? "waiting"));
const tcRoomCodeFinal = computed<string>(() => {
  const s = tcRoom.value;
  if (!s) return "";
  const room = s.room;
  if (room && typeof room === "object") {
    const code = (room as Record<string, unknown>).code;
    if (code) return String(code);
  }
  return String(s.room ?? "");
});
const tcUrl = computed<string>(() => String(tcRoom.value?.url ?? ""));

// 解析玩家列表（profiles 数组），自适应常见字段名
const tcPlayers = computed<Record<string, unknown>[]>(() => {
  const raw = tcRoom.value?.profiles;
  if (!Array.isArray(raw)) return [];
  return raw.filter((p): p is Record<string, unknown> => !!p && typeof p === "object");
});
const playerName = (p: Record<string, unknown>): string =>
  String(p.name ?? p.username ?? p.playerName ?? p.displayName ?? "未知玩家");
const playerUuid = (p: Record<string, unknown>): string =>
  String(p.uuid ?? p.id ?? "");
const playerPing = (p: Record<string, unknown>): string => {
  const v = p.ping ?? p.latency;
  return v === undefined || v === null ? "" : String(v);
};
const playerDesc = (p: Record<string, unknown>): string =>
  String(p.description ?? p.desc ?? p.comment ?? "");

const tcStateText = computed(() => {
  const map: Record<string, string> = {
    waiting: "等待中，可创建或加入房间",
    "host-scanning": "正在扫描局域网世界…",
    "host-starting": "房间创建中…",
    "host-ok": "房间已创建",
    "guest-connecting": "正在连接房间…",
    "guest-starting": "正在加入房间…",
    "guest-ok": "已加入房间",
    exception: "连接发生错误",
  };
  return map[tcStateName.value] ?? "状态同步中…";
});

async function refreshTc() {
  tcLoading.value = true;
  try {
    tc.value = await api.terracottaDetect();
    if (tc.value?.running) startTcPoll();
  } catch (e) {
    message.error(String(e));
  } finally {
    tcLoading.value = false;
  }
}

function openTcDownload() {
  const url = tc.value?.download_url;
  if (url) openUrl(url).catch(() => {});
}

const tcDownloadPercent = computed(() => {
  const p = tcDownloadProgress.value;
  if (!p) return 0;
  if (p.done || p.extracting) return 100;
  if (p.percent >= 0) return Math.min(100, p.percent);
  return 0;
});

const tcDownloadText = computed(() => {
  const p = tcDownloadProgress.value;
  if (!p) return "正在准备下载…";
  if (p.extracting) return "下载完成，正在解压安装…";
  if (p.done) return "安装完成";
  if (p.total > 0) {
    const mb = (v: number) => (v / 1024 / 1024).toFixed(1);
    return `${tcDownloadPercent.value}%（${mb(p.downloaded)} / ${mb(p.total)} MB）`;
  }
  return "正在获取下载地址…";
});

async function downloadTc() {
  if (tcDownloading.value || tcBusy.value) return;
  tcDownloading.value = true;
  tcDownloadProgress.value = { downloaded: 0, total: 0, percent: 0 };
  try {
    tcDlUnlisten = await listen<TerracottaDownloadProgress>(
      "terracotta://download-progress",
      (ev) => {
        tcDownloadProgress.value = ev.payload;
      },
    );
    await api.terracottaDownload();
    tcDownloadProgress.value = { downloaded: 1, total: 1, percent: 100, done: true };
    message.success("陶瓦联机已安装完成");
    await refreshTc();
  } catch (e) {
    message.error(String(e));
  } finally {
    if (tcDlUnlisten) {
      tcDlUnlisten();
      tcDlUnlisten = null;
    }
    tcDownloading.value = false;
  }
}

async function launchTc() {
  if (tcBusy.value) return;
  tcBusy.value = true;
  try {
    const l = await api.terracottaLaunch();
    if (tc.value) tc.value = { ...tc.value, running: true, port: l.port };
    startTcPoll();
    message.success("陶瓦联机已启动");
  } catch (e) {
    message.error(String(e));
  } finally {
    tcBusy.value = false;
  }
}

async function stopTc() {
  try {
    await api.terracottaStop();
    stopTcPoll();
    if (tc.value) tc.value = { ...tc.value, running: false, port: null };
    tcRoom.value = null;
    message.success("已停止陶瓦联机");
  } catch (e) {
    message.error(String(e));
  }
}

function openTcUi() {
  const p = tc.value?.port;
  if (p) openUrl(`http://127.0.0.1:${p}/`).catch(() => {});
}

// 正在进行中的状态：轮询时不会被空闲(waiting/空)状态覆盖
const TC_PENDING_STATES = new Set([
  "host-scanning",
  "host-starting",
  "guest-connecting",
  "guest-starting",
]);

async function pollTcState() {
  try {
    const s = (await api.terracottaStatus()) as Record<string, unknown> | null;
    const next = String(s?.state ?? "waiting");
    // 本地正处于进行中状态，而陶瓦返回空闲/空，说明还没进入对应状态，
    // 保留本地状态，避免界面掉回「创建/加入」面板
    if (TC_PENDING_STATES.has(tcStateName.value) && next === "waiting") {
      return;
    }
    tcRoom.value = s;
  } catch {
    // 轮询失败忽略，等待下一次
  }
}

function startTcPoll() {
  stopTcPoll();
  pollTcState();
  tcPollTimer = window.setInterval(pollTcState, 1500);
}

function stopTcPoll() {
  if (tcPollTimer !== null) {
    window.clearInterval(tcPollTimer);
    tcPollTimer = null;
  }
}

async function createRoom() {
  if (tcBusy.value) return;
  tcBusy.value = true;
  // 点击后立即进入扫描引导，避免等待轮询期间掉回创建/加入面板
  tcRoom.value = { state: "host-scanning" };
  try {
    const name = tcPlayerName.value.trim();
    await api.terracottaCreateRoom(name || undefined);
    await pollTcState();
  } catch (e) {
    tcRoom.value = null;
    message.error(String(e));
  } finally {
    tcBusy.value = false;
  }
}

async function joinRoom() {
  if (tcBusy.value) return;
  const code = tcRoomCode.value.trim();
  if (!code) return message.warning("请输入房间码");
  tcBusy.value = true;
  // 点击后立即进入连接引导，避免等待轮询期间掉回创建/加入面板
  tcRoom.value = { state: "guest-connecting" };
  try {
    const name = tcPlayerName.value.trim();
    await api.terracottaJoinRoom(code, name || undefined);
    await pollTcState();
  } catch (e) {
    tcRoom.value = null;
    message.error(String(e));
  } finally {
    tcBusy.value = false;
  }
}

async function leaveRoom() {
  try {
    await api.terracottaLeave();
    tcRoom.value = null;
    message.success("已退出房间");
  } catch (e) {
    message.error(String(e));
  }
}

async function copyRoomCode() {
  if (!tcRoomCodeFinal.value) return;
  try {
    await navigator.clipboard.writeText(tcRoomCodeFinal.value);
    tcJoined.value = true;
    setTimeout(() => (tcJoined.value = false), 1600);
  } catch {
    // 忽略剪贴板失败
  }
}

// 默认读取当前账号名，切换账号时若玩家名未被手动修改则同步更新
watch(
  () => accounts.current?.username ?? "",
  (name) => {
    if (name && (!tcPlayerName.value || tcPlayerName.value === "")) tcPlayerName.value = name;
  },
  { immediate: true },
);

onMounted(async () => {
  servers.load();
  refreshTc();
  try {
    const m = await api.getVersionManifest();
    versions.value = m.versions;
  } catch (e) {
    message.error(String(e));
  }
});

// 标题栏的“创建服务器”按钮仅在“服务器”标签页显示
watch(tab, (v) => servers.setCanCreate(v === "servers"), { immediate: true });
onUnmounted(() => servers.setCanCreate(false));

// 响应标题栏的“创建服务器”按钮
watch(
  () => servers.createRequest,
  (n) => {
    if (n > 0) openCreate();
  },
);

onUnmounted(() => stopTcPoll());
</script>

<template>
  <div class="mp-view">
    <div class="mode-tabs glass">
      <button :class="{ active: tab === 'servers' }" @click="tab = 'servers'">
        <IconServer /> 服务器
      </button>
      <button :class="{ active: tab === 'rooms' }" @click="tab = 'rooms'">
        <IconUsers /> 联机房间
      </button>
    </div>

    <!-- 服务器 -->
    <template v-if="tab === 'servers'">
      <div v-if="servers.count" class="grid">
        <div
          v-for="s in servers.servers"
          :key="s.id"
          class="server-card glass clickable"
          @click="router.push(`/multiplayer/${s.id}`)"
        >
          <div class="card-head">
            <span
              class="core-badge"
              :style="{ color: CORE_COLORS[s.core], borderColor: CORE_COLORS[s.core] }"
            >
              {{ CORE_LABELS[s.core] }}
            </span>
            <h3 class="card-name">{{ s.name }}</h3>
          </div>
          <div class="card-meta">
            <div class="meta-row"><span>版本</span><b class="mono">{{ s.mc_version }}</b></div>
            <div class="meta-row"><span>端口</span><b>{{ s.port }}</b></div>
          </div>
          <div class="card-motd">{{ s.motd || "A Minecraft Server" }}</div>
          <div class="card-foot" @click.stop>
            <span class="status" :class="{ on: servers.isRunning(s.id) }">
              {{ servers.isRunning(s.id) ? "运行中" : "未启动" }}
            </span>
            <div class="ops">
              <button
                class="op"
                :class="servers.isRunning(s.id) ? 'stop' : 'start'"
                @click="toggleRun(s.id, servers.isRunning(s.id))"
              >
                <IconStop v-if="servers.isRunning(s.id)" />
                <IconPlay v-else />
                {{ servers.isRunning(s.id) ? "停止" : "启动" }}
              </button>
              <button class="op danger" title="删除" @click="confirmDelete(s.id, s.name)">
                <IconTrash />
              </button>
            </div>
          </div>
        </div>
      </div>

      <div v-else class="empty glass">
        <div class="empty-icon"><IconServer /></div>
        <p>还没有服务器，创建一个开始联机</p>
        <button class="btn primary" @click="openCreate">创建第一个服务器</button>
      </div>
    </template>

    <!-- 联机房间（陶瓦联机） -->
    <template v-else>
      <!-- 检测中 -->
      <div v-if="tcLoading || !tc" class="empty glass">
        <div class="empty-icon"><IconRefresh /></div>
        <p>正在检测陶瓦联机…</p>
      </div>

      <!-- 未安装 -->
      <div v-else-if="!tc.found" class="tc-download">
        <div class="tc-hero glass">
          <div v-if="tc.icon" class="tc-hero-icon tc-icon-img">
            <img :src="tc.icon" alt="陶瓦联机" />
          </div>
          <div class="tc-hero-text">
            <h2>陶瓦联机</h2>
            <p>通过房间码与好友 NAT 穿透联机，无需公网 IP 和端口映射。</p>
          </div>
        </div>

        <div class="glass tc-dl-card">
          <div class="tc-dl-title"><IconDownload /> 需要下载陶瓦联机</div>
          <p class="tc-dl-desc">
            未检测到陶瓦联机程序（Terracotta.exe）。点击下方按钮直接下载并自动安装，完成后即可创建或加入房间。
          </p>

          <div v-if="tcDownloading" class="tc-dl-progress">
            <div class="tc-dl-bar">
              <div class="tc-dl-fill" :style="{ width: tcDownloadPercent + '%' }"></div>
            </div>
            <span class="tc-dl-text">{{ tcDownloadText }}</span>
          </div>
          <div v-else class="tc-dl-actions">
            <button class="btn primary" :disabled="tcLoading" @click="downloadTc">
              <IconDownload /> 下载陶瓦联机
            </button>
            <button class="btn ghost" :disabled="tcLoading" @click="refreshTc">
              <IconRefresh /> 重新检测
            </button>
          </div>

          <p class="tc-hint">
            安装后程序通常位于 <code>%LOCALAPPDATA%\Programs\Terracotta\Terracotta.exe</code>
          </p>
        </div>
      </div>

      <!-- 已安装未运行 -->
      <div v-else-if="!tc.running" class="tc-download">
        <div class="tc-hero glass">
          <div v-if="tc.icon" class="tc-hero-icon tc-icon-img">
            <img :src="tc.icon" alt="陶瓦联机" />
          </div>
          <div class="tc-hero-text">
            <h2>陶瓦联机</h2>
            <p>已安装陶瓦联机程序，启动后即可创建或加入房间。</p>
          </div>
        </div>

        <div class="tc-actions">
          <button class="tc-action glass" :disabled="tcBusy" @click="launchTc">
            <div class="tc-action-icon"><IconPlay /></div>
            <div class="tc-action-name">启动陶瓦联机</div>
            <div class="tc-action-desc">启动后台进程，开始创建或加入房间</div>
          </button>
          <button class="tc-action glass" @click="openTcDownload">
            <div class="tc-action-icon"><IconDownload /></div>
            <div class="tc-action-name">检查更新</div>
            <div class="tc-action-desc">前往陶瓦联机发布页查看最新版本</div>
          </button>
        </div>
      </div>

      <!-- 运行中 -->
      <div v-else class="tc-download">
        <div class="tc-hero glass">
          <div v-if="tc.icon" class="tc-hero-icon tc-icon-img">
            <img :src="tc.icon" alt="陶瓦联机" />
          </div>
          <div class="tc-hero-text">
            <h2>陶瓦联机</h2>
            <p class="tc-state">{{ tcStateText }}</p>
          </div>
          <div class="tc-hero-ops">
            <button class="op" @click="openTcUi"><IconExternal /> 打开界面</button>
            <button class="op stop" @click="stopTc"><IconStop /> 停止</button>
          </div>
        </div>

        <!-- 主机：房间已创建 -->
        <div v-if="tcStateName === 'host-ok'" class="glass tc-code-card">
          <div class="tc-code-label"><IconUsers /> 房间已创建，把房间码分享给好友</div>
          <div class="tc-code-value mono">{{ tcRoomCodeFinal }}</div>
          <p class="tc-code-hint">好友在「加入房间」中输入该房间码即可加入</p>
          <div class="tc-code-actions">
            <button class="btn primary" @click="copyRoomCode">
              <IconCopy /> {{ tcJoined ? "已复制" : "复制房间码" }}
            </button>
            <button class="btn ghost" @click="leaveRoom"><IconClose /> 退出房间</button>
          </div>
        </div>

        <!-- 主机：玩家列表 -->
        <div v-if="tcStateName === 'host-ok' && tcPlayers.length" class="glass tc-players-card">
          <div class="tc-players-head">
            <span class="tc-players-title"><IconUsers /> 房间玩家</span>
            <span class="tc-players-count">{{ tcPlayers.length }}</span>
          </div>
          <div class="tc-players-list">
            <div v-for="(p, i) in tcPlayers" :key="i" class="tc-player-row">
              <span class="tc-player-avatar">{{ (playerName(p) || "?").slice(0, 1) }}</span>
              <div class="tc-player-info">
                <div class="tc-player-name">{{ playerName(p) }}</div>
                <div v-if="playerUuid(p)" class="tc-player-uuid">{{ playerUuid(p) }}</div>
              </div>
              <div v-if="playerDesc(p)" class="tc-player-desc">{{ playerDesc(p) }}</div>
              <span v-if="playerPing(p)" class="tc-player-ping">{{ playerPing(p) }}ms</span>
            </div>
          </div>
        </div>

        <!-- 访客：已加入 -->
        <div v-if="tcStateName === 'guest-ok'" class="glass tc-code-card">
          <div class="tc-code-label"><IconCheck /> 已加入房间</div>
          <p class="tc-code-hint">
            在游戏「多人游戏 → 直接连接」中填入地址：
            <b class="mono">{{ tcUrl || "127.0.0.1" }}</b>
          </p>
          <div class="tc-code-actions">
            <button class="btn ghost" @click="leaveRoom"><IconClose /> 退出房间</button>
          </div>
        </div>

        <!-- 访客：玩家列表 -->
        <div
          v-if="tcStateName === 'guest-ok' && tcPlayers.length"
          class="glass tc-players-card"
        >
          <div class="tc-players-head">
            <span class="tc-players-title"><IconUsers /> 房间玩家</span>
            <span class="tc-players-count">{{ tcPlayers.length }}</span>
          </div>
          <div class="tc-players-list">
            <div v-for="(p, i) in tcPlayers" :key="i" class="tc-player-row">
              <span class="tc-player-avatar">{{ (playerName(p) || "?").slice(0, 1) }}</span>
              <div class="tc-player-info">
                <div class="tc-player-name">{{ playerName(p) }}</div>
                <div v-if="playerUuid(p)" class="tc-player-uuid">{{ playerUuid(p) }}</div>
              </div>
              <div v-if="playerDesc(p)" class="tc-player-desc">{{ playerDesc(p) }}</div>
              <span v-if="playerPing(p)" class="tc-player-ping">{{ playerPing(p) }}ms</span>
            </div>
          </div>
        </div>

        <!-- 访客：连接中 -->
        <div
          v-if="tcStateName === 'guest-connecting' || tcStateName === 'guest-starting'"
          class="glass tc-scan-card"
        >
          <div class="tc-scan-head">
            <span class="tc-scan-spinner"></span>
            <div>
              <div class="tc-scan-title">{{ tcStateText }}</div>
              <p class="tc-scan-sub">正在通过陶瓦联机连接房间，请稍候…</p>
            </div>
          </div>
        </div>

        <!-- 扫描中：引导对局域网开放 -->
        <div
          v-if="tcStateName === 'host-scanning' || tcStateName === 'host-starting'"
          class="glass tc-scan-card"
        >
          <div class="tc-scan-head">
            <span class="tc-scan-spinner"></span>
            <div>
              <div class="tc-scan-title">正在扫描局域网世界…</div>
              <p class="tc-scan-sub">陶瓦联机正在等待检测你开放的 Minecraft 局域网世界</p>
            </div>
          </div>
          <div class="tc-scan-steps">
            <div class="tc-scan-step">
              <span class="tc-step-num">1</span>
              <span>启动 Minecraft，进入你想要联机的世界</span>
            </div>
            <div class="tc-scan-step">
              <span class="tc-step-num">2</span>
              <span>按 <b>ESC</b> 打开暂停菜单</span>
            </div>
            <div class="tc-scan-step">
              <span class="tc-step-num">3</span>
              <span>点击「<b>对局域网开放</b>」，端口保持默认即可</span>
            </div>
          </div>
          <p class="tc-scan-tip">
            开放后陶瓦联机会自动检测并完成房间创建，请耐心等待，无需重复操作。
          </p>
        </div>

        <!-- 空闲：创建 / 加入 -->
        <div v-if="tcStateName === 'waiting'" class="glass tc-room-controls">
          <div class="tc-name-input">
            <label class="tc-field-label">玩家名</label>
            <n-input
              v-model:value="tcPlayerName"
              placeholder="默认读取当前账号"
              maxlength="16"
              clearable
            />
          </div>
          <div class="tc-control-grid">
            <div class="tc-col">
              <div class="tc-sub-title"><IconBox /> 创建房间</div>
              <p class="tc-sub-desc">作为主机开放房间，自动生成房间码</p>
              <button class="btn primary" :disabled="tcBusy" @click="createRoom">
                <IconPlus /> 创建房间
              </button>
            </div>
            <div class="tc-divider"></div>
            <div class="tc-col">
              <div class="tc-sub-title"><IconDoorOpen /> 加入房间</div>
              <p class="tc-sub-desc">输入好友分享的房间码加入</p>
              <div class="tc-join-input">
                <n-input
                  v-model:value="tcRoomCode"
                  placeholder="例如 ABCD-EFGH-ABCD-EFGH"
                  @keyup.enter="joinRoom"
                />
                <button class="btn primary" :disabled="tcBusy" @click="joinRoom">
                  <IconPlay /> 加入
                </button>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- 第三方版权标注（AGPL 例外条款要求） -->
      <div class="tc-license">
        由
        <a href="https://github.com/burningtnt/Terracotta" target="_blank" rel="noopener">
          Terracotta
        </a>
        |
        <a href="https://github.com/burningtnt/Terracotta" target="_blank" rel="noopener">
          陶瓦联机
        </a>
        强力驱动
      </div>
    </template>

    <!-- 创建服务器（仅名称 / 核心 / 版本）-->
    <n-modal
      :show="dialog !== null"
      preset="card"
      title="创建服务器"
      style="width: 560px; max-width: 92vw"
      @update:show="(v: boolean) => { if (!v) dialog = null; }"
    >
      <div v-if="dialog" class="dialog-body">
        <div class="field">
          <label>服务器名称</label>
          <n-input v-model:value="dialog.name" placeholder="留空则自动命名" maxlength="40" />
        </div>

        <div class="field">
          <label>服务器核心</label>
          <div class="core-grid">
            <button
              v-for="c in CORES"
              :key="c.value"
              class="core-btn"
              :class="{ active: dialog.core === c.value }"
              @click="dialog.core = c.value"
            >
              <span class="core-name">{{ c.label }}</span>
              <span class="core-desc">{{ c.desc }}</span>
            </button>
          </div>
        </div>

        <div class="field">
          <label>游戏版本</label>
          <div class="ver-cats">
            <button
              v-for="c in [
                { key: 'release', label: '正式版' },
                { key: 'snapshot', label: '快照版' },
                { key: 'april', label: '愚人节版' },
              ]"
              :key="c.key"
              :class="{ active: versionCat === c.key }"
              @click="versionCat = c.key"
            >
              {{ c.label }}
            </button>
          </div>
          <div class="ver-list">
            <button
              v-for="v in filteredVersions"
              :key="v.id"
              class="ver-item"
              :class="{ active: dialog.mc_version === v.id }"
              @click="dialog.mc_version = v.id"
            >
              <span class="ver-id mono">{{ v.id }}</span>
              <span class="ver-type">{{ v.type === "release" || v.type.startsWith("old_") ? "正式" : "快照" }}</span>
            </button>
            <div v-if="!filteredVersions.length" class="ver-empty">该分类下暂无版本</div>
          </div>
        </div>

        <div class="dialog-foot">
          <button class="btn ghost" @click="dialog = null">取消</button>
          <button class="btn primary" :disabled="!canSave || saving" @click="saveDialog">
            <IconPlus /> {{ saving ? "创建中…" : "创建" }}
          </button>
        </div>
      </div>
    </n-modal>

    <!-- 删除确认 -->
    <n-modal
      :show="confirmState !== null"
      preset="card"
      title="删除服务器"
      style="width: 420px; max-width: 92vw"
      @update:show="(v: boolean) => { if (!v) confirmState = null; }"
    >
      <div v-if="confirmState" class="confirm-body">
        <p class="confirm-text">确定要删除服务器「<b>{{ confirmState.name }}</b>」吗？该操作不可撤销。</p>
        <div class="dialog-foot">
          <button class="btn ghost" @click="confirmState = null">取消</button>
          <button class="btn danger" @click="doDelete">删除</button>
        </div>
      </div>
    </n-modal>

    <!-- 核心下载进度 -->
    <n-modal
      :show="installing !== null"
      preset="card"
      title="正在准备服务器"
      style="width: 460px; max-width: 92vw"
      :mask-closable="false"
      :close-on-esc="false"
    >
      <div v-if="installing" class="install-body">
        <div class="install-spin"><IconServer /></div>
        <p class="install-name">{{ installing.name }}</p>
        <p class="install-phase">{{ installing.phase }}</p>
        <div v-if="installHasBytes" class="install-progress">
          <n-progress
            type="line"
            :percentage="installPct"
            :height="8"
            :border-radius="4"
            :show-indicator="false"
            color="#96b5e1"
          />
          <span class="install-bytes">
            {{ fmtBytes(installing.done) }} / {{ fmtBytes(installing.total) }} · {{ installPct }}%
          </span>
        </div>
      </div>
    </n-modal>
  </div>
</template>

<style scoped>
.mp-view {
  display: flex;
  flex-direction: column;
  gap: 18px;
}
.mode-tabs {
  display: inline-flex;
  gap: 4px;
  padding: 5px;
  align-self: flex-start;
}
.mode-tabs button {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: none;
  background: transparent;
  color: var(--text-2);
  padding: 8px 18px;
  border-radius: 9px;
  font-size: 14px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
}
.mode-tabs button.active {
  background: var(--accent-soft);
  color: var(--accent);
}
.btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  border: none;
  border-radius: 10px;
  padding: 9px 18px;
  font-size: 13px;
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
.btn.ghost {
  background: rgba(255, 255, 255, 0.06);
  color: var(--text-1);
  border: 1px solid var(--border);
}
.btn.ghost:hover {
  background: rgba(255, 255, 255, 0.1);
}
.btn.danger {
  background: rgba(229, 83, 75, 0.16);
  color: #f0907f;
  border: 1px solid rgba(229, 83, 75, 0.4);
}
.btn.danger:hover {
  background: rgba(229, 83, 75, 0.26);
}
.btn:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}
.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
  gap: 16px;
}
.server-card {
  padding: 18px;
  display: flex;
  flex-direction: column;
  gap: 14px;
  cursor: pointer;
}
.card-head {
  display: flex;
  align-items: center;
  gap: 10px;
}
.core-badge {
  font-size: 11px;
  font-weight: 700;
  padding: 3px 9px;
  border-radius: 999px;
  border: 1px solid;
  background: rgba(255, 255, 255, 0.04);
  letter-spacing: 0.02em;
  flex-shrink: 0;
}
.card-name {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.card-meta {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px 12px;
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.03);
  border: 1px solid var(--border);
}
.meta-row {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  font-size: 12px;
}
.meta-row span {
  color: var(--text-3);
}
.meta-row b {
  color: var(--text-1);
  font-weight: 600;
}
.card-motd {
  font-size: 12px;
  color: var(--text-2);
  font-style: italic;
  padding: 0 2px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.card-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-top: 2px;
}
.status {
  font-size: 11px;
  font-weight: 600;
  color: var(--text-3);
  padding: 3px 8px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.05);
}
.status.on {
  color: #57c98a;
  background: rgba(87, 201, 138, 0.14);
}
.ops {
  display: flex;
  align-items: center;
  gap: 6px;
}
.op {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  border: 1px solid var(--border);
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-2);
  border-radius: 8px;
  padding: 6px 10px;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  transition: all 0.13s;
}
.op:hover {
  background: rgba(255, 255, 255, 0.09);
  color: var(--text-1);
}
.op.start {
  color: var(--accent);
  border-color: var(--accent-35);
}
.op.start:hover {
  background: var(--accent-soft);
}
.op.stop {
  color: #e0a85a;
  border-color: rgba(224, 168, 90, 0.4);
}
.op.stop:hover {
  background: rgba(224, 168, 90, 0.12);
}
.op.danger:hover {
  color: #e5534b;
  border-color: rgba(229, 83, 75, 0.5);
}
.empty {
  padding: 56px 30px;
  text-align: center;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  color: var(--text-3);
}
.empty-icon {
  font-size: 36px;
  opacity: 0.6;
}
.empty p {
  margin: 0;
  font-size: 14px;
}
.empty .sub {
  font-size: 12px;
  color: var(--text-3);
}
.rooms-empty {
  margin-top: 40px;
}
.dialog-body {
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.field label {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-2);
}
.core-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
  gap: 8px;
}
.core-btn {
  display: flex;
  flex-direction: column;
  gap: 2px;
  align-items: flex-start;
  padding: 9px 12px;
  border-radius: 10px;
  border: 1px solid var(--border);
  background: rgba(255, 255, 255, 0.04);
  color: var(--text-2);
  cursor: pointer;
  font-family: inherit;
  text-align: left;
  transition: all 0.13s;
}
.core-btn:hover {
  background: rgba(255, 255, 255, 0.08);
}
.core-btn.active {
  background: var(--accent-soft);
  border-color: var(--accent-45);
  color: var(--accent);
}
.core-name {
  font-size: 13px;
  font-weight: 700;
}
.core-desc {
  font-size: 11px;
  color: var(--text-3);
}
.core-btn.active .core-desc {
  color: var(--accent-60);
}
.ver-cats {
  display: flex;
  gap: 4px;
}
.ver-cats button {
  border: none;
  background: rgba(255, 255, 255, 0.05);
  color: var(--text-2);
  padding: 7px 16px;
  border-radius: 8px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
}
.ver-cats button.active {
  background: var(--accent-soft);
  color: var(--accent);
}
.ver-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 240px;
  overflow-y: auto;
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 4px;
}
.ver-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 8px;
  border: 1px solid transparent;
  background: transparent;
  color: var(--text-2);
  padding: 7px 10px;
  border-radius: 8px;
  cursor: pointer;
  font-family: inherit;
  text-align: left;
}
.ver-item:hover {
  background: rgba(255, 255, 255, 0.05);
}
.ver-item.active {
  border-color: var(--accent-05);
  background: var(--accent-soft);
  color: var(--accent);
}
.ver-id {
  font-size: 12px;
  font-weight: 600;
}
.ver-type {
  font-size: 10px;
  color: var(--text-3);
}
.ver-empty {
  text-align: center;
  color: var(--text-3);
  font-size: 13px;
  padding: 16px 0;
}
.dialog-foot {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}
.confirm-body {
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.confirm-text {
  margin: 0;
  font-size: 14px;
  color: var(--text-2);
  line-height: 1.6;
}
.confirm-text b {
  color: var(--text-1);
}
.install-body {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  padding: 18px 0 8px;
}
.install-spin {
  width: 48px;
  height: 48px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  background: var(--accent-soft);
  color: var(--accent);
  font-size: 24px;
  animation: spin 1.1s linear infinite;
}
@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
.install-name {
  margin: 0;
  font-size: 15px;
  font-weight: 600;
  color: var(--text-1);
}
.install-phase {
  margin: 0;
  font-size: 13px;
  color: var(--text-3);
}
.install-progress {
  width: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  margin-top: 2px;
}
.install-bytes {
  font-size: 12px;
  color: var(--text-3);
  font-variant-numeric: tabular-nums;
}

/* ---- 陶瓦联机 ---- */
.tc-download {
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.tc-hero {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 20px 24px;
}
.tc-hero-icon {
  width: 52px;
  height: 52px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 14px;
  background: var(--accent-soft);
  color: var(--accent);
  font-size: 26px;
  overflow: hidden;
}
.tc-icon-img img {
  width: 100%;
  height: 100%;
  object-fit: contain;
  border-radius: 10px;
}
.tc-hero-text {
  flex: 1;
  min-width: 0;
}
.tc-hero-text h2 {
  margin: 0 0 4px;
  font-size: 17px;
}
.tc-hero-text p {
  margin: 0;
  font-size: 12px;
  color: var(--text-3);
  line-height: 1.5;
}
.tc-state {
  font-weight: 600;
  color: var(--accent) !important;
}
.tc-hero-ops {
  display: flex;
  gap: 8px;
}
.tc-dl-card {
  display: flex;
  flex-direction: column;
  gap: 16px;
  padding: 20px 24px;
}
.tc-dl-title {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 15px;
  font-weight: 700;
  color: var(--text-1);
}
.tc-dl-desc {
  margin: 0;
  font-size: 13px;
  color: var(--text-2);
  line-height: 1.6;
}
.tc-dl-actions {
  display: flex;
  gap: 10px;
  flex-wrap: wrap;
}
.tc-dl-progress {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.tc-dl-bar {
  height: 12px;
  border-radius: 6px;
  background: rgba(255, 255, 255, 0.07);
  border: 1px solid var(--border);
  overflow: hidden;
}
.tc-dl-fill {
  height: 100%;
  border-radius: 6px;
  background: linear-gradient(90deg, var(--accent), var(--accent-2, var(--accent)));
  transition: width 0.2s ease;
}
.tc-dl-text {
  font-size: 12px;
  color: var(--text-2);
}
.tc-hint {
  margin: 0;
  font-size: 12px;
  color: var(--text-3);
}
.tc-hint code {
  padding: 2px 6px;
  border-radius: 6px;
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid var(--border);
  font-size: 11px;
}
.tc-license {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  position: fixed;
  left: 50%;
  transform: translateX(-50%);
  bottom: 12px;
  z-index: 40;
  padding: 5px 14px;
  border-radius: 999px;
  background: var(--panel);
  border: 1px solid var(--border);
  backdrop-filter: blur(var(--glass-blur, 8px));
  font-size: 11px;
  color: var(--text-3);
  text-align: center;
  white-space: nowrap;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.2);
}
.tc-license a {
  color: var(--text-2);
  text-decoration: none;
}
.tc-license a:hover {
  color: var(--accent);
  text-decoration: underline;
}
.tc-actions {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));
  gap: 14px;
  margin-top: 4px;
}
.tc-action {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 6px;
  padding: 20px;
  text-align: left;
  color: var(--text-1);
  border: 1px solid var(--border);
  border-radius: 14px;
  background: var(--panel);
  cursor: pointer;
  font-family: inherit;
  transition: all 0.15s;
}
.tc-action:hover:not(:disabled) {
  background: var(--panel-hover);
  border-color: var(--accent-45);
  transform: translateY(-1px);
}
.tc-action:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}
.tc-action-icon {
  font-size: 22px;
  color: var(--accent);
}
.tc-action-name {
  font-size: 15px;
  font-weight: 700;
}
.tc-action-desc {
  font-size: 12px;
  color: var(--text-3);
}
.tc-code-card {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
  padding: 20px 24px;
  text-align: center;
}
.tc-code-label {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-1);
}
.tc-code-value {
  font-size: 30px;
  font-weight: 700;
  letter-spacing: 0.12em;
  color: var(--accent);
  background: var(--accent-soft);
  padding: 12px 22px;
  border-radius: 14px;
}
.tc-code-hint {
  margin: 0;
  font-size: 12px;
  color: var(--text-3);
}
.tc-code-hint .mono {
  color: var(--text-1);
}
.tc-players-card {
  padding: 18px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.tc-players-head {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-1);
}
.tc-players-count {
  min-width: 20px;
  height: 20px;
  padding: 0 6px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 10px;
  font-size: 12px;
  font-weight: 600;
  color: var(--accent);
  background: var(--accent-soft);
}
.tc-players-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.tc-player-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--panel);
}
.tc-player-avatar {
  width: 32px;
  height: 32px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 8px;
  font-weight: 600;
  color: var(--accent);
  background: var(--accent-soft);
}
.tc-player-info {
  flex: 1;
  min-width: 0;
}
.tc-player-name {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tc-player-uuid {
  font-size: 11px;
  color: var(--text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.tc-player-desc {
  flex-shrink: 0;
  font-size: 12px;
  color: var(--text-2);
}
.tc-player-ping {
  flex-shrink: 0;
  font-size: 12px;
  font-weight: 500;
  color: var(--accent);
  background: var(--accent-soft);
  padding: 2px 8px;
  border-radius: 10px;
}
.tc-scan-card {
  display: flex;
  flex-direction: column;
  gap: 16px;
  padding: 24px;
}
.tc-scan-head {
  display: flex;
  align-items: center;
  gap: 14px;
}
.tc-scan-spinner {
  width: 22px;
  height: 22px;
  flex-shrink: 0;
  border: 3px solid var(--accent-soft);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: tc-spin 0.8s linear infinite;
}
@keyframes tc-spin {
  to {
    transform: rotate(360deg);
  }
}
.tc-scan-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-1);
}
.tc-scan-sub {
  margin: 2px 0 0;
  font-size: 12px;
  color: var(--text-3);
}
.tc-scan-steps {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 14px;
  border: 1px solid var(--border);
  border-radius: 12px;
  background: var(--panel);
}
.tc-scan-step {
  display: flex;
  align-items: center;
  gap: 10px;
  font-size: 13px;
  color: var(--text-2);
}
.tc-step-num {
  width: 20px;
  height: 20px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  font-size: 12px;
  font-weight: 600;
  color: var(--accent);
  background: var(--accent-soft);
}
.tc-scan-step b {
  color: var(--text-1);
}
.tc-scan-tip {
  margin: 0;
  font-size: 12px;
  color: var(--text-3);
}
.tc-code-actions {
  display: flex;
  gap: 10px;
  flex-wrap: wrap;
  justify-content: center;
}
.tc-room-controls {
  padding: 20px 24px;
}
.tc-room-controls > .tc-name-input {
  margin-bottom: 18px;
  max-width: 320px;
}
.tc-control-grid {
  display: grid;
  grid-template-columns: 1fr 1px 1fr;
  gap: 20px;
  align-items: stretch;
}
.tc-col {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 8px;
}
.tc-sub-title {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 14px;
  font-weight: 700;
  color: var(--text-1);
}
.tc-sub-desc {
  margin: 0 0 4px;
  font-size: 12px;
  color: var(--text-3);
}
.tc-join-input {
  display: flex;
  gap: 8px;
  width: 100%;
}
.tc-join-input :deep(.n-input) {
  flex: 1;
}
.tc-name-input {
  width: 100%;
}
.tc-field-label {
  display: block;
  margin-bottom: 4px;
  font-size: 12px;
  color: var(--text-3);
}
.tc-divider {
  width: 1px;
  background: var(--border);
}
</style>
