use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

fn encode_token<S>(token: &str, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&STANDARD.encode(token.as_bytes()))
}

fn decode_token<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    match STANDARD.decode(&s) {
        Ok(bytes) => Ok(String::from_utf8(bytes).unwrap_or(s)),
        Err(_) => Ok(s),
    }
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    /// Absolute path to the launcher data root (instances, libraries, assets...)
    pub data_dir: String,
    /// User-selected Java executable (absolute path). None = auto detect
    pub java_path: Option<String>,
    pub max_memory_mb: u32,
    pub min_memory_mb: u32,
    /// Memory mode: "auto" | "custom" (default "custom")
    pub memory_mode: String,
    /// Extra JVM arguments (one per line or space separated)
    pub jvm_args: String,
    /// Extra game arguments
    pub game_args: String,
    /// Parallel download tasks (how many files to download concurrently)
    pub download_threads: usize,
    /// Threads per file for chunked (ranged) download
    pub download_chunk_threads: usize,
    /// CurseForge API key (optional; required for CurseForge features)
    pub curseforge_api_key: Option<String>,
    /// "dark" | "light"
    pub theme: String,
    /// 主题强调色（hex，如 "#e89a4b"）
    pub theme_color: String,
    /// What happens when the window is closed: "minimize" | "quit"
    pub close_behavior: String,
    /// Auto launch last played instance on startup
    pub auto_launch: bool,
    /// Keep the launcher window on top of the game while it runs
    pub keep_open: bool,
    /// Microsoft OAuth application (client) id used for sign-in
    pub ms_client_id: String,
    /// Currently selected account (uuid) used when an instance has no override
    pub selected_account: Option<String>,
    /// 下载代理模式："system"（系统代理，默认）| "direct"（直连）| "custom"（自定义）
    pub proxy_mode: String,
    /// HTTP/SOCKS proxy URL for downloads（`proxy_mode == "custom"` 时生效，如 "http://127.0.0.1:7890"）
    pub proxy: Option<String>,
    /// 下载镜像源 id："official" | "bmclapi" | "custom"
    pub mirror: String,
    /// 自定义镜像根地址（`mirror == "custom"` 时生效），需兼容 BMCLAPI 接口
    pub mirror_custom: String,
    /// 自定义背景图片绝对路径（None = 使用默认渐变背景）
    pub background_image: Option<String>,
    /// 背景图片模糊半径 px（0-50）
    pub background_blur: u32,
    /// 背景图片暗化程度 0-100（数值越大前景内容越清晰）
    pub background_dim: u32,
    /// 磨砂卡片模糊半径 px（0-30）
    pub glass_blur: u32,
    /// 首页主标题卡片（hero）是否显示
    pub show_home_hero: bool,
    /// 侧边栏展开/收缩按钮是否显示
    pub show_sidebar_collapse_btn: bool,
    /// 用户点击「忽略此版本」后记录的版本号。启动时若为同一版本则不再弹窗提示，
    /// 出现更新的版本时恢复提醒。
    pub dismissed_update_version: Option<String>,
    /// 启动时自动检查并下载更新（检测到新版本直接下载安装，无需手动确认）
    pub auto_update: bool,
    /// 应用自更新源："bucket"（对象存储，默认） | "github"（GitHub Releases 官方源）
    pub update_source: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            data_dir: String::new(),
            java_path: None,
            max_memory_mb: 4096,
            min_memory_mb: 512,
            memory_mode: "auto".into(),
            jvm_args: String::new(),
            game_args: String::new(),
            download_threads: 8,
            download_chunk_threads: 4,
            curseforge_api_key: None,
            theme: "dark".into(),
            theme_color: "#e89a4b".into(),
            close_behavior: "minimize".into(),
            auto_launch: false,
            keep_open: true,
            ms_client_id: "00000000-0000-0000-0000-000000000000".into(),
            selected_account: None,
            proxy_mode: "system".into(),
            proxy: None,
            mirror: "official".into(),
            mirror_custom: String::new(),
            background_image: None,
            background_blur: 0,
            background_dim: 45,
            glass_blur: 8,
            show_home_hero: false,
            show_sidebar_collapse_btn: false,
            dismissed_update_version: None,
            auto_update: false,
            update_source: "bucket".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Loader / Instance
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum LoaderType {
    Vanilla,
    Fabric,
    Quilt,
    Forge,
    NeoForge,
}

impl LoaderType {
    pub fn as_str(&self) -> &'static str {
        match self {
            LoaderType::Vanilla => "vanilla",
            LoaderType::Fabric => "fabric",
            LoaderType::Quilt => "quilt",
            LoaderType::Forge => "forge",
            LoaderType::NeoForge => "neoforge",
        }
    }
}

/// A single installed content item (mod / resource pack / shader) tracked
/// so it can be upgraded later.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct InstalledContent {
    pub filename: String,
    /// modrinth | curseforge | manual
    pub source: String,
    /// Modrinth project id or CurseForge mod id
    pub project_id: Option<String>,
    /// Modrinth/CurseForge project slug (用于 WikiEntries 中文名映射)
    #[serde(default)]
    pub slug: Option<String>,
    /// Modrinth version id or CurseForge file id
    pub version_id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    /// Mod 内部 id（fabric 的 "sodium" / forge 的 modId），用于中文名映射与精确识别
    #[serde(default)]
    pub mod_id: Option<String>,
    /// 作者列表（从 fabric.mod.json / mods.toml 提取）
    #[serde(default)]
    pub authors: Option<Vec<String>>,
    /// Mod 描述（可选，从 jar 内元数据提取）
    #[serde(default)]
    pub description: Option<String>,
    pub installed_at: u64,
    pub size: u64,
    /// Project icon URL (shown in content lists)
    #[serde(default)]
    pub icon: Option<String>,
    /// Whether the content is enabled (mods can be disabled per-instance)
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool { true }

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Instance {
    pub id: String,
    pub name: String,
    pub mc_version: String,
    pub loader: LoaderType,
    pub loader_version: Option<String>,
    pub created: u64,
    pub last_played: Option<u64>,
    /// 累计游玩时长（秒）。游戏进程退出时由 launch 钩子累加；
    /// serde(default) 保证旧版本 qookix.json 缺字段时兼容。
    #[serde(default)]
    pub total_play_time: u64,
    /// Game files are fully installed and ready to launch
    pub installed: bool,
    pub icon: Option<String>,
    // per-instance overrides (fall back to settings)
    pub max_memory_mb: Option<u32>,
    /// Memory allocation mode: "global" | "auto" | "custom" (defaults to global)
    #[serde(default)]
    pub memory_mode: Option<String>,
    pub jvm_args: Option<String>,
    pub game_args: Option<String>,
    pub java_path: Option<String>,
    pub account_id: Option<String>,
    pub resolution: Option<(u32, u32)>,
    pub mods: Vec<InstalledContent>,
    pub resource_packs: Vec<InstalledContent>,
    pub shaders: Vec<InstalledContent>,
    /// 实例是否通过符号链接方式从外部 .minecraft 导入
    #[serde(default)]
    pub is_symlink: bool,
    /// 导入来源的 .minecraft 路径（符号链接模式下用于提醒与溯源）
    #[serde(default)]
    pub source_path: Option<String>,
    /// 所属分组 id（None 表示未分组）
    #[serde(default)]
    pub group: Option<String>,
}

// ---------------------------------------------------------------------------
// Instance groups
// ---------------------------------------------------------------------------

/// 实例分组，单独持久化在 `instance_groups.json`。
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct InstanceGroup {
    pub id: String,
    pub name: String,
    /// 主题色（任意 CSS 颜色）；为空时前端回退到默认强调色
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub created: u64,
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Account {
    Offline {
        uuid: String,
        username: String,
        created: u64,
    },
    Microsoft {
        uuid: String,
        username: String,
        created: u64,
        #[serde(serialize_with = "encode_token", deserialize_with = "decode_token")]
        msa_refresh_token: String,
        #[serde(serialize_with = "encode_token", deserialize_with = "decode_token")]
        msa_access_token: String,
        msa_expires_at: u64,
    },
}

impl Account {
    pub fn uuid(&self) -> &str {
        match self {
            Account::Offline { uuid, .. } => uuid,
            Account::Microsoft { uuid, .. } => uuid,
        }
    }
    pub fn username(&self) -> &str {
        match self {
            Account::Offline { username, .. } => username,
            Account::Microsoft { username, .. } => username,
        }
    }
    pub fn is_microsoft(&self) -> bool {
        matches!(self, Account::Microsoft { .. })
    }
}

// ---------------------------------------------------------------------------
// Mojang metadata
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VersionManifest {
    pub latest: Latest,
    pub versions: Vec<ManifestVersion>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Latest {
    pub release: String,
    pub snapshot: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ManifestVersion {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    pub time: String,
    #[serde(rename = "releaseTime")]
    pub release_time: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VersionJson {
    pub id: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(rename = "mainClass", default)]
    pub main_class: Option<String>,
    #[serde(rename = "minecraftArguments", default)]
    pub minecraft_arguments: Option<String>,
    #[serde(default)]
    pub arguments: Option<Arguments>,
    #[serde(rename = "assetIndex", default)]
    pub asset_index: Option<AssetIndex>,
    #[serde(default)]
    pub downloads: VersionDownloads,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default)]
    pub logging: Option<Logging>,
    #[serde(rename = "inheritsFrom", default)]
    pub inherits_from: Option<String>,
    #[serde(default)]
    pub java_version: Option<JavaVersion>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct VersionDownloads {
    pub client: Option<DownloadFile>,
    pub server: Option<DownloadFile>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DownloadFile {
    pub sha1: String,
    pub size: u64,
    pub url: String,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AssetIndex {
    pub id: String,
    pub sha1: String,
    pub size: u64,
    #[serde(rename = "totalSize", default)]
    pub total_size: u64,
    pub url: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AssetIndexFile {
    #[serde(default)]
    pub objects: HashMap<String, AssetObject>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub downloads: Option<LibraryDownloads>,
    #[serde(default)]
    pub rules: Option<Vec<Rule>>,
    #[serde(default)]
    pub natives: Option<HashMap<String, String>>,
    #[serde(default)]
    pub extract: Option<Extract>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LibraryDownloads {
    pub artifact: Option<DownloadFile>,
    #[serde(default)]
    pub classifiers: Option<HashMap<String, DownloadFile>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Extract {
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Rule {
    pub action: String,
    #[serde(default)]
    pub os: Option<OsRule>,
    #[serde(default)]
    pub features: Option<HashMap<String, bool>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OsRule {
    pub name: Option<String>,
    pub arch: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Arguments {
    #[serde(default)]
    pub game: Option<Vec<ArgumentValue>>,
    #[serde(default)]
    pub jvm: Option<Vec<ArgumentValue>>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum ArgumentValue {
    Str(String),
    Rule(ArgumentRule),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ArgumentRule {
    #[serde(default)]
    pub rules: Vec<Rule>,
    pub value: ArgumentValueInner,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum ArgumentValueInner {
    Str(String),
    List(Vec<String>),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Logging {
    #[serde(default)]
    pub client: Option<LoggingClient>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LoggingClient {
    pub argument: String,
    pub file: DownloadFile,
    #[serde(rename = "type", default)]
    pub kind: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct JavaVersion {
    #[serde(rename = "majorVersion")]
    pub major_version: u32,
}

/// Pending Microsoft device-code login flow.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MsFlow {
    pub device_code: String,
    pub interval: u64,
    pub expires_at: u64,
    pub client_id: String,
}

// ---------------------------------------------------------------------------
// Loader metadata (Fabric / Quilt)
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LoaderMetaEntry {
    pub loader: LoaderMeta,
    pub intermediary: IntermediaryMeta,
    #[serde(rename = "launcherMeta")]
    pub launcher_meta: LauncherMeta,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LoaderMeta {
    pub version: String,
    #[serde(default)]
    pub stable: bool,
    #[serde(default)]
    pub maven: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct IntermediaryMeta {
    pub version: String,
    #[serde(default)]
    pub maven: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LauncherMeta {
    #[serde(rename = "mainClass", default, deserialize_with = "de_main_class")]
    pub main_class: Option<String>,
    #[serde(default)]
    pub libraries: Option<HashMap<String, Vec<MetaLibrary>>>,
}

/// Fabric/Quilt meta: `mainClass` is either a plain string (older metas) or
/// an object `{ "client": ..., "server": ... }` (newer metas).
fn de_main_class<'de, D>(d: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = serde_json::Value::deserialize(d)?;
    match v {
        serde_json::Value::String(s) => Ok(Some(s)),
        serde_json::Value::Object(m) => Ok(m
            .get("client")
            .and_then(|c| c.as_str())
            .map(|s| s.to_string())),
        _ => Ok(None),
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MetaLibrary {
    pub name: String,
    #[serde(default)]
    pub url: Option<String>,
}

// ---------------------------------------------------------------------------
// Frontend-facing DTOs
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct JavaInfo {
    pub path: String,
    pub version: String,
    pub major: u32,
    pub vendor: String,
    pub arch: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct JavaDetection {
    pub candidates: Vec<JavaInfo>,
    pub selected: Option<JavaInfo>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct InstallPlan {
    pub instance_id: String,
    pub total_bytes: u64,
    pub file_count: usize,
    /// `true` when the user asked for symlinks but the OS denied the privilege,
    /// so the migration silently fell back to a normal copy.
    pub symlink_fallback: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LaunchResult {
    pub pid: u32,
    pub command: Vec<String>,
}

/// Maven coordinate -> local path under `libraries/`
pub fn maven_to_path(name: &str) -> Option<PathBuf> {
    // group:artifact:version[:classifier]
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let (group, artifact, version) = (parts[0], parts[1], parts[2]);
    let classifier = if parts.len() > 3 { parts[3] } else { "" };
    let mut p = PathBuf::new();
    for seg in group.split('.') {
        p.push(seg);
    }
    p.push(artifact);
    p.push(version);
    let file = if classifier.is_empty() {
        format!("{artifact}-{version}.jar")
    } else {
        format!("{artifact}-{version}-{classifier}.jar")
    };
    p.push(file);
    Some(p)
}

// ---------------------------------------------------------------------------
// Hosted game servers
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum ServerCore {
    Vanilla,
    Paper,
    Spigot,
    Purpur,
    Forge,
    Fabric,
}

impl ServerCore {
    pub fn as_str(&self) -> &'static str {
        match self {
            ServerCore::Vanilla => "vanilla",
            ServerCore::Paper => "paper",
            ServerCore::Spigot => "spigot",
            ServerCore::Purpur => "purpur",
            ServerCore::Forge => "forge",
            ServerCore::Fabric => "fabric",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ServerConfig {
    pub id: String,
    pub name: String,
    pub core: ServerCore,
    pub mc_version: String,
    #[serde(default = "default_server_port")]
    pub port: u16,
    #[serde(default = "default_server_max_mem")]
    pub max_memory_mb: u32,
    #[serde(default = "default_server_min_mem")]
    pub min_memory_mb: u32,
    #[serde(default = "default_server_motd")]
    pub motd: String,
    #[serde(default)]
    pub eula: bool,
    pub created: u64,
    #[serde(default)]
    pub last_started: Option<u64>,
    #[serde(default)]
    pub java_path: Option<String>,
    #[serde(default)]
    pub jvm_args: Option<String>,
    #[serde(default)]
    pub stop_command: Option<String>,
}

fn default_server_port() -> u16 {
    25565
}
fn default_server_max_mem() -> u32 {
    2048
}
fn default_server_min_mem() -> u32 {
    1024
}
fn default_server_motd() -> String {
    "A Minecraft Server".into()
}
