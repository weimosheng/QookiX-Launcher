export type Loader = "vanilla" | "fabric" | "quilt" | "forge" | "neoforge";

export interface Settings {
  data_dir: string;
  java_path: string | null;
  max_memory_mb: number;
  min_memory_mb: number;
  memory_mode: string;
  jvm_args: string;
  game_args: string;
  download_threads: number;
  curseforge_api_key: string | null;
  theme: string;
  close_behavior: string;
  auto_launch: boolean;
  keep_open: boolean;
  ms_client_id: string;
  selected_account: string | null;
  isolation: boolean;
  proxy: string | null;
}

export interface JavaInfo {
  path: string;
  version: string;
  major: number;
  vendor: string;
  arch: string;
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
