import { invoke as rawInvoke } from "@tauri-apps/api/core";
import { trackStart, trackEnd, trackError } from "./loadingBar";
import type { PinItem } from "./stores/pins";
import type {
  Account,
  CacheClearResult,
  ContentItem,
  CrashDiagnosis,
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
} from "./types";

/**
 * 包装 tauri invoke：默认触发顶部加载条（trackStart/trackEnd）。
 * 页面内已有独立加载反馈的高频/长任务命令，须在调用点显式传 { silent: true }——
 * 显式声明替代旧的 SILENT_COMMANDS 全局黑名单，新命令默认有加载条，静默必须声明。
 */
function invoke<T>(
  cmd: string,
  args?: Record<string, unknown>,
  opts?: { silent?: boolean }
): Promise<T> {
  const silent = opts?.silent ?? false;
  if (!silent) trackStart();
  return rawInvoke<T>(cmd, args).then(
    (res) => {
      if (!silent) trackEnd();
      return res;
    },
    (err) => {
      if (!silent) {
        trackError();
        trackEnd();
      }
      throw err;
    },
  );
}

export const api = {
  // settings & java
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (patch: Record<string, unknown>) => invoke<Settings>("set_settings", { patch }),

  // pinned items (首页 / 侧边栏)
  getPins: () => invoke<PinItem[]>("get_pins"),
  setPins: (items: PinItem[]) => invoke<void>("set_pins", { items }),
  // 镜像测速：页面内已有独立加载态，不触发顶部加载条
  listMirrors: () => invoke<MirrorPreset[]>("list_mirrors", undefined, { silent: true }),
  testMirror: (base: string) => invoke<MirrorTestResult>("test_mirror", { base }, { silent: true }),
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
  downloadJava: (major: number) => invoke<JavaInfo>("download_java", { major }, { silent: true }),
  recommendJava: (instanceId: string) =>
    invoke<{ required: number; java: JavaInfo | null; needDownload: boolean }>("recommend_java", {
      instanceId,
    }),

  // versions
  getVersionManifest: () =>
    invoke<{
      versions: { id: string; type: string; releaseTime: string }[];
      latest: { release: string; snapshot: string };
    }>("get_version_manifest"),
  getLoaderVersions: (loader: string, mc_version: string) =>
    invoke<string[]>("get_loader_versions", { loader, mcVersion: mc_version }),

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
    invoke<{ instance_id: string; total_bytes: number; file_count: number }>("install_game", { instanceId }, { silent: true }),
  cancelInstall: () => invoke<void>("cancel_install"),
  launchInstance: (instanceId: string, world?: string, server?: string) =>
    invoke<{ pid: number; command: string[] }>(
      "launch_instance",
      {
        instanceId,
        world: world ?? null,
        server: server ?? null,
      },
      { silent: true }
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
  loginMsStart: () => invoke<{ userCode: string; verificationUri: string; expiresIn: number }>("login_ms_start"),
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
    }),
  projectVersions: (provider: string, projectId: string, mcVersion: string, loader: string) =>
    invoke<{ provider: string; versions: ProjectVersion[] }>("project_versions", {
      provider,
      projectId,
      mcVersion,
      loader,
    }),
  projectDependencies: (provider: string, projectId: string, versionId: string) =>
    invoke<ProjectDependency[]>(
      "project_dependencies",
      {
        provider,
        projectId,
        versionId,
      },
      { silent: true }
    ),
  mcWikiUrl: (name: string, slug?: string, provider?: string) =>
    invoke<string>("mc_wiki_url", { name, slug, provider }, { silent: true }),
  curseforgeCategories: (projectType: string) =>
    invoke<{ categories: { id: number; name: string }[] }>("curseforge_categories", { projectType }),
  projectInfo: (provider: string, projectId: string) =>
    invoke<ProjectHit>("project_info", { provider, projectId }),
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
      },
      { silent: true }
    ),
  checkUpdates: (instanceId: string, kind: string) =>
    invoke<UpdateInfo[]>("check_updates", { instanceId, kind }),
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
      },
      { silent: true }
    ),
  uninstallContent: (instanceId: string, kind: string, filename: string) =>
    invoke<void>("uninstall_content", { instanceId, kind, filename }),
  listContent: (instanceId: string, kind: string) =>
    invoke<{ items: ContentItem[]; onDisk: string[] }>("list_content", { instanceId, kind }),
  identifyContent: (instanceId: string, kind: string) =>
    invoke<void>("identify_content", { instanceId, kind }, { silent: true }),
  toggleContentEnabled: (instanceId: string, kind: string, filename: string, enabled: boolean) =>
    invoke<void>("toggle_content_enabled", { instanceId, kind, filename, enabled }),
  importLocalFile: (instanceId: string, kind: string, sourcePath: string) =>
    invoke<{ ok: boolean }>("import_local_file", { instanceId, kind, sourcePath }),
  saveTextFile: (path: string, content: string) => invoke<void>("save_text_file", { path, content }),

  // instance file manager
  listInstanceDir: (instanceId: string, rel: string) =>
    invoke<{ rel: string; entries: FsEntry[] }>("list_instance_dir", { instanceId, rel }, { silent: true }),
  readInstanceFile: (instanceId: string, rel: string) =>
    invoke<{ rel: string; content: string; size: number; modified: number }>(
      "read_instance_file",
      { instanceId, rel },
      { silent: true }
    ),
  writeInstanceFile: (instanceId: string, rel: string, content: string) =>
    invoke<{ rel: string; size: number; modified: number }>(
      "write_instance_file",
      { instanceId, rel, content },
      { silent: true }
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
    ),
  deleteSkin: (filename: string) => invoke<void>("delete_skin", { filename }),
  fetchPlayerSkin: (username: string) =>
    invoke<{ data_url: string; model: string; cape_data_url: string | null }>("fetch_player_skin", { username }),
  fetchImageDataURL: (url: string) => invoke<string>("fetch_image_data_url", { url }),
  fetchPlayerCapes: (accountUuid: string) =>
    invoke<{ id: string; name: string; data_url: string; active: boolean }[]>("fetch_player_capes", {
      accountUuid,
    }),
  applySkinToAccount: (accountUuid: string, skinData: string, variant: string) =>
    invoke<void>("apply_skin_to_account", { accountUuid, skinData, variant }),
  applyCapeToAccount: (accountUuid: string, capeId: string | null) =>
    invoke<void>("apply_cape_to_account", { accountUuid, capeId }),
  applySkinOffline: (skinData: string, variant: string, uuid: string) =>
    invoke<void>("apply_skin_offline", { skinData, variant, uuid }),
  getOfflineSkin: (uuid: string) =>
    invoke<{ src: string; variant: "slim" | "classic" | null } | null>("get_offline_skin", {
      uuid,
    }),

  // multiplayer servers
  listServers: (instanceId: string) =>
    invoke<{ servers: ServerEntry[] }>("list_servers", { instanceId }).then((r) => r.servers),
  pingServer: (address: string) => invoke<ServerStatus>("ping_mc_server", { address }),

  // hosted game servers
  listHostedServers: () => invoke<ServerConfig[]>("list_hosted_servers"),
  getHostedServer: (id: string) => invoke<ServerConfig>("get_hosted_server", { id }),
  createHostedServer: (name: string, core: string, mcVersion: string) =>
    invoke<ServerConfig>("create_hosted_server", { name, core, mcVersion }),
  updateHostedServer: (patch: Record<string, unknown>) =>
    invoke<ServerConfig>("update_hosted_server", { patch }),
  deleteHostedServer: (id: string) => invoke<void>("delete_hosted_server", { id }),
  installHostedServerCore: (id: string) =>
    invoke<void>("install_hosted_server_core", { id }, { silent: true }),
  startHostedServer: (id: string) =>
    invoke<{ pid: number }>("start_hosted_server", { id }, { silent: true }),
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
      { id, rel },
      { silent: true }
    ),
  writeHostedServerFile: (id: string, rel: string, content: string) =>
    invoke<{ rel: string; size: number; modified: number }>(
      "write_hosted_server_file",
      { id, rel, content },
      { silent: true }
    ),
  listHostedServerConfigFiles: (id: string) =>
    invoke<{ name: string; rel: string; size: number; modified: number }[]>(
      "list_hosted_server_config_files",
      { id },
    ),

  // terracotta (陶瓦联机)：高频轮询与状态操作，页面内已有独立加载反馈，不触发顶部加载条
  terracottaDetect: () => invoke<TerracottaInfo>("terracotta_detect", undefined, { silent: true }),
  terracottaDownload: () => invoke<string>("terracotta_download", undefined, { silent: true }),
  terracottaLaunch: () => invoke<TerracottaLaunch>("terracotta_launch", undefined, { silent: true }),
  terracottaStop: () => invoke<void>("terracotta_stop", undefined, { silent: true }),
  terracottaStatus: () => invoke<Record<string, unknown>>("terracotta_status", undefined, { silent: true }),
  terracottaCreateRoom: (player?: string) =>
    invoke<Record<string, unknown>>("terracotta_create_room", { player: player ?? null }, { silent: true }),
  terracottaJoinRoom: (room: string, player?: string) =>
    invoke<Record<string, unknown>>("terracotta_join_room", { room, player: player ?? null }, { silent: true }),
  terracottaLeave: () => invoke<Record<string, unknown>>("terracotta_leave", undefined, { silent: true }),

  // storage
  getStorageStats: () => invoke<StorageStats>("get_storage_stats"),
  refreshStorageStats: () => invoke<StorageStats>("refresh_storage_stats"),
  clearCache: () => invoke<CacheClearResult>("clear_cache"),

  // crash analysis
  crashAnalysis: (instanceId: string) =>
    invoke<{ filename: string; modified: number; size: number; kind: string }[]>("list_crash_logs", { id: instanceId }),
  analyzeCrash: (instanceId: string, filename: string) =>
    invoke<CrashDiagnosis>("analyze_crash_log", { id: instanceId, filename }),
  getCrashReportContent: (instanceId: string, filename: string) =>
    invoke<string>("get_crash_report_content", { id: instanceId, filename }),

  // news
  fetchNews: () => invoke<NewsItem[]>("fetch_news"),
};
