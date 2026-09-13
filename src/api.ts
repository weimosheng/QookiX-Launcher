import { invoke as rawInvoke } from "@tauri-apps/api/core";
import { trackStart, trackEnd, trackError } from "./loadingBar";
import type { PinItem } from "./stores/pins";
import type {
  Account,
  CacheClearResult,
  ContentItem,
  CrashDiagnosis,
  DependencyReport,
  ResolvedMissingMod,
  FsEntry,
  Instance,
  InstanceGroup,
  JavaInfo,
  MirrorPreset,
  MirrorTestResult,
  NewsItem,
  ProjectDependency,
  ProjectHit,
  ProjectVersion,
  ServerConfig,
  ServerEntry,
  ServerStatus,
  Settings,
  StorageStats,
  TerracottaInfo,
  TerracottaLaunch,
  UpdateInfo,
  PlaytimeStats,
  WorldBackupInfo,
  WorldInfo,
  ExportPreview,
  ExportSelection,
  IdentifiedMod,
  DiagnosticReport,
  DiagnosticReportEntry,
} from "./types";

/**
 * 包装 tauri invoke：只有**网络请求**（显式传 { net: true }）才触发顶部加载条。
 * 本地命令（读写文件、保存设置、列表查询等）默认静默，避免加载条随交互频繁闪烁。
 * 下载/安装类长任务由页面内进度 UI 负责，即使走网络也不传 net。
 * 顶部加载条的另一个出现时机是「页面加载」（路由切换，见 router.ts）。
 */
function invoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
  opts?: { net?: boolean }
): Promise<T> {
  if (!opts?.net) return rawInvoke<T>(cmd, args);
  trackStart();
  return rawInvoke<T>(cmd, args).then(
    (res) => {
      trackEnd();
      return res;
    },
    (err) => {
      trackError();
      trackEnd();
      throw err;
    },
  );
}

export const api = {
  // settings & java
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (patch: Record<string, unknown>) => invoke<Settings>("set_settings", { patch }),
  /** 关闭窗口确认弹窗：执行所选动作（remember 为 true 时写回设置，之后不再询问） */
  resolveCloseRequest: (action: "minimize" | "quit", remember: boolean) =>
    invoke<void>("resolve_close_request", { action, remember }),

  // pinned items (首页 / 侧边栏)
  getPins: () => invoke<PinItem[]>("get_pins"),
  setPins: (items: PinItem[]) => invoke<void>("set_pins", { items }),
  // 镜像测速：页面内已有独立加载态，不触发顶部加载条
  listMirrors: () => invoke<MirrorPreset[]>("list_mirrors"),
  testMirror: (base: string) => invoke<MirrorTestResult>("test_mirror", { base }),
  testProxy: (proxyMode: string, proxy: string | null) =>
    invoke<MirrorTestResult>("test_proxy", { proxyMode, proxy }),
  changeDataDir: (newDir: string, mode: "move" | "copy" | "pointer") =>
    invoke<{ ok: boolean; new_dir: string; need_restart: boolean }>("change_data_dir", {
      newDir,
      mode,
    }),
  autoDetectMemory: () =>
    invoke<{ total_mb: number; used_mb: number; available_mb: number; max_mb: number; min_mb: number }>("auto_detect_memory"),
  detectJava: (refresh?: boolean) =>
    invoke<{ candidates: JavaInfo[]; selected: JavaInfo | null }>("detect_java", {
      refresh: refresh ?? false,
    }),
  downloadJava: (major: number) => invoke<JavaInfo>("download_java", { major }),
  recommendJava: (instanceId: string) =>
    invoke<{ required: number; java: JavaInfo | null; needDownload: boolean }>("recommend_java", {
      instanceId,
    }),

  // versions
  getVersionManifest: () =>
    invoke<{
      versions: { id: string; type: string; releaseTime: string }[];
      latest: { release: string; snapshot: string };
    }>("get_version_manifest", undefined, { net: true }),
  getLoaderVersions: (loader: string, mc_version: string) =>
    invoke<string[]>("get_loader_versions", { loader, mcVersion: mc_version }, { net: true }),

  // instances
  listInstances: () => invoke<Instance[]>("list_instances"),
  getInstance: (id: string) => invoke<Instance>("get_instance_info", { id }),
  createInstance: (name: string, mc_version: string, loader: string, loader_version: string | null) =>
    invoke<Instance>("create_instance", { name, mcVersion: mc_version, loader, loaderVersion: loader_version }),
  updateInstance: (patch: Record<string, unknown>) => invoke<Instance>("update_instance_settings", { patch }),
  deleteInstance: (id: string) => invoke<void>("delete_instance", { id }),

  // instance groups
  listGroups: () => invoke<InstanceGroup[]>("list_instance_groups"),
  createGroup: (name: string, color?: string | null) =>
    invoke<InstanceGroup>("create_instance_group", { name, color: color ?? null }),
  renameGroup: (id: string, name: string, color?: string | null) =>
    invoke<InstanceGroup>("rename_instance_group", { id, name, color: color ?? null }),
  deleteGroup: (id: string) => invoke<void>("delete_instance_group", { id }),
  reorderGroups: (ids: string[]) => invoke<InstanceGroup[]>("reorder_instance_groups", { ids }),
  installGame: (instanceId: string) =>
    invoke<{ instance_id: string; total_bytes: number; file_count: number }>("install_game", { instanceId }),
  cancelInstall: () => invoke<void>("cancel_install"),
  launchInstance: (instanceId: string, world?: string, server?: string) =>
    invoke<{ pid: number; command: string[] }>(
      "launch_instance",
      {
        instanceId,
        world: world ?? null,
        server: server ?? null,
      }
    ),
  stopGame: () => invoke<void>("stop_game"),
  isGameRunning: () => invoke<boolean>("is_game_running"),
  openInstanceFolder: (instanceId: string, sub?: string) =>
    invoke<void>("open_instance_folder", { instanceId, sub: sub ?? null }),
  listInstanceFolders: (instanceId: string) =>
    invoke<{ folders: { name: string; exists: boolean }[] }>("list_instance_folders", { instanceId }),
  listInstanceFiles: (instanceId: string, sub: string) =>
    invoke<{
      files: {
        name: string;
        size: number;
        modified: number;
        isDir: boolean;
        path: string;
        icon: string | null;
      }[];
    }>("list_instance_files", { instanceId, sub }),
  importModpack: (filePath: string) => invoke<Instance>("import_modpack", { filePath }),
  importInstanceImage: (sourcePath: string) => invoke<string>("import_instance_image", { sourcePath }),
  importBackgroundImage: (sourcePath: string) => invoke<string>("import_background_image", { sourcePath }),
  scanMinecraftImport: (source: string) => invoke<void>("scan_minecraft_import", { source }),
  exportPreview: (instanceId: string) => invoke<ExportPreview>("export_preview", { instanceId }),
  identifyManualMods: (instanceId: string) =>
    invoke<IdentifiedMod[]>("identify_manual_mods", { instanceId }),
  collectDiagnostics: (instanceId?: string | null) =>
    invoke<DiagnosticReport>("collect_diagnostics", { instanceId: instanceId ?? null }),
  saveDiagnosticsReport: (path: string, content: string) =>
    invoke<void>("save_diagnostics_report", { path, content }),
  listDiagnosticsReports: () =>
    invoke<DiagnosticReportEntry[]>("list_diagnostics_reports"),
  readDiagnosticsReport: (filename: string) =>
    invoke<string>("read_diagnostics_report", { filename }),
  deleteDiagnosticsReport: (filename: string) =>
    invoke<void>("delete_diagnostics_report", { filename }),
  exportInstancePack: (instanceId: string, destPath: string, selection: ExportSelection) =>
    invoke<number>("export_instance_pack", { instanceId, destPath, selection }),
  importInstancePack: (filePath: string) =>
    invoke<{ instance: Instance; pendingDownloads: number }>("import_instance_pack", { filePath }),
  playtimeStats: () => invoke<PlaytimeStats>("playtime_stats"),
  logDebug: (msg: string) => invoke<void>("log_debug", { msg }),
  listWorldBackups: (instanceId: string, world: string) =>
    invoke<WorldBackupInfo[]>("list_world_backups", { instanceId, world }),
  createWorldBackup: (instanceId: string, world: string) =>
    invoke<WorldBackupInfo>("create_world_backup", { instanceId, world }),
  restoreWorldBackup: (instanceId: string, world: string, filename: string) =>
    invoke<void>("restore_world_backup", { instanceId, world, filename }),
  deleteWorldBackup: (instanceId: string, world: string, filename: string) =>
    invoke<void>("delete_world_backup", { instanceId, world, filename }),
  estimateDownload: (mcVersion: string) =>
    invoke<{
      download_files: number;
      download_bytes: number;
      assets_known: boolean;
    }>("estimate_download", { mcVersion }),
  estimateImport: (source: string, rawIds: string[]) =>
    invoke<{
      import_files: number;
      import_bytes: number;
    }>("estimate_import", { source, rawIds }),
  importMinecraftFolder: (
    source: string,
    name: string,
    rawIds: string[],
    mcVersions: string[],
    loaders: string[],
    loaderVersions: (string | null)[],
    mode: "copy" | "symlink"
  ) =>
    invoke<{
      instance_id: string;
      total_bytes: number;
      file_count: number;
      symlink_fallback?: boolean;
    }[]>("import_minecraft_folder", {
      source,
      name,
      rawIds,
      mcVersions,
      loaders,
      loaderVersions,
      mode,
    }),

  // accounts
  listAccounts: () => invoke<Account[]>("list_accounts"),
  loginOffline: (username: string) => invoke<Account>("login_offline", { username }),
  loginMsStart: () =>
    invoke<{ userCode: string; verificationUri: string; expiresIn: number }>(
      "login_ms_start",
      undefined,
      { net: true }
    ),
  // 轮询由登录弹窗自己的等待态呈现，不触发顶部加载条
  loginMsPoll: () => invoke<Account>("login_ms_poll"),
  logoutAccount: (uuid: string) => invoke<void>("logout_account", { uuid }),

  // browse & content
  browse: (
    provider: string,
    query: string,
    projectType: string,
    category: string,
    page: number,
    gameVersion?: string,
    loader?: string,
    sort?: string,
    pageSize?: number
  ) =>
    invoke<{ hits: ProjectHit[]; total: number; cf_error?: string | null; cf_count?: number }>("browse", {
      provider,
      query,
      projectType,
      category,
      page,
      gameVersion: gameVersion ?? "",
      loader: loader ?? "",
      sort: sort ?? "downloads",
      pageSize: pageSize ?? 20,
    }, { net: true }),
  projectVersions: (provider: string, projectId: string, mcVersion: string, loader: string) =>
    invoke<{ provider: string; versions: ProjectVersion[] }>("project_versions", {
      provider,
      projectId,
      mcVersion,
      loader,
    }, { net: true }),
  projectDependencies: (provider: string, projectId: string, versionId: string) =>
    invoke<ProjectDependency[]>(
      "project_dependencies",
      {
        provider,
        projectId,
        versionId,
      }
    ),
  mcWikiUrl: (name: string, slug?: string, provider?: string) =>
    invoke<string>("mc_wiki_url", { name, slug, provider }),
  curseforgeCategories: (projectType: string) =>
    invoke<{ categories: { id: number; name: string }[] }>(
      "curseforge_categories",
      { projectType },
      { net: true }
    ),
  projectInfo: (provider: string, projectId: string) =>
    invoke<ProjectHit>("project_info", { provider, projectId }, { net: true }),
  installContent: (
    instanceId: string,
    provider: string,
    projectId: string,
    versionId: string,
    kind: string
  ) =>
    invoke<{ ok: boolean; filename?: string; mods?: number }>(
      "install_content",
      {
        instanceId,
        provider,
        projectId,
        versionId,
        kind,
      }
    ),
  translateDescriptions: (provider: string, slugs: string[]) =>
    invoke<{ translations: Record<string, string>; failed: string[]; rateLimited: boolean }>(
      "translate_mod_descriptions",
      { provider, slugs }
    ),
  reportStaleTranslation: (provider: string, slug: string) =>
    invoke<string>("report_translation_stale", { provider, slug }, { net: true }),
  reportQualityFeedback: (
    provider: string,
    slug: string,
    issueType: string,
    userSuggestion?: string,
    userComment?: string
  ) =>
    invoke<string>(
      "report_translation_quality",
      {
        provider,
        slug,
        issueType,
        userSuggestion: userSuggestion ?? null,
        userComment: userComment ?? null,
      }
    ),
  clearTranslationCache: (service?: "default" | "custom") =>
    invoke<number>("clear_translation_cache", { service: service ?? null }),
  testTranslateApi: (base: string, key: string, model: string) =>
    invoke<void>("test_translate_api", { base, key, model }),
  checkDependencies: (instanceId: string) =>
    invoke<DependencyReport>("check_dependencies", { instanceId }),
  resolveMissingMods: (instanceId: string, modIds: string[]) =>
    invoke<ResolvedMissingMod[]>("resolve_missing_mods", { instanceId, modIds }, { net: true }),
  checkUpdates: (instanceId: string, kind: string) =>
    invoke<UpdateInfo[]>("check_updates", { instanceId, kind }, { net: true }),
  applyUpdate: (
    instanceId: string,
    kind: string,
    oldFilename: string,
    provider: string,
    projectId: string,
    newVersionId: string
  ) =>
    invoke<{ ok: boolean; filename?: string }>(
      "apply_update",
      {
        instanceId,
        kind,
        oldFilename,
        provider,
        projectId,
        newVersionId,
      }
    ),
  uninstallContent: (instanceId: string, kind: string, filename: string) =>
    invoke<void>("uninstall_content", { instanceId, kind, filename }),
  listContent: (instanceId: string, kind: string) =>
    invoke<{ items: ContentItem[]; onDisk: string[] }>("list_content", { instanceId, kind }),
  identifyContent: (instanceId: string, kind: string) =>
    invoke<void>("identify_content", { instanceId, kind }),
  toggleContentEnabled: (instanceId: string, kind: string, filename: string, enabled: boolean) =>
    invoke<void>("toggle_content_enabled", { instanceId, kind, filename, enabled }),
  importLocalFile: (instanceId: string, kind: string, sourcePath: string) =>
    invoke<{ ok: boolean }>("import_local_file", { instanceId, kind, sourcePath }),
  saveTextFile: (path: string, content: string) => invoke<void>("save_text_file", { path, content }),

  // instance file manager
  listInstanceDir: (instanceId: string, rel: string) =>
    invoke<{ rel: string; entries: FsEntry[] }>("list_instance_dir", { instanceId, rel }),
  readInstanceFile: (instanceId: string, rel: string) =>
    invoke<{ rel: string; content: string; size: number; modified: number }>(
      "read_instance_file",
      { instanceId, rel }
    ),
  writeInstanceFile: (instanceId: string, rel: string, content: string) =>
    invoke<{ rel: string; size: number; modified: number }>(
      "write_instance_file",
      { instanceId, rel, content }
    ),
  createInstanceEntry: (instanceId: string, rel: string, isDir: boolean) =>
    invoke<{ rel: string; is_dir: boolean }>("create_instance_entry", { instanceId, rel, isDir }),
  deleteInstancePath: (instanceId: string, rel: string) =>
    invoke<void>("delete_instance_path", { instanceId, rel }),
  renameInstancePath: (instanceId: string, rel: string, newName: string) =>
    invoke<{ rel: string; name: string }>("rename_instance_path", { instanceId, rel, newName }),
  revealInstancePath: (instanceId: string, rel: string) =>
    invoke<void>("reveal_instance_path", { instanceId, rel }),
  extractGameIcons: (instanceId?: string) =>
    invoke<{ name: string; label: string; path: string }[]>("extract_game_icons", {
      instanceId: instanceId ?? null,
    }),

  // skins
  listSkins: () =>
    invoke<{ name: string; filename: string; path: string; size: number; modified: number }[]>("list_skins"),
  readSkinDataUrl: (filename: string) => invoke<string>("read_skin_data_url", { filename }),
  saveSkinFromData: (name: string, data: string) =>
    invoke<{ name: string; filename: string; path: string; size: number; modified: number }>("save_skin_from_data", {
      name,
      data,
    }),
  downloadSkinFromUrl: (name: string, url: string) =>
    invoke<{ name: string; filename: string; path: string; size: number; modified: number }>(
      "download_skin_from_url",
      { name, url },
      { net: true }
    ),
  deleteSkin: (filename: string) => invoke<void>("delete_skin", { filename }),
  fetchPlayerSkin: (username: string) =>
    invoke<{ data_url: string; model: string; cape_data_url: string | null }>(
      "fetch_player_skin",
      { username },
      { net: true }
    ),
  fetchImageDataURL: (url: string) =>
    invoke<string>("fetch_image_data_url", { url }, { net: true }),
  fetchPlayerCapes: (accountUuid: string) =>
    invoke<{ id: string; name: string; data_url: string; active: boolean }[]>(
      "fetch_player_capes",
      { accountUuid },
      { net: true }
    ),
  applySkinToAccount: (accountUuid: string, skinData: string, variant: string) =>
    invoke<void>("apply_skin_to_account", { accountUuid, skinData, variant }, { net: true }),
  applyCapeToAccount: (accountUuid: string, capeId: string | null) =>
    invoke<void>("apply_cape_to_account", { accountUuid, capeId }, { net: true }),
  applySkinOffline: (skinData: string, variant: string, uuid: string) =>
    invoke<void>("apply_skin_offline", { skinData, variant, uuid }),
  getOfflineSkin: (uuid: string) =>
    invoke<{ src: string; variant: "slim" | "classic" | null } | null>("get_offline_skin", {
      uuid,
    }),

  // multiplayer servers
  listServers: (instanceId: string) =>
    invoke<{ servers: ServerEntry[] }>("list_servers", { instanceId }).then((r) => r.servers),
  pingServer: (address: string) => invoke<ServerStatus>("ping_mc_server", { address }, { net: true }),

  // hosted game servers
  listHostedServers: () => invoke<ServerConfig[]>("list_hosted_servers"),
  getHostedServer: (id: string) => invoke<ServerConfig>("get_hosted_server", { id }),
  createHostedServer: (name: string, core: string, mcVersion: string) =>
    invoke<ServerConfig>("create_hosted_server", { name, core, mcVersion }),
  updateHostedServer: (patch: Record<string, unknown>) =>
    invoke<ServerConfig>("update_hosted_server", { patch }),
  deleteHostedServer: (id: string) => invoke<void>("delete_hosted_server", { id }),
  installHostedServerCore: (id: string) =>
    invoke<void>("install_hosted_server_core", { id }),
  startHostedServer: (id: string) =>
    invoke<{ pid: number }>("start_hosted_server", { id }),
  stopHostedServer: (id: string) => invoke<void>("stop_hosted_server", { id }),
  isHostedServerRunning: (id: string) => invoke<boolean>("is_hosted_server_running", { id }),
  readHostedServerLog: (id: string) => invoke<string[]>("read_hosted_server_log", { id }),
  openHostedServerFolder: (id: string, sub?: string) =>
    invoke<void>("open_hosted_server_folder", { id, sub: sub ?? null }),
  listHostedServerFolders: (id: string) =>
    invoke<{ folders: { name: string; exists: boolean }[] }>("list_hosted_server_folders", { id }),
  listHostedServerFiles: (id: string, sub: string) =>
    invoke<{
      files: { name: string; path: string; size: number; modified: number; isDir: boolean; icon: string | null }[];
    }>("list_hosted_server_files", { id, sub }),
  listHostedServerDir: (id: string, rel: string) =>
    invoke<{ rel: string; entries: FsEntry[] }>("list_hosted_server_dir", { id, rel }),
  revealHostedServerPath: (id: string, rel: string) =>
    invoke<void>("reveal_hosted_server_path", { id, rel }),
  readHostedServerFile: (id: string, rel: string) =>
    invoke<{ rel: string; content: string; size: number; modified: number }>(
      "read_hosted_server_file",
      { id, rel }
    ),
  writeHostedServerFile: (id: string, rel: string, content: string) =>
    invoke<{ rel: string; size: number; modified: number }>(
      "write_hosted_server_file",
      { id, rel, content }
    ),
  listHostedServerConfigFiles: (id: string) =>
    invoke<{ name: string; rel: string; size: number; modified: number }[]>(
      "list_hosted_server_config_files",
      { id },
    ),

  // terracotta (陶瓦联机)：高频轮询与状态操作，页面内已有独立加载反馈，不触发顶部加载条
  terracottaDetect: () => invoke<TerracottaInfo>("terracotta_detect"),
  terracottaDownload: () => invoke<string>("terracotta_download"),
  terracottaLaunch: () => invoke<TerracottaLaunch>("terracotta_launch"),
  terracottaStop: () => invoke<void>("terracotta_stop"),
  terracottaStatus: () => invoke<Record<string, unknown>>("terracotta_status"),
  terracottaCreateRoom: (player?: string) =>
    invoke<Record<string, unknown>>("terracotta_create_room", { player: player ?? null }),
  terracottaJoinRoom: (room: string, player?: string) =>
    invoke<Record<string, unknown>>("terracotta_join_room", { room, player: player ?? null }),
  terracottaLeave: () => invoke<Record<string, unknown>>("terracotta_leave"),

  // storage
  getStorageStats: () => invoke<StorageStats>("get_storage_stats"),
  refreshStorageStats: () => invoke<StorageStats>("refresh_storage_stats"),
  clearCache: () => invoke<CacheClearResult>("clear_cache"),

  yggdrasilLogin: (serverUrl: string, username: string, password: string) =>
    invoke<{
      accessToken: string;
      clientToken: string;
      server: string;
      serverName: string;
      links: { homepage: string; register: string };
      profiles: { id: string; name: string }[];
    }>("yggdrasil_login", { serverUrl, username, password }),
  yggdrasilAddAccount: (payload: {
    serverUrl: string;
    serverName: string;
    accessToken: string;
    clientToken: string;
    profileId: string;
    profileName: string;
  }) => invoke<Account>("yggdrasil_add_account", payload),
  /** 拉取皮肤站账号的皮肤/披风（data URL），无纹理时为 null */
  yggdrasilTextures: (server: string, profileId: string) =>
    invoke<{ skin: string | null; cape: string | null } | null>(
      "yggdrasil_textures",
      { server, profileId }
    ),
  /** 正文：translate=false 仅拉原文，true 时翻译（内置服务仅 Modrinth） */
  translateBody: (provider: string, slug: string, translate: boolean) =>
    invoke<{
      body: string | null;
      bodyCached: boolean;
      original: string;
      supported: boolean;
      error?: string;
    }>("translate_project_body", { provider, slug, translate }, { net: true }),
  // crash analysis
  crashAnalysis: (instanceId: string) =>
    invoke<{ filename: string; modified: number; size: number; kind: string }[]>("list_crash_logs", { id: instanceId }),
  analyzeCrash: (instanceId: string, filename: string) =>
    invoke<CrashDiagnosis>("analyze_crash_log", { id: instanceId, filename }),
  getCrashReportContent: (instanceId: string, filename: string) =>
    invoke<string>("get_crash_report_content", { id: instanceId, filename }),

  // news
  fetchNews: () => invoke<NewsItem[]>("fetch_news", undefined, { net: true }),

  // toolbox (工具箱：种子地图，底层 cubiomes)
  // 种子统一用十进制字符串传：64 位种子超出 JS 安全整数范围，number 会被静默舍入
  toolboxQueryBiome: (
    seed: string,
    mc: string,
    dim: number,
    x: number,
    y: number,
    z: number,
    largeBiomes = false,
  ) => invoke<{ id: number; name: string }>("toolbox_query_biome", { seed, mc, dim, x, y, z, largeBiomes }),
  toolboxBiomeTable: () =>
    invoke<{ id: number; name: string; depth: number; scale: number }[]>("toolbox_biome_table"),
  toolboxQueryStructures: (
    seed: string,
    mc: string,
    centerX: number,
    centerZ: number,
    radiusChunks: number,
    types: string[],
  ) =>
    invoke<{ type: string; x: number; z: number; region_x: number; region_z: number }[]>(
      "toolbox_query_structures",
      { seed, mc, centerX, centerZ, radiusChunks, types },
    ),
  toolboxSlimeChunk: (seed: string, chunkX: number, chunkZ: number) =>
    invoke<boolean>("toolbox_slime_chunk", { seed, chunkX, chunkZ }),
  /** 主世界出生点（cubiomes getSpawn） */
  toolboxWorldSpawn: (seed: string, mc: string, largeBiomes = false) =>
    invoke<{ x: number; z: number }>("toolbox_world_spawn", { seed, mc, largeBiomes }),
  /** 估算 (x,z) 处的地表高度（方块），给 /tp 指令的 y 用 */
  toolboxSurfaceHeight: (seed: string, mc: string, dim: number, x: number, z: number) =>
    invoke<number>("toolbox_surface_height", { seed, mc, dim, x, z }),
  /**
   * 生成一块地图瓦片，返回原始字节（ArrayBuffer）：
   *   u32 width | u32 height | u32 shadeSize | u32 reserved
   *   群系 id 字节（width*height）| 山体阴影亮度字节（shadeSize²）
   * 用原始字节而不是 JSON 数组，单块只有几十 KB，前端也无需解析数字数组。
   */
  toolboxQueryBiomeMap: (
    seed: string,
    mc: string,
    dim: number,
    centerX: number,
    centerZ: number,
    size: number,
    scale: number,
    largeBiomes = false,
    wantShade = true,
  ) =>
    invoke<ArrayBuffer>("toolbox_query_biome_map", {
      seed, mc, dim, centerX, centerZ, size, scale, largeBiomes, wantShade,
    }),
  /** 读取存档的世界种子 + 存档版本（导入实例存档用） */
  toolboxReadWorldInfo: (instanceId: string, world: string) =>
    invoke<WorldInfo>("toolbox_read_world_info", { instanceId, world }),
};
