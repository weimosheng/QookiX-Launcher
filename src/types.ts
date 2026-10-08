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

/**
 * 前端可见的设置（后端 `settings::frontend_view` 的投影）。
 * API Key 这类敏感字段不会下发，只有 `*_set` / `*_hint`。
 */
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
  /** CurseForge API Key 是否已配置（Key 原文不下发前端） */
  curseforge_api_key_set: boolean;
  /** 已保存的 CurseForge Key 尾号（如 `····ab12`），未配置为空串 */
  curseforge_api_key_hint: string;
  theme: string;
  theme_color: string;
  /** 界面语言："zh-CN" | "en-US" */
  language: string;
  /** 关闭窗口行为："ask"（每次询问，默认）| "minimize" | "quit" */
  close_behavior: string;
  auto_launch: boolean;
  keep_open: boolean;
  ms_client_id: string;
  selected_account: string | null;
  /** 下载代理模式："system"（系统代理）| "direct"（直连）| "custom"（自定义） */
  proxy_mode: string;
  /** 自定义代理地址（proxy_mode === "custom" 时生效） */
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
  /** 新闻页面与侧边栏新闻入口是否显示（默认 true） */
  show_news: boolean;
  /** 用户在设置页选择隐藏的侧边栏导航项 name 列表 */
  hidden_nav_items: string[];
  dismissed_update_version: string | null;
  auto_update: boolean;
  /** 应用自更新源："bucket"（对象存储，默认） | "github"（GitHub Releases 官方源） */
  update_source: "bucket" | "github";
  /** 内容描述翻译服务："default"（自建翻译服务） | "custom"（OpenAI 兼容接口） */
  translate_provider: string;
  /** 打开内容详情时自动加载正文译文 */
  body_translate_auto: boolean;
  /** 自定义翻译 API 的 OpenAI 兼容地址 */
  translate_api_base: string;
  /** 自定义翻译 Key 是否已配置（Key 原文不下发前端） */
  translate_api_key_set: boolean;
  /** 已保存的翻译 Key 尾号（如 `····ab12`） */
  translate_api_key_hint: string;
  /** 自定义翻译使用的模型名 */
  translate_api_model: string;
  /** 新手向导是否已完成（首次启动为 false，完成后置 true） */
  onboarding_completed: boolean;
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
  /** 累计游玩时长（秒），由后端在游戏进程退出时累加 */
  total_play_time: number;
  installed: boolean;
  icon: string | null;
  /** 实例别名（qookix://launch/<alias> 协议启动用，全局唯一） */
  alias?: string | null;
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
  }
  | {
  type: "yggdrasil";
  uuid: string;
  username: string;
  created: number;
  /** 皮肤站 Yggdrasil API root */
  server: string;
  /** 展示名（如 LittleSkin） */
  server_name: string;
  };

export interface ProjectHit {
  /** spigot 来源仅用于展示/翻译，不支持程序化安装 */
  provider: "modrinth" | "curseforge" | "spigot";
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

/** 世界存档备份快照元信息 */
export interface WorldBackupInfo {
  filename: string;
  size: number;
  /** unix 秒 */
  modified: number;
}

/** NBT 编辑：世界列表条目 */
export interface WorldSummary {
  /** saves 下的目录名，手动指定时是绝对路径 */
  dir: string;
  levelName: string | null;
  gameType: number | null;
  difficulty: number | null;
  seed: number | null;
  size: number;
  /** unix 秒 */
  modified: number;
  /** 有 playerdata 的玩家数量 */
  playerCount?: number;
  /** 第一个玩家名（usercache 解析不到时是 UUID 短串） */
  playerName?: string | null;
}

/** NBT 编辑：level.dat 表单字段 */
export interface WorldForm {
  levelName: string | null;
  gameType: number | null;
  difficulty: number | null;
  difficultyLocked: number | null;
  hardcore: number | null;
  /** 十进制字符串：种子是 Long，超过 JS 安全整数范围，走数字会丢精度 */
  seed: string | null;
  time: number | null;
  dayTime: number | null;
  raining: number | null;
  thundering: number | null;
  spawnX: number | null;
  spawnY: number | null;
  spawnZ: number | null;
  allowCommands: number | null;
  /** 游戏规则：值统一是字符串（"true"/"false"/数字） */
  gameRules: Record<string, string> | null;
  border: { centerX: number | null; centerZ: number | null; size: number | null } | null;
  /** 边界用扁平的 Border* 字段存（1.16-）还是 WorldBorder 子复合节点（部分版本） */
  borderFlat: boolean | null;
}

/** NBT 编辑：已生成的区块（从 region 文件头部读出） */
export interface ChunkEntry {
  cx: number;
  cz: number;
  /** 占用的字节数（按扇区对齐） */
  size: number;
  /** unix 秒 */
  modified: number;
}

/** 区块地图：存档里已生成区块的范围 */
export interface MapBounds {
  empty: boolean;
  minCx?: number;
  minCz?: number;
  maxCx?: number;
  maxCz?: number;
  chunks?: number;
  /** 方块坐标：区块最密集的区域中心（存档跨度大时用它当初始视野） */
  denseX?: number;
  denseZ?: number;
}

/** 区块地图：一次渲染的结果（每像素一个调色板下标） */
export interface MapImageData {
  originX: number;
  originZ: number;
  /** 每个像素代表多少方块 */
  step: number;
  width: number;
  height: number;
  /** 真正画进图里的区块数 */
  rendered: number;
  /** 拍平的 RGB（每 3 个一组） */
  palette: number[];
  /** base64：width*height 个调色板下标 */
  data: string;
  /** 图例：视野里占比最高的几种方块 */
  legend: { name: string; color: string; pixels: number }[];
}

/** NBT 编辑：单文件备份条目 */
export interface NbtBackupInfo {
  /** 相对世界目录的路径：level.dat 或 playerdata/<uuid>.dat */
  file: string;
  name: string;
  size: number;
  /** unix 秒 */
  modified: number;
}

/** NBT 编辑：玩家条目 */
export interface PlayerSummary {
  uuid: string;
  name: string;
  pos: number[];
  dimension: string | null;
  health: number | null;
}

/** NBT 编辑：玩家可编辑数据 */
export interface PlayerData {
  pos: number[];
  dimension: string;
  health: number;
  foodLevel: number;
  xpLevel: number;
  xpP: number;
  gameType: number;
  abilities: { flying: number; mayfly: number } | null;
}

/** NBT 编辑：物品槽位 */
export interface ItemSlot {
  /** 在 NBT 列表里的下标（写回时的兜底定位依据） */
  index: number;
  /** 实际槽位号（0-35 背包、36-39 护甲、40 副手） */
  slot: number;
  id: string;
  count: number;
  name: string | null;
  lore: string[];
  unbreakable: number;
  enchantments: { id: string; level: number }[];
  modifiers: {
    name: string;
    attribute: string;
    amount: number;
    operation: number;
    slot: string | null;
  }[];
  /** 1.20.5+ 的 components 格式 */
  modern: boolean;
}

/** NBT 树节点（树形模式） */
export interface NbtNode {
  name: string;
  type: string;
  value?: unknown;
  children?: NbtNode[];
}

/**
 * 时光机时间轴的一条记忆点（本地 + 云端合并后的统一形态）。
 *
 * 本地优先：`localPath` 有值时前端直接显示（不用先下载）；只有云端条目才回源下载。
 */
export interface TimelineShot {
  /** 唯一 key：`local:<实例>:<文件>` 或 `cloud:<release>:<asset>` */
  id: string;
  instanceId: string;
  /** 截图文件名（本地文件 / 云端附件名） */
  file: string;
  /** 触发来源：interval / world_enter / death / advance… / manual */
  trigger: string | null;
  /** unix 秒 */
  createdAt: number;
  /** 是否已经传上云端 */
  uploaded: boolean;
  /** 本地文件绝对路径（已删/纯云端条目为 null） */
  localPath: string | null;
  /** 截图那一刻游戏日志的末尾几行（老截图没有） */
  logSnippet: string | null;
  releaseId: number;
  assetId: number;
}

/** 时间轴上的世界快照事件（本地备份 / 云端快照），对应"回到那一刻" */
export interface TimelineSnapshot {
  kind: "local" | "cloud";
  instanceId: string | null;
  instanceName: string | null;
  /** 世界目录名（云端为 worldId） */
  world: string;
  worldName: string | null;
  /** unix 秒 */
  createdAt: number;
  size: number;
  /** 本地备份文件名（kind=local） */
  file?: string;
  /** 云端（kind=cloud） */
  releaseId?: number;
  assetId?: number;
}

/** 单次游玩（启动游戏 → 退出）；「冒险日志」按它把截图分组成"这次游玩" */
export interface PlaySession {
  id: number;
  instanceId: string;
  /** unix 秒 */
  startedAt: number;
  /** unix 秒；进程被系统直接杀掉时可能为 null（表示"未记录结束"） */
  endedAt: number | null;
}

/** 云端存档快照（GitHub Release；大存档可能有多个分卷附件） */
export interface CloudSnapshot {
  releaseId: number;
  tag: string;
  /** ISO8601 */
  createdAt: string;
  assetId: number;
  /** 全部分卷附件 id（按分卷顺序） */
  assetIds: number[];
  assetSize: number;
  /** 分卷数（1 = 单文件） */
  partCount: number;
  worldId: string;
  worldName: string | null;
  /** 上传时所属的实例（旧快照可能为 null） */
  instanceId: string | null;
  instanceName: string | null;
  gameVersion: string | null;
  sha256: string | null;
  /** 时光机：这张截图由什么触发（interval / death / world_enter / custom:<id> …） */
  trigger?: string | null;
}

/** 从存档 level.dat 里读出的世界信息 */
export interface WorldInfo {
  /** 种子的十进制字符串（64 位，可能超出 JS 安全整数范围）；随机种子世界为 null */
  seed: string | null;
  /** `Data.Version.Name`，如 "26.2"；老存档可能没有 */
  version: string | null;
}

/** 实例导出预览：一个可勾选条目 */
export interface ExportItem {
  key: string;
  label: string;
  size: number;
  hint?: string | null;
}

/** 实例导出预览：一组勾选项 */
export interface ExportGroup {
  key: string;
  label: string;
  /** 必含（游戏本体），不可取消 */
  required: boolean;
  hint?: string | null;
  items: ExportItem[];
}

export interface ExportPreview {
  name: string;
  mcVersion?: string;
  mc_version: string;
  loader: string;
  loader_version?: string | null;
  groups: ExportGroup[];
}

/** 诊断报告的一节 */
export interface DiagnosticSection {
  key: string;
  title: string;
  lines: string[];
  /** 长文本（日志等） */
  block?: string | null;
}

export interface DiagnosticReport {
  generated_at: number;
  sections: DiagnosticSection[];
  /** 完整 Markdown 报告（复制 / 另存用） */
  markdown: string;
  /** 已脱敏替换处数 */
  redactions: number;
}

/** 已持久化的历史诊断报告 */
export interface DiagnosticReportEntry {
  filename: string;
  generated_at: number;
  size: number;
}

/** 未登记模组在 Modrinth 上的识别结果 */
export interface IdentifiedMod {
  filename: string;
  projectId: string;
  versionId: string;
  name: string;
  /** "hash" 精确匹配 | "name" 按文件名猜测（可能错配） */
  confidence: string;
}

/** 导出时实际提交的选择 */
export interface ExportSelection {
  name?: string | null;
  version?: string | null;
  mods: string[];
  includeDisabledMods?: boolean;
  resourcepacks: string[];
  shaders: string[];
  screenshots: string[];
  worlds: string[];
  folders: string[];
  optionsTxt?: boolean;
  serversDat?: boolean;
  /** 在线来源的模组直接打包文件（默认只记引用，导入时重新下载） */
  bundleOnlineFiles?: boolean;
  /** 打包在线文件时仅限 Modrinth 来源（规避 CurseForge 分发协议限制） */
  modrinthOnly?: boolean;
}

/** 游玩时长统计（后端聚合） */
export interface PlaytimeStats {
  totalSeconds: number;
  byInstance: {
    id: string;
    name: string;
    icon: string | null;
    seconds: number;
    lastPlayed: number | null;
  }[];
  /** 最近 30 天，day 为 (unix+8h)/86400 的天数索引 */
  byDay: { day: number; seconds: number }[];
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

export interface MissingDependency {
  modId: string;
  requirement: string;
  kind: string;
  requiredBy: string[];
  disabledFile: string | null;
}

export interface DependencyReport {
  instanceId: string;
  checkedMods: number;
  unreadable: number;
  missing: MissingDependency[];
  duplicates: { modId: string; files: string[] }[];
}

export interface ResolvedMissingMod {
  modId: string;
  provider: string;
  projectId: string;
  slug: string;
  title: string;
  icon: string;
  downloads: number;
  latestVersionId: string;
  exact: boolean;
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
// 主因字段（severity/title/reason/advice）取自置信度最高的一条，兼容旧展示逻辑；
// causes 里是全量命中原因，stacktrace/details 是报告结构化解析结果。
export type CrashSeverity =
  | "oom"
  | "jvm"
  | "lwjgl"
  | "java_ver"
  | "gl"
  | "mod"
  | "unknown";

export interface CrashCause {
  id: string;
  severity: CrashSeverity;
  title: string;
  reason: string;
  advice: string;
  /** 命中该原因的证据（崩溃报告原文片段） */
  evidence: string;
  /** 置信度 0-100 */
  confidence: number;
}

export interface CrashDetail {
  key: string;
  value: string;
}

export interface CrashDiagnosis {
  severity: CrashSeverity;
  title: string;
  reason: string;
  advice: string;
  excerpt: string;
  exit_code: number | null;
  crash_report: string | null;
  affected_mods: string[];
  /** 全部命中原因，按置信度降序 */
  causes: CrashCause[];
  /** 关键堆栈帧 */
  stacktrace: string[];
  /** 环境信息（Minecraft / Java / 内存 / 显卡 / 系统） */
  details: CrashDetail[];
  /** 主因置信度 0-100，0 表示未能定位 */
  confidence: number;
}
