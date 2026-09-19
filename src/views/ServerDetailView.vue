<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch, nextTick } from "vue";
import { useI18n } from "vue-i18n";
import { fmtSize } from "../utils/format";
import { useRoute, useRouter } from "vue-router";
import { NInput, NInputNumber, NCheckbox, NSwitch, NSelect, NModal, useMessage } from "naive-ui";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import { useServersStore } from "../stores/servers";
import ServerFileManager from "../components/ServerFileManager.vue";
import { CORE_COLORS, CORE_LABELS } from "../utils/cores";
import {
  IconBox,
  IconChevronLeft,
  IconCopy,
  IconDownload,
  IconFile,
  IconFolder,
  IconPlay,
  IconRefresh,
  IconStop,
  IconTrash,
} from "../components/icons";

const route = useRoute();
const router = useRouter();
const { t } = useI18n();
const servers = useServersStore();
const message = useMessage();

const serverId = route.params.id as string;

const server = computed(() => servers.byId(serverId));
const running = computed(() => servers.isRunning(serverId));

// ---- tabs ----
const tab = ref<string>("logs");
const folders = ref<Record<string, boolean>>({});
const ALL_TABS = [
  { key: "logs", label: "serverDetail.tab.logs" },
  { key: "settings", label: "serverDetail.tab.settings" },
  { key: "config", label: "serverDetail.tab.config" },
  { key: "mods", label: "serverDetail.tab.mods", folder: "mods" },
  { key: "plugins", label: "serverDetail.tab.plugins", folder: "plugins" },
  { key: "files", label: "serverDetail.tab.files" },
];
const tabs = computed(() =>
  ALL_TABS.filter((tb) => !tb.folder || folders.value[tb.folder] || tb.key === tab.value),
);
watch(tabs, (ts) => {
  if (!ts.some((tb) => tb.key === tab.value) && ts.length > 0) tab.value = ts[0].key;
});

// ---- 启动设置表单 ----
const form = ref({
  name: "",
  maxMem: 2048,
  minMem: 1024,
  eula: false,
  javaPath: "",
  jvmArgs: "",
  stopCommand: "",
});
const saving = ref(false);
const javaCandidates = ref<{ path: string; version: string; major: number }[]>([]);

function syncForm() {
  const s = server.value;
  if (!s) return;
  form.value = {
    name: s.name,
    maxMem: s.max_memory_mb,
    minMem: s.min_memory_mb,
    eula: s.eula,
    javaPath: s.java_path ?? "",
    jvmArgs: s.jvm_args ?? "",
    stopCommand: s.stop_command ?? "",
  };
}
watch(server, syncForm, { immediate: true });

async function loadJavaCandidates() {
  try {
    const { useSettingsStore } = await import("../stores/settings");
    javaCandidates.value = await useSettingsStore().loadJava();
  } catch {
    /* ignore */
  }
}

async function pickJava() {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const file = await open({
    multiple: false,
    filters: [{ name: "Java", extensions: ["exe"] }],
    directory: false,
  });
  if (file) form.value.javaPath = file as string;
}

async function saveConfig() {
  saving.value = true;
  try {
    await servers.update({
      id: serverId,
      name: form.value.name,
      max_memory_mb: form.value.maxMem,
      min_memory_mb: form.value.minMem,
      eula: form.value.eula,
      java_path: form.value.javaPath,
      jvm_args: form.value.jvmArgs,
      stop_command: form.value.stopCommand,
    });
  } catch (e) {
    throw e;
  } finally {
    saving.value = false;
  }
}

// ---- 自动保存：修改任一设置字段即自动更新 ----
let autoSaveTimer: number | null = null;
let autoSaving = false;
function scheduleAutoSave() {
  if (autoSaveTimer !== null) window.clearTimeout(autoSaveTimer);
  autoSaveTimer = window.setTimeout(runAutoSave, 700);
}
async function runAutoSave() {
  autoSaveTimer = null;
  if (autoSaving || !server.value) return;
  autoSaving = true;
  try {
    await saveConfig();
  } catch (e) {
    message.error(String(e));
  } finally {
    autoSaving = false;
  }
}
onBeforeUnmount(() => {
  if (autoSaveTimer !== null) {
    window.clearTimeout(autoSaveTimer);
    autoSaveTimer = null;
  }
});

// ---- server.properties 配置项映射 ----
type PropField = {
  key: string;
  label: string;
  desc: string;
  type: "bool" | "int" | "string" | "enum";
  options?: string[];
  min?: number;
  max?: number;
  group: string;
};

const PROPS_SCHEMA: PropField[] = [
  { key: "server-port", label: "serverDetail.props.serverPortLabel", desc: "serverDetail.props.serverPortDesc", type: "int", min: 1, max: 65535, group: "basic" },
  { key: "max-players", label: "serverDetail.props.maxPlayersLabel", desc: "serverDetail.props.maxPlayersDesc", type: "int", min: 0, group: "basic" },
  { key: "motd", label: "serverDetail.props.motdLabel", desc: "serverDetail.props.motdDesc", type: "string", group: "basic" },
  { key: "player-idle-timeout", label: "serverDetail.props.playerIdleTimeoutLabel", desc: "serverDetail.props.playerIdleTimeoutDesc", type: "int", min: 0, group: "basic" },
  { key: "gamemode", label: "serverDetail.props.gamemodeLabel", desc: "serverDetail.props.gamemodeDesc", type: "enum", options: ["survival", "creative", "adventure", "spectator"], group: "gameplay" },
  { key: "difficulty", label: "serverDetail.props.difficultyLabel", desc: "serverDetail.props.difficultyDesc", type: "enum", options: ["peaceful", "easy", "normal", "hard"], group: "gameplay" },
  { key: "pvp", label: "serverDetail.props.pvpLabel", desc: "serverDetail.props.pvpDesc", type: "bool", group: "gameplay" },
  { key: "hardcore", label: "serverDetail.props.hardcoreLabel", desc: "serverDetail.props.hardcoreDesc", type: "bool", group: "gameplay" },
  { key: "force-gamemode", label: "serverDetail.props.forceGamemodeLabel", desc: "serverDetail.props.forceGamemodeDesc", type: "bool", group: "gameplay" },
  { key: "allow-flight", label: "serverDetail.props.allowFlightLabel", desc: "serverDetail.props.allowFlightDesc", type: "bool", group: "gameplay" },
  { key: "enable-command-block", label: "serverDetail.props.enableCommandBlockLabel", desc: "serverDetail.props.enableCommandBlockDesc", type: "bool", group: "gameplay" },
  { key: "level-name", label: "serverDetail.props.levelNameLabel", desc: "serverDetail.props.levelNameDesc", type: "string", group: "world" },
  { key: "level-seed", label: "serverDetail.props.levelSeedLabel", desc: "serverDetail.props.levelSeedDesc", type: "string", group: "world" },
  { key: "level-type", label: "serverDetail.props.levelTypeLabel", desc: "serverDetail.props.levelTypeDesc", type: "enum", options: ["minecraft:normal", "minecraft:flat", "minecraft:large_biomes", "minecraft:amplified"], group: "world" },
  { key: "generate-structures", label: "serverDetail.props.generateStructuresLabel", desc: "serverDetail.props.generateStructuresDesc", type: "bool", group: "world" },
  { key: "allow-nether", label: "serverDetail.props.allowNetherLabel", desc: "serverDetail.props.allowNetherDesc", type: "bool", group: "world" },
  { key: "spawn-animals", label: "serverDetail.props.spawnAnimalsLabel", desc: "serverDetail.props.spawnAnimalsDesc", type: "bool", group: "world" },
  { key: "spawn-monsters", label: "serverDetail.props.spawnMonstersLabel", desc: "serverDetail.props.spawnMonstersDesc", type: "bool", group: "world" },
  { key: "spawn-npcs", label: "serverDetail.props.spawnNpcsLabel", desc: "serverDetail.props.spawnNpcsDesc", type: "bool", group: "world" },
  { key: "max-world-size", label: "serverDetail.props.maxWorldSizeLabel", desc: "serverDetail.props.maxWorldSizeDesc", type: "int", min: 0, group: "world" },
  { key: "online-mode", label: "serverDetail.props.onlineModeLabel", desc: "serverDetail.props.onlineModeDesc", type: "bool", group: "security" },
  { key: "white-list", label: "serverDetail.props.whiteListLabel", desc: "serverDetail.props.whiteListDesc", type: "bool", group: "security" },
  { key: "enforce-secure-profile", label: "serverDetail.props.enforceSecureProfileLabel", desc: "serverDetail.props.enforceSecureProfileDesc", type: "bool", group: "security" },
  { key: "prevent-proxy-connections", label: "serverDetail.props.preventProxyConnectionsLabel", desc: "serverDetail.props.preventProxyConnectionsDesc", type: "bool", group: "security" },
  { key: "view-distance", label: "serverDetail.props.viewDistanceLabel", desc: "serverDetail.props.viewDistanceDesc", type: "int", min: 3, max: 32, group: "security" },
  { key: "simulation-distance", label: "serverDetail.props.simulationDistanceLabel", desc: "serverDetail.props.simulationDistanceDesc", type: "int", min: 3, max: 32, group: "performance" },
  { key: "network-compression-threshold", label: "serverDetail.props.networkCompressionThresholdLabel", desc: "serverDetail.props.networkCompressionThresholdDesc", type: "int", group: "performance" },
  { key: "max-tick-time", label: "serverDetail.props.maxTickTimeLabel", desc: "serverDetail.props.maxTickTimeDesc", type: "int", group: "performance" },
  { key: "use-native-transport", label: "serverDetail.props.useNativeTransportLabel", desc: "serverDetail.props.useNativeTransportDesc", type: "bool", group: "performance" },
  { key: "sync-chunk-writes", label: "serverDetail.props.syncChunkWritesLabel", desc: "serverDetail.props.syncChunkWritesDesc", type: "bool", group: "performance" },
  { key: "entity-broadcast-range-percentage", label: "serverDetail.props.entityBroadcastRangePercentageLabel", desc: "serverDetail.props.entityBroadcastRangePercentageDesc", type: "int", min: 0, max: 100, group: "performance" },
  { key: "enable-jmx-monitoring", label: "serverDetail.props.enableJmxMonitoringLabel", desc: "serverDetail.props.enableJmxMonitoringDesc", type: "bool", group: "performance" },
  { key: "enable-rcon", label: "serverDetail.props.enableRconLabel", desc: "serverDetail.props.enableRconDesc", type: "bool", group: "remote" },
  { key: "rcon.port", label: "serverDetail.props.rconPortLabel", desc: "serverDetail.props.rconPortDesc", type: "int", min: 1, max: 65535, group: "remote" },
  { key: "rcon.password", label: "serverDetail.props.rconPasswordLabel", desc: "serverDetail.props.rconPasswordDesc", type: "string", group: "remote" },
  { key: "enable-query", label: "serverDetail.props.enableQueryLabel", desc: "serverDetail.props.enableQueryDesc", type: "bool", group: "remote" },
  { key: "query.port", label: "serverDetail.props.queryPortLabel", desc: "serverDetail.props.queryPortDesc", type: "int", min: 1, max: 65535, group: "remote" },
  { key: "resource-pack", label: "serverDetail.props.resourcePackLabel", desc: "serverDetail.props.resourcePackDesc", type: "string", group: "resourcePack" },
  { key: "resource-pack-sha1", label: "serverDetail.props.resourcePackSha1Label", desc: "serverDetail.props.resourcePackSha1Desc", type: "string", group: "resourcePack" },
  { key: "require-resource-pack", label: "serverDetail.props.requireResourcePackLabel", desc: "serverDetail.props.requireResourcePackDesc", type: "bool", group: "resourcePack" },
];

const PROPS_DEFAULTS: Record<string, string> = {
  "server-port": "25565", "max-players": "20", "motd": "A Minecraft Server", "player-idle-timeout": "0",
  "gamemode": "survival", "difficulty": "easy", "pvp": "true", "hardcore": "false",
  "force-gamemode": "false", "allow-flight": "false", "enable-command-block": "false",
  "level-name": "world", "level-seed": "", "level-type": "minecraft:normal",
  "generate-structures": "true", "allow-nether": "true", "spawn-animals": "true",
  "spawn-monsters": "true", "spawn-npcs": "true", "max-world-size": "29999984",
  "online-mode": "true", "white-list": "false", "enforce-secure-profile": "true",
  "prevent-proxy-connections": "false", "view-distance": "10", "simulation-distance": "10",
  "network-compression-threshold": "256", "max-tick-time": "60000", "use-native-transport": "true",
  "sync-chunk-writes": "true", "entity-broadcast-range-percentage": "100", "enable-jmx-monitoring": "false",
  "enable-rcon": "false", "rcon.port": "25575", "rcon.password": "", "enable-query": "false",
  "query.port": "25565", "resource-pack": "", "resource-pack-sha1": "", "require-resource-pack": "false",
};

const PROPS_GROUPS = computed(() => {
  const groups: string[] = [];
  for (const f of PROPS_SCHEMA) {
    if (!groups.includes(f.group)) groups.push(f.group);
  }
  return groups;
});

const propsMode = ref<"form" | "source">("form");
const propsData = ref<Record<string, string>>({});
const propsSource = ref("");
const propsExtra = ref<Record<string, string>>({});
const loadingProps = ref(false);
const savingProps = ref(false);

function parseProperties(text: string): { data: Record<string, string>; extra: Record<string, string> } {
  const data: Record<string, string> = {};
  const extra: Record<string, string> = {};
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const eq = trimmed.indexOf("=");
    if (eq < 0) continue;
    const key = trimmed.slice(0, eq).trim();
    const val = trimmed.slice(eq + 1).trim();
    data[key] = val;
    if (!PROPS_SCHEMA.some((f) => f.key === key)) extra[key] = val;
  }
  return { data, extra };
}

function buildProperties(): string {
  const lines: string[] = ["# Minecraft server properties", "# Generated by QookiX Launcher"];
  for (const f of PROPS_SCHEMA) {
    lines.push(`${f.key}=${propsData.value[f.key] ?? PROPS_DEFAULTS[f.key] ?? ""}`);
  }
  for (const [k, v] of Object.entries(propsExtra.value)) {
    lines.push(`${k}=${v}`);
  }
  return lines.join("\n") + "\n";
}

async function loadServerProperties() {
  loadingProps.value = true;
  try {
    const r = await api.readHostedServerFile(serverId, "server.properties");
    propsSource.value = r.content;
    const { data, extra } = parseProperties(r.content);
    const merged: Record<string, string> = {};
    for (const f of PROPS_SCHEMA) merged[f.key] = data[f.key] ?? PROPS_DEFAULTS[f.key] ?? "";
    propsData.value = merged;
    propsExtra.value = extra;
  } catch {
    propsData.value = { ...PROPS_DEFAULTS };
    propsExtra.value = {};
    propsSource.value = buildProperties();
  } finally {
    loadingProps.value = false;
  }
}

async function savePropsForm() {
  savingProps.value = true;
  try {
    const text = buildProperties();
    await api.writeHostedServerFile(serverId, "server.properties", text);
    propsSource.value = text;
    message.success(t("serverDetail.msg.configSaved"));
  } catch (e) {
    message.error(String(e));
  } finally {
    savingProps.value = false;
  }
}

async function savePropsSource() {
  savingProps.value = true;
  try {
    await api.writeHostedServerFile(serverId, "server.properties", propsSource.value);
    const { data, extra } = parseProperties(propsSource.value);
    const merged: Record<string, string> = {};
    for (const f of PROPS_SCHEMA) merged[f.key] = data[f.key] ?? PROPS_DEFAULTS[f.key] ?? "";
    propsData.value = merged;
    propsExtra.value = extra;
    message.success(t("serverDetail.msg.configSaved"));
  } catch (e) {
    message.error(String(e));
  } finally {
    savingProps.value = false;
  }
}

// ---- 其他配置文件 ----
const CONFIG_DOCS: Record<string, string> = {
  "eula.txt": "serverDetail.configDoc.eula.txt",
  "ops.json": "serverDetail.configDoc.ops.json",
  "whitelist.json": "serverDetail.configDoc.whitelist.json",
  "banned-players.json": "serverDetail.configDoc.banned-players.json",
  "banned-ips.json": "serverDetail.configDoc.banned-ips.json",
  "spigot.yml": "serverDetail.configDoc.spigot.yml",
  "paper.yml": "serverDetail.configDoc.paper.yml",
  "paper-global.yml": "serverDetail.configDoc.paper-global.yml",
  "paper-world-defaults.yml": "serverDetail.configDoc.paper-world-defaults.yml",
  "purpur.yml": "serverDetail.configDoc.purpur.yml",
  "bukkit.yml": "serverDetail.configDoc.bukkit.yml",
  "commands.yml": "serverDetail.configDoc.commands.yml",
  "pufferfish.yml": "serverDetail.configDoc.pufferfish.yml",
  "permissions.yml": "serverDetail.configDoc.permissions.yml",
  "help.yml": "serverDetail.configDoc.help.yml",
  "fabric-server.properties": "serverDetail.configDoc.fabric-server.properties",
  "logs.yml": "serverDetail.configDoc.logs.yml",
};

type ConfigFile = { name: string; rel: string; size: number; modified: number };
const configFiles = ref<ConfigFile[]>([]);
const loadingConfigs = ref(false);

function configDoc(rel: string): string {
  const name = rel.split("/").pop() ?? rel;
  if (CONFIG_DOCS[name]) return t(CONFIG_DOCS[name]);
  if (rel.startsWith("config/")) return t("serverDetail.configDoc.modConfig");
  return t("serverDetail.configDoc.custom");
}

async function loadConfigFiles() {
  loadingConfigs.value = true;
  try {
    const all = await api.listHostedServerConfigFiles(serverId);
    configFiles.value = all.filter((f) => f.rel !== "server.properties");
  } catch {
    configFiles.value = [];
  } finally {
    loadingConfigs.value = false;
  }
}

// ---- 配置文件编辑器 ----
type EditorState = {
  rel: string;
  name: string;
  doc: string;
  content: string;
  loading: boolean;
  saving: boolean;
  error: string | null;
};
const editor = ref<EditorState | null>(null);

async function openEditor(f: ConfigFile) {
  editor.value = {
    rel: f.rel,
    name: f.name,
    doc: configDoc(f.rel),
    content: "",
    loading: true,
    saving: false,
    error: null,
  };
  try {
    const r = await api.readHostedServerFile(serverId, f.rel);
    let content = r.content;
    if (f.name.endsWith(".json")) {
      try {
        content = JSON.stringify(JSON.parse(content), null, 2);
      } catch {
        /* 非合法 JSON，原样显示 */
      }
    }
    if (editor.value) editor.value.content = content;
  } catch (e) {
    if (editor.value) editor.value.error = String(e);
  } finally {
    if (editor.value) editor.value.loading = false;
  }
}

async function saveEditor() {
  const ed = editor.value;
  if (!ed) return;
  ed.saving = true;
  try {
    await api.writeHostedServerFile(serverId, ed.rel, ed.content);
    message.success(t("serverDetail.msg.configSaved"));
    editor.value = null;
    loadConfigFiles();
  } catch (e) {
    message.error(String(e));
  } finally {
    if (editor.value) editor.value.saving = false;
  }
}

// ---- mods / plugins 列表 ----
type FileEntry = { name: string; size: number; modified: number; isDir: boolean };
const fileList = ref<FileEntry[]>([]);
const loadingFiles = ref(false);

async function loadFileList(sub: string) {
  loadingFiles.value = true;
  try {
    const r = await api.listHostedServerFiles(serverId, sub);
    fileList.value = r.files.map((f) => ({
      name: f.name,
      size: f.size,
      modified: f.modified,
      isDir: f.isDir,
    }));
  } catch (e) {
    fileList.value = [];
    message.error(String(e));
  } finally {
    loadingFiles.value = false;
  }
}

watch(tab, (t) => {
  if (t === "mods") loadFileList("mods");
  else if (t === "plugins") loadFileList("plugins");
  else if (t === "settings") loadJavaCandidates();
  else if (t === "config") {
    loadServerProperties();
    loadConfigFiles();
  }
});

// ---- 日志 ----
type LogLine = { stream: "out" | "err"; line: string };
const logs = ref<LogLine[]>([]);
const logBox = ref<HTMLElement | null>(null);
const MAX_LOG_LINES = 2000;

function pushLog(stream: "out" | "err", line: string) {
  logs.value.push({ stream, line });
  if (logs.value.length > MAX_LOG_LINES) {
    logs.value = logs.value.slice(-MAX_LOG_LINES);
  }
  nextTick(() => {
    if (logBox.value) logBox.value.scrollTop = logBox.value.scrollHeight;
  });
}
function clearLogs() {
  logs.value = [];
}

const logText = computed(() => logs.value.map((l) => l.line).join("\n"));

async function copyLogs() {
  if (!logText.value) return message.info(t("serverDetail.msg.noLogs"));
  try {
    await navigator.clipboard.writeText(logText.value);
    message.success(t("serverDetail.msg.logsCopied"));
  } catch {
    const ta = document.createElement("textarea");
    ta.value = logText.value;
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand("copy");
    document.body.removeChild(ta);
    if (ok) message.success(t("serverDetail.msg.logsCopied"));
    else message.error(t("serverDetail.msg.copyFailed"));
  }
}

async function exportLogs() {
  if (!logText.value) return message.info(t("serverDetail.msg.noLogs"));
  const ts = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  const defaultName = `server-${ts.getFullYear()}${pad(ts.getMonth() + 1)}${pad(ts.getDate())}-${pad(ts.getHours())}${pad(ts.getMinutes())}${pad(ts.getSeconds())}.log`;
  const path = await save({
    defaultPath: defaultName,
    filters: [{ name: t("serverDetail.msg.logFileFilter"), extensions: ["log", "txt"] }],
  });
  if (!path) return;
  try {
    await api.saveTextFile(path as string, logText.value);
    message.success(t("serverDetail.msg.logsExported"));
  } catch (e) {
    message.error(String(e));
  }
}

// ---- 启动 / 停止 ----
const starting = ref(false);
async function start() {
  if (!server.value) return;
  if (!server.value.eula) {
    message.warning(t("serverDetail.msg.agreeEula"));
    tab.value = "settings";
    return;
  }
  starting.value = true;
  tab.value = "logs";
  try {
    try {
      await servers.start(serverId);
      message.success(t("serverDetail.msg.serverStarted"));
      tab.value = "logs";
    } catch (e) {
      const msg = String(e);
      if (msg.includes("server.jar 不存在")) {
        message.info(t("serverDetail.msg.preparingCore"));
        await servers.installCore(serverId);
        await servers.start(serverId);
        message.success(t("serverDetail.msg.serverStarted"));
        tab.value = "logs";
      } else {
        throw e;
      }
    }
  } catch (e) {
    message.error(String(e));
  } finally {
    starting.value = false;
  }
}

async function stop() {
  try {
    await servers.stop(serverId);
    message.success(t("serverDetail.msg.serverStopped"));
  } catch (e) {
    message.error(String(e));
  }
}

function openFolder(sub?: string) {
  api.openHostedServerFolder(serverId, sub).catch((e) => message.error(String(e)));
}

// ---- 删除 ----
const confirmDelete = ref(false);
async function doDelete() {
  try {
    await servers.remove(serverId);
    message.success(t("serverDetail.msg.serverDeleted"));
    router.push("/multiplayer");
  } catch (e) {
    message.error(String(e));
  }
}

// ---- 生命周期 ----
let unlisteners: UnlistenFn[] = [];
onMounted(async () => {
  await servers.load();
  syncForm();
  loadConfigFiles();
  try {
    const r = await api.listHostedServerFolders(serverId);
    folders.value = Object.fromEntries(r.folders.map((f) => [f.name, f.exists]));
  } catch {
    /* ignore */
  }
  try {
    const history = await api.readHostedServerLog(serverId);
    if (history.length) {
      logs.value = history.map((line) => ({ stream: "out" as const, line }));
      nextTick(() => {
        if (logBox.value) logBox.value.scrollTop = logBox.value.scrollHeight;
      });
    }
  } catch {
    /* ignore */
  }
  const u1 = await listen<{ serverId: string; stream: "out" | "err"; line: string }>(
    "server://log",
    (ev) => {
      if (ev.payload.serverId === serverId) pushLog(ev.payload.stream, ev.payload.line);
    },
  );
  unlisteners.push(u1);
  const u2 = await listen<{ serverId: string; state: string; pid: number; code: number | null }>(
    "server://state",
    (ev) => {
      if (ev.payload.serverId !== serverId) return;
      servers.setRunning(serverId, ev.payload.state === "running");
      if (ev.payload.state === "exited") {
        pushLog("err", t("serverDetail.msg.processExited", { code: ev.payload.code ?? "?" }));
      }
    },
  );
  unlisteners.push(u2);
  const u3 = await listen<{ serverId: string; code: number; tail: string[] }>(
    "server://error",
    (ev) => {
      if (ev.payload.serverId !== serverId) return;
      const detail = ev.payload.tail.length
        ? "\n" + ev.payload.tail.join("\n")
        : "";
      message.error(t("serverDetail.msg.processError", { code: ev.payload.code, detail }), { duration: 8000 });
    },
  );
  unlisteners.push(u3);
});

onBeforeUnmount(() => {
  for (const u of unlisteners) u();
  unlisteners = [];
});
</script>

<template>
  <div class="server-detail" v-if="server">
    <button class="back" @click="router.push('/multiplayer')">
      <IconChevronLeft /> {{ t('serverDetail.back') }}
    </button>

    <div class="head glass">
      <div class="head-info">
        <span
          class="core-badge"
          :style="{ color: CORE_COLORS[server.core], borderColor: CORE_COLORS[server.core] }"
        >
          {{ CORE_LABELS[server.core] }}
        </span>
        <h2>{{ server.name }}</h2>
        <p class="head-sub">
          <span class="mono">{{ server.mc_version }}</span> · {{ t('serverDetail.port') }} {{ server.port }} ·
          {{ server.max_memory_mb }} MB
        </p>
      </div>
      <div class="head-ops">
        <button v-if="!running" class="btn primary" :disabled="starting" @click="start">
          <IconPlay /> {{ starting ? t('serverDetail.starting') : t('serverDetail.start') }}
        </button>
        <button v-else class="btn warn" @click="stop">
          <IconStop /> {{ t('serverDetail.stop') }}
        </button>
        <button class="btn ghost" @click="openFolder()"><IconFolder /> {{ t('serverDetail.folder') }}</button>
      </div>
    </div>

    <div class="tabs glass">
      <button
        v-for="tabItem in tabs"
        :key="tabItem.key"
        :class="{ active: tab === tabItem.key }"
        @click="tab = tabItem.key"
      >
        {{ t(tabItem.label) }}
      </button>
    </div>

    <!-- 设置 -->
    <div v-if="tab === 'settings'" class="panel glass">
      <div class="section">
        <h3 class="section-title">{{ t('serverDetail.settings.basic') }}</h3>
        <p class="section-hint">{{ t('serverDetail.settings.autoSaveHint') }}</p>
        <div class="field">
          <label>{{ t('serverDetail.settings.serverName') }}</label>
          <n-input v-model:value="form.name" maxlength="40" @update:value="scheduleAutoSave" />
        </div>
        <div class="field-row">
          <div class="field">
            <label>{{ t('serverDetail.settings.maxMem') }}</label>
            <n-input-number v-model:value="form.maxMem" :min="256" :step="256" @update:value="scheduleAutoSave" />
          </div>
          <div class="field">
            <label>{{ t('serverDetail.settings.minMem') }}</label>
            <n-input-number v-model:value="form.minMem" :min="128" :step="256" @update:value="scheduleAutoSave" />
          </div>
        </div>
        <div class="field eula">
          <n-checkbox v-model:checked="form.eula" @update:checked="scheduleAutoSave">
            {{ t('serverDetail.settings.eulaAgree') }}
            <a href="https://account.mojang.com/documents/Minecraft_EULA" target="_blank" rel="noopener">Minecraft EULA</a>
          </n-checkbox>
        </div>
      </div>

      <div class="section">
        <h3 class="section-title">{{ t('serverDetail.settings.javaRuntime') }}</h3>
        <p class="section-hint">{{ t('serverDetail.settings.javaHint') }}</p>
        <div class="java-row">
          <n-input v-model:value="form.javaPath" :placeholder="t('serverDetail.settings.javaPlaceholder')" @update:value="scheduleAutoSave" />
          <button class="btn sm ghost" @click="pickJava">{{ t('serverDetail.settings.browse') }}</button>
          <button class="btn sm ghost" @click="loadJavaCandidates">{{ t('serverDetail.settings.refresh') }}</button>
        </div>
        <div v-if="javaCandidates.length" class="java-list">
          <button
            v-for="j in javaCandidates.slice(0, 10)"
            :key="j.path"
            class="java-item"
            :class="{ active: form.javaPath === j.path }"
            @click="form.javaPath = j.path; scheduleAutoSave()"
          >
            <span class="java-name">Java {{ j.major }} ({{ j.version }})</span>
            <span class="java-path">{{ j.path }}</span>
          </button>
        </div>
      </div>

      <div class="section">
        <h3 class="section-title">{{ t('serverDetail.settings.jvmArgs') }}</h3>
        <p class="section-hint">{{ t('serverDetail.settings.jvmArgsHint') }}</p>
        <n-input v-model:value="form.jvmArgs" :placeholder="t('serverDetail.settings.jvmArgsPlaceholder')" @update:value="scheduleAutoSave" />
      </div>

      <div class="section">
        <h3 class="section-title">{{ t('serverDetail.settings.stopCommand') }}</h3>
        <p class="section-hint">{{ t('serverDetail.settings.stopCommandHint') }}</p>
        <n-input v-model:value="form.stopCommand" placeholder="stop" @update:value="scheduleAutoSave" />
      </div>

      <div class="panel-foot">
        <button class="btn danger" @click="confirmDelete = true"><IconTrash /> {{ t('serverDetail.settings.deleteServer') }}</button>
        <span v-if="autoSaving" class="save-hint">{{ t('serverDetail.settings.saving') }}</span>
      </div>
    </div>

    <!-- 配置文件 -->
    <div v-else-if="tab === 'config'" class="panel glass">
      <!-- server.properties -->
      <div class="section">
        <div class="section-head">
          <h3 class="section-title">server.properties</h3>
          <div class="props-mode-tabs">
            <button :class="{ active: propsMode === 'form' }" @click="propsMode = 'form'">{{ t('serverDetail.config.formMode') }}</button>
            <button :class="{ active: propsMode === 'source' }" @click="propsMode = 'source'">{{ t('serverDetail.config.sourceMode') }}</button>
          </div>
        </div>
        <p class="section-hint">{{ t('serverDetail.config.propsHint') }}</p>

        <div v-if="propsMode === 'form'" class="props-form">
          <div v-if="loadingProps" class="empty-inline">{{ t('serverDetail.config.loadingConfig') }}</div>
          <template v-else>
            <div v-for="g in PROPS_GROUPS" :key="g" class="props-group">
              <h4 class="props-group-title">{{ t('serverDetail.config.propsGroup.' + g) }}</h4>
              <div class="props-grid">
                <div
                  v-for="f in PROPS_SCHEMA.filter(s => s.group === g)"
                  :key="f.key"
                  class="prop-item"
                >
                  <div class="prop-label">
                    <span class="prop-name">{{ t(f.label) }}</span>
                    <span class="prop-desc">{{ t(f.desc) }}</span>
                  </div>
                  <div class="prop-control">
                    <n-switch
                      v-if="f.type === 'bool'"
                      :value="propsData[f.key] === 'true'"
                      @update:value="(v: boolean) => propsData[f.key] = v ? 'true' : 'false'"
                    />
                    <n-select
                      v-else-if="f.type === 'enum'"
                      :value="propsData[f.key]"
                      :options="f.options!.map(o => ({ label: o, value: o }))"
                      size="small"
                      @update:value="(v: string) => propsData[f.key] = v"
                    />
                    <n-input-number
                      v-else-if="f.type === 'int'"
                      :value="Number(propsData[f.key])"
                      :min="f.min"
                      :max="f.max"
                      size="small"
                      @update:value="(v: number | null) => propsData[f.key] = String(v ?? 0)"
                    />
                    <n-input
                      v-else
                      :value="propsData[f.key]"
                      size="small"
                      @update:value="(v: string) => propsData[f.key] = v"
                    />
                  </div>
                </div>
              </div>
            </div>
          </template>
          <div class="panel-foot">
            <button class="btn primary" :disabled="savingProps || loadingProps" @click="savePropsForm">
              {{ savingProps ? t('serverDetail.config.saving') : t('serverDetail.config.saveConfig') }}
            </button>
          </div>
        </div>

        <div v-else class="props-source">
          <div v-if="loadingProps" class="empty-inline">{{ t('serverDetail.config.loadingConfig') }}</div>
          <template v-else>
            <textarea
              v-model="propsSource"
              class="editor-textarea"
              spellcheck="false"
            ></textarea>
            <div class="panel-foot">
              <button class="btn primary" :disabled="savingProps" @click="savePropsSource">
                {{ savingProps ? t('serverDetail.config.saving') : t('serverDetail.config.saveConfig') }}
              </button>
            </div>
          </template>
        </div>
      </div>

      <!-- 其他配置文件 -->
      <div class="section">
        <div class="section-head">
          <h3 class="section-title">{{ t('serverDetail.config.otherConfigs') }}</h3>
          <button class="btn sm ghost" @click="loadConfigFiles"><IconRefresh /> {{ t('serverDetail.config.refresh') }}</button>
        </div>
        <p class="section-hint">{{ t('serverDetail.config.otherConfigsHint') }}</p>
        <div v-if="loadingConfigs" class="empty-inline">{{ t('serverDetail.config.scanningConfigs') }}</div>
        <div v-else-if="!configFiles.length" class="empty-inline">
          {{ t('serverDetail.config.noOtherConfigs') }}
        </div>
        <div v-else class="config-list">
          <button
            v-for="f in configFiles"
            :key="f.rel"
            class="config-row"
            @click="openEditor(f)"
          >
            <div class="config-info">
              <span class="config-name mono">{{ f.rel }}</span>
              <span class="config-doc">{{ configDoc(f.rel) }}</span>
            </div>
            <span class="config-size">{{ fmtSize(f.size) }}</span>
          </button>
        </div>
      </div>
    </div>

    <!-- 模组 -->
    <div v-else-if="tab === 'mods'" class="panel glass">
      <div class="panel-head">
        <h3><IconBox /> {{ t('serverDetail.mods.title') }}</h3>
        <button class="btn sm ghost" @click="openFolder('mods')">{{ t('serverDetail.mods.openFolder') }}</button>
      </div>
      <div v-if="loadingFiles" class="empty-inline">{{ t('serverDetail.mods.loading') }}</div>
      <div v-else-if="!fileList.length" class="empty-inline">
        {{ t('serverDetail.mods.emptyPrefix') }} <code>mods/</code> {{ t('serverDetail.mods.emptySuffix') }}
      </div>
      <div v-else class="file-list">
        <div v-for="f in fileList" :key="f.name" class="file-row">
          <span class="file-name">{{ f.name }}</span>
          <span class="file-size">{{ fmtSize(f.size) }}</span>
        </div>
      </div>
    </div>

    <!-- 插件 -->
    <div v-else-if="tab === 'plugins'" class="panel glass">
      <div class="panel-head">
        <h3><IconBox /> {{ t('serverDetail.plugins.title') }}</h3>
        <button class="btn sm ghost" @click="openFolder('plugins')">{{ t('serverDetail.plugins.openFolder') }}</button>
      </div>
      <div v-if="loadingFiles" class="empty-inline">{{ t('serverDetail.plugins.loading') }}</div>
      <div v-else-if="!fileList.length" class="empty-inline">
        {{ t('serverDetail.plugins.emptyPrefix') }} <code>plugins/</code> {{ t('serverDetail.plugins.emptySuffix') }}
      </div>
      <div v-else class="file-list">
        <div v-for="f in fileList" :key="f.name" class="file-row">
          <span class="file-name">{{ f.name }}</span>
          <span class="file-size">{{ fmtSize(f.size) }}</span>
        </div>
      </div>
    </div>

    <!-- 日志 -->
    <div v-else-if="tab === 'logs'" class="panel glass log-panel">
      <div class="panel-head">
        <h3><IconFile /> {{ t('serverDetail.logs.title') }}</h3>
        <div class="log-ops">
          <span class="run-dot" :class="{ on: running }"></span>
          <span class="run-text">{{ running ? t('serverDetail.logs.running') : t('serverDetail.logs.stopped') }}</span>
          <button class="btn sm ghost" :title="t('serverDetail.logs.copyAll')" @click="copyLogs"><IconCopy /> {{ t('serverDetail.logs.copy') }}</button>
          <button class="btn sm ghost" :title="t('serverDetail.logs.exportFile')" @click="exportLogs"><IconDownload /> {{ t('serverDetail.logs.export') }}</button>
          <button class="btn sm ghost" @click="clearLogs">{{ t('serverDetail.logs.clear') }}</button>
        </div>
      </div>
      <div ref="logBox" class="log-box">
        <div v-if="!logs.length" class="log-empty">{{ t('serverDetail.logs.empty') }}</div>
        <div
          v-for="(l, i) in logs"
          :key="i"
          class="log-line"
          :class="l.stream"
        >{{ l.line }}</div>
      </div>
    </div>

    <!-- 文件 -->
    <div v-else-if="tab === 'files'" class="panel glass">
      <div class="panel-head">
        <h3><IconFolder /> {{ t('serverDetail.files.title') }}</h3>
      </div>
      <p class="section-hint">{{ t('serverDetail.files.hint') }}</p>
      <ServerFileManager :server-id="serverId" />
    </div>

    <!-- 配置文件编辑器 -->
    <n-modal
      :show="editor !== null"
      preset="card"
      :title="editor?.name ?? ''"
      style="width: 780px; max-width: 94vw"
      @update:show="(v: boolean) => { if (!v) editor = null; }"
    >
      <div v-if="editor" class="editor-body">
        <p class="editor-doc">{{ editor.doc }}</p>
        <div v-if="editor.loading" class="editor-loading">{{ t('serverDetail.editor.reading') }}</div>
        <div v-else-if="editor.error" class="editor-error">{{ editor.error }}</div>
        <textarea
          v-else
          v-model="editor.content"
          class="editor-textarea"
          spellcheck="false"
        ></textarea>
        <div class="editor-foot">
          <button class="btn ghost" @click="editor = null">{{ t('serverDetail.editor.cancel') }}</button>
          <button
            class="btn primary"
            :disabled="editor.loading || editor.saving"
            @click="saveEditor"
          >
            {{ editor.saving ? t('serverDetail.editor.saving') : t('serverDetail.editor.save') }}
          </button>
        </div>
      </div>
    </n-modal>

    <!-- 删除确认 -->
    <div v-if="confirmDelete" class="mask" @click="confirmDelete = false">
      <div class="confirm-card glass" @click.stop>
        <h3>{{ t('serverDetail.delete.title') }}</h3>
        <p>{{ t('serverDetail.delete.confirmPrefix') }}<b>{{ server.name }}</b>{{ t('serverDetail.delete.confirmSuffix') }}</p>
        <div class="confirm-foot">
          <button class="btn ghost" @click="confirmDelete = false">{{ t('serverDetail.delete.cancel') }}</button>
          <button class="btn danger" @click="doDelete">{{ t('serverDetail.delete.delete') }}</button>
        </div>
      </div>
    </div>
  </div>

  <div v-else class="loading">{{ t('serverDetail.loading') }}</div>
</template>

<style scoped>
.server-detail {
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.back {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  align-self: flex-start;
  background: none;
  border: none;
  color: var(--text-3);
  font-size: 13px;
  cursor: pointer;
  font-family: inherit;
  padding: 6px 10px;
  border-radius: 8px;
}
.back:hover {
  color: var(--text-1);
  background: var(--w-06);
}
.head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding: 20px 24px;
  flex-wrap: wrap;
}
.head-info {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.core-badge {
  align-self: flex-start;
  font-size: 11px;
  font-weight: 700;
  padding: 3px 9px;
  border-radius: 999px;
  border: 1px solid;
  background: var(--w-04);
  letter-spacing: 0.02em;
}
.head-info h2 {
  margin: 4px 0 0;
  font-size: 20px;
}
.head-sub {
  margin: 0;
  font-size: 12px;
  color: var(--text-3);
}
.head-ops {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
.btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 7px;
  border: none;
  border-radius: 10px;
  padding: 9px 16px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  transition: all 0.15s;
}
.btn.sm {
  padding: 6px 12px;
  font-size: 12px;
}
.btn.primary {
  background: linear-gradient(135deg, var(--accent), var(--accent-deep));
  color: #1a1208;
}
.btn.primary:hover:not(:disabled) {
  filter: brightness(1.08);
}
.btn.ghost {
  background: var(--w-06);
  color: var(--text-1);
  border: 1px solid var(--border);
}
.btn.ghost:hover {
  background: var(--w-10);
}
.btn.warn {
  background: rgba(224, 168, 90, 0.18);
  color: #e0a85a;
  border: 1px solid rgba(224, 168, 90, 0.4);
}
.btn.warn:hover {
  background: rgba(224, 168, 90, 0.28);
}
.btn.danger {
  background: var(--danger-16);
  color: #f0907f;
  border: 1px solid var(--danger-40);
}
.btn.danger:hover {
  background: var(--danger-26);
}
.btn:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}
.tabs {
  display: inline-flex;
  gap: 4px;
  padding: 5px;
  align-self: flex-start;
}
.tabs button {
  border: none;
  background: transparent;
  color: var(--text-2);
  padding: 8px 18px;
  border-radius: 9px;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
}
.tabs button.active {
  background: var(--accent-soft);
  color: var(--accent);
}
.panel {
  padding: 20px 24px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.section {
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.section-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.section-title {
  margin: 0;
  font-size: 15px;
  font-weight: 600;
}
.section-hint {
  margin: -6px 0 0;
  font-size: 12px;
  color: var(--text-3);
}
.panel-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.panel-head h3 {
  margin: 0;
  font-size: 15px;
  display: inline-flex;
  align-items: center;
  gap: 8px;
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
.field-row {
  display: flex;
  gap: 12px;
  flex-wrap: wrap;
}
.field-row .field {
  flex: 1;
  min-width: 120px;
}
.eula a {
  color: var(--accent);
  text-decoration: none;
}
.eula a:hover {
  text-decoration: underline;
}
.panel-foot {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 10px;
  margin-top: 4px;
}
.save-hint {
  font-size: 13px;
  color: var(--text-3);
}
.empty-inline {
  padding: 24px;
  text-align: center;
  color: var(--text-3);
  font-size: 13px;
  border: 1px dashed var(--border);
  border-radius: 10px;
}
.empty-inline code {
  font-family: "Cascadia Code", Consolas, monospace;
  color: var(--text-2);
}
.config-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.config-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 14px;
  padding: 11px 14px;
  border-radius: 10px;
  border: 1px solid var(--border);
  background: var(--w-03);
  cursor: pointer;
  font-family: inherit;
  text-align: left;
  transition: all 0.13s;
}
.config-row:hover {
  background: var(--accent-soft);
  border-color: var(--accent-35);
}
.config-info {
  display: flex;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
  flex: 1;
}
.config-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.config-doc {
  font-size: 11px;
  color: var(--text-3);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.config-row:hover .config-doc {
  color: var(--accent-60);
}
.config-size {
  font-size: 11px;
  color: var(--text-3);
  flex-shrink: 0;
}
.file-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 420px;
  overflow-y: auto;
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 6px;
}
.file-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  padding: 8px 12px;
  border-radius: 8px;
  font-size: 13px;
}
.file-row:hover {
  background: var(--w-05);
}
.file-name {
  color: var(--text-1);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.file-size {
  color: var(--text-3);
  font-size: 11px;
  flex-shrink: 0;
}
.log-panel {
  gap: 12px;
}
.log-ops {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}
.run-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--text-3);
}
.run-dot.on {
  background: #57c98a;
  box-shadow: 0 0 6px rgba(87, 201, 138, 0.6);
}
.run-text {
  font-size: 12px;
  color: var(--text-3);
}
.log-box {
  height: 420px;
  overflow-y: auto;
  background: rgba(0, 0, 0, 0.32);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 12px 14px;
  font-family: "Cascadia Code", Consolas, "Courier New", monospace;
  font-size: 12px;
  line-height: 1.55;
}
.log-empty {
  color: var(--text-3);
  text-align: center;
  padding: 40px 0;
}
.log-line {
  white-space: pre-wrap;
  word-break: break-all;
  color: var(--text-2);
}
.log-line.err {
  color: #f0907f;
}
.file-shortcuts {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
}
.shortcut {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  padding: 9px 14px;
  border-radius: 10px;
  border: 1px solid var(--border);
  background: var(--w-04);
  color: var(--text-2);
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
  transition: all 0.13s;
}
.shortcut:hover {
  background: var(--accent-soft);
  color: var(--accent);
  border-color: var(--accent-35);
}
.loading {
  padding: 60px;
  text-align: center;
  color: var(--text-3);
}
.mask {
  position: fixed;
  inset: 0;
  background: var(--k-45);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
}
.confirm-card {
  width: 420px;
  max-width: 92vw;
  padding: 20px 24px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.confirm-card h3 {
  margin: 0;
  font-size: 16px;
}
.confirm-card p {
  margin: 0;
  font-size: 13px;
  color: var(--text-2);
  line-height: 1.6;
}
.confirm-card b {
  color: var(--text-1);
}
.confirm-foot {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  margin-top: 4px;
}
.editor-body {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.editor-doc {
  margin: 0;
  font-size: 12px;
  color: var(--text-3);
  padding: 8px 12px;
  background: var(--accent-soft);
  border-radius: 8px;
  border-left: 3px solid var(--accent);
}
.editor-loading,
.editor-error {
  padding: 30px;
  text-align: center;
  font-size: 13px;
  color: var(--text-3);
}
.editor-error {
  color: #f0907f;
}
.editor-textarea {
  width: 100%;
  min-height: 360px;
  max-height: 56vh;
  resize: vertical;
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 12px 14px;
  background: rgba(0, 0, 0, 0.28);
  color: var(--text-1);
  font-family: "Cascadia Code", Consolas, "Courier New", monospace;
  font-size: 12.5px;
  line-height: 1.55;
  outline: none;
  box-sizing: border-box;
  transition: border-color 0.13s;
}
.editor-textarea:focus {
  border-color: var(--accent-45);
}
.editor-foot {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}
.props-mode-tabs {
  display: inline-flex;
  gap: 4px;
  padding: 3px;
  border-radius: 8px;
  background: var(--w-05);
  border: 1px solid var(--border);
}
.props-mode-tabs button {
  border: none;
  background: transparent;
  color: var(--text-3);
  padding: 5px 14px;
  border-radius: 6px;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  font-family: inherit;
}
.props-mode-tabs button.active {
  background: var(--accent-soft);
  color: var(--accent);
}
.props-form {
  display: flex;
  flex-direction: column;
  gap: 18px;
}
.props-group {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.props-group-title {
  margin: 0;
  font-size: 13px;
  font-weight: 700;
  color: var(--text-2);
  padding-bottom: 6px;
  border-bottom: 1px solid var(--border);
}
.props-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
  gap: 12px;
}
.prop-item {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px 12px;
  border-radius: 10px;
  background: var(--w-03);
  border: 1px solid var(--border);
}
.prop-label {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.prop-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-1);
}
.prop-desc {
  font-size: 11px;
  color: var(--text-3);
  line-height: 1.4;
}
.prop-control {
  display: flex;
  align-items: center;
}
.props-source {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.java-row {
  display: flex;
  gap: 8px;
  align-items: center;
}
.java-row .btn {
  flex-shrink: 0;
}
.java-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-top: 8px;
}
.java-item {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 8px 12px;
  border-radius: 8px;
  border: 1px solid var(--border);
  background: var(--w-03);
  cursor: pointer;
  font-family: inherit;
  text-align: left;
  transition: all 0.13s;
}
.java-item:hover {
  background: var(--w-07);
}
.java-item.active {
  border-color: var(--accent-45);
  background: var(--accent-soft);
}
.java-name {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-1);
}
.java-item.active .java-name {
  color: var(--accent);
}
.java-path {
  font-size: 11px;
  color: var(--text-3);
  font-family: var(--mono, monospace);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
