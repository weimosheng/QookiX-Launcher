import { invoke } from "@tauri-apps/api/core";
import type {
  Account,
  Instance,
  JavaInfo,
  ProjectHit,
  ProjectVersion,
  Settings,
} from "./types";

export const api = {
  // settings & java
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (patch: Record<string, unknown>) => invoke<Settings>("set_settings", { patch }),
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
  installGame: (instanceId: string) =>
    invoke<{ instance_id: string; total_bytes: number; file_count: number }>("install_game", { instanceId }),
  cancelInstall: () => invoke<void>("cancel_install"),
  launchInstance: (instanceId: string, world?: string) =>
    invoke<{ pid: number; command: string[] }>("launch_instance", {
      instanceId,
      world: world ?? null,
    }),
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

  // accounts
  listAccounts: () => invoke<Account[]>("list_accounts"),
  loginOffline: (username: string) => invoke<Account>("login_offline", { username }),
  loginMsStart: () => invoke<{ userCode: string; verificationUri: string; expiresIn: number }>("login_ms_start"),
  loginMsPoll: () => invoke<Account>("login_ms_poll"),
  logoutAccount: (uuid: string) => invoke<void>("logout_account", { uuid }),

  // browse & content
  browse: (provider: string, query: string, projectType: string, category: string, page: number) =>
    invoke<{ hits: ProjectHit[]; total: number }>("browse", {
      provider,
      query,
      projectType,
      category,
      page,
    }),
  projectVersions: (provider: string, projectId: string, mcVersion: string, loader: string) =>
    invoke<{ provider: string; versions: ProjectVersion[] }>("project_versions", {
      provider,
      projectId,
      mcVersion,
      loader,
    }),
  projectDependencies: (provider: string, versionId: string) =>
    invoke<import("./types").ProjectDependency[]>("project_dependencies", {
      provider,
      versionId,
    }),
  curseforgeCategories: (projectType: string) =>
    invoke<{ categories: { id: number; name: string }[] }>("curseforge_categories", { projectType }),
  installContent: (
    instanceId: string,
    provider: string,
    projectId: string,
    versionId: string,
    kind: string
  ) =>
    invoke<{ ok: boolean; filename?: string; mods?: number }>("install_content", {
      instanceId,
      provider,
      projectId,
      versionId,
      kind,
    }),
  checkUpdates: (instanceId: string, kind: string) =>
    invoke<import("./types").UpdateInfo[]>("check_updates", { instanceId, kind }),
  applyUpdate: (
    instanceId: string,
    kind: string,
    oldFilename: string,
    provider: string,
    projectId: string,
    newVersionId: string
  ) => invoke<{ ok: boolean; filename?: string }>("apply_update", {
    instanceId,
    kind,
    oldFilename,
    provider,
    projectId,
    newVersionId,
  }),
  uninstallContent: (instanceId: string, kind: string, filename: string) =>
    invoke<void>("uninstall_content", { instanceId, kind, filename }),
  listContent: (instanceId: string, kind: string) =>
    invoke<{ items: import("./types").ContentItem[]; onDisk: string[] }>("list_content", { instanceId, kind }),
  toggleContentEnabled: (instanceId: string, kind: string, filename: string, enabled: boolean) =>
    invoke<void>("toggle_content_enabled", { instanceId, kind, filename, enabled }),
  importLocalFile: (instanceId: string, kind: string, sourcePath: string) =>
    invoke<{ ok: boolean }>("import_local_file", { instanceId, kind, sourcePath }),
  saveTextFile: (path: string, content: string) => invoke<void>("save_text_file", { path, content }),
  extractGameIcons: (instanceId?: string) =>
    invoke<{ name: string; label: string; path: string }[]>("extract_game_icons", {
      instanceId: instanceId ?? null,
    }),
};
