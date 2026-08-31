import type { Component } from "vue";

export type Loader = "vanilla" | "fabric" | "quilt" | "forge" | "neoforge";

/** 右键菜单项；`sep` 为真时渲染为分隔线，其余字段忽略 */
export interface ContextMenuItem {
  key: string;
  label?: string;
  icon?: Component;
  /** 右侧显示的快捷键提示，仅作展示 */
  shortcut?: string;
  danger?: boolean;
  disabled?: boolean;
  sep?: boolean;
  action?: () => void;
}

export interface Settings {
  data_dir: string;
  java_path: string | null;
  max_memory_mb: number;
  min_memory_mb: number;
  memory_mode: string;
  jvm_args: string;
  game_args: string;
  download_threads: number;
  download_chunk_threads: number;
  curseforge_api_key: string | null;
  theme: string;
  theme_color: string;
  close_behavior: string;
  auto_launch: boolean;
  keep_open: boolean;
  ms_client_id: string;
  selected_account: string | null;
  proxy: string | null;
  /** 下载镜像源 id："official" | "bmclapi" | "custom" */
  mirror: string;
  /** 自定义镜像根地址（mirror === "custom" 时生效） */
  mirror_custom: string;
  background_image: string | null;
  background_blur: number;
  background_dim: number;
  glass_blur: number;
  show_home_hero: boolean;
  show_sidebar_collapse_btn: boolean;
  dismissed_update_version: string | null;
  auto_update: boolean;
}

/** 下载镜像源预设 */
export interface MirrorPreset {
  id: string;
  label: string;
  /** 镜像站根地址，官方源为空串 */
  base: string;
  desc: string;
}

export interface MirrorTestResult {
  ok: boolean;
  ms: number;
  url: string;
}

export interface JavaInfo {
  path: string;
  version: string;
  major: number;
  vendor: string;
  arch: string;
}

export interface StorageCategory {
  key: string;
  label: string;
  size: number;
  files: number;
}

export interface InstanceStorage {
  id: string;
  name: string;
  size: number;
  files: number;
}

export interface StorageStats {
  categories: StorageCategory[];
  instances: InstanceStorage[];
  servers: InstanceStorage[];
  total: number;
  instance_count: number;
  server_count: number;
  updated_at: number;
  cached: boolean;
}

export interface CacheClearResult {
  freed: number;
}

export interface Instance {
  id: string;
  name: string;
  mc_version: string;
  loader: Loader;
  loader_version: string | null;
  created: number;
  last_played: number | null;
  installed: boolean;
  icon: string | null;
  max_memory_mb: number | null;
  memory_mode: string | null;
  jvm_args: string | null;
  game_args: string | null;
  java_path: string | null;
  account_id: string | null;
  resolution: [number, number] | null;
  mods: InstalledContent[];
  resource_packs: InstalledContent[];
  shaders: InstalledContent[];
  is_symlink?: boolean;
  source_path?: string | null;
  /** 所属分组 id，null / undefined 表示未分组 */
  group?: string | null;
}

/** 实例分组（持久化在 instance_groups.json） */
export interface InstanceGroup {
  id: string;
  name: string;
  color: string | null;
  created: number;
}

export type Account =
  | { type: "offline"; uuid: string; username: string; created: number }
  | {
      type: "microsoft";
      uuid: string;
      username: string;
      created: number;
      msa_expires_at: number;
    };

export interface ProjectHit {
  provider: "modrinth" | "curseforge";
  id: string;
  slug: string;
  title: string;
  description: string;
  author: string;
  downloads: number;
  follows: number;
  icon_url: string;
  project_type: string;
  categories: string[];
  latest_version: string;
  game_versions: string[];
  updated: string;
  featured_image: string;
}

export interface ProjectFile {
  url: string;
  filename: string;
  size: number;
  primary: boolean;
  hashes?: Record<string, string>;
}

export interface ProjectVersion {
  id: string;
  name: string;
  version_number: string;
  version_type?: string;
  date_published: string;
  game_versions: string[];
  loaders: string[];
  files: ProjectFile[];
  dependencies?: unknown[];
  download_url?: string;
  filename?: string;
  size?: number;
  release_type?: number;
}

export interface ProjectDependency {
  projectId: string;
  title: string;
  slug: string;
  dependencyType: "required" | "optional" | "incompatible" | "embedded" | string;
}

export interface ContentItem {
  record: InstalledContent;
  exists: boolean;
}

export interface InstalledContent {
  filename: string;
  source: string;
  project_id: string | null;
  /** Modrinth/CurseForge 项目 slug，用于内容中心检索与中文名映射 */
  slug?: string | null;
  /** 后端按 WikiEntries 映射出的中文名（未命中为 null） */
  cn_name?: string | null;
  version_id: string | null;
  name: string | null;
  version: string | null;
  /** Mod 内部 id（fabric 的 sodium / forge 的 modId） */
  mod_id?: string | null;
  /** 作者列表 */
  authors?: string[] | null;
  /** Mod 描述 */
  description?: string | null;
  installed_at: number;
  size: number;
  icon: string | null;
  enabled: boolean;
}

export interface UpdateInfo {
  filename: string;
  projectId: string;
  currentVersion: string | null;
  latestVersion: string;
  latestVersionId: string;
  projectTitle: string | null;
  kind: string;
  provider: string;
}

export interface InstallProgressEvent {
  taskId: number;
  stage: string;
  message: string;
  done: number;
  total: number;
  instanceId?: string;
  instanceName?: string;
  source?: string;
  ok?: boolean;
}

export interface DownloadProgressEvent {
  taskId: number;
  phase: string;
  done: number;
  total: number;
  current: string;
  ok: boolean;
  bytesDone?: number;
  bytesTotal?: number;
  ts?: number;
  activeFiles?: ActiveFile[];
}

/** 正在下载的文件，由后端 `download.rs` 每 400ms 上报一次实时字节数 */
export interface ActiveFile {
  name: string;
  bytesDone: number;
  bytesTotal: number;
}

export interface NewsItem {
  title: string;
  description?: string;
  content?: string;
  author?: string;
  time: number;
  image?: string;
  image_alt?: string;
  url?: string;
  important?: boolean;
}

export interface LaunchLogEvent {
  instanceId: string;
  stream: "out" | "err";
  line: string;
}

export interface LaunchStateEvent {
  instanceId: string;
  state: "running" | "exited";
  pid: number;
  code: number | null;
}

// 实例的多人游戏服务器条目（来自游戏内 servers.json / servers.dat）
export interface ServerEntry {
  name: string;
  address: string;
  icon: string | null; // 原始 base64（无 data: 前缀）
}

/** 实例文件管理器中的一条目录项 */
export interface FsEntry {
  name: string;
  /** 相对实例根目录的路径（用 / 分隔） */
  rel: string;
  size: number;
  modified: number;
  is_dir: boolean;
  /** 小写扩展名，目录为空字符串 */
  ext: string;
}

// 经 Server List Ping 获取的实时状态
export interface ServerStatus {
  online: boolean;
  address: string;
  name: string | null;
  version: string | null;
  players_online: number | null;
  players_max: number | null;
  motd: string | null;
  favicon: string | null; // 完整 data:image/png;base64,...
  latency_ms: number | null;
  error: string | null;
}

// 本地托管的游戏服务器核心类型
export type ServerCore =
  | "vanilla"
  | "paper"
  | "spigot"
  | "purpur"
  | "forge"
  | "fabric";

// 陶瓦联机（Terracotta）
export interface TerracottaInfo {
  found: boolean;
  path: string | null;
  running: boolean;
  port: number | null;
  download_url: string;
  icon: string | null;
}

export interface TerracottaLaunch {
  port: number;
  ui_url: string;
  path: string;
}

// 陶瓦联机下载进度
export interface TerracottaDownloadProgress {
  downloaded: number;
  total: number;
  percent: number;
  extracting?: boolean;
  done?: boolean;
}

export type TerracottaRoomState =
  | "waiting"
  | "scanning"
  | "host-starting"
  | "host-ok"
  | "guest-connecting"
  | "guest-starting"
  | "guest-ok"
  | "exception";

// 用户在"多人游戏 → 服务器"中创建的本地服务器配置
export interface ServerConfig {
  id: string;
  name: string;
  core: ServerCore;
  mc_version: string;
  port: number;
  max_memory_mb: number;
  min_memory_mb: number;
  motd: string;
  eula: boolean;
  created: number;
  last_started: number | null;
  java_path: string | null;
  jvm_args: string | null;
  stop_command: string | null;
}

// 崩溃分析结果
export interface CrashDiagnosis {
  severity: "oom" | "jvm" | "lwjgl" | "java_ver" | "gl" | "mod" | "unknown";
  title: string;
  reason: string;
  advice: string;
  excerpt: string;
  exit_code: number | null;
  crash_report: string | null;
  affected_mods: string[];
}
