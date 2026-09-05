<script setup lang="ts">
/**
 * 实例详情 · 世界 tab（单人存档 + 多人服务器）。
 * 从 InstanceDetailView 拆出，自行负责：存档列表加载、服务器列表与 ping、
 * MOTD 彩色解析、从存档/服务器直接启动、固定到首页。
 */
import { onMounted, ref, watch } from "vue";
import { useInstancesStore } from "../../stores/instances";
import { useAccountsStore } from "../../stores/accounts";
import { usePinsStore } from "../../stores/pins";
import { useMessage } from "naive-ui";
import { convertFileSrc } from "@tauri-apps/api/core";
import { api } from "../../api";
import { supportsQuickPlay } from "../../version";
import { fmtDateLocale as fmtDate, latencyInfo } from "../../utils/format";
import {
  IconFolder,
  IconGlobe,
  IconMapPin,
  IconPlay,
  IconRefresh,
} from "../icons";
import type { ServerEntry, ServerStatus } from "../../types";

const props = defineProps<{ instanceId: string }>();

const instances = useInstancesStore();
const accounts = useAccountsStore();
const message = useMessage();
const pins = usePinsStore();

function assetUrl(p: string) {
  return convertFileSrc(p);
}

const worldSub = ref<"sp" | "mp">("sp");
const servers = ref<ServerEntry[]>([]);
const serverStatus = ref<Record<string, ServerStatus>>({});
const pinging = ref<Set<string>>(new Set());
const loadingServers = ref(false);

async function loadServers() {
  loadingServers.value = true;
  try {
    servers.value = await api.listServers(props.instanceId);
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

// ---- 单人存档列表 ----
const fileItems = ref<
  { name: string; size: number; modified: number; isDir: boolean; path: string; icon: string | null }[]
>([]);
const loadingFiles = ref(false);
const launchingWorld = ref("");

async function loadFiles() {
  const seq = ++loadSeqFiles;
  loadingFiles.value = true;
  try {
    const r = await api.listInstanceFiles(props.instanceId, "saves");
    if (seq !== loadSeqFiles) return;
    fileItems.value = r.files;
  } catch (e) {
    if (seq !== loadSeqFiles) return;
    message.error(String(e));
  } finally {
    if (seq === loadSeqFiles) loadingFiles.value = false;
  }
}
let loadSeqFiles = 0;

async function launchWorld(name: string) {
  const i = instances.get(props.instanceId);
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
  const i = instances.get(props.instanceId);
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

// —— 固定到首页（存档 / 服务器只会出现在首页）——
function worldPinId(name: string) {
  return pins.makeId("world", props.instanceId, name, "home");
}
function serverPinId(address: string) {
  return pins.makeId("server", props.instanceId, address, "home");
}
function toggleWorldPin(w: { name: string; icon: string | null }) {
  const i = instances.get(props.instanceId);
  if (!i) return;
  pins.toggle({
    id: worldPinId(w.name),
    type: "world",
    target: "home",
    instanceId: props.instanceId,
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
  const i = instances.get(props.instanceId);
  if (!i) return;
  pins.toggle({
    id: serverPinId(entry.address),
    type: "server",
    target: "home",
    instanceId: props.instanceId,
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

onMounted(() => {
  loadFiles();
});

// 实例安装完成后重新加载存档
watch(
  () => instances.get(props.instanceId)?.installed,
  () => loadFiles()
);
</script>

<template>
  <div>
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
        <p class="hint">安装游戏后创建的世界会出现在这里</p>
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
  </div>
</template>

<style scoped>
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
  border-color: var(--accent-05);
}
.content-list {
  padding: 18px;
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
.c-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}
.ver {
  color: var(--text-3);
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
.mini-btn.play {
  color: var(--accent);
  border-color: var(--accent-04);
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
  border-color: var(--accent-04);
  background: var(--accent-soft);
}
.center {
  padding: 60px;
  text-align: center;
  color: var(--text-3);
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
.hint {
  font-size: 12px;
  color: var(--text-3);
  margin-top: 4px;
}
</style>
