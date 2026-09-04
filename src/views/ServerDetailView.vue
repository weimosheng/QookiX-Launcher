<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch, nextTick } from "vue";
import { useRoute, useRouter } from "vue-router";
import { NInput, NInputNumber, NCheckbox, NSwitch, NSelect, NModal, useMessage } from "naive-ui";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/plugin-dialog";
import { api } from "../api";
import { useServersStore } from "../stores/servers";
import ServerFileManager from "../components/ServerFileManager.vue";
import type { ServerCore } from "../types";
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
const servers = useServersStore();
const message = useMessage();

const serverId = route.params.id as string;

const CORE_LABELS: Record<ServerCore, string> = {
  vanilla: "Vanilla",
  paper: "Paper",
  spigot: "Spigot",
  purpur: "Purpur",
  forge: "Forge",
  fabric: "Fabric",
};
const CORE_COLORS: Record<ServerCore, string> = {
  vanilla: "#a0a4b8",
  paper: "#5aa2f0",
  spigot: "#4ecdc4",
  purpur: "#c78aff",
  forge: "#e89a4b",
  fabric: "#b48ead",
};

const server = computed(() => servers.byId(serverId));
const running = computed(() => servers.isRunning(serverId));

// ---- tabs ----
const tab = ref<string>("logs");
const folders = ref<Record<string, boolean>>({});
const ALL_TABS = [
  { key: "logs", label: "日志" },
  { key: "settings", label: "设置" },
  { key: "config", label: "配置文件" },
  { key: "mods", label: "模组", folder: "mods" },
  { key: "plugins", label: "插件", folder: "plugins" },
  { key: "files", label: "文件" },
];
const tabs = computed(() =>
  ALL_TABS.filter((t) => !t.folder || folders.value[t.folder] || t.key === tab.value),
);
watch(tabs, (ts) => {
  if (!ts.some((t) => t.key === tab.value) && ts.length > 0) tab.value = ts[0].key;
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
  { key: "server-port", label: "服务器端口", desc: "玩家连接的端口号", type: "int", min: 1, max: 65535, group: "基本" },
  { key: "max-players", label: "最大玩家数", desc: "同时在线玩家上限", type: "int", min: 0, group: "基本" },
  { key: "motd", label: "服务器描述", desc: "在服务器列表中显示的文本", type: "string", group: "基本" },
  { key: "player-idle-timeout", label: "空闲踢出", desc: "玩家无操作多少分钟后踢出，0 为禁用", type: "int", min: 0, group: "基本" },
  { key: "gamemode", label: "游戏模式", desc: "新玩家进入时的默认模式", type: "enum", options: ["survival", "creative", "adventure", "spectator"], group: "玩法" },
  { key: "difficulty", label: "难度", desc: "怪物强度与饥饿伤害", type: "enum", options: ["peaceful", "easy", "normal", "hard"], group: "玩法" },
  { key: "pvp", label: "允许 PVP", desc: "玩家之间能否互相攻击", type: "bool", group: "玩法" },
  { key: "hardcore", label: "极限模式", desc: "死亡后封禁账号，仅可旁观", type: "bool", group: "玩法" },
  { key: "force-gamemode", label: "强制游戏模式", desc: "玩家加入时强制切换到默认模式", type: "bool", group: "玩法" },
  { key: "allow-flight", label: "允许飞行", desc: "允许玩家在生存模式下飞行", type: "bool", group: "玩法" },
  { key: "enable-command-block", label: "命令方块", desc: "启用命令方块功能", type: "bool", group: "玩法" },
  { key: "level-name", label: "世界名称", desc: "主世界存档文件夹名", type: "string", group: "世界" },
  { key: "level-seed", label: "世界种子", desc: "留空则随机生成", type: "string", group: "世界" },
  { key: "level-type", label: "世界类型", desc: "地形生成方式", type: "enum", options: ["minecraft:normal", "minecraft:flat", "minecraft:large_biomes", "minecraft:amplified"], group: "世界" },
  { key: "generate-structures", label: "生成结构", desc: "村庄、神殿、废弃矿井等结构", type: "bool", group: "世界" },
  { key: "allow-nether", label: "允许下界", desc: "生成下界维度", type: "bool", group: "世界" },
  { key: "spawn-animals", label: "生成动物", desc: "生成牛、羊、猪等动物", type: "bool", group: "世界" },
  { key: "spawn-monsters", label: "生成怪物", desc: "生成僵尸、骷髅等怪物", type: "bool", group: "世界" },
  { key: "spawn-npcs", label: "生成 NPC", desc: "生成村民等 NPC", type: "bool", group: "世界" },
  { key: "max-world-size", label: "最大世界大小", desc: "世界边界半径（方块数）", type: "int", min: 0, group: "世界" },
  { key: "online-mode", label: "正版验证", desc: "验证玩家账号，关闭可允许离线模式加入", type: "bool", group: "安全" },
  { key: "white-list", label: "白名单", desc: "仅白名单内玩家可加入", type: "bool", group: "安全" },
  { key: "enforce-secure-profile", label: "强制安全配置", desc: "1.19+ 聊天签名验证", type: "bool", group: "安全" },
  { key: "prevent-proxy-connections", label: "防止代理连接", desc: "拒绝通过代理连接的玩家", type: "bool", group: "安全" },
  { key: "view-distance", label: "视距", desc: "发送给玩家的区块半径", type: "int", min: 3, max: 32, group: "性能" },
  { key: "simulation-distance", label: "模拟距离", desc: "实体与方块更新的区块半径", type: "int", min: 3, max: 32, group: "性能" },
  { key: "network-compression-threshold", label: "网络压缩阈值", desc: "数据包大于此值才压缩，-1 禁用", type: "int", group: "性能" },
  { key: "max-tick-time", label: "最大 Tick 时间", desc: "单 tick 超时毫秒数，-1 禁用看门狗", type: "int", group: "性能" },
  { key: "use-native-transport", label: "原生传输", desc: "Linux 上使用 io_uring 加速网络", type: "bool", group: "性能" },
  { key: "sync-chunk-writes", label: "同步区块写入", desc: "区块数据同步写入磁盘", type: "bool", group: "性能" },
  { key: "entity-broadcast-range-percentage", label: "实体广播范围", desc: "实体动作同步范围百分比", type: "int", min: 0, max: 100, group: "性能" },
  { key: "enable-jmx-monitoring", label: "JMX 监控", desc: "启用 JMX 性能监控", type: "bool", group: "性能" },
  { key: "enable-rcon", label: "RCON 远程控制", desc: "允许通过 RCON 协议远程执行命令", type: "bool", group: "远程" },
  { key: "rcon.port", label: "RCON 端口", desc: "RCON 服务端口", type: "int", min: 1, max: 65535, group: "远程" },
  { key: "rcon.password", label: "RCON 密码", desc: "RCON 认证密码", type: "string", group: "远程" },
  { key: "enable-query", label: "Query 协议", desc: "允许外部查询服务器状态", type: "bool", group: "远程" },
  { key: "query.port", label: "Query 端口", desc: "Query 服务端口", type: "int", min: 1, max: 65535, group: "远程" },
  { key: "resource-pack", label: "资源包 URL", desc: "玩家加入时下载的资源包地址", type: "string", group: "资源包" },
  { key: "resource-pack-sha1", label: "资源包校验", desc: "资源包 SHA1 哈希值", type: "string", group: "资源包" },
  { key: "require-resource-pack", label: "强制资源包", desc: "拒绝加载资源包则踢出玩家", type: "bool", group: "资源包" },
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
    message.success("配置已保存");
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
    message.success("配置已保存");
  } catch (e) {
    message.error(String(e));
  } finally {
    savingProps.value = false;
  }
}

// ---- 其他配置文件 ----
const CONFIG_DOCS: Record<string, string> = {
  "eula.txt": "Mojang 最终用户许可协议，必须设为 eula=true 服务器才能启动",
  "ops.json": "管理员（OP）列表，记录拥有管理权限的玩家及其权限等级",
  "whitelist.json": "白名单列表，开启白名单后仅其中的玩家可加入服务器",
  "banned-players.json": "被封禁的玩家列表，记录封禁原因与到期时间",
  "banned-ips.json": "被封禁的 IP 地址列表",
  "spigot.yml": "Spigot 服务端配置：性能调优、调试、命令、网络设置",
  "paper.yml": "Paper 旧版配置（新版已迁移到 paper-global.yml 与 paper-world-defaults.yml）",
  "paper-global.yml": "Paper 全局配置：异步区块、性能修复、压缩、网络等服务器级选项",
  "paper-world-defaults.yml": "Paper 世界默认配置：每个世界的默认优化与修复选项",
  "purpur.yml": "Purpur 配置：更细粒度的玩法调整与性能选项",
  "bukkit.yml": "Bukkit 配置：世界生成、数据库、调试设置",
  "commands.yml": "命令配置：命令别名与权限映射",
  "pufferfish.yml": "Pufferfish 配置：性能与优化选项",
  "permissions.yml": "权限配置",
  "help.yml": "帮助命令配置",
  "fabric-server.properties": "Fabric 服务端配置",
  "logs.yml": "日志输出配置",
};

type ConfigFile = { name: string; rel: string; size: number; modified: number };
const configFiles = ref<ConfigFile[]>([]);
const loadingConfigs = ref(false);

function configDoc(rel: string): string {
  const name = rel.split("/").pop() ?? rel;
  if (CONFIG_DOCS[name]) return CONFIG_DOCS[name];
  if (rel.startsWith("config/")) return "模组 / 插件配置文件，由对应模组生成，可调整其行为参数";
  return "自定义配置文件";
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
    message.success("配置已保存");
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

function fmtSize(n: number) {
  if (n >= 1024 * 1024 * 1024) return (n / 1024 / 1024 / 1024).toFixed(2) + " GB";
  if (n >= 1024 * 1024) return (n / 1024 / 1024).toFixed(1) + " MB";
  if (n >= 1024) return (n / 1024).toFixed(1) + " KB";
  return n + " B";
}

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
  if (!logText.value) return message.info("暂无日志内容");
  try {
    await navigator.clipboard.writeText(logText.value);
    message.success("已复制全部日志");
  } catch {
    const ta = document.createElement("textarea");
    ta.value = logText.value;
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand("copy");
    document.body.removeChild(ta);
    if (ok) message.success("已复制全部日志");
    else message.error("复制失败");
  }
}

async function exportLogs() {
  if (!logText.value) return message.info("暂无日志内容");
  const ts = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  const defaultName = `server-${ts.getFullYear()}${pad(ts.getMonth() + 1)}${pad(ts.getDate())}-${pad(ts.getHours())}${pad(ts.getMinutes())}${pad(ts.getSeconds())}.log`;
  const path = await save({
    defaultPath: defaultName,
    filters: [{ name: "日志文件", extensions: ["log", "txt"] }],
  });
  if (!path) return;
  try {
    await api.saveTextFile(path as string, logText.value);
    message.success("日志已导出");
  } catch (e) {
    message.error(String(e));
  }
}

// ---- 启动 / 停止 ----
const starting = ref(false);
async function start() {
  if (!server.value) return;
  if (!server.value.eula) {
    message.warning("请先在「设置」中同意 Minecraft EULA");
    tab.value = "settings";
    return;
  }
  starting.value = true;
  tab.value = "logs";
  try {
    try {
      await servers.start(serverId);
      message.success("服务器已启动");
      tab.value = "logs";
    } catch (e) {
      const msg = String(e);
      if (msg.includes("server.jar 不存在")) {
        message.info("首次启动，正在准备服务器核心…");
        await servers.installCore(serverId);
        await servers.start(serverId);
        message.success("服务器已启动");
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
    message.success("已停止服务器");
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
    message.success("服务器已删除");
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
        pushLog("err", `[进程已退出，代码 ${ev.payload.code ?? "?"}]`);
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
      message.error(`服务器进程异常退出（代码 ${ev.payload.code}）${detail}`, { duration: 8000 });
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
      <IconChevronLeft /> 返回服务器列表
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
          <span class="mono">{{ server.mc_version }}</span> · 端口 {{ server.port }} ·
          {{ server.max_memory_mb }} MB
        </p>
      </div>
      <div class="head-ops">
        <button v-if="!running" class="btn primary" :disabled="starting" @click="start">
          <IconPlay /> {{ starting ? "启动中…" : "启动" }}
        </button>
        <button v-else class="btn warn" @click="stop">
          <IconStop /> 停止
        </button>
        <button class="btn ghost" @click="openFolder()"><IconFolder /> 目录</button>
      </div>
    </div>

    <div class="tabs glass">
      <button
        v-for="t in tabs"
        :key="t.key"
        :class="{ active: tab === t.key }"
        @click="tab = t.key"
      >
        {{ t.label }}
      </button>
    </div>

    <!-- 设置 -->
    <div v-if="tab === 'settings'" class="panel glass">
      <div class="section">
        <h3 class="section-title">基本</h3>
        <p class="section-hint">修改任意设置会自动保存</p>
        <div class="field">
          <label>服务器名称</label>
          <n-input v-model:value="form.name" maxlength="40" @update:value="scheduleAutoSave" />
        </div>
        <div class="field-row">
          <div class="field">
            <label>最大内存 (MB)</label>
            <n-input-number v-model:value="form.maxMem" :min="256" :step="256" @update:value="scheduleAutoSave" />
          </div>
          <div class="field">
            <label>最小内存 (MB)</label>
            <n-input-number v-model:value="form.minMem" :min="128" :step="256" @update:value="scheduleAutoSave" />
          </div>
        </div>
        <div class="field eula">
          <n-checkbox v-model:checked="form.eula" @update:checked="scheduleAutoSave">
            我已阅读并同意
            <a href="https://account.mojang.com/documents/Minecraft_EULA" target="_blank" rel="noopener">Minecraft EULA</a>
          </n-checkbox>
        </div>
      </div>

      <div class="section">
        <h3 class="section-title">Java 运行时</h3>
        <p class="section-hint">留空则自动选择合适版本</p>
        <div class="java-row">
          <n-input v-model:value="form.javaPath" placeholder="自动选择" @update:value="scheduleAutoSave" />
          <button class="btn sm ghost" @click="pickJava">浏览…</button>
          <button class="btn sm ghost" @click="loadJavaCandidates">刷新</button>
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
        <h3 class="section-title">JVM 参数</h3>
        <p class="section-hint">额外的 JVM 启动参数，如 -XX:+UseG1GC</p>
        <n-input v-model:value="form.jvmArgs" placeholder="例如：-XX:+UseG1GC -Dfile.encoding=UTF-8" @update:value="scheduleAutoSave" />
      </div>

      <div class="section">
        <h3 class="section-title">停止命令</h3>
        <p class="section-hint">停止服务器时发送的命令，留空默认为 stop</p>
        <n-input v-model:value="form.stopCommand" placeholder="stop" @update:value="scheduleAutoSave" />
      </div>

      <div class="panel-foot">
        <button class="btn danger" @click="confirmDelete = true"><IconTrash /> 删除服务器</button>
        <span v-if="autoSaving" class="save-hint">保存中…</span>
      </div>
    </div>

    <!-- 配置文件 -->
    <div v-else-if="tab === 'config'" class="panel glass">
      <!-- server.properties -->
      <div class="section">
        <div class="section-head">
          <h3 class="section-title">server.properties</h3>
          <div class="props-mode-tabs">
            <button :class="{ active: propsMode === 'form' }" @click="propsMode = 'form'">表单</button>
            <button :class="{ active: propsMode === 'source' }" @click="propsMode = 'source'">源文件</button>
          </div>
        </div>
        <p class="section-hint">Minecraft 服务器核心配置，表单模式提供结构化编辑与说明</p>

        <div v-if="propsMode === 'form'" class="props-form">
          <div v-if="loadingProps" class="empty-inline">正在加载配置…</div>
          <template v-else>
            <div v-for="g in PROPS_GROUPS" :key="g" class="props-group">
              <h4 class="props-group-title">{{ g }}</h4>
              <div class="props-grid">
                <div
                  v-for="f in PROPS_SCHEMA.filter(s => s.group === g)"
                  :key="f.key"
                  class="prop-item"
                >
                  <div class="prop-label">
                    <span class="prop-name">{{ f.label }}</span>
                    <span class="prop-desc">{{ f.desc }}</span>
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
              {{ savingProps ? "保存中…" : "保存配置" }}
            </button>
          </div>
        </div>

        <div v-else class="props-source">
          <div v-if="loadingProps" class="empty-inline">正在加载配置…</div>
          <template v-else>
            <textarea
              v-model="propsSource"
              class="editor-textarea"
              spellcheck="false"
            ></textarea>
            <div class="panel-foot">
              <button class="btn primary" :disabled="savingProps" @click="savePropsSource">
                {{ savingProps ? "保存中…" : "保存配置" }}
              </button>
            </div>
          </template>
        </div>
      </div>

      <!-- 其他配置文件 -->
      <div class="section">
        <div class="section-head">
          <h3 class="section-title">其他配置文件</h3>
          <button class="btn sm ghost" @click="loadConfigFiles"><IconRefresh /> 刷新</button>
        </div>
        <p class="section-hint">ops.json、whitelist.json、spigot.yml 等，点击即可编辑</p>
        <div v-if="loadingConfigs" class="empty-inline">正在扫描配置文件…</div>
        <div v-else-if="!configFiles.length" class="empty-inline">
          暂未发现其他配置文件
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
        <h3><IconBox /> 模组</h3>
        <button class="btn sm ghost" @click="openFolder('mods')">打开 mods 目录</button>
      </div>
      <div v-if="loadingFiles" class="empty-inline">加载中…</div>
      <div v-else-if="!fileList.length" class="empty-inline">
        mods 目录为空，将模组 jar 放入 <code>mods/</code> 目录后即可加载
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
        <h3><IconBox /> 插件</h3>
        <button class="btn sm ghost" @click="openFolder('plugins')">打开 plugins 目录</button>
      </div>
      <div v-if="loadingFiles" class="empty-inline">加载中…</div>
      <div v-else-if="!fileList.length" class="empty-inline">
        plugins 目录为空，将插件 jar 放入 <code>plugins/</code> 目录后即可加载
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
        <h3><IconFile /> 服务器日志</h3>
        <div class="log-ops">
          <span class="run-dot" :class="{ on: running }"></span>
          <span class="run-text">{{ running ? "运行中" : "未运行" }}</span>
          <button class="btn sm ghost" title="复制全部日志" @click="copyLogs"><IconCopy /> 复制</button>
          <button class="btn sm ghost" title="导出日志文件" @click="exportLogs"><IconDownload /> 导出</button>
          <button class="btn sm ghost" @click="clearLogs">清空</button>
        </div>
      </div>
      <div ref="logBox" class="log-box">
        <div v-if="!logs.length" class="log-empty">启动服务器后日志会显示在这里</div>
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
        <h3><IconFolder /> 服务器文件</h3>
      </div>
      <p class="section-hint">左边浏览服务器目录，右边用内置编辑器修改文本文件</p>
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
        <div v-if="editor.loading" class="editor-loading">正在读取文件…</div>
        <div v-else-if="editor.error" class="editor-error">{{ editor.error }}</div>
        <textarea
          v-else
          v-model="editor.content"
          class="editor-textarea"
          spellcheck="false"
        ></textarea>
        <div class="editor-foot">
          <button class="btn ghost" @click="editor = null">取消</button>
          <button
            class="btn primary"
            :disabled="editor.loading || editor.saving"
            @click="saveEditor"
          >
            {{ editor.saving ? "保存中…" : "保存" }}
          </button>
        </div>
      </div>
    </n-modal>

    <!-- 删除确认 -->
    <div v-if="confirmDelete" class="mask" @click="confirmDelete = false">
      <div class="confirm-card glass" @click.stop>
        <h3>删除服务器</h3>
        <p>确定要删除服务器「<b>{{ server.name }}</b>」吗？所有服务器文件将被永久删除。</p>
        <div class="confirm-foot">
          <button class="btn ghost" @click="confirmDelete = false">取消</button>
          <button class="btn danger" @click="doDelete">删除</button>
        </div>
      </div>
    </div>
  </div>

  <div v-else class="loading">加载中…</div>
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
  background: rgba(255, 255, 255, 0.06);
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
  background: rgba(255, 255, 255, 0.04);
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
  background: rgba(255, 255, 255, 0.06);
  color: var(--text-1);
  border: 1px solid var(--border);
}
.btn.ghost:hover {
  background: rgba(255, 255, 255, 0.1);
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
  background: rgba(255, 255, 255, 0.03);
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
  background: rgba(255, 255, 255, 0.05);
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
  background: rgba(255, 255, 255, 0.04);
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
  background: rgba(0, 0, 0, 0.45);
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
  background: rgba(255, 255, 255, 0.05);
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
  background: rgba(255, 255, 255, 0.03);
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
  background: rgba(255, 255, 255, 0.03);
  cursor: pointer;
  font-family: inherit;
  text-align: left;
  transition: all 0.13s;
}
.java-item:hover {
  background: rgba(255, 255, 255, 0.07);
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
