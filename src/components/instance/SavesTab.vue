<script setup lang="ts">
/**
 * 实例详情 · 世界 tab（单人存档 + 多人服务器）。
 * 从 InstanceDetailView 拆出，自行负责：存档列表加载、服务器列表与 ping、
 * MOTD 彩色解析、从存档/服务器直接启动、固定到首页。
 */
import { onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useInstancesStore } from "../../stores/instances";
import { useAccountsStore } from "../../stores/accounts";
import { usePinsStore } from "../../stores/pins";
import { NButton, NModal, useMessage } from "naive-ui";
import { convertFileSrc } from "@tauri-apps/api/core";
import { api } from "../../api";
import { supportsQuickPlay } from "../../version";
import { fmtDateLocale as fmtDate, fmtSize, latencyInfo } from "../../utils/format";
import {
  IconBox,
  IconCloud,
  IconFolder,
  IconGlobe,
  IconMapPin,
  IconPlay,
  IconRefresh,
  IconTrash,
} from "../icons";
import CloudSyncDialog from "../CloudSyncDialog.vue";
import type { ServerEntry, ServerStatus, WorldBackupInfo } from "../../types";

const props = defineProps<{ instanceId: string }>();

const { t } = useI18n();
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
    message.warning(t("instanceSaves.addAccountFirst"));
    accounts.showManager = true;
    return;
  }
  launchingWorld.value = name;
  if (!supportsQuickPlay(i.mc_version)) {
    message.info(t("instanceSaves.quickPlayUnsupported", { version: i.mc_version }));
  }
  try {
    await instances.launch(i.id, name);
    message.success(t("instanceSaves.enteringWorld", { name }));
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
    message.warning(t("instanceSaves.addAccountFirst"));
    accounts.showManager = true;
    return;
  }
  launchingServer.value = entry.address;
  try {
    await instances.launch(i.id, undefined, entry.address);
    message.success(t("instanceSaves.joiningServer", { name: entry.name || entry.address }));
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

// ---- 存档备份 ----
const backupOpen = ref(false);
const backupWorld = ref("");
const backups = ref<WorldBackupInfo[]>([]);
const loadingBackups = ref(false);
const backingUp = ref(false);
const restoring = ref("");

async function loadBackups(world: string) {
  loadingBackups.value = true;
  try {
    backups.value = await api.listWorldBackups(props.instanceId, world);
  } catch (e) {
    backups.value = [];
    message.error(String(e));
  } finally {
    loadingBackups.value = false;
  }
}

function openBackups(world: string) {
  backupWorld.value = world;
  backupOpen.value = true;
  loadBackups(world);
}

async function createBackup() {
  backingUp.value = true;
  try {
    await api.createWorldBackup(props.instanceId, backupWorld.value);
    message.success(t("instanceSaves.backupDone"));
    await loadBackups(backupWorld.value);
  } catch (e) {
    message.error(String(e));
  } finally {
    backingUp.value = false;
  }
}

async function restoreBackup(filename: string) {
  restoring.value = filename;
  try {
    // 恢复前後端会自动为当前存档留一份安全快照，不会丢档
    await api.restoreWorldBackup(props.instanceId, backupWorld.value, filename);
    message.success(t("instanceSaves.restored"));
    await loadBackups(backupWorld.value);
    await loadFiles();
  } catch (e) {
    message.error(String(e));
  } finally {
    restoring.value = "";
  }
}

async function deleteBackup(filename: string) {
  try {
    await api.deleteWorldBackup(props.instanceId, backupWorld.value, filename);
    await loadBackups(backupWorld.value);
  } catch (e) {
    message.error(String(e));
  }
}

// ---- 云同步 ----
const cloudOpen = ref(false);
const cloudWorld = ref("");

function openCloudSync(world: string) {
  cloudWorld.value = world;
  cloudOpen.value = true;
}
// 从云端恢复（不指定世界 → 浏览模式，可找回本地已丢失的世界）
function openCloudBrowse() {
  openCloudSync("");
}

async function onCloudRestored() {
  await loadFiles();
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
        <IconFolder /> {{ t("instanceSaves.singleplayer") }}
      </button>
      <button class="seg" :class="{ active: worldSub === 'mp' }" @click="selectWorldSub('mp')">
        <IconGlobe /> {{ t("instanceSaves.multiplayer") }}
      </button>
    </div>

    <!-- 单人游戏：本地世界存档 -->
    <template v-if="worldSub === 'sp'">
      <div class="sp-toolbar">
        <button class="mini-btn" :title="t('instanceSaves.cloudBrowseTitle')" @click="openCloudBrowse">
          <IconCloud /> {{ t("instanceSaves.cloudRestore") }}
        </button>
      </div>
      <div v-if="loadingFiles" class="center">{{ t("instanceSaves.loading") }}</div>
      <div v-else-if="!fileItems.length" class="empty glass">
        <p>{{ t("instanceSaves.noWorlds") }}</p>
        <p class="hint">{{ t("instanceSaves.noWorldsHint") }}</p>
        <p class="hint">{{ t("instanceSaves.noWorldsCloudHint") }}</p>
      </div>
      <div v-else class="content-list glass">
        <div v-for="f in fileItems.filter((x) => x.isDir)" :key="f.name" class="world-row">
          <img v-if="f.icon" :src="assetUrl(f.icon)" class="world-icon" alt="" />
          <div v-else class="world-icon ph"><IconFolder /></div>
          <div class="c-info">
            <div class="c-name text-ellipsis">{{ f.name }}</div>
            <div class="c-meta">
              <span class="ver">{{ t("instanceSaves.worldSave") }}</span>
              <span v-if="f.modified" class="ver">{{ fmtDate(f.modified) }}</span>
            </div>
          </div>
          <div class="c-actions">
            <button
              class="mini-btn"
              :title="t('instanceSaves.backupRestoreTitle')"
              @click="openBackups(f.name)"
            >
              <IconBox /> {{ t("instanceSaves.backup") }}
            </button>
            <button
              class="mini-btn"
              :title="t('instanceSaves.cloudSyncTitle')"
              @click="openCloudSync(f.name)"
            >
              <IconCloud /> {{ t("instanceSaves.cloudSync") }}
            </button>
            <button
              class="mini-btn pin"
              :class="{ active: pins.isPinned(worldPinId(f.name)) }"
              :title="pins.isPinned(worldPinId(f.name)) ? t('instanceSaves.unpin') : t('instanceSaves.pinToHome')"
              @click="toggleWorldPin(f)"
            >
              <IconMapPin />
            </button>
            <button
              class="mini-btn play"
              :disabled="!!launchingWorld"
              @click="launchWorld(f.name)"
            >
              <IconPlay /> {{ launchingWorld === f.name ? t("instanceSaves.launching") : t("instanceSaves.launch") }}
            </button>
          </div>
        </div>
        <div v-if="!fileItems.some((x) => x.isDir)" class="center">{{ t("instanceSaves.noWorldsInInstance") }}</div>
      </div>
    </template>

    <!-- 多人游戏：服务器列表 -->
    <template v-else>
      <div v-if="loadingServers" class="center">{{ t("instanceSaves.loading") }}</div>
      <div v-else-if="!servers.length" class="empty glass">
        <p>{{ t("instanceSaves.noServers") }}</p>
        <p class="hint">{{ t("instanceSaves.noServersHint") }}</p>
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
                <span v-else-if="serverStatus[s.address] && !serverStatus[s.address]?.online">{{ t("instanceSaves.offline") }}</span>
                <span v-else>…</span>
              </span>
              <span v-if="serverStatus[s.address]?.players_online != null" class="players">
                {{ serverStatus[s.address]?.players_online }}<template v-if="serverStatus[s.address]?.players_max != null">/{{ serverStatus[s.address]?.players_max }}</template> {{ t("instanceSaves.playersOnlineUnit") }}
              </span>
              <span v-else-if="serverStatus[s.address]?.error" class="err" :title="serverStatus[s.address]?.error ?? undefined">{{ t("instanceSaves.cannotConnect") }}</span>
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
              :title="pins.isPinned(serverPinId(s.address)) ? t('instanceSaves.unpin') : t('instanceSaves.pinToHome')"
              @click="toggleServerPin(s)"
            >
              <IconMapPin />
            </button>
            <button
              class="mini-btn play"
              :disabled="!!launchingServer || pinging.has(s.address)"
              :title="launchingServer === s.address ? t('instanceSaves.launching') : t('instanceSaves.launchAndJoin')"
              @click="launchServer(s)"
            >
              <IconPlay /> {{ launchingServer === s.address ? t("instanceSaves.launching") : t("instanceSaves.launch") }}
            </button>
            <button class="mini-btn" :disabled="pinging.has(s.address) || !!launchingServer" @click="pingOne(s.address)">
              <IconRefresh /> {{ pinging.has(s.address) ? t("instanceSaves.testing") : t("instanceSaves.refresh") }}
            </button>
          </div>
        </div>
      </div>
    </template>

    <!-- 备份管理弹窗 -->
    <n-modal
      v-model:show="backupOpen"
      preset="card"
      :title="t('instanceSaves.backupTitle', { name: backupWorld })"
      style="width: 560px; max-width: 94vw"
      :mask-closable="true"
      :close-on-esc="true"
    >
      <div class="bk-body">
        <div class="bk-toolbar">
          <span class="hint">{{ t("instanceSaves.backupHint") }}</span>
          <n-button size="small" type="primary" :loading="backingUp" @click="createBackup">
            {{ t("instanceSaves.createBackup") }}
          </n-button>
        </div>
        <div v-if="loadingBackups" class="center">{{ t("instanceSaves.loading") }}</div>
        <div v-else-if="!backups.length" class="center">{{ t("instanceSaves.noBackups") }}</div>
        <div v-else class="bk-list">
          <div v-for="b in backups" :key="b.filename" class="bk-row">
            <div class="c-info">
              <div class="c-name">{{ fmtDate(b.modified) }}</div>
              <div class="c-meta"><span class="ver">{{ fmtSize(b.size) }}</span></div>
            </div>
            <div class="c-actions">
              <n-button
                size="small"
                type="warning"
                :loading="restoring === b.filename"
                @click="restoreBackup(b.filename)"
              >
                {{ t("instanceSaves.restore") }}
              </n-button>
              <button class="bk-del" :title="t('instanceSaves.deleteBackup')" @click="deleteBackup(b.filename)">
                <IconTrash />
              </button>
            </div>
          </div>
        </div>
      </div>
    </n-modal>

    <!-- 云存档同步弹窗 -->
    <CloudSyncDialog
      v-model:show="cloudOpen"
      :instance-id="instanceId"
      :world="cloudWorld"
      :game-version="instances.get(instanceId)?.mc_version ?? ''"
      @restored="onCloudRestored"
    />
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
  background: var(--w-05);
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
  background: var(--w-05);
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
  background: var(--w-04);
  color: var(--text-2);
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  transition: all 0.12s;
}
.mini-btn:hover {
  background: var(--w-08);
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
.sp-toolbar {
  display: flex;
  justify-content: flex-end;
  margin-bottom: 10px;
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
.bk-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.bk-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.bk-list {
  display: flex;
  flex-direction: column;
  max-height: 320px;
  overflow-y: auto;
}
.bk-row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 6px;
  border-bottom: 1px solid var(--border);
}
.bk-row:last-child {
  border-bottom: none;
}
.bk-row .c-name {
  font-size: 13px;
  font-weight: 600;
}
.bk-del {
  width: 30px;
  height: 30px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: transparent;
  color: var(--text-2);
  display: inline-flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  font-size: 13px;
  transition: all 0.12s;
}
.bk-del:hover {
  color: #e5534b;
  border-color: var(--danger-50);
}
</style>
