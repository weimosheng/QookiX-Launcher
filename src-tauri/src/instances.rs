use crate::models::{InstalledContent, Instance, InstanceGroup, LoaderType};
use crate::state::AppState;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Emitter;

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn instance_path(state: &AppState, id: &str) -> std::path::PathBuf {
    state.instances_dir().join(id).join("qookix.json")
}

/// Instance ids are internally generated (8-char hex). Reject anything that
/// could escape the instances directory via path traversal before it is joined
/// onto a filesystem path or passed to `remove_dir_all`.
pub fn validate_instance_id(id: &str) -> Result<(), String> {
    if id.is_empty()
        || id == "."
        || id == ".."
        || id.contains('/')
        || id.contains('\\')
        || id.contains("..")
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err("非法实例 ID".into());
    }
    Ok(())
}

pub fn load_instances(state: &AppState) -> Vec<Instance> {
    let dir = state.instances_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let meta = p.join("qookix.json");
        if let Ok(text) = std::fs::read_to_string(&meta) {
            if let Ok(inst) = serde_json::from_str::<Instance>(&text) {
                out.push(inst);
            }
        }
    }
    out.sort_by(|a, b| b.created.cmp(&a.created));
    out
}

pub fn get_instance(state: &AppState, id: &str) -> Result<Instance, String> {
    validate_instance_id(id)?;
    let text = std::fs::read_to_string(instance_path(state, id))
        .map_err(|_| format!("实例 {id} 不存在"))?;
    serde_json::from_str(&text).map_err(|e| format!("实例数据损坏: {e}"))
}

pub fn save_instance(state: &AppState, instance: &Instance) -> Result<(), String> {
    let path = instance_path(state, &instance.id);
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(instance).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}

pub fn create_instance(
    state: &AppState,
    name: String,
    mc_version: String,
    loader: LoaderType,
    loader_version: Option<String>,
) -> Result<Instance, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("实例名称不能为空".into());
    }
    if mc_version.is_empty() {
        return Err("请选择 Minecraft 版本".into());
    }
    // unique short id
    let mut id = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    while instance_path(state, &id).exists() {
        id = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    }
    let instance = Instance {
        id,
        name,
        mc_version,
        loader,
        loader_version,
        created: now(),
        last_played: None,
        total_play_time: 0,
        installed: true,
        icon: None,
        max_memory_mb: None,
        memory_mode: None,
        jvm_args: None,
        game_args: None,
        java_path: None,
        account_id: None,
        resolution: None,
        mods: Vec::new(),
        resource_packs: Vec::new(),
        shaders: Vec::new(),
        is_symlink: false,
        source_path: None,
        group: None,
    };
    // game dir
    std::fs::create_dir_all(state.instances_dir().join(&instance.id)).map_err(|e| e.to_string())?;
    save_instance(state, &instance)?;
    Ok(instance)
}

pub fn delete_instance(state: &AppState, id: &str) -> Result<(), String> {
    validate_instance_id(id)?;
    let dir = state.instances_dir().join(id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("删除实例目录失败: {e}"))?;
    }
    // remove cached version json/jar
    let ver_dir = state.versions_dir().join(id);
    if ver_dir.exists() {
        let _ = std::fs::remove_dir_all(&ver_dir);
    }
    Ok(())
}

pub fn mark_installed(state: &AppState, id: &str) -> Result<(), String> {
    let mut inst = get_instance(state, id)?;
    inst.installed = true;
    save_instance(state, &inst)
}

pub fn update_instance(state: &AppState, patch: serde_json::Value) -> Result<Instance, String> {
    let id = patch
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("缺少实例 id")?;
    let mut inst = get_instance(state, id)?;
    if let Some(v) = patch.get("name").and_then(|v| v.as_str()) {
        inst.name = v.to_string();
    }
    if let Some(v) = patch.get("icon").and_then(|v| v.as_str()) {
        inst.icon = if v.is_empty() { None } else { Some(v.to_string()) };
    }
    if let Some(v) = patch.get("max_memory_mb").and_then(|v| v.as_u64()) {
        inst.max_memory_mb = if v > 0 { Some(v as u32) } else { None };
    }
    if let Some(v) = patch.get("memory_mode").and_then(|v| v.as_str()) {
        inst.memory_mode = if v.is_empty() { None } else { Some(v.to_string()) };
    }
    if let Some(v) = patch.get("jvm_args").and_then(|v| v.as_str()) {
        inst.jvm_args = if v.trim().is_empty() { None } else { Some(v.to_string()) };
    }
    if let Some(v) = patch.get("game_args").and_then(|v| v.as_str()) {
        inst.game_args = if v.trim().is_empty() { None } else { Some(v.to_string()) };
    }
    if let Some(v) = patch.get("java_path") {
        inst.java_path = v.as_str().map(|s| s.to_string()).filter(|s| !s.is_empty());
    }
    if let Some(v) = patch.get("account_id") {
        inst.account_id = v.as_str().map(|s| s.to_string()).filter(|s| !s.is_empty());
    }
    if let Some(v) = patch.get("resolution") {
        if let Some(arr) = v.as_array() {
            if arr.len() == 2 {
                let w = arr[0].as_u64().unwrap_or(0) as u32;
                let h = arr[1].as_u64().unwrap_or(0) as u32;
                inst.resolution = if w > 0 && h > 0 { Some((w, h)) } else { None };
            }
        }
    }
    if let Some(v) = patch.get("group") {
        let gid = v.as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        if let Some(ref gid) = gid {
            // 防止前端传入已删除/不存在的分组 id 造成"幽灵分组"
            let known = load_groups(state).iter().any(|g| g.id == *gid);
            if !known {
                return Err("分组不存在".into());
            }
        }
        inst.group = gid;
    }
    save_instance(state, &inst)?;
    Ok(inst)
}

pub fn touch_last_played(state: &AppState, id: &str) {
    if let Ok(mut inst) = get_instance(state, id) {
        inst.last_played = Some(now());
        let _ = save_instance(state, &inst);
    }
}

/// 累加实例的累计游玩时长（秒）。游戏进程退出时由 launch.rs 的退出钩子调用，
/// 正常退出与 kill_game 强杀都会走到该钩子。
pub fn add_play_time(state: &AppState, id: &str, secs: u64) {
    if secs == 0 {
        return;
    }
    if let Ok(mut inst) = get_instance(state, id) {
        inst.total_play_time = inst.total_play_time.saturating_add(secs);
        let _ = save_instance(state, &inst);
    }
}

// ---------------------------------------------------------------------------
// Instance groups
// ---------------------------------------------------------------------------

pub fn groups_path(state: &AppState) -> std::path::PathBuf {
    state.root.join("instance_groups.json")
}

pub fn load_groups(state: &AppState) -> Vec<InstanceGroup> {
    std::fs::read_to_string(groups_path(state))
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<InstanceGroup>>(&s).ok())
        .unwrap_or_default()
}

pub fn save_groups(state: &AppState, groups: &[InstanceGroup]) -> Result<(), String> {
    let path = groups_path(state);
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(groups).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

/// 仅读取分组，缺失时回退为空列表（groups.json 本身就是唯一数据源）。
pub fn create_group(
    state: &AppState,
    name: String,
    color: Option<String>,
) -> Result<InstanceGroup, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("分组名称不能为空".into());
    }
    if name.len() > 40 {
        return Err("分组名称过长（最多 40 个字符）".into());
    }
    let mut groups = load_groups(state);
    if groups.iter().any(|g| g.name == name) {
        return Err("已存在同名分组".into());
    }
    let mut id = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    while groups.iter().any(|g| g.id == id) {
        id = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    }
    let group = InstanceGroup {
        id,
        name,
        color: color
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty()),
        created: now(),
    };
    groups.push(group.clone());
    save_groups(state, &groups)?;
    Ok(group)
}

pub fn rename_group(
    state: &AppState,
    id: &str,
    name: String,
    color: Option<String>,
) -> Result<InstanceGroup, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("分组名称不能为空".into());
    }
    if name.len() > 40 {
        return Err("分组名称过长（最多 40 个字符）".into());
    }
    let mut groups = load_groups(state);
    if groups.iter().any(|g| g.name == name && g.id != id) {
        return Err("已存在同名分组".into());
    }
    let Some(group) = groups.iter_mut().find(|g| g.id == id) else {
        return Err("分组不存在".into());
    };
    group.name = name;
    if let Some(c) = color {
        group.color = if c.trim().is_empty() { None } else { Some(c) };
    }
    let updated = group.clone();
    save_groups(state, &groups)?;
    Ok(updated)
}

/// 删除分组：其中的实例会被移回"未分组"，不会被删除。
pub fn delete_group(state: &AppState, id: &str) -> Result<(), String> {
    let mut groups = load_groups(state);
    let before = groups.len();
    groups.retain(|g| g.id != id);
    if groups.len() == before {
        return Err("分组不存在".into());
    }
    save_groups(state, &groups)?;
    for inst in load_instances(state) {
        if inst.group.as_deref() == Some(id) {
            if let Ok(mut i) = get_instance(state, &inst.id) {
                i.group = None;
                let _ = save_instance(state, &i);
            }
        }
    }
    Ok(())
}

/// 按给定 id 顺序重排分组（未知 id 忽略，未列出的分组保持相对顺序追加到末尾）。
pub fn reorder_groups(state: &AppState, ids: Vec<String>) -> Result<Vec<InstanceGroup>, String> {
    let groups = load_groups(state);
    let mut ordered: Vec<InstanceGroup> = Vec::with_capacity(groups.len());
    for id in ids {
        if let Some(g) = groups.iter().find(|g| g.id == id) {
            if !ordered.iter().any(|o| o.id == g.id) {
                ordered.push(g.clone());
            }
        }
    }
    for g in groups {
        if !ordered.iter().any(|o| o.id == g.id) {
            ordered.push(g);
        }
    }
    save_groups(state, &ordered)?;
    Ok(ordered)
}

// ---------------------------------------------------------------------------
// Installed content tracking
// ---------------------------------------------------------------------------

pub fn list_content(state: &AppState, id: &str, kind: &str) -> Vec<InstalledContent> {
    let Ok(inst) = get_instance(state, id) else {
        return Vec::new();
    };
    match kind {
        "resourcepack" => inst.resource_packs,
        "shader" => inst.shaders,
        _ => inst.mods,
    }
}

pub fn add_content(state: &AppState, id: &str, kind: &str, record: InstalledContent) -> Result<(), String> {
    let mut inst = get_instance(state, id)?;
    let list = match kind {
        "resourcepack" => &mut inst.resource_packs,
        "shader" => &mut inst.shaders,
        _ => &mut inst.mods,
    };
    list.retain(|c| c.filename != record.filename);
    list.push(record);
    save_instance(state, &inst)
}

pub fn add_content_batch(state: &AppState, id: &str, kind: &str, records: Vec<InstalledContent>) -> Result<(), String> {
    let mut inst = get_instance(state, id)?;
    let list = match kind {
        "resourcepack" => &mut inst.resource_packs,
        "shader" => &mut inst.shaders,
        _ => &mut inst.mods,
    };
    for rec in records {
        list.retain(|c| c.filename != rec.filename);
        list.push(rec);
    }
    save_instance(state, &inst)
}

pub fn remove_content(state: &AppState, id: &str, kind: &str, filename: &str) -> Result<(), String> {
    let mut inst = get_instance(state, id)?;
    let list = match kind {
        "resourcepack" => &mut inst.resource_packs,
        "shader" => &mut inst.shaders,
        _ => &mut inst.mods,
    };
    list.retain(|c| c.filename != filename);
    save_instance(state, &inst)
}

pub fn set_content_enabled(state: &AppState, id: &str, kind: &str, filename: &str, enabled: bool) -> Result<(), String> {
    if !crate::util::is_safe_filename(filename) {
        return Err("非法文件名".into());
    }
    let mut inst = get_instance(state, id)?;
    let list = match kind {
        "resourcepack" => &mut inst.resource_packs,
        "shader" => &mut inst.shaders,
        _ => &mut inst.mods,
    };
    let Some(item) = list.iter_mut().find(|c| c.filename == filename) else {
        return Err("内容记录不存在".into());
    };
    let folder = crate::modrinth::kind_folder(kind);
    let dir = state.instances_dir().join(id).join(folder);
    let active = dir.join(filename);
    let disabled = dir.join(format!("{filename}.disabled"));
    if enabled {
        if !active.is_file() && disabled.is_file() {
            std::fs::rename(&disabled, &active).map_err(|e| e.to_string())?;
        }
    } else {
        if active.is_file() {
            std::fs::rename(&active, &disabled).map_err(|e| e.to_string())?;
        }
    }
    item.enabled = enabled;
    save_instance(state, &inst)
}

// ---------------------------------------------------------------------------
// Import an existing `.minecraft` folder as a new instance
// ---------------------------------------------------------------------------

/// How user data from the source `.minecraft` folder is migrated.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum ImportMode {
    /// Copy files (uses disk space, fully independent).
    Copy,
    /// Create symbolic links (no extra disk space, but the original folder
    /// must stay on the same machine / volume).
    Symlink,
}

/// Every version found under the `versions/` subfolder, with its size.
/// A `.minecraft` folder can hold several versions; the user picks one.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MinecraftVersionInfo {
    /// The version id (from the version json `id`, preferring `inheritsFrom`
    /// for Forge/NeoForge so we get the real MC version).
    pub id: String,
    /// The literal folder name under `versions/` (e.g. `1.20.1-forge-47.1.0`).
    /// This is the source folder we actually migrate; `id` may be the resolved
    /// vanilla base after `inheritsFrom`.
    pub raw_id: String,
    /// `true` when this entry is a modded profile that inherits from a vanilla
    /// version (e.g. forge / neoforge). The displayed `id` is then the vanilla
    /// base version.
    pub inherits_base: bool,
    /// Detected loader for this specific version (forge / neoforge / fabric /
    /// quilt / vanilla), derived from the version json.
    pub loader: String,
    /// The loader's own version (e.g. `47.1.0` for forge), if it can be parsed
    /// from the version json id.
    pub loader_version: Option<String>,
    /// Size on disk of the whole `versions/<name>/` directory.
    pub size_bytes: u64,
}

/// Download-size estimate for a given MC version (client + libraries + assets).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MinecraftDownloadEstimate {
    pub download_files: u64,
    pub download_bytes: u64,
    /// True when the remote asset index was fetched and its object count is known.
    pub assets_known: bool,
}

/// Migration-size estimate for a set of chosen versions: the shared user-data
/// directories (copied once per created instance) plus each selected version
/// folder. Computed only after the user selects versions.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct ImportSizeEstimate {
    pub import_files: u64,
    pub import_bytes: u64,
}

// `MinecraftVersionInfo` is defined above (line ~263).

/// User-data entries that should be migrated from a `.minecraft` folder.
/// We use an allow-list so we never accidentally swallow the launcher's own
/// huge cache. `versions` is included because the per-version jars/json are
/// part of the user's setup and would be missing after the import.
const IMPORT_DIRS: &[&str] = &[
    "saves",
    "mods",
    "config",
    "resourcepacks",
    "shaderpacks",
    "screenshots",
    "logs",
    "crash-reports",
    "mods-old",
    "mods-disabled",
];
const IMPORT_FILES: &[&str] = &[
    "options.txt",
    "optionsof.txt",
    "optionsshaders.txt",
    "servers.dat",
    "servers.json",
    "launcher_profiles.json",
    "usercache.json",
    "usernamecache.json",
];

/// Recursively walk `dir`, calling `on_file` for every regular file found
/// (with its size). Used to stream live file-count progress to the UI.
fn walk_dir(dir: &std::path::Path, on_file: &mut dyn FnMut(u64)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk_dir(&p, on_file);
        } else if p.is_file() {
            if let Ok(m) = std::fs::metadata(&p) {
                on_file(m.len());
            }
        }
    }
}

/// Detect the loader + its version from a version json. We inspect, in order of
/// reliability:
///   1. the `libraries` artifacts (e.g. `net.minecraftforge:forge:1.20.1-47.1.0`,
///      `org.quiltmc:quilt-loader:...`, `net.fabricmc:fabric-loader:0.15.11`,
///      `net.neoforged:neoforge:...`)
///   2. the `mainClass` (knot / FML / Quilt / launchwrapper markers)
///   3. the version `id` itself (`1.20.1-forge-47.1.0`,
///      `1.16.5-Fabric 0.15.11-OptiFine_G8`, `1.21.8-OptiFine_J6_pre8` ...)
fn detect_loader(
    json: &serde_json::Value,
    id: &str,
    inherits_base: bool,
) -> (&'static str, Option<String>) {
    let main_class = json
        .get("mainClass")
        .and_then(|x| x.as_str())
        .unwrap_or("");

    // 1) scan libraries (most reliable source of loader + version)
    if let Some(libs) = json.get("libraries").and_then(|l| l.as_array()) {
        for lib in libs {
            let name = lib
                .get("name")
                .and_then(|n| n.as_str())
                .or_else(|| lib.as_str())
                .unwrap_or("");
            let lower = name.to_lowercase();
            if lower.contains("neoforge") || lower.contains("net.neoforged") {
                return ("neoforge", extract_loader_version(name));
            }
            if lower.contains(":forge:") || lower.contains("net.minecraftforge") {
                return ("forge", extract_loader_version(name));
            }
            if lower.contains("fabric-loader") || lower.contains("net.fabricmc") {
                return ("fabric", extract_loader_version(name));
            }
            if lower.contains("quilt-loader") || lower.contains("org.quiltmc") {
                return ("quilt", extract_loader_version(name));
            }
        }
    }

    // 2) mainClass
    let mc = main_class.to_lowercase();
    if mc.contains("fabric") {
        return ("fabric", None);
    }
    if mc.contains("quilt") {
        return ("quilt", None);
    }
    if mc.contains("forge") || mc.contains("fml") {
        return ("forge", None);
    }

    // 3) version id markers (case-insensitive; allow spaces, e.g. "Fabric 0.15.11")
    let lower_id = id.to_lowercase();
    if lower_id.contains("neoforge") || lower_id.contains("neo-forge") {
        return ("neoforge", parse_loader_version(id, "neoforge"));
    }
    if lower_id.contains("fabric") {
        return ("fabric", parse_loader_version(id, "fabric"));
    }
    if lower_id.contains("quilt") {
        return ("quilt", parse_loader_version(id, "quilt"));
    }
    if lower_id.contains("forge") {
        return ("forge", parse_loader_version(id, "forge"));
    }
    // OptiFine is not a standalone loader (it rides on vanilla / forge); mark it
    // so the UI can show it distinctly, but it installs as vanilla.
    if lower_id.contains("optifine") {
        return ("optifine", None);
    }

    // 4) inherits a vanilla base but no loader marker -> still vanilla
    let _ = inherits_base;
    ("vanilla", None)
}

/// From a maven artifact like `net.minecraftforge:forge:1.20.1-47.1.0`, return
/// the version portion (`1.20.1-47.1.0`). Returns None if it isn't a
/// `group:artifact:version` triplet.
fn extract_loader_version(name: &str) -> Option<String> {
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() >= 3 {
        Some(parts[2].to_string())
    } else {
        None
    }
}

/// Extract the loader version from a free-form version id. Handles both
/// hyphen styles (`1.20.1-forge-47.1.0`) and space styles
/// (`1.16.5-Fabric 0.15.11-OptiFine_G8`). Returns None if nothing follows the
/// loader keyword.
fn parse_loader_version(id: &str, loader: &str) -> Option<String> {
    let lower = id.to_lowercase();
    let kw = loader.to_lowercase();
    // find the loader keyword (may be followed by '-' or ' ')
    if let Some(pos) = lower.find(&kw) {
        let after = &id[pos + kw.len()..];
        // skip a single separator char (- or space)
        let rest = after
            .strip_prefix('-')
            .or_else(|| after.strip_prefix(' '))
            .unwrap_or(after);
        // take up to the next '-' (which begins the OptiFine / extra suffix)
        let ver: String = rest
            .chars()
            .take_while(|c| *c != '-')
            .collect();
        if ver.is_empty() {
            None
        } else {
            Some(ver)
        }
    } else {
        None
    }
}

/// Resolve a single installed version from its `versions/<name>/` directory.
fn one_version(dir_path: &std::path::Path, name: &str) -> MinecraftVersionInfo {
    let json_path = dir_path.join(format!("{name}.json"));
    // The raw version id, e.g. `1.20.1-forge-47.1.0`. This is what we use to
    // detect the loader, because the loader marker lives in it.
    let mut raw_id = name.to_string();
    // The "real" MC version to install/display: for a modded profile this is the
    // `inheritsFrom` vanilla base (e.g. `1.20.1`).
    let mut id = name.to_string();
    let mut inherits_base = false;
    let mut loader = "vanilla".to_string();
    let mut loader_version = None;
    if let Ok(content) = std::fs::read_to_string(&json_path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(s) = v.get("id").and_then(|x| x.as_str()) {
                raw_id = s.to_string();
                id = raw_id.clone();
            }
            if let Some(inherits) = v.get("inheritsFrom").and_then(|x| x.as_str()) {
                id = inherits.to_string();
                inherits_base = true;
            }
            // Detect using the RAW id (keeps the `-forge-` / `-fabric-` marker),
            // the libraries, and the mainClass.
            let (l, lv) = detect_loader(&v, &raw_id, inherits_base);
            loader = l.to_string();
            loader_version = lv;
            // `install_game` resolves the vanilla base via mcmeta, which only
            // knows standard Mojang versions. Modded profiles without an
            // `inheritsFrom` (e.g. standalone OptiFine / old Forge) must have
            // their id rewritten to the vanilla base extracted from the raw id,
            // otherwise `fetch_version_json` fails with "未找到 Minecraft 版本".
            if !inherits_base && loader != "vanilla" {
                // standalone modded json (e.g. OptiFine `1.16.5-OptiFine_HD_U_G8`):
                // strip the loader suffix to get the vanilla base Mojang knows.
                let base: String = raw_id
                    .chars()
                    .take_while(|c| *c != '-')
                    .collect();
                if !base.is_empty() {
                    id = base;
                    inherits_base = true;
                }
            }
        }
    }
    // NOTE: we deliberately do NOT walk the version folder here to compute
    // `size_bytes` — that would defeat the "list versions first, stats later"
    // flow. The migration size is computed on demand in `estimate_import_size`
    // once the user has selected versions. The field is left at 0 here.
    let size_bytes = 0u64;
    MinecraftVersionInfo {
        id,
        raw_id: name.to_string(),
        inherits_base,
        loader,
        loader_version,
        size_bytes,
    }
}

/// Visit every installed version under `versions/`, one at a time, so the
/// caller can emit each as it is discovered.
pub fn for_each_version(source: &std::path::Path, mut on_version: impl FnMut(MinecraftVersionInfo)) {
    let versions_dir = source.join("versions");
    let Ok(entries) = std::fs::read_dir(&versions_dir) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let dir_path = e.path();
        if !dir_path.is_dir() {
            continue;
        }
        on_version(one_version(&dir_path, &name));
    }
}

/// Validate the source folder. The actual scanning is streamed via events;
/// this only guards against a non-folder input.
pub fn scan_minecraft_import(source: &std::path::Path) -> Result<(), String> {
    if !source.is_dir() {
        return Err("所选路径不是一个文件夹".into());
    }
    Ok(())
}

/// Estimate the download size (client + libraries + assets index) for a single
/// MC version. Only hits the network; does not touch the disk.
pub async fn estimate_download(
    state: &AppState,
    mc_version: &str,
) -> MinecraftDownloadEstimate {
    let mut out = MinecraftDownloadEstimate {
        download_files: 0,
        download_bytes: 0,
        assets_known: false,
    };
    if mc_version.is_empty() {
        return out;
    }
    if let Ok(vanilla) = crate::mcmeta::fetch_version_json(state, mc_version).await {
        if let Some(client) = &vanilla.downloads.client {
            out.download_files += 1;
            out.download_bytes += client.size;
        }
        let features = std::collections::HashMap::new();
        for lib in &vanilla.libraries {
            if !crate::util::rules_allow(lib.rules.as_deref().unwrap_or(&[]), &features) {
                continue;
            }
            if let Some(art) = lib.downloads.as_ref().and_then(|d| d.artifact.as_ref()) {
                out.download_files += 1;
                out.download_bytes += art.size;
            }
        }
        if let Some(index) = &vanilla.asset_index {
            out.download_files += 1;
            out.download_bytes += index.size;
            out.assets_known = true;
        }
    }
    out
}

/// Migration-size estimate for a chosen set of versions, computed on demand
/// (after the user selects versions) rather than up-front. Counts the shared
/// user-data directories once and multiplies by the number of instances that
/// will be created, then adds each selected version folder's size.
pub fn estimate_import_size(source: &std::path::Path, raw_ids: &[String]) -> ImportSizeEstimate {
    let mut shared_files = 0u64;
    let mut shared_bytes = 0u64;
    for d in IMPORT_DIRS {
        let p = source.join(d);
        if p.is_dir() {
            walk_dir(&p, &mut |len| {
                shared_files += 1;
                shared_bytes += len;
            });
        }
    }
    for f in IMPORT_FILES {
        let p = source.join(f);
        if p.is_file() {
            if let Ok(m) = std::fs::metadata(&p) {
                shared_files += 1;
                shared_bytes += m.len();
            }
        }
    }
    let mut ver_files = 0u64;
    let mut ver_bytes = 0u64;
    for raw in raw_ids {
        let vd = source.join("versions").join(raw);
        if vd.is_dir() {
            walk_dir(&vd, &mut |len| {
                ver_files += 1;
                ver_bytes += len;
            });
        }
    }
    let n = raw_ids.len().max(1) as u64;
    ImportSizeEstimate {
        import_files: shared_files * n + ver_files,
        import_bytes: shared_bytes * n + ver_bytes,
    }
}

/// Create an instance from an existing `.minecraft` folder.
/// Import an existing `.minecraft` folder as one or more instances. When
/// `mc_versions` contains several entries, a separate instance is created for
/// each version (all sharing the same migration of the source folder).
///
/// `raw_ids` are the literal folder names under `versions/` (what we migrate
/// from); `mc_versions` are the resolved install/display versions (vanilla base
/// for modded profiles). `loaders` / `loader_versions` are aligned by index.
pub async fn import_minecraft_folder(
    app: tauri::AppHandle,
    state: &AppState,
    source: std::path::PathBuf,
    _name: String,
    raw_ids: Vec<String>,
    mc_versions: Vec<String>,
    loaders: Vec<String>,
    loader_versions: Vec<Option<String>>,
    mode: ImportMode,
) -> Result<Vec<crate::models::InstallPlan>, String> {
    if !source.is_dir() {
        return Err("所选路径不是一个文件夹".into());
    }
    // Resolve the version list. Each entry is (raw_id, mc_version, loader, lv).
    // `raw_id` is the literal folder name under versions/ we migrate from.
    let versions: Vec<(String, String, String, Option<String>)> =
        if mc_versions.is_empty() {
            let mut all = Vec::new();
            crate::instances::for_each_version(&source, |v| {
                all.push((v.raw_id, v.id, v.loader, v.loader_version));
            });
            if all.is_empty() {
                return Err("无法从文件夹识别游戏版本，请手动选择版本后重试".into());
            }
            all
        } else {
            // align the supplied raw-ids/loaders/loader-versions with the chosen versions
            mc_versions
                .into_iter()
                .enumerate()
                .map(|(i, ver)| {
                    let raw = raw_ids.get(i).cloned().unwrap_or_else(|| ver.clone());
                    let loader = loaders.get(i).cloned().unwrap_or_else(|| "vanilla".into());
                    let lv = loader_versions.get(i).cloned().flatten();
                    (raw, ver, loader, lv)
                })
                .collect::<Vec<_>>()
        };

    // `raw_ver` is joined onto `source/versions/` during migration; reject any
    // value that could escape that folder via path traversal.
    for (raw, ..) in &versions {
        if raw.is_empty()
            || raw == "."
            || raw == ".."
            || raw.contains("..")
            || raw.contains('/')
            || raw.contains('\\')
            || raw.contains(':')
        {
            return Err("非法的版本文件夹名".into());
        }
    }

    let link = mode == ImportMode::Symlink;
    let total = versions.len() as u64;
    let mut plans = Vec::new();
    for (idx, (raw_ver, ver, loader_str, lv)) in versions.iter().enumerate() {
        let loader = match loader_str.as_str() {
            "fabric" => crate::models::LoaderType::Fabric,
            "quilt" => crate::models::LoaderType::Quilt,
            "forge" => crate::models::LoaderType::Forge,
            "neoforge" => crate::models::LoaderType::NeoForge,
            _ => crate::models::LoaderType::Vanilla,
        };
        let inst_name = raw_ver.clone();
        let _ = app.emit(
            "import://progress",
            serde_json::json!({
                "phase": "migrate",
                "current": idx as u64 + 1,
                "total": total,
                "name": inst_name,
                "done": false
            }),
        );
        let plan = import_one(
            app.clone(),
            state,
            &source,
            inst_name.clone(),
            raw_ver.clone(),
            ver.clone(),
            loader,
            lv.clone(),
            link,
        )
        .await?;
        plans.push(plan);
        let _ = app.emit(
            "import://progress",
            serde_json::json!({
                "phase": "done",
                "current": idx as u64 + 1,
                "total": total,
                "name": inst_name.clone(),
                "done": true
            }),
        );
    }
    Ok(plans)
}

/// Create a single instance from the import source and install its game core.
/// `raw_ver` is the literal source folder name under `versions/`; `mc_version`
/// is the resolved install version (vanilla base for modded profiles), used for
/// the instance record and mcmeta lookup. The source version folder is migrated
/// into `versions/<instance.id>/` and its json/jar renamed to `<instance.id>`
/// so the launcher's normal launch path can find it.
async fn import_one(
    app: tauri::AppHandle,
    state: &AppState,
    source: &std::path::Path,
    name: String,
    raw_ver: String,
    mc_version: String,
    loader: LoaderType,
    loader_version: Option<String>,
    link: bool,
) -> Result<crate::models::InstallPlan, String> {
    let mut instance = create_instance(state, name, mc_version, loader, loader_version)?;
    instance.is_symlink = link;
    instance.source_path = if link {
        Some(source.to_string_lossy().to_string())
    } else {
        None
    };
    let bgs = ["amber", "blue", "green", "purple", "red", "slate", "dark"];
    let pick = bgs[uuid::Uuid::new_v4().as_u128() as usize % bgs.len()];
    instance.icon = Some(format!("bg:{pick}"));
    save_instance(state, &instance)?;
    let dest = state.instances_dir().join(&instance.id);
    // becomes true if a requested symlink had to fall back to a copy
    let mut symlink_fallback = false;

    // ---- migrate shared user data (saves, mods, config, ...) ----
    if !dest.exists() {
        std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    }
    {
        let mut migrated = 0u64;
        // In symlink mode, only the version folder is linked — NOT the shared
        // user-data dirs (saves/mods/resourcepacks/…).  Those are shared across
        // versions in the source .minecraft and linking them would cause content
        // to "bleed" between instances.  Each instance gets its own independent
        // empty dirs (created below).
        if !link {
            for d in IMPORT_DIRS {
                let src = source.join(d);
                if !src.is_dir() {
                    continue;
                }
                let dst = dest.join(d);
                if let Some(p) = dst.parent() {
                    std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
                }
                migrate_tree(&src, &dst, link, &mut symlink_fallback)?;
                migrated += 1;
            }
            for f in IMPORT_FILES {
                let src = source.join(f);
                if src.is_file() {
                    crate::util::copy_or_link(&src, &dest.join(f), link, &mut symlink_fallback)?;
                    migrated += 1;
                }
            }
        }
        let _ = migrated;
    }

    // ---- migrate ONLY the selected version's folder, never the whole versions/ ----
    // Version files (<id>.json / <id>.jar) are placed directly in instances/<id>/
    // (flattened layout) so resolve_version_dir finds them without nesting.
    let src_ver = source.join("versions").join(&raw_ver);
    if src_ver.is_dir() {
        migrate_version_dir(&src_ver, &dest, &instance.id, &raw_ver, link, &mut symlink_fallback)?;
    }

    // In symlink mode, warn if the source version had no version-isolation
    // (no mods/saves inside versions/<ver>/).  The shared root-level mods/saves
    // are intentionally not linked to avoid content bleed between instances.
    if link && src_ver.is_dir() {
        let has_isolation = IMPORT_DIRS.iter().any(|d| src_ver.join(d).is_dir());
        if !has_isolation {
            let _ = app.emit("import://warning", serde_json::json!({
                "name": raw_ver,
                "message": "未开启版本隔离，mods / 存档 / 材质包等共享内容未导入",
            }));
        }
    }

    // make sure standard folders exist
    for sub in ["mods", "shaderpacks", "resourcepacks", "saves", "screenshots", "config"] {
        let _ = std::fs::create_dir_all(dest.join(sub));
    }

    // ---- reuse libraries & assets from source to avoid re-downloading ----
    reuse_runtime_files(&source.join("libraries"), &state.libraries_dir());
    reuse_runtime_files(&source.join("assets"), &state.assets_dir());

    // ---- install the game (client / libraries / assets / loader) ----
    let mut plan = crate::install::install_game(app, state, &instance).await?;
    plan.symlink_fallback = symlink_fallback;
    Ok(plan)
}

/// Copy files from src to dst, skipping files that already exist with the same size.
/// Used to reuse libraries/assets from the source .minecraft so install_game doesn't re-download them.
fn reuse_runtime_files(src: &std::path::Path, dst: &std::path::Path) {
    if !src.is_dir() { return; }
    let _ = std::fs::create_dir_all(dst);
    let entries = match std::fs::read_dir(src) { Ok(e) => e, Err(_) => return };
    for entry in entries.flatten() {
        let p = entry.path();
        let target = dst.join(entry.file_name());
        if p.is_dir() {
            reuse_runtime_files(&p, &target);
        } else if p.is_file() {
            let src_size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            let need_copy = match std::fs::metadata(&target) {
                Ok(m) => m.len() != src_size,
                Err(_) => true,
            };
            if need_copy {
                let _ = std::fs::create_dir_all(target.parent().unwrap_or(dst));
                let _ = std::fs::copy(&p, &target);
            }
        }
    }
}

/// Recursively copy or symlink a directory tree. `fallback` is set to true if
/// any symlink attempt had to fall back to a copy (OS denied the privilege).
fn migrate_tree(src: &std::path::Path, dst: &std::path::Path, link: bool, fallback: &mut bool) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    let entries = std::fs::read_dir(src).map_err(|e| e.to_string())?;
    for e in entries.flatten() {
        let p = e.path();
        let name = match p.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => continue,
        };
        let target = dst.join(&name);
        if p.is_dir() {
            migrate_tree(&p, &target, link, fallback)?;
        } else if p.is_file() {
            crate::util::copy_or_link(&p, &target, link, fallback)?;
        }
    }
    Ok(())
}

/// Migrate a single source version folder into `versions/<instance_id>/`,
/// renaming its `<raw_ver>.json` -> `<instance_id>.json` (and rewriting the
/// internal `id`) and `<raw_ver>.jar` -> `<instance_id>.jar`, so the launcher's
/// launch path (which looks up `versions/<id>/<id>.json`) can find it.
fn migrate_version_dir(
    src: &std::path::Path,
    dst: &std::path::Path,
    instance_id: &str,
    raw_ver: &str,
    link: bool,
    fallback: &mut bool,
) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    let entries = std::fs::read_dir(src).map_err(|e| e.to_string())?;
    for e in entries.flatten() {
        let p = e.path();
        let name = match p.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => continue,
        };
        // Rename the version json and rewrite its internal `id` so launch.rs
        // resolves `versions/<instance_id>/<instance_id>.json`.
        if name == format!("{raw_ver}.json") {
            let target = dst.join(format!("{instance_id}.json"));
            let content = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
            if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(obj) = v.as_object_mut() {
                    obj.insert("id".into(), serde_json::Value::String(instance_id.to_string()));
                    let _ = std::fs::write(&target, serde_json::to_string_pretty(&v).unwrap_or(content));
                    continue;
                }
            }
            crate::util::copy_or_link(&p, &target, link, fallback)?;
            continue;
        }
        // Rename the client jar to match the instance id.
        if name == format!("{raw_ver}.jar") {
            let target = dst.join(format!("{instance_id}.jar"));
            crate::util::copy_or_link(&p, &target, link, fallback)?;
            continue;
        }
        let target = dst.join(&name);
        if p.is_dir() {
            migrate_tree(&p, &target, link, fallback)?;
        } else if p.is_file() {
            crate::util::copy_or_link(&p, &target, link, fallback)?;
        }
    }
    // 确保版本 json 文件名为 <instance_id>.json（源文件名可能不匹配 raw_ver）
    let target_json = dst.join(format!("{instance_id}.json"));
    if !target_json.exists() {
        if let Ok(entries) = std::fs::read_dir(dst) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.ends_with(".json") && name != format!("{instance_id}.json") {
                    let src_path = e.path();
                    if let Ok(content) = std::fs::read_to_string(&src_path) {
                        if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&content) {
                            if let Some(obj) = v.as_object_mut() {
                                obj.insert("id".into(), serde_json::Value::String(instance_id.to_string()));
                                let _ = std::fs::write(&target_json, serde_json::to_string_pretty(&v).unwrap_or(content));
                                let _ = std::fs::remove_file(&src_path);
                                break;
                            }
                        }
                    }
                    let _ = std::fs::rename(&src_path, &target_json);
                    break;
                }
            }
        }
    }
    // 确保版本 jar 文件名为 <instance_id>.jar
    let target_jar = dst.join(format!("{instance_id}.jar"));
    if !target_jar.exists() {
        if let Ok(entries) = std::fs::read_dir(dst) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.ends_with(".jar") && name != format!("{instance_id}.jar") {
                    let _ = std::fs::rename(e.path(), &target_jar);
                    break;
                }
            }
        }
    }
    Ok(())
}
