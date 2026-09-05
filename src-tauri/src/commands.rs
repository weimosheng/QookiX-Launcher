use crate::accounts;
use crate::curseforge;
use crate::install;
use crate::launch;
use crate::crash::CrashDiagnosis;
use crate::mcmeta;
use crate::models::*;
use crate::modrinth;
use crate::settings;
use crate::mcping;
use crate::state::AppState;
use serde_json::{json, Value};
use tauri::Emitter;
use tauri::Manager;
use tauri::State;

// ---------------------------------------------------------------------------
// Settings & Java
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Result<Settings, String> {
    Ok(state.settings.read().unwrap().clone())
}

#[tauri::command]
pub fn set_settings(state: State<AppState>, patch: Value) -> Result<Settings, String> {
    settings::update_settings(&state, patch)
}

// ---------------------------------------------------------------------------
// Pinned items (首页快捷卡片 / 侧边栏图标)
// ---------------------------------------------------------------------------

/// 读取全部固定项（首页与侧边栏各自独立存储）。
#[tauri::command]
pub fn get_pins(state: State<AppState>) -> Vec<crate::models::PinItem> {
    crate::pins::load_pins(&state.root)
}

/// 覆盖写入全部固定项到 `pins.json`。
#[tauri::command]
pub fn set_pins(state: State<AppState>, items: Vec<crate::models::PinItem>) -> Result<(), String> {
    crate::pins::save_pins(&state.root, &items)
}

/// 可用的下载镜像源预设列表。
#[tauri::command]
pub fn list_mirrors() -> Value {
    crate::mirror::presets()
}

/// 测试镜像源连通性并返回首字节耗时（毫秒）。
/// `base` 为空串表示测试官方源。仅发一个 GET 并读取状态，不下载正文。
#[tauri::command]
pub async fn test_mirror(state: State<'_, AppState>, base: String) -> Result<Value, String> {
    let base = base.trim().trim_end_matches('/').to_string();
    let url = if base.is_empty() {
        crate::mirror::OFFICIAL_MANIFEST.to_string()
    } else {
        format!("{base}/mc/game/version_manifest_v2.json")
    };
    let start = std::time::Instant::now();
    let resp = state
        .client
        .get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("连接失败: {e}"))?;
    let ms = start.elapsed().as_millis() as u64;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    Ok(json!({ "ok": true, "ms": ms, "url": url }))
}

/// 测试下载代理配置是否可用。
/// 用传入的 `proxy_mode`/`proxy` 临时构建客户端访问官方 manifest，
/// 返回首字节耗时（毫秒）。`proxy` 仅在 `proxy_mode == "custom"` 时生效。
#[tauri::command]
pub async fn test_proxy(proxy_mode: String, proxy: Option<String>) -> Result<Value, String> {
    // 自定义模式必须给出合法的代理地址，否则 http_client 会退化为直连，
    // 导致「没填地址也测试成功」——这里先校验，避免误报成功。
    if proxy_mode == "custom" {
        let addr = proxy.as_deref().unwrap_or("").trim();
        if addr.is_empty() {
            return Err("请先填写代理地址".to_string());
        }
        if reqwest::Proxy::all(addr).is_err() {
            return Err("代理地址格式不正确，应类似 http://127.0.0.1:7890 或 socks5://127.0.0.1:1080".to_string());
        }
    }
    let client = settings::http_client(&proxy_mode, proxy.as_deref());
    let url = crate::mirror::OFFICIAL_MANIFEST.to_string();
    let start = std::time::Instant::now();
    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("连接失败: {e}"))?;
    let ms = start.elapsed().as_millis() as u64;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    Ok(json!({ "ok": true, "ms": ms, "url": url }))
}

/// Move/copy the launcher data root to a new directory.
/// `mode`: "move" | "copy" | "pointer". Returns the new data dir; the caller
/// should restart for the change to fully take effect.
#[tauri::command]
pub fn change_data_dir(
    state: State<AppState>,
    new_dir: String,
    mode: String,
) -> Result<Value, String> {
    let new_dir = settings::change_data_dir(&state, &new_dir, &mode)?;
    Ok(json!({ "ok": true, "new_dir": new_dir, "need_restart": true }))
}

/// Auto-detect system memory and return (total, used, available) + recommended (max, min) in MB.
#[tauri::command]
pub fn auto_detect_memory() -> Result<Value, String> {
    let total = crate::settings::total_memory_mb().ok_or("无法检测系统内存")?;
    let used = crate::settings::used_memory_mb().unwrap_or(0);
    let available = crate::settings::available_memory_mb().unwrap_or(total.saturating_sub(used));
    let (max, min) = crate::settings::recommended_memory(available, 0);
    Ok(json!({
        "total_mb": total,
        "used_mb": used,
        "available_mb": available,
        "max_mb": max,
        "min_mb": min
    }))
}

#[tauri::command]
pub fn detect_java(
    state: State<AppState>,
    refresh: Option<bool>,
) -> Result<JavaDetection, String> {
    let force = refresh.unwrap_or(false);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Use cache unless manually forced to refresh
    if !force {
        if let Some((_ts, list)) = state.java_cache.lock().unwrap().as_ref() {
            return Ok(JavaDetection {
                candidates: list.clone(),
                selected: list.first().cloned(),
            });
        }
    }
    let root = state.root.clone();
    let runtimes = root.join("runtimes");
    // Reuse the persisted cache when possible; `force` re-scans and updates it.
    let (ts, candidates) = crate::java::cached_detect(&root, &runtimes, now, force);
    let selected = candidates.first().cloned();
    *state.java_cache.lock().unwrap() = Some((ts, candidates.clone()));
    Ok(JavaDetection { candidates, selected })
}

/// Download a Java runtime of the given major version (if not already present).
#[tauri::command]
pub async fn download_java(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    major: u32,
) -> Result<JavaInfo, String> {
    crate::java::download_java_runtime(app, &state, major).await
}

/// Recommended Java for an instance: required major + best available match.
#[tauri::command]
pub async fn recommend_java(state: State<'_, AppState>, instance_id: String) -> Result<Value, String> {
    let instance = crate::instances::get_instance(&state, &instance_id)?;
    let required = launch::required_java_for(&state, &instance);
    let best = launch::find_best_java(&state, required).await;
    Ok(json!({
        "required": required,
        "java": best,
        "needDownload": best.as_ref().map(|j| j.major < required).unwrap_or(true),
    }))
}

// ---------------------------------------------------------------------------
// Version metadata
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_version_manifest(state: State<'_, AppState>) -> Result<Value, String> {
    let manifest = mcmeta::fetch_manifest(&state).await?;
    // the Mojang manifest is already ordered newest -> oldest
    let versions: Vec<Value> = manifest
        .versions
        .iter()
        .map(|v| {
            json!({
                "id": v.id,
                "type": v.kind,
                "releaseTime": v.release_time,
            })
        })
        .collect();
    Ok(json!({
        "versions": versions,
        "latest": { "release": manifest.latest.release, "snapshot": manifest.latest.snapshot }
    }))
}

#[tauri::command]
pub async fn get_loader_versions(
    state: State<'_, AppState>,
    loader: String,
    mc_version: String,
) -> Result<Vec<String>, String> {
    let l = match loader.as_str() {
        "fabric" => LoaderType::Fabric,
        "quilt" => LoaderType::Quilt,
        "forge" => LoaderType::Forge,
        "neoforge" => LoaderType::NeoForge,
        _ => return Err("未知加载器".into()),
    };
    install::loader_versions(&state, l, &mc_version).await
}

// ---------------------------------------------------------------------------
// Instances
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_instances(state: State<AppState>) -> Result<Vec<Instance>, String> {
    Ok(crate::instances::load_instances(&state))
}

#[tauri::command]
pub fn get_instance_info(state: State<AppState>, id: String) -> Result<Instance, String> {
    crate::instances::get_instance(&state, &id)
}

#[tauri::command]
pub fn create_instance(
    app: tauri::AppHandle,
    state: State<AppState>,
    name: String,
    mc_version: String,
    loader: String,
    loader_version: Option<String>,
) -> Result<Instance, String> {
    let l = match loader.as_str() {
        "vanilla" => LoaderType::Vanilla,
        "fabric" => LoaderType::Fabric,
        "quilt" => LoaderType::Quilt,
        "forge" => LoaderType::Forge,
        "neoforge" => LoaderType::NeoForge,
        _ => return Err("未知加载器".into()),
    };
    let mc = mc_version.clone();
    let instance = crate::instances::create_instance(&state, name, mc_version, l, loader_version)?;

    // Fabric（及兼容的 Quilt）实例自动补装 Fabric API：绝大多数模组的前置，
    // 后台 best-effort 安装，失败不影响实例创建。进度走通用 install://progress 事件。
    if matches!(instance.loader, LoaderType::Fabric | LoaderType::Quilt) {
        let app2 = app.clone();
        let iid = instance.id.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = modrinth::auto_install_fabric_api(&app2, &iid, &mc).await {
                eprintln!("[fabric-api] 自动安装失败（实例 {iid}，MC {mc}）: {e}");
            }
        });
    }
    Ok(instance)
}

#[tauri::command]
pub fn update_instance_settings(state: State<AppState>, patch: Value) -> Result<Instance, String> {
    crate::instances::update_instance(&state, patch)
}

#[tauri::command]
pub fn delete_instance(state: State<AppState>, id: String) -> Result<(), String> {
    crate::instances::delete_instance(&state, &id)
}

#[tauri::command]
pub fn list_instance_groups(state: State<AppState>) -> Vec<InstanceGroup> {
    crate::instances::load_groups(&state)
}

#[tauri::command]
pub fn create_instance_group(
    state: State<AppState>,
    name: String,
    color: Option<String>,
) -> Result<InstanceGroup, String> {
    crate::instances::create_group(&state, name, color)
}

#[tauri::command]
pub fn rename_instance_group(
    state: State<AppState>,
    id: String,
    name: String,
    color: Option<String>,
) -> Result<InstanceGroup, String> {
    crate::instances::rename_group(&state, &id, name, color)
}

#[tauri::command]
pub fn delete_instance_group(state: State<AppState>, id: String) -> Result<(), String> {
    crate::instances::delete_group(&state, &id)
}

#[tauri::command]
pub fn reorder_instance_groups(
    state: State<AppState>,
    ids: Vec<String>,
) -> Result<Vec<InstanceGroup>, String> {
    crate::instances::reorder_groups(&state, ids)
}

#[tauri::command]
pub async fn install_game(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<InstallPlan, String> {
    let instance = crate::instances::get_instance(&state, &instance_id)?;
    let plan = install::install_game(app.clone(), &state, &instance).await?;
    crate::instances::mark_installed(&state, &instance_id)?;
    Ok(plan)
}

#[tauri::command]
pub fn cancel_install(state: State<AppState>) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    state.install_cancel.store(true, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub async fn launch_instance(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    world: Option<String>,
    server: Option<String>,
) -> Result<LaunchResult, String> {
    let instance = crate::instances::get_instance(&state, &instance_id)?;
    // resolve account: instance override -> global selected -> first
    let accounts = accounts::load_accounts(&state);
    let selected = {
        let s = state.settings.read().unwrap();
        s.selected_account.clone()
    };
    let account = if let Some(aid) = &instance.account_id {
        accounts.iter().find(|a| a.uuid() == aid).cloned()
    } else if let Some(aid) = &selected {
        accounts.iter().find(|a| a.uuid() == aid).cloned()
    } else {
        accounts.first().cloned()
    };
    let account = account.ok_or("请先在左下角账号栏添加账号（正版或离线）")?;
    let _ = app.emit("launch://progress", serde_json::json!({ "step": "正在登录账号…", "progress": 10 }));
    let account = accounts::refresh_microsoft(&state, &account).await?;
    let _ = app.emit("launch://progress", serde_json::json!({ "step": "账号准备完成", "progress": 25 }));
    let resolved = launch::ResolvedAccount {
        username: account.username().to_string(),
        uuid: account.uuid().to_string(),
        access_token: match &account {
            Account::Microsoft { msa_access_token, .. } => msa_access_token.clone(),
            Account::Offline { .. } => "0".into(),
        },
        user_type: if account.is_microsoft() { "msa".into() } else { "legacy".into() },
        user_properties: "{}".into(),
    };
    let result = launch::launch_game(app.clone(), &state, &instance, resolved, world, server).await?;
    crate::instances::touch_last_played(&state, &instance_id);
    Ok(result)
}

#[tauri::command]
pub async fn stop_game(state: State<'_, AppState>) -> Result<(), String> {
    launch::kill_game(&state).await
}

#[tauri::command]
pub fn is_game_running(state: State<AppState>) -> Result<bool, String> {
    Ok(launch::is_running(&state))
}

#[tauri::command]
pub fn open_instance_folder(
    app: tauri::AppHandle,
    state: State<AppState>,
    instance_id: String,
    sub: Option<String>,
) -> Result<(), String> {
    let mut dir = state.instances_dir().join(&instance_id);
    if let Some(s) = sub {
        if !SUBFOLDERS.contains(&s.as_str()) {
            return Err("非法目录".into());
        }
        dir = dir.join(s);
    }
    if !dir.exists() {
        return Err("目录不存在".into());
    }
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Known instance subfolders usable in folder/file commands.
pub const SUBFOLDERS: [&str; 9] = [
    "mods", "shaderpacks", "resourcepacks", "saves", "screenshots", "config", "logs", "natives", "icons",
];

#[tauri::command]
pub fn list_instance_folders(state: State<AppState>, instance_id: String) -> Result<Value, String> {
    let dir = state.instances_dir().join(&instance_id);
    let folders: Vec<Value> = SUBFOLDERS
        .iter()
        .map(|f| json!({ "name": f, "exists": dir.join(f).is_dir() }))
        .collect();
    Ok(json!({ "folders": folders }))
}

#[tauri::command]
pub async fn list_instance_files(
    state: State<'_, AppState>,
    instance_id: String,
    sub: String,
) -> Result<Value, String> {
    if !SUBFOLDERS.contains(&sub.as_str()) {
        return Err("非法目录".into());
    }
    let dir = state.instances_dir().join(&instance_id).join(&sub);
    if !dir.exists() {
        return Ok(json!({ "files": [] }));
    }
    let files = tokio::task::spawn_blocking(move || {
        let mut files: Vec<Value> = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
            let e = entry.map_err(|e| e.to_string())?;
            let meta = e.metadata().map_err(|e| e.to_string())?;
            let path = e.path();
            let mut icon: Option<String> = None;
            if sub == "saves" && meta.is_dir() {
                let icon_path = path.join("icon.png");
                if icon_path.is_file() {
                    icon = Some(icon_path.to_string_lossy().to_string());
                }
            }
            files.push(json!({
                "name": e.file_name().to_string_lossy().to_string(),
                "path": path.to_string_lossy().to_string(),
                "size": meta.len(),
                "modified": meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0),
                "isDir": meta.is_dir(),
                "icon": icon,
            }));
        }
        files.sort_by(|a, b| {
            let da = a.get("isDir").and_then(|v| v.as_bool()).unwrap_or(false);
            let db = b.get("isDir").and_then(|v| v.as_bool()).unwrap_or(false);
            db.cmp(&da).then_with(|| {
                a.get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_lowercase()
                    .cmp(&b.get("name").and_then(|v| v.as_str()).unwrap_or("").to_lowercase())
            })
        });
        Ok::<Vec<Value>, String>(files)
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(json!({ "files": files }))
}

// ---------------------------------------------------------------------------
// Instance file manager
// ---------------------------------------------------------------------------

/// Maximum file size (bytes) the built-in editor is willing to load.
const MAX_EDIT_BYTES: u64 = 4 * 1024 * 1024;

fn fmt_bytes(n: u64) -> String {
    if n >= 1024 * 1024 {
        format!("{:.1} MB", n as f64 / 1024.0 / 1024.0)
    } else if n >= 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

fn modified_secs(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn ext_of(name: &str) -> String {
    std::path::Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase()
}

/// Resolve an instance-relative path while guaranteeing the result never
/// escapes the instance directory (blocks `..`, absolute paths and symlinks
/// that point outside). Paths that do not exist yet (create / rename targets)
/// are validated lexically instead.
fn resolve_instance_path(
    state: &AppState,
    instance_id: &str,
    rel: &str,
) -> Result<std::path::PathBuf, String> {
    if instance_id.is_empty()
        || instance_id.contains("..")
        || instance_id.contains('/')
        || instance_id.contains('\\')
    {
        return Err("非法实例 ID".into());
    }
    let root = state
        .instances_dir()
        .join(instance_id)
        .canonicalize()
        .map_err(|e| format!("实例目录不可用: {e}"))?;
    let cleaned = rel.replace('\\', "/");
    let cleaned = cleaned.trim_start_matches('/');
    let target = if cleaned.is_empty() {
        root.clone()
    } else {
        root.join(cleaned)
    };
    match target.canonicalize() {
        Ok(c) => {
            if c != root && !c.starts_with(&root) {
                return Err("路径超出实例目录范围".into());
            }
            Ok(c)
        }
        Err(_) => {
            // Target does not exist yet: verify every component stays inside.
            let mut depth = 0i32;
            for part in std::path::Path::new(cleaned).components() {
                match part {
                    std::path::Component::Normal(_) => depth += 1,
                    std::path::Component::ParentDir => depth -= 1,
                    std::path::Component::CurDir => {}
                    other => {
                        return Err(format!("非法路径: {}", other.as_os_str().to_string_lossy()))
                    }
                }
                if depth < 0 {
                    return Err("路径超出实例目录范围".into());
                }
            }
            Ok(target)
        }
    }
}

/// Reject names that would create nested paths or escape the parent folder.
fn validate_name(name: &str) -> Result<(), String> {
    let t = name.trim();
    if t.is_empty() || t == "." || t == ".." {
        return Err("名称不能为空".into());
    }
    if t.contains('/') || t.contains('\\') {
        return Err("名称不能包含路径分隔符".into());
    }
    Ok(())
}

#[derive(serde::Serialize)]
pub struct FsEntry {
    pub name: String,
    pub rel: String,
    pub size: u64,
    pub modified: u64,
    pub is_dir: bool,
    pub ext: String,
}

/// List the contents of any directory inside an instance folder.
#[tauri::command]
pub async fn list_instance_dir(
    state: State<'_, AppState>,
    instance_id: String,
    rel: String,
) -> Result<Value, String> {
    let dir = resolve_instance_path(&state, &instance_id, &rel)?;
    if !dir.is_dir() {
        return Err("不是一个目录".into());
    }
    let base = rel.trim_end_matches('/').to_string();
    let entries = tokio::task::spawn_blocking(move || {
        let mut out: Vec<FsEntry> = Vec::new();
        let rd = std::fs::read_dir(&dir).map_err(|e| format!("读取目录失败: {e}"))?;
        for e in rd.flatten() {
            let meta = match e.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let name = e.file_name().to_string_lossy().to_string();
            let is_dir = meta.is_dir();
            let child_rel = if base.is_empty() {
                name.clone()
            } else {
                format!("{base}/{name}")
            };
            out.push(FsEntry {
                ext: if is_dir { String::new() } else { ext_of(&name) },
                name,
                rel: child_rel,
                size: if is_dir { 0 } else { meta.len() },
                modified: modified_secs(&meta),
                is_dir,
            });
        }
        out.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok::<Vec<FsEntry>, String>(out)
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(json!({ "rel": rel, "entries": entries }))
}

/// Read a text file inside an instance folder for the built-in editor.
#[tauri::command]
pub fn read_instance_file(
    state: State<AppState>,
    instance_id: String,
    rel: String,
) -> Result<Value, String> {
    let path = resolve_instance_path(&state, &instance_id, &rel)?;
    if !path.is_file() {
        return Err("不是一个文件".into());
    }
    let meta = std::fs::metadata(&path).map_err(|e| format!("读取文件失败: {e}"))?;
    if meta.len() > MAX_EDIT_BYTES {
        return Err(format!(
            "文件过大（{}），内置编辑器最多支持 {}",
            fmt_bytes(meta.len()),
            fmt_bytes(MAX_EDIT_BYTES)
        ));
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("读取文件失败: {e}"))?;
    if bytes.iter().take(4096).any(|b| *b == 0) {
        return Err("这是二进制文件，无法在内置编辑器中打开".into());
    }
    let content = String::from_utf8(bytes)
        .map_err(|_| String::from("文件不是 UTF-8 编码，无法在内置编辑器中打开"))?;
    let meta2 = std::fs::metadata(&path).ok();
    Ok(json!({
        "rel": rel,
        "content": content,
        "size": meta.len(),
        "modified": meta2.as_ref().map(modified_secs).unwrap_or(0),
    }))
}

/// Write text content back to a file inside an instance folder.
#[tauri::command]
pub fn write_instance_file(
    state: State<AppState>,
    instance_id: String,
    rel: String,
    content: String,
) -> Result<Value, String> {
    let path = resolve_instance_path(&state, &instance_id, &rel)?;
    if path.is_dir() {
        return Err("目标是一个目录".into());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let len = content.len() as u64;
    std::fs::write(&path, content).map_err(|e| format!("写入文件失败: {e}"))?;
    let meta = std::fs::metadata(&path).ok();
    Ok(json!({
        "rel": rel,
        "size": meta.as_ref().map(|m| m.len()).unwrap_or(len),
        "modified": meta.as_ref().map(modified_secs).unwrap_or(0),
    }))
}

/// Create a new empty file or a new folder inside an instance folder.
#[tauri::command]
pub fn create_instance_entry(
    state: State<AppState>,
    instance_id: String,
    rel: String,
    is_dir: bool,
) -> Result<Value, String> {
    let last = rel.rsplit('/').next().unwrap_or("");
    validate_name(last)?;
    let path = resolve_instance_path(&state, &instance_id, &rel)?;
    if path.exists() {
        return Err("已存在同名的文件或文件夹".into());
    }
    if is_dir {
        std::fs::create_dir_all(&path).map_err(|e| format!("创建文件夹失败: {e}"))?;
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
        }
        std::fs::write(&path, "").map_err(|e| format!("创建文件失败: {e}"))?;
    }
    Ok(json!({ "rel": rel, "is_dir": is_dir }))
}

/// Delete a file or a folder (recursively) inside an instance folder.
#[tauri::command]
pub fn delete_instance_path(
    state: State<AppState>,
    instance_id: String,
    rel: String,
) -> Result<(), String> {
    if rel.trim().is_empty() {
        return Err("不能删除实例根目录".into());
    }
    let path = resolve_instance_path(&state, &instance_id, &rel)?;
    if !path.exists() {
        return Err("文件或文件夹不存在".into());
    }
    if path.is_dir() {
        std::fs::remove_dir_all(&path).map_err(|e| format!("删除文件夹失败: {e}"))?;
    } else {
        std::fs::remove_file(&path).map_err(|e| format!("删除文件失败: {e}"))?;
    }
    Ok(())
}

/// Rename a file or folder inside an instance folder.
#[tauri::command]
pub fn rename_instance_path(
    state: State<AppState>,
    instance_id: String,
    rel: String,
    new_name: String,
) -> Result<Value, String> {
    if rel.trim().is_empty() {
        return Err("不能重命名实例根目录".into());
    }
    validate_name(&new_name)?;
    let path = resolve_instance_path(&state, &instance_id, &rel)?;
    if !path.exists() {
        return Err("文件或文件夹不存在".into());
    }
    let parent = path.parent().ok_or("无法重命名该路径")?;
    let target = parent.join(new_name.trim());
    if target.exists() {
        return Err("已存在同名的文件或文件夹".into());
    }
    std::fs::rename(&path, &target).map_err(|e| format!("重命名失败: {e}"))?;
    let parent_rel = match rel.rfind('/') {
        Some(i) => rel[..i].to_string(),
        None => String::new(),
    };
    let new_rel = if parent_rel.is_empty() {
        new_name.trim().to_string()
    } else {
        format!("{}/{}", parent_rel, new_name.trim())
    };
    Ok(json!({ "rel": new_rel, "name": new_name.trim().to_string() }))
}

/// Open the path (or its parent folder for files) in the system file manager.
#[tauri::command]
pub fn reveal_instance_path(
    app: tauri::AppHandle,
    state: State<AppState>,
    instance_id: String,
    rel: String,
) -> Result<(), String> {
    let path = resolve_instance_path(&state, &instance_id, &rel)?;
    let open_target = if path.is_file() {
        path.parent().map(|p| p.to_path_buf()).unwrap_or(path.clone())
    } else {
        path.clone()
    };
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(open_target.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Import a local modpack (.mrpack / CurseForge zip): creates an instance
/// with the pack's Minecraft version + loader and stages its files.
#[tauri::command]
pub async fn import_modpack(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    file_path: String,
) -> Result<Instance, String> {
    let path = std::path::PathBuf::from(&file_path);
    if !path.exists() {
        return Err("文件不存在".into());
    }
    let (name, mc_version, loader, loader_version) =
        crate::modpack::detect(&path).await.map_err(|e| format!("无法解析整合包: {e}"))?;

    let instance = crate::instances::create_instance(
        &state,
        name,
        mc_version,
        loader,
        Some(loader_version),
    )?;
    crate::modpack::apply(&app, &state, &instance, &path).await?;
    Ok(instance)
}

/// Copy an image file into the launcher icons dir; returns the absolute path.
#[tauri::command]
pub fn import_instance_image(
    state: State<AppState>,
    source_path: String,
) -> Result<String, String> {
    const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico"];
    let source = std::path::Path::new(&source_path);
    let ext = source
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_else(|| "png".into());
    if !IMAGE_EXTS.contains(&ext.as_str()) {
        return Err("不支持的图片格式".into());
    }
    let dir = state.root.join("icons");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(format!("{}.{}", uuid::Uuid::new_v4().simple(), ext));
    std::fs::copy(source, &dest).map_err(|e| format!("复制图片失败: {e}"))?;
    Ok(dest.to_string_lossy().to_string())
}

/// Copy a user-selected image into the launcher backgrounds dir; returns the absolute path.
#[tauri::command]
pub fn import_background_image(
    state: State<AppState>,
    source_path: String,
) -> Result<String, String> {
    const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];
    let source = std::path::Path::new(&source_path);
    let ext = source
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_else(|| "png".into());
    if !IMAGE_EXTS.contains(&ext.as_str()) {
        return Err("不支持的图片格式".into());
    }
    let dir = state.root.join("backgrounds");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(format!("{}.{}", uuid::Uuid::new_v4().simple(), ext));
    std::fs::copy(source, &dest).map_err(|e| format!("复制背景图片失败: {e}"))?;
    Ok(dest.to_string_lossy().to_string())
}

/// Scan a `.minecraft` folder. Returns immediately; the actual work is streamed
/// to the frontend through events so the UI renders progressively:
///   - `import://scan-version`   { id, inherits_base, size_bytes }  one per version
///   - `import://scan-progress`  { import_files, import_bytes }     throttled, live
///   - `import://scan-progress`  { ..., download_files, download_bytes, assets_known, done }
///     sent once at the end with the download estimate.
#[tauri::command]
pub async fn scan_minecraft_import(
    app: tauri::AppHandle,
    _state: State<'_, AppState>,
    source: String,
) -> Result<(), String> {
    let src = std::path::PathBuf::from(&source);
    crate::instances::scan_minecraft_import(&src)?;

    let app2 = app.clone();
    let src2 = src.clone();
    tauri::async_runtime::spawn(async move {
        let src = src2;

        // ---- Phase 1: enumerate versions only (fast, reads each versions/<id>/<id>.json).
        //      The heavy user-data walk is DEFERRED until the user picks versions
        //      (see `estimate_import`), so we never thrash the disk up-front. ----
        let app_versions = app2.clone();
        let src_versions = src.clone();
        tokio::task::spawn_blocking(move || {
            let app = app_versions;
            crate::instances::for_each_version(&src_versions, |v| {
                let _ = app.emit(
                    "import://scan-version",
                    serde_json::json!({
                        "id": v.id,
                        "raw_id": v.raw_id,
                        "inherits_base": v.inherits_base,
                        "loader": v.loader,
                        "loader_version": v.loader_version,
                        "size_bytes": v.size_bytes
                    }),
                );
            });
        })
        .await
        .ok();

        // Signal the version list is complete. Import size is filled in later by
        // `estimate_import` once the user makes a selection.
        let _ = app2.emit(
            "import://scan-progress",
            serde_json::json!({
                "import_files": 0u64,
                "import_bytes": 0u64,
                "done": true
            }),
        );
    });

    Ok(())
}

/// Re-compute the download-size estimate for a specific MC version (e.g. when
/// the user switches the selected version in the picker). Fast: network only.
#[tauri::command]
pub async fn estimate_download(
    state: State<'_, AppState>,
    mc_version: String,
) -> Result<crate::instances::MinecraftDownloadEstimate, String> {
    Ok(crate::instances::estimate_download(&state, &mc_version).await)
}

/// Migration-size estimate for the user's current selection. Runs only after the
/// user has chosen versions (contrast with the old up-front full-folder walk),
/// and counts the shared user-data dirs (copied once per created instance) plus
/// each selected version folder.
#[tauri::command]
pub async fn estimate_import(
    source: String,
    raw_ids: Vec<String>,
) -> Result<crate::instances::ImportSizeEstimate, String> {
    let src = std::path::PathBuf::from(source);
    let res = tokio::task::spawn_blocking(move || {
        crate::instances::estimate_import_size(&src, &raw_ids)
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(res)
}

/// Import an existing `.minecraft` folder, creating one instance per selected
/// version. `raw_ids` / `loaders` / `loader_versions` are parallel arrays
/// aligned by index (the loader is auto-detected per version by the UI).
/// `raw_ids` are the literal folder names under `versions/`; `mc_versions` are
/// the resolved display/install versions (vanilla base for modded profiles).
#[tauri::command]
pub async fn import_minecraft_folder(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    source: String,
    name: String,
    raw_ids: Vec<String>,
    mc_versions: Vec<String>,
    loaders: Vec<String>,
    loader_versions: Vec<Option<String>>,
    mode: String,
) -> Result<Vec<crate::models::InstallPlan>, String> {
    let mode = if mode == "symlink" {
        crate::instances::ImportMode::Symlink
    } else {
        crate::instances::ImportMode::Copy
    };
    crate::instances::import_minecraft_folder(
        app,
        &state,
        std::path::PathBuf::from(source),
        name,
        raw_ids,
        mc_versions,
        loaders,
        loader_versions,
        mode,
    )
    .await
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_accounts(state: State<AppState>) -> Result<Vec<Value>, String> {
    Ok(accounts::load_accounts(&state)
        .into_iter()
        .map(|a| strip_account_tokens(serde_json::to_value(&a).unwrap_or_default()))
        .collect())
}

#[tauri::command]
pub fn login_offline(state: State<AppState>, username: String) -> Result<Account, String> {
    accounts::create_offline(&state, &username)
}

#[tauri::command]
pub async fn login_ms_start(state: State<'_, AppState>) -> Result<Value, String> {
    accounts::ms_start(&state).await
}

#[tauri::command]
pub async fn login_ms_poll(state: State<'_, AppState>) -> Result<Value, String> {
    let acc = accounts::ms_poll(&state).await?;
    Ok(strip_account_tokens(serde_json::to_value(&acc).unwrap_or_default()))
}

/// Remove sensitive token fields before sending an Account to the frontend.
fn strip_account_tokens(mut v: Value) -> Value {
    if let Some(obj) = v.as_object_mut() {
        obj.remove("msa_refresh_token");
        obj.remove("msa_access_token");
    }
    v
}

#[tauri::command]
pub fn logout_account(state: State<AppState>, uuid: String) -> Result<(), String> {
    accounts::remove_account(&state, &uuid)
}

// ---------------------------------------------------------------------------
// Browse & install content
// ---------------------------------------------------------------------------

/// Replace `title` with Chinese name from WikiEntries where available.
fn apply_chinese_names(hits: &mut [Value]) {
    for h in hits.iter_mut() {
        let slug = h.get("slug").and_then(|v| v.as_str()).unwrap_or("");
        let provider = h.get("provider").and_then(|v| v.as_str()).unwrap_or("");
        if !slug.is_empty() {
            if let Some(name) = crate::mcmod::lookup_chinese_name(slug, provider) {
                if let Some(obj) = h.as_object_mut() {
                    obj.insert("title".to_string(), Value::String(name));
                }
            }
        }
    }
}

#[tauri::command]
pub async fn browse(
    state: State<'_, AppState>,
    provider: String,
    query: String,
    project_type: String,
    category: String,
    page: u32,
    game_version: String,
    loader: String,
    sort: String,
    page_size: u32,
) -> Result<Value, String> {
    let ps = (page_size.max(1)) as usize;
    match provider.as_str() {
        "modrinth" => {
            let mut result = modrinth::search(
                &state,
                &query,
                &project_type,
                &category,
                &sort,
                (page as usize) * ps,
                ps,
                &game_version,
                &loader,
            )
            .await?;
            if let Some(hits) = result.get_mut("hits").and_then(|v| v.as_array_mut()) {
                apply_chinese_names(hits);
            }
            Ok(result)
        }
        "curseforge" => {
            let cat = category.parse::<u32>().unwrap_or(0);
            let mut result = curseforge::search(&state, &query, &project_type, cat, page as usize, ps, &game_version, &loader, &sort).await?;
            if let Some(hits) = result.get_mut("hits").and_then(|v| v.as_array_mut()) {
                apply_chinese_names(hits);
            }
            Ok(result)
        }
        // "全部来源"：各平台独立分页，各取当前页 ps 条，合并排序后取前 ps 条。
        // total 为两平台真实总数之和。分类只作用于 Modrinth。
        "all" => {
            let offset = (page as usize) * ps;
            let m = modrinth::search(
                &state,
                &query,
                &project_type,
                &category,
                &sort,
                offset,
                ps,
                &game_version,
                &loader,
            )
            .await?;
            let mut hits = m
                .get("hits")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let mr_total = m.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
            let mut cf_total = 0u64;
            let mut cf_error: Option<String> = None;
            let mut cf_count = 0u64;
            match curseforge::search(
                &state,
                &query,
                &project_type,
                0,
                page as usize,
                ps,
                &game_version,
                &loader,
                &sort,
            )
            .await
            {
                Ok(c) => {
                    cf_total = c.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
                    if let Some(ch) = c.get("hits").and_then(|v| v.as_array()) {
                        cf_count = ch.len() as u64;
                        hits.extend(ch.iter().cloned());
                    }
                }
                Err(e) => {
                    if page == 0 {
                        cf_error = Some(e);
                    }
                }
            }
            let total = mr_total.max(cf_total);
            // 合并后按所选排序维度统一排序（relevance 无可比性，保持平台各自顺序）
            match sort.as_str() {
                "follows" => hits.sort_by(|a, b| {
                    let fa = a.get("follows").and_then(|v| v.as_u64()).unwrap_or(0);
                    let fb = b.get("follows").and_then(|v| v.as_u64()).unwrap_or(0);
                    fb.cmp(&fa)
                }),
                "newest" | "updated" => hits.sort_by(|a, b| {
                    let ta = a.get("_sort_ts").and_then(|v| v.as_str()).unwrap_or("");
                    let tb = b.get("_sort_ts").and_then(|v| v.as_str()).unwrap_or("");
                    tb.cmp(ta)
                }),
                _ => hits.sort_by(|a, b| {
                    let da = a.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0);
                    let db = b.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0);
                    db.cmp(&da)
                }),
            }
            if hits.len() > ps {
                hits.truncate(ps);
            }
            apply_chinese_names(&mut hits);
            Ok(json!({ "hits": hits, "total": total, "cf_error": cf_error, "cf_count": cf_count }))
        }
        _ => Err("未知内容源".into()),
    }
}

#[tauri::command]
pub async fn project_versions(
    state: State<'_, AppState>,
    provider: String,
    project_id: String,
    mc_version: String,
    loader: String,
) -> Result<Value, String> {
    match provider.as_str() {
        "modrinth" => {
            let list = modrinth::versions(&state, &project_id, &mc_version, &loader).await?;
            Ok(json!({ "provider": "modrinth", "versions": list }))
        }
        "curseforge" => {
            let list = curseforge::files(&state, &project_id, &mc_version).await?;
            Ok(json!({ "provider": "curseforge", "versions": list }))
        }
        _ => Err("未知内容源".into()),
    }
}

#[tauri::command]
pub async fn curseforge_categories(
    state: State<'_, AppState>,
    project_type: String,
) -> Result<Value, String> {
    let list = curseforge::categories(&state, &project_type).await?;
    Ok(json!({ "categories": list }))
}

/// Fetch a single project's full info by id.
#[tauri::command]
pub async fn project_info(
    state: State<'_, AppState>,
    provider: String,
    project_id: String,
) -> Result<Value, String> {
    match provider.as_str() {
        "modrinth" => modrinth::project_info(&state, &project_id).await,
        "curseforge" => curseforge::project_info(&state, &project_id).await,
        _ => Err("未知内容源".into()),
    }
}

/// Required/optional dependency projects of a project version.
#[tauri::command]
pub async fn project_dependencies(
    state: State<'_, AppState>,
    provider: String,
    project_id: String,
    version_id: String,
) -> Result<Vec<Value>, String> {
    match provider.as_str() {
        "modrinth" => modrinth::dependencies(&state, &version_id).await,
        "curseforge" => curseforge::dependencies(&state, &project_id, &version_id).await,
        _ => Ok(vec![]),
    }
}

/// Resolve a direct MC wiki (mcmod.cn) mod page URL.
/// Tries local slug→wiki_id mapping first, falls back to search page extraction.
#[tauri::command]
pub async fn mc_wiki_url(
    state: State<'_, AppState>,
    name: String,
    slug: Option<String>,
    provider: Option<String>,
) -> Result<String, String> {
    // 1. Try local WikiEntries mapping by slug
    if let (Some(s), Some(p)) = (&slug, &provider) {
        if let Some(id) = crate::mcmod::lookup_wiki_id(s, p) {
            return Ok(format!("https://www.mcmod.cn/class/{id}.html"));
        }
    }
    // 2. Fallback: fetch search page and extract first class/{id}.html
    let search_url = format!("https://search.mcmod.cn/s?key={}", modrinth::urlencode(&name));
    let resp = state
        .client
        .get(&search_url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .header("Accept", "text/html,application/xhtml+xml")
        .header("Accept-Language", "zh-CN,zh;q=0.9")
        .send()
        .await
        .map_err(|e| format!("请求 MC 百科失败: {e}"))?;
    let html = resp
        .text()
        .await
        .map_err(|e| format!("读取 MC 百科响应失败: {e}"))?;
    let needle = "class/";
    let mut pos = 0;
    while let Some(idx) = html[pos..].find(needle) {
        let start = pos + idx + needle.len();
        let rest = &html[start..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() && rest[digits.len()..].starts_with(".html") {
            return Ok(format!("https://www.mcmod.cn/class/{digits}.html"));
        }
        pos = start;
    }
    Ok(search_url)
}

/// Install a project version into an instance.
/// kind: mod | modpack | resourcepack | shader
#[tauri::command]
pub async fn install_content(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    provider: String,
    project_id: String,
    version_id: String,
    kind: String,
) -> Result<Value, String> {
    // modpack creates its own new instance; no existing instance needed
    if kind == "modpack" {
        return match provider.as_str() {
            "modrinth" => {
                let ver = modrinth::version(&state, &version_id).await?;
                modrinth::install_modpack(app, &state, &ver).await
            }
            "curseforge" => curseforge::install_modpack(app, &state, &project_id, &version_id).await,
            _ => Err("未知内容源".into()),
        };
    }
    let instance = crate::instances::get_instance(&state, &instance_id)?;
    match provider.as_str() {
        "modrinth" => modrinth::install_version(app, &state, &instance, &version_id, &kind).await,
        "curseforge" => {
            curseforge::install_file(app, &state, &instance, &project_id, &version_id, &kind).await
        }
        _ => Err("未知内容源".into()),
    }
}

#[tauri::command]
pub async fn check_updates(
    state: State<'_, AppState>,
    instance_id: String,
    kind: String,
) -> Result<Vec<Value>, String> {
    let instance = crate::instances::get_instance(&state, &instance_id)?;
    let mut updates = modrinth::check_updates(&state, &instance, &kind).await?;
    let cf = crate::curseforge::check_updates(&state, &instance, &kind).await?;
    updates.extend(cf);
    Ok(updates)
}

#[tauri::command]
pub async fn apply_update(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    kind: String,
    old_filename: String,
    provider: String,
    project_id: String,
    new_version_id: String,
) -> Result<Value, String> {
    let _ = &state; // state is re-acquired inside the spawned task via app.state()
    let app2 = app.clone();
    let instance_id2 = instance_id.clone();
    let kind2 = kind.clone();
    let old_filename2 = old_filename.clone();
    let provider2 = provider.clone();
    let project_id2 = project_id.clone();
    let new_version_id2 = new_version_id.clone();
    // Run the download + install in the background so the UI can show
    // "已加入下载队列" immediately instead of blocking on the download.
    tauri::async_runtime::spawn(async move {
        let res: Result<Value, String> = async {
            let state2 = app2.state::<crate::state::AppState>();
            let instance = crate::instances::get_instance(&state2, &instance_id2)?;
            // install new version first (adds a new record)
            let result = match provider2.as_str() {
                "modrinth" => {
                    modrinth::install_version(app2.clone(), &state2, &instance, &new_version_id2, &kind2).await?
                }
                "curseforge" => {
                    curseforge::install_file(app2.clone(), &state2, &instance, &project_id2, &new_version_id2, &kind2).await?
                }
                _ => return Err("未知内容源".into()),
            };
            // remove the old file + record only after the new one succeeded
            let _ = modrinth::uninstall(&state2, &instance, &kind2, &old_filename2);
            Ok(result)
        }
        .await;
        let payload = match &res {
            Ok(_) => serde_json::json!({ "filename": old_filename2, "ok": true }),
            Err(e) => serde_json::json!({ "filename": old_filename2, "ok": false, "error": e }),
        };
        let _ = app2.emit("content://update-finished", payload);
    });
    Ok(serde_json::json!({ "queued": true }))
}

#[tauri::command]
pub fn uninstall_content(
    state: State<AppState>,
    instance_id: String,
    kind: String,
    filename: String,
) -> Result<(), String> {
    let instance = crate::instances::get_instance(&state, &instance_id)?;
    modrinth::uninstall(&state, &instance, &kind, &filename)
}

#[tauri::command]
pub fn toggle_content_enabled(
    state: State<AppState>,
    instance_id: String,
    kind: String,
    filename: String,
    enabled: bool,
) -> Result<(), String> {
    let instance = crate::instances::get_instance(&state, &instance_id)?;
    crate::instances::set_content_enabled(&state, &instance.id, &kind, &filename, enabled)
}

/// Import a local file (jar/zip) into an instance folder as manual content.
#[tauri::command]
pub fn list_content(
    state: State<AppState>,
    instance_id: String,
    kind: String,
) -> Result<Value, String> {
    let instance = crate::instances::get_instance(&state, &instance_id)?;
    let folder = modrinth::kind_folder(&kind);
    let dir = state.instances_dir().join(&instance.id).join(folder);
    let mut records = crate::instances::list_content(&state, &instance_id, &kind);
    let mut on_disk: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if let Some(fname) = entry.file_name().to_str() {
                if entry.path().is_file() {
                    on_disk.push(fname.to_string());
                }
            }
        }
    }
    // auto-register mods that exist on disk but have no record
    let ext = if kind == "mod" { ".jar" } else { ".zip" };
    let mut new_records: Vec<InstalledContent> = Vec::new();
    for fname in &on_disk {
        if !fname.ends_with(ext) {
            continue;
        }
        if records.iter().any(|r: &InstalledContent| r.filename == *fname) {
            continue;
        }
        let size = std::fs::metadata(dir.join(fname)).map(|m| m.len()).unwrap_or(0);
        let mut rec = InstalledContent {
            filename: fname.clone(),
            source: "manual".into(),
            project_id: None,
            slug: None,
            version_id: None,
            name: None,
            version: None,
            mod_id: None,
            authors: None,
            description: None,
            installed_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            size,
            icon: None,
            enabled: true,
        };
        crate::util::fill_content_from_jar(&mut rec, &dir.join(fname));
        new_records.push(rec);
    }
    if !new_records.is_empty() {
        let _ = crate::instances::add_content_batch(&state, &instance_id, &kind, new_records.clone());
        records.extend(new_records);
    }
    let mut updated = false;
    for rec in &mut records {
        let abs = dir.join(&rec.filename);
        if abs.is_file() {
            let before = rec.clone();
            crate::util::fill_content_from_jar(rec, &abs);
            if rec.name != before.name || rec.description != before.description || rec.icon != before.icon {
                updated = true;
            }
        }
    }

    if updated {
        let _ = crate::instances::add_content_batch(&state, &instance_id, &kind, records.clone());
    }
    let items: Vec<Value> = records
        .iter()
        .map(|r| {
            let exists = dir.join(&r.filename).is_file()
                || dir.join(format!("{}.disabled", r.filename)).is_file();
            let cn = crate::mcmod::cn_name_for_record(&r.source, r.slug.as_deref(), r.name.as_deref());
            let mut rec_val = serde_json::to_value(r).unwrap_or(Value::Null);
            if let Some(obj) = rec_val.as_object_mut() {
                obj.insert("cn_name".to_string(), serde_json::to_value(&cn).unwrap_or(Value::Null));
            }
            json!({ "record": rec_val, "exists": exists })
        })
        .collect();
    Ok(json!({ "items": items, "onDisk": on_disk }))
}

/// Asynchronously identify unidentified content via Modrinth.
/// Tries hash lookup first, then falls back to name search.
/// Emits `content::identified` events per item as they are resolved.
#[tauri::command]
pub async fn identify_content(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    kind: String,
) -> Result<(), String> {
    let instance = crate::instances::get_instance(&state, &instance_id)?;
    let folder = modrinth::kind_folder(&kind);
    let dir = state.instances_dir().join(&instance.id).join(folder);
    let records = crate::instances::list_content(&state, &instance_id, &kind);
    let to_identify: Vec<(String, String)> = records.iter()
        .filter(|r| r.project_id.is_none() && dir.join(&r.filename).is_file())
        .filter_map(|r| {
            let path = dir.join(&r.filename);
            crate::util::file_sha1(&path).map(|h| (r.filename.clone(), h))
        })
        .collect();
    if to_identify.is_empty() {
        return Ok(());
    }

    let project_type = match kind.as_str() {
        "shader" => "shader",
        "resourcepack" => "resourcepack",
        _ => "mod",
    };

    // ---- pass 1: hash lookup ----
    let hash_strs: Vec<String> = to_identify.iter().map(|(_, h)| h.clone()).collect();
    let resolved = modrinth::resolve_by_hashes(&state, &hash_strs).await;
    let mut resolved_files: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for (filename, hash) in &to_identify {
        if let Some((pid, vid)) = resolved.get(hash) {
            if let Ok(info) = modrinth::project_info(&state, pid).await {
                let slug = info.get("slug").and_then(|v| v.as_str()).map(|s| s.to_string());
                let name = info.get("title").and_then(|v| v.as_str()).map(|s| s.to_string());
                let desc = info.get("description").and_then(|v| v.as_str()).map(|s| s.to_string());
                let icon = info.get("icon_url").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string());
                let authors = modrinth::project_authors(&state, pid).await;
                let mut rec = crate::instances::list_content(&state, &instance_id, &kind)
                    .into_iter()
                    .find(|r| r.filename == *filename);
                if let Some(ref mut rec) = rec {
                    rec.source = "modrinth".into();
                    rec.project_id = Some(pid.clone());
                    rec.version_id = Some(vid.clone());
                    rec.slug = slug.clone();
                    if let Some(n) = &name { rec.name = Some(n.clone()); }
                    if let Some(d) = &desc { rec.description = Some(d.clone()); }
                    if let Some(ic) = &icon { rec.icon = Some(ic.clone()); }
                    if !authors.is_empty() { rec.authors = Some(authors); }
                    let _ = crate::instances::add_content_batch(&state, &instance_id, &kind, vec![rec.clone()]);
                }
                let _ = app.emit("content::identified", json!({
                    "instanceId": instance_id, "kind": kind, "filename": filename,
                    "source": "modrinth", "projectId": pid, "versionId": vid,
                    "slug": slug, "name": name, "description": desc, "icon": icon,
                    "authors": rec.as_ref().and_then(|r| r.authors.clone()),
                }));
                resolved_files.insert(filename.as_str());
            }
        }
    }

    // ---- pass 2: name search fallback for unresolved files ----
    for (filename, _) in &to_identify {
        if resolved_files.contains(filename.as_str()) { continue; }
        let query = extract_search_query(filename);
        if query.is_empty() { continue; }
        let search_result = modrinth::search(&state, &query, project_type, "", "relevance", 0, 1, "", "").await;
        if let Ok(sr) = search_result {
            if let Some(hit) = sr.get("hits").and_then(|h| h.as_array()).and_then(|a| a.first()) {
                let pid = hit.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let slug = hit.get("slug").and_then(|v| v.as_str()).map(|s| s.to_string());
                let name = hit.get("title").and_then(|v| v.as_str()).map(|s| s.to_string());
                let desc = hit.get("description").and_then(|v| v.as_str()).map(|s| s.to_string());
                let icon = hit.get("icon_url").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(|s| s.to_string());
                let author = hit.get("author").and_then(|v| v.as_str()).map(|s| vec![s.to_string()]);
                if pid.is_empty() { continue; }
                let mut rec = crate::instances::list_content(&state, &instance_id, &kind)
                    .into_iter()
                    .find(|r| r.filename == *filename);
                if let Some(ref mut rec) = rec {
                    rec.source = "modrinth".into();
                    rec.project_id = Some(pid.clone());
                    rec.slug = slug.clone();
                    if let Some(n) = &name { rec.name = Some(n.clone()); }
                    if let Some(d) = &desc { rec.description = Some(d.clone()); }
                    if let Some(ic) = &icon { rec.icon = Some(ic.clone()); }
                    if let Some(a) = &author { rec.authors = Some(a.clone()); }
                    let _ = crate::instances::add_content_batch(&state, &instance_id, &kind, vec![rec.clone()]);
                }
                let _ = app.emit("content::identified", json!({
                    "instanceId": instance_id, "kind": kind, "filename": filename,
                    "source": "modrinth", "projectId": pid, "versionId": null,
                    "slug": slug, "name": name, "description": desc, "icon": icon,
                    "authors": author,
                }));
            }
        }
    }
    Ok(())
}

/// Extract a search query from a filename (strip extension + version suffixes).
fn extract_search_query(filename: &str) -> String {
    let stem = filename.trim_end_matches(".zip").trim_end_matches(".jar");
    let parts: Vec<&str> = stem.split(|c: char| c == '_' || c == '-' || c == '+').collect();
    let mut keep: Vec<&str> = Vec::new();
    for p in &parts {
        if p.is_empty() { continue; }
        if p.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) { break; }
        if p.len() <= 2 && p.chars().all(|c| c.is_ascii_digit() || c == '.') { break; }
        keep.push(p);
    }
    keep.join(" ")
}

/// Import a local file (jar/zip) into an instance folder as manual content.
#[tauri::command]
pub fn import_local_file(
    state: State<AppState>,
    instance_id: String,
    kind: String,
    source_path: String,
) -> Result<Value, String> {
    let instance = crate::instances::get_instance(&state, &instance_id)?;
    let source = std::path::Path::new(&source_path);
    let filename = source
        .file_name()
        .ok_or("无效的文件路径")?
        .to_string_lossy()
        .to_string();
    let dest = state
        .instances_dir()
        .join(&instance.id)
        .join(modrinth::kind_folder(&kind))
        .join(&filename);
    std::fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::copy(source, &dest).map_err(|e| format!("复制文件失败: {e}"))?;
    let size = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
    let icon = crate::util::extract_archive_icon(&dest, &kind);
    let mut record = InstalledContent {
        filename,
        source: "manual".into(),
        project_id: None,
        slug: None,
        version_id: None,
        name: None,
        version: None,
        mod_id: None,
        authors: None,
        description: None,
        installed_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        size,
        icon,
        enabled: true,
    };
    crate::util::fill_content_from_jar(&mut record, &dest);
    crate::instances::add_content(&state, &instance_id, &kind, record)?;
    Ok(json!({ "ok": true }))
}

/// Extract a curated set of Minecraft textures from the installed game's
/// client.jar to use as instance icons. Returns the list of extracted icons.
#[derive(serde::Serialize, Clone)]
pub struct GameIcon {
    pub name: String,
    pub label: String,
    pub path: String,
}

const GAME_TEXTURES: &[(&str, &str, &[&str])] = &[
    ("diamond", "钻石", &["assets/minecraft/textures/item/diamond.png", "assets/minecraft/textures/items/diamond.png"]),
    ("emerald", "绿宝石", &["assets/minecraft/textures/item/emerald.png", "assets/minecraft/textures/items/emerald.png"]),
    ("iron_ingot", "铁锭", &["assets/minecraft/textures/item/iron_ingot.png", "assets/minecraft/textures/items/iron_ingot.png"]),
    ("gold_ingot", "金锭", &["assets/minecraft/textures/item/gold_ingot.png", "assets/minecraft/textures/items/gold_ingot.png"]),
    ("netherite_ingot", "下界合金锭", &["assets/minecraft/textures/item/netherite_ingot.png", "assets/minecraft/textures/items/netherite_ingot.png"]),
    ("iron_sword", "铁剑", &["assets/minecraft/textures/item/iron_sword.png", "assets/minecraft/textures/items/iron_sword.png"]),
    ("diamond_sword", "钻石剑", &["assets/minecraft/textures/item/diamond_sword.png", "assets/minecraft/textures/items/diamond_sword.png"]),
    ("netherite_sword", "下界合金剑", &["assets/minecraft/textures/item/netherite_sword.png", "assets/minecraft/textures/items/netherite_sword.png"]),
    ("bow", "弓", &["assets/minecraft/textures/item/bow.png", "assets/minecraft/textures/items/bow.png"]),
    ("shield", "盾牌", &["assets/minecraft/textures/item/shield.png", "assets/minecraft/textures/items/shield.png"]),
    ("apple", "苹果", &["assets/minecraft/textures/item/apple.png", "assets/minecraft/textures/items/apple.png"]),
    ("golden_apple", "金苹果", &["assets/minecraft/textures/item/golden_apple.png", "assets/minecraft/textures/items/golden_apple.png"]),
    ("clock", "时钟", &["assets/minecraft/textures/item/clock_0.png", "assets/minecraft/textures/items/clock_0.png"]),
    ("compass", "指南针", &["assets/minecraft/textures/item/compass_0.png", "assets/minecraft/textures/items/compass_0.png"]),
    ("map", "地图", &["assets/minecraft/textures/item/map.png", "assets/minecraft/textures/items/map.png"]),
    ("bucket", "桶", &["assets/minecraft/textures/item/bucket.png", "assets/minecraft/textures/items/bucket.png"]),
    ("fishing_rod", "钓鱼竿", &["assets/minecraft/textures/item/fishing_rod.png", "assets/minecraft/textures/items/fishing_rod.png"]),
    ("shears", "剪刀", &["assets/minecraft/textures/item/shears.png", "assets/minecraft/textures/items/shears.png"]),
    ("flint_and_steel", "打火石", &["assets/minecraft/textures/item/flint_and_steel.png", "assets/minecraft/textures/items/flint_and_steel.png"]),
    ("ender_eye", "末影之眼", &["assets/minecraft/textures/item/ender_eye.png", "assets/minecraft/textures/items/ender_eye.png"]),
    ("ender_pearl", "末影珍珠", &["assets/minecraft/textures/item/ender_pearl.png", "assets/minecraft/textures/items/ender_pearl.png"]),
    ("firework_rocket", "烟花火箭", &["assets/minecraft/textures/item/firework_rocket.png", "assets/minecraft/textures/items/firework_rocket.png"]),
    ("book", "书", &["assets/minecraft/textures/item/book.png", "assets/minecraft/textures/items/book.png"]),
    ("enchanted_book", "附魔书", &["assets/minecraft/textures/item/enchanted_book.png", "assets/minecraft/textures/items/enchanted_book.png"]),
    ("totem_of_undying", "不死图腾", &["assets/minecraft/textures/item/totem_of_undying.png", "assets/minecraft/textures/items/totem_of_undying.png"]),
    ("nether_star", "下界之星", &["assets/minecraft/textures/item/nether_star.png", "assets/minecraft/textures/items/nether_star.png"]),
    ("blaze_rod", "烈焰棒", &["assets/minecraft/textures/item/blaze_rod.png", "assets/minecraft/textures/items/blaze_rod.png"]),
    ("experience_bottle", "经验瓶", &["assets/minecraft/textures/item/experience_bottle.png", "assets/minecraft/textures/items/experience_bottle.png"]),
    ("grass_block", "草方块", &["assets/minecraft/textures/block/grass_block_top.png", "assets/minecraft/textures/blocks/grass_top.png"]),
    ("stone", "石头", &["assets/minecraft/textures/block/stone.png", "assets/minecraft/textures/blocks/stone.png"]),
    ("diamond_block", "钻石块", &["assets/minecraft/textures/block/diamond_block.png", "assets/minecraft/textures/blocks/diamond_block.png"]),
    ("gold_block", "金块", &["assets/minecraft/textures/block/gold_block.png", "assets/minecraft/textures/blocks/gold_block.png"]),
    ("iron_block", "铁块", &["assets/minecraft/textures/block/iron_block.png", "assets/minecraft/textures/blocks/iron_block.png"]),
    ("emerald_block", "绿宝石块", &["assets/minecraft/textures/block/emerald_block.png", "assets/minecraft/textures/blocks/emerald_block.png"]),
    ("netherite_block", "下界合金块", &["assets/minecraft/textures/block/netherite_block.png", "assets/minecraft/textures/blocks/netherite_block.png"]),
    ("crafting_table", "工作台", &["assets/minecraft/textures/block/crafting_table_front.png", "assets/minecraft/textures/blocks/crafting_table_front.png"]),
    ("furnace", "熔炉", &["assets/minecraft/textures/block/furnace_front.png", "assets/minecraft/textures/blocks/furnace_front.png"]),
    ("beacon", "信标", &["assets/minecraft/textures/block/beacon.png", "assets/minecraft/textures/blocks/beacon.png"]),
    ("enchanting_table", "附魔台", &["assets/minecraft/textures/block/enchanting_table_front.png", "assets/minecraft/textures/blocks/enchanting_table_front.png"]),
    ("anvil", "铁砧", &["assets/minecraft/textures/block/anvil.png", "assets/minecraft/textures/blocks/anvil.png"]),
    ("tnt", "TNT", &["assets/minecraft/textures/block/tnt.png", "assets/minecraft/textures/blocks/tnt.png"]),
    ("obsidian", "黑曜石", &["assets/minecraft/textures/block/obsidian.png", "assets/minecraft/textures/blocks/obsidian.png"]),
    ("glowstone", "荧石", &["assets/minecraft/textures/block/glowstone.png", "assets/minecraft/textures/blocks/glowstone.png"]),
    ("bookshelf", "书架", &["assets/minecraft/textures/block/bookshelf.png", "assets/minecraft/textures/blocks/bookshelf.png"]),
    ("pumpkin", "南瓜", &["assets/minecraft/textures/block/pumpkin_front.png", "assets/minecraft/textures/blocks/pumpkin_front.png"]),
    ("cake", "蛋糕", &["assets/minecraft/textures/block/cake_top.png", "assets/minecraft/textures/blocks/cake_top.png"]),
    ("sponge", "海绵", &["assets/minecraft/textures/block/sponge.png", "assets/minecraft/textures/blocks/sponge.png"]),
    ("ice", "冰", &["assets/minecraft/textures/block/ice.png", "assets/minecraft/textures/blocks/ice.png"]),
    ("netherrack", "下界岩", &["assets/minecraft/textures/block/netherrack.png", "assets/minecraft/textures/blocks/netherrack.png"]),
    ("redstone", "红石粉", &["assets/minecraft/textures/item/redstone.png", "assets/minecraft/textures/items/redstone.png"]),
    ("gunpowder", "火药", &["assets/minecraft/textures/item/gunpowder.png", "assets/minecraft/textures/items/gunpowder.png"]),
    ("slime_ball", "粘液球", &["assets/minecraft/textures/item/slime_ball.png", "assets/minecraft/textures/items/slime_ball.png"]),
    ("bone", "骨头", &["assets/minecraft/textures/item/bone.png", "assets/minecraft/textures/items/bone.png"]),
    ("stick", "木棍", &["assets/minecraft/textures/item/stick.png", "assets/minecraft/textures/items/stick.png"]),
    ("coal", "煤炭", &["assets/minecraft/textures/item/coal.png", "assets/minecraft/textures/items/coal.png"]),
    ("wheat", "小麦", &["assets/minecraft/textures/item/wheat.png", "assets/minecraft/textures/items/wheat.png"]),
    ("carrot", "胡萝卜", &["assets/minecraft/textures/item/carrot.png", "assets/minecraft/textures/items/carrot.png"]),
    ("potato", "马铃薯", &["assets/minecraft/textures/item/potato.png", "assets/minecraft/textures/items/potato.png"]),
];

#[tauri::command]
pub async fn extract_game_icons(
    state: State<'_, AppState>,
    instance_id: Option<String>,
) -> Result<Vec<GameIcon>, String> {
    use std::io::Read;
    let versions_dir = state.versions_dir();
    let mut client_jar: Option<std::path::PathBuf> = None;

    if let Some(id) = &instance_id {
        let jar = versions_dir.join(id).join(format!("{}.jar", id));
        if jar.exists() {
            client_jar = Some(jar);
        }
        if client_jar.is_none() {
            if let Ok(inst) = crate::instances::get_instance(&state, id) {
                let jar = versions_dir.join(&inst.mc_version).join(format!("{}.jar", inst.mc_version));
                if jar.exists() {
                    client_jar = Some(jar);
                }
            }
        }
    }
    if client_jar.is_none() {
        if let Ok(entries) = std::fs::read_dir(&versions_dir) {
            for e in entries.flatten() {
                let dir = e.path();
                if !dir.is_dir() { continue; }
                let name = e.file_name().to_string_lossy().to_string();
                let jar = dir.join(format!("{}.jar", name));
                if jar.exists() {
                    client_jar = Some(jar);
                    break;
                }
            }
        }
    }
    let jar_path = client_jar.ok_or("未找到已安装的游戏版本，请先安装游戏后再设置图标")?;

    let icons_dir = state.root.join("game-icons");
    std::fs::create_dir_all(&icons_dir).map_err(|e| format!("创建图标目录失败: {e}"))?;

    let file = std::fs::File::open(&jar_path).map_err(|e| format!("打开 client.jar 失败: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("解析 client.jar 失败: {e}"))?;

    let mut icons = Vec::new();
    for (key, label, candidates) in GAME_TEXTURES {
        for cand in *candidates {
            if let Ok(mut entry) = archive.by_name(cand) {
                if entry.is_dir() { continue; }
                let mut buf = Vec::new();
                if Read::read_to_end(&mut entry, &mut buf).is_err() { continue; }
                if buf.len() < 8 { continue; }
                let out_path = icons_dir.join(format!("{}.png", key));
                if std::fs::write(&out_path, &buf).is_err() { continue; }
                icons.push(GameIcon {
                    name: key.to_string(),
                    label: label.to_string(),
                    path: out_path.to_string_lossy().to_string(),
                });
                break;
            }
        }
    }
    if icons.is_empty() {
        return Err("未能从游戏文件中提取任何图标素材".into());
    }
    Ok(icons)
}

/// Write text content to a file (used for log export via the save dialog).
#[tauri::command]
pub fn save_text_file(path: String, content: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    std::fs::write(p, content).map_err(|e| format!("写入文件失败: {e}"))
}

// ---------------------------------------------------------------------------
// Skins
// ---------------------------------------------------------------------------

#[derive(serde::Serialize, Clone)]
pub struct SkinEntry {
    pub name: String,
    pub filename: String,
    pub path: String,
    pub size: u64,
    pub modified: u64,
}

/// List all `.png` skins in the `skins` directory of the data root.
#[tauri::command]
pub fn list_skins(state: State<AppState>) -> Result<Vec<SkinEntry>, String> {
    let dir = state.root.join("skins");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建皮肤目录失败: {e}"))?;
    let mut out = Vec::new();
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("读取皮肤目录失败: {e}"))?;
    for e in entries.flatten() {
        let path = e.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("png")) != Some(true) {
            continue;
        }
        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string();
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        out.push(SkinEntry {
            name,
            filename,
            path: path.to_string_lossy().to_string(),
            size: meta.len(),
            modified,
        });
    }
    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    Ok(out)
}

/// Read a skin file (by filename in the skins dir, or absolute path from a
/// native file-picker) as a data URL.
#[tauri::command]
pub fn read_skin_data_url(state: State<AppState>, filename: String) -> Result<String, String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let is_abs = filename.contains('/') || filename.contains('\\');
    let path = if is_abs {
        std::path::PathBuf::from(&filename)
    } else {
        if !crate::util::is_safe_filename(&filename) {
            return Err("非法文件名".into());
        }
        state.root.join("skins").join(&filename)
    };
    if !path.is_file() {
        return Err("皮肤文件不存在".into());
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("读取皮肤文件失败: {e}"))?;
    Ok(format!("data:image/png;base64,{}", STANDARD.encode(&bytes)))
}

/// Save a skin PNG (base64 without data: prefix or with it) to the skins directory.
#[tauri::command]
pub fn save_skin_from_data(state: State<AppState>, name: String, data: String) -> Result<SkinEntry, String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let raw = data.trim();
    let b64 = raw.strip_prefix("data:image/png;base64,").unwrap_or(raw);
    let bytes = STANDARD.decode(b64).map_err(|e| format!("解析皮肤数据失败: {e}"))?;
    if bytes.len() < 8 || &bytes[0..8] != b"\x89PNG\r\n\x1a\n" {
        return Err("文件不是有效的 PNG".into());
    }
    let safe_name: String = name
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => c,
            _ => '_',
        })
        .collect();
    if safe_name.is_empty() {
        return Err("皮肤名称不能为空".into());
    }
    let dir = state.root.join("skins");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建皮肤目录失败: {e}"))?;
    let path = dir.join(format!("{}.png", safe_name));
    std::fs::write(&path, &bytes).map_err(|e| format!("写入皮肤文件失败: {e}"))?;
    let meta = std::fs::metadata(&path).map_err(|e| format!("读取皮肤元信息失败: {e}"))?;
    Ok(SkinEntry {
        name: safe_name,
        filename: path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string(),
        path: path.to_string_lossy().to_string(),
        size: meta.len(),
        modified: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    })
}

/// Download a skin PNG from a URL and save it to the skins directory.
#[tauri::command]
pub async fn download_skin_from_url(
    state: State<'_, AppState>,
    name: String,
    url: String,
) -> Result<SkinEntry, String> {
    let resp = state
        .client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("下载皮肤失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("下载皮肤失败: HTTP {}", resp.status()));
    }
    let bytes = resp.bytes().await.map_err(|e| format!("读取皮肤数据失败: {e}"))?;
    if bytes.len() < 8 || &bytes[0..8] != b"\x89PNG\r\n\x1a\n" {
        return Err("下载的内容不是有效的 PNG".into());
    }
    let safe_name: String = name
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => c,
            _ => '_',
        })
        .collect();
    if safe_name.is_empty() {
        return Err("皮肤名称不能为空".into());
    }
    let dir = state.root.join("skins");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建皮肤目录失败: {e}"))?;
    let path = dir.join(format!("{}.png", safe_name));
    std::fs::write(&path, &bytes).map_err(|e| format!("写入皮肤文件失败: {e}"))?;
    let meta = std::fs::metadata(&path).map_err(|e| format!("读取皮肤元信息失败: {e}"))?;
    Ok(SkinEntry {
        name: safe_name,
        filename: path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_string(),
        path: path.to_string_lossy().to_string(),
        size: meta.len(),
        modified: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    })
}

/// Delete a skin file by filename in the skins directory.
#[tauri::command]
pub fn delete_skin(state: State<AppState>, filename: String) -> Result<(), String> {
    if !crate::util::is_safe_filename(&filename) {
        return Err("非法文件名".into());
    }
    let path = state.root.join("skins").join(&filename);
    if !path.exists() {
        return Err("皮肤文件不存在".into());
    }
    std::fs::remove_file(&path).map_err(|e| format!("删除皮肤失败: {e}"))
}

/// Fetch a player's skin by Minecraft username via Mojang API.
/// Returns the skin PNG as a data URL plus model type ("classic" or "slim").
#[derive(serde::Serialize)]
pub struct PlayerSkinResult {
    pub data_url: String,
    pub model: String,
    pub cape_data_url: Option<String>,
}

#[tauri::command]
pub async fn fetch_image_data_url(state: State<'_, AppState>, url: String) -> Result<String, String> {
    let resp = state
        .client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("请求失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("读取失败: {e}"))?;
    use base64::{engine::general_purpose::STANDARD, Engine};
    let b64 = STANDARD.encode(&bytes);
    Ok(format!("data:image/png;base64,{}", b64))
}

#[tauri::command]
pub async fn fetch_player_skin(state: State<'_, AppState>, username: String) -> Result<PlayerSkinResult, String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let trimmed = username.trim();
    if trimmed.is_empty() {
        return Err("玩家名不能为空".into());
    }
    let profile_url = format!("https://api.mojang.com/users/profiles/minecraft/{}", trimmed);
    let profile: serde_json::Value = state
        .client
        .get(&profile_url)
        .send()
        .await
        .map_err(|e| format!("查询玩家失败: {e}"))?
        .json()
        .await
        .map_err(|e| format!("解析玩家信息失败: {e}"))?;
    let uuid = profile
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("未找到该玩家（可能不存在或为离线账号）")?
        .to_string();
    let session_url = format!("https://sessionserver.mojang.com/session/minecraft/profile/{}", uuid);
    let session: serde_json::Value = state
        .client
        .get(&session_url)
        .send()
        .await
        .map_err(|e| format!("获取会话信息失败: {e}"))?
        .json()
        .await
        .map_err(|e| format!("解析会话信息失败: {e}"))?;
    let props = session.get("properties").and_then(|v| v.as_array()).ok_or("玩家无皮肤信息")?;
    let mut skin_url: Option<String> = None;
    let mut skin_model = "classic".to_string();
    let mut cape_url: Option<String> = None;
    for p in props {
        if p.get("name").and_then(|v| v.as_str()) == Some("textures") {
            let value = p.get("value").and_then(|v| v.as_str()).unwrap_or("");
            let decoded = STANDARD.decode(value).map_err(|e| format!("解码 textures 失败: {e}"))?;
            let json_str = String::from_utf8(decoded).map_err(|e| format!("textures 不是 UTF-8: {e}"))?;
            let tex: serde_json::Value = serde_json::from_str(&json_str).map_err(|e| format!("解析 textures JSON 失败: {e}"))?;
            if let Some(url) = tex
                .pointer("/textures/SKIN/url")
                .and_then(|v| v.as_str())
            {
                skin_url = Some(url.to_string());
            }
            if let Some(model) = tex
                .pointer("/textures/SKIN/metadata/model")
                .and_then(|v| v.as_str())
            {
                skin_model = model.to_string();
            }
            if let Some(url) = tex
                .pointer("/textures/CAPE/url")
                .and_then(|v| v.as_str())
            {
                cape_url = Some(url.to_string());
            }
        }
    }
    let skin_url = skin_url.ok_or("该玩家未设置自定义皮肤")?;
    let bytes = state
        .client
        .get(&skin_url)
        .send()
        .await
        .map_err(|e| format!("下载皮肤图片失败: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("读取皮肤图片失败: {e}"))?;

    let cape_data_url = if let Some(cu) = cape_url {
        match state.client.get(&cu).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.bytes().await {
                    Ok(cb) if cb.len() >= 8 && &cb[0..8] == b"\x89PNG\r\n\x1a\n" => {
                        Some(format!("data:image/png;base64,{}", STANDARD.encode(&cb)))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    } else {
        None
    };

    Ok(PlayerSkinResult {
        data_url: format!("data:image/png;base64,{}", STANDARD.encode(&bytes)),
        model: skin_model,
        cape_data_url,
    })
}

/// Fetch all capes owned by a Microsoft account via Mojang API.
#[derive(serde::Serialize)]
pub struct CapeInfo {
    pub id: String,
    pub name: String,
    pub data_url: String,
    pub active: bool,
}

#[tauri::command]
pub async fn fetch_player_capes(
    state: State<'_, AppState>,
    account_uuid: String,
) -> Result<Vec<CapeInfo>, String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let accounts = accounts::load_accounts(&state);
    let account = accounts
        .iter()
        .find(|a| a.uuid() == &account_uuid)
        .ok_or("账号不存在")?
        .clone();
    if !account.is_microsoft() {
        return Ok(Vec::new());
    }
    let account = accounts::refresh_microsoft(&state, &account).await?;
    let mc_token = match &account {
        Account::Microsoft {
            msa_access_token, ..
        } => msa_access_token.clone(),
        _ => return Err("账号类型错误".into()),
    };
    let resp = state
        .client
        .get("https://api.minecraftservices.com/minecraft/profile")
        .bearer_auth(&mc_token)
        .send()
        .await
        .map_err(|e| format!("获取披风列表失败: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("获取披风列表失败 (HTTP {status}): {body}"));
    }
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析披风列表失败: {e}"))?;
    let raw_capes = json
        .get("capes")
        .and_then(|v| v.as_array())
        .ok_or("披风列表格式异常")?;
    let mut result = Vec::new();
    for c in raw_capes {
        let id = c.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let name = c
            .get("alias")
            .and_then(|v| v.as_str())
            .unwrap_or("未命名披风")
            .to_string();
        let active = c
            .get("state")
            .and_then(|v| v.as_str())
            .map(|s| s.eq_ignore_ascii_case("ACTIVE"))
            .unwrap_or(false);
        let url = match c.get("url").and_then(|v| v.as_str()) {
            Some(u) => u.to_string(),
            None => continue,
        };
        let bytes = match state.client.get(&url).send().await {
            Ok(r) if r.status().is_success() => match r.bytes().await {
                Ok(b) if b.len() >= 8 && &b[0..8] == b"\x89PNG\r\n\x1a\n" => b,
                _ => continue,
            },
            _ => continue,
        };
        result.push(CapeInfo {
            id,
            name,
            active,
            data_url: format!("data:image/png;base64,{}", STANDARD.encode(&bytes)),
        });
    }
    Ok(result)
}

/// Apply a cape to a Microsoft account. `cape_id` = None hides the cape.
#[tauri::command]
pub async fn apply_cape_to_account(
    state: State<'_, AppState>,
    account_uuid: String,
    cape_id: Option<String>,
) -> Result<(), String> {
    let accounts = accounts::load_accounts(&state);
    let account = accounts
        .iter()
        .find(|a| a.uuid() == &account_uuid)
        .ok_or("账号不存在")?
        .clone();
    if !account.is_microsoft() {
        return Err("离线账号无法应用披风".into());
    }
    let account = accounts::refresh_microsoft(&state, &account).await?;
    let mc_token = match &account {
        Account::Microsoft {
            msa_access_token, ..
        } => msa_access_token.clone(),
        _ => return Err("账号类型错误".into()),
    };
    let resp = if let Some(cid) = cape_id {
        state
            .client
            .put("https://api.minecraftservices.com/minecraft/profile/capes/active")
            .bearer_auth(&mc_token)
            .header("Content-Type", "application/json")
            .body(serde_json::json!({ "capeId": cid }).to_string())
            .send()
            .await
            .map_err(|e| format!("应用披风失败: {e}"))?
    } else {
        state
            .client
            .delete("https://api.minecraftservices.com/minecraft/profile/capes/active")
            .bearer_auth(&mc_token)
            .send()
            .await
            .map_err(|e| format!("隐藏披风失败: {e}"))?
    };
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("应用披风失败 (HTTP {status}): {body}"));
    }
    Ok(())
}

/// Upload a skin PNG to the player's Mojang account.
/// `skin_data` is a base64 string or a `data:image/png;base64,...` URL.
/// `variant` is `"classic"` (default arms) or `"slim"`.
#[tauri::command]
pub async fn apply_skin_to_account(
    state: State<'_, AppState>,
    account_uuid: String,
    skin_data: String,
    variant: String,
) -> Result<(), String> {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let accounts = accounts::load_accounts(&state);
    let account = accounts
        .iter()
        .find(|a| a.uuid() == &account_uuid)
        .ok_or("账号不存在")?
        .clone();
    if !account.is_microsoft() {
        return Err("离线账号无法上传皮肤，仅支持正版账号".into());
    }
    let account = accounts::refresh_microsoft(&state, &account).await?;
    let mc_token = match &account {
        Account::Microsoft {
            msa_access_token, ..
        } => msa_access_token.clone(),
        _ => return Err("账号类型错误".into()),
    };
    let raw = skin_data.trim();
    let b64 = raw
        .strip_prefix("data:image/png;base64,")
        .unwrap_or(raw);
    let bytes = STANDARD
        .decode(b64)
        .map_err(|e| format!("解析皮肤数据失败: {e}"))?;
    if bytes.len() < 8 || &bytes[0..8] != b"\x89PNG\r\n\x1a\n" {
        return Err("文件不是有效的 PNG".into());
    }
    let v = if variant == "slim" { "slim" } else { "classic" };
    let file_part = reqwest::multipart::Part::bytes(bytes)
        .file_name("skin.png")
        .mime_str("image/png")
        .map_err(|e| format!("构造上传数据失败: {e}"))?;
    let form = reqwest::multipart::Form::new()
        .text("variant", v.to_string())
        .part("file", file_part);
    let resp = state
        .client
        .post("https://api.minecraftservices.com/minecraft/profile/skins")
        .bearer_auth(&mc_token)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("上传皮肤失败: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("上传皮肤失败 (HTTP {status}): {body}"));
    }
    Ok(())
}

/// Save the offline skin PNG so it can be injected into the version jar at
/// launch time.  No jar modification happens here — that would be slow.
/// The skin variant ("slim"/"classic") is persisted alongside in a JSON meta
/// file so it survives any frontend cache (localStorage) clears.
#[tauri::command]
pub fn apply_skin_offline(
    state: State<AppState>,
    skin_data: String,
    variant: String,
    uuid: String,
) -> Result<(), String> {
    use base64::{engine::general_purpose::STANDARD, Engine};

    let raw = skin_data.trim();
    let b64 = raw.strip_prefix("data:image/png;base64,").unwrap_or(raw);
    let bytes = STANDARD.decode(b64).map_err(|e| format!("解析皮肤数据失败: {e}"))?;
    if bytes.len() < 8 || &bytes[0..8] != b"\x89PNG\r\n\x1a\n" {
        return Err("文件不是有效的 PNG".into());
    }

    let skin_dir = state.root.join("skins").join("offline");
    std::fs::create_dir_all(&skin_dir).map_err(|e| format!("创建皮肤目录失败: {e}"))?;
    std::fs::write(skin_dir.join(format!("{uuid}.png")), &bytes)
        .map_err(|e| format!("保存皮肤失败: {e}"))?;

    let variant = if variant == "slim" { "slim" } else { "classic" };
    let meta = serde_json::json!({ "variant": variant });
    let meta_str = serde_json::to_string_pretty(&meta)
        .unwrap_or_else(|_| r#"{"variant":"classic"}"#.to_string());
    std::fs::write(skin_dir.join(format!("{uuid}.json")), meta_str)
        .map_err(|e| format!("保存皮肤变体失败: {e}"))?;
    Ok(())
}

/// Read back a saved offline skin (PNG as a base64 data URL) plus its variant.
/// Returns `null` when no skin has been saved for that uuid.
#[tauri::command]
pub fn get_offline_skin(
    state: State<AppState>,
    uuid: String,
) -> Result<Option<serde_json::Value>, String> {
    use base64::{engine::general_purpose::STANDARD, Engine};

    let skin_dir = state.root.join("skins").join("offline");
    let png_path = skin_dir.join(format!("{uuid}.png"));
    if !png_path.is_file() {
        return Ok(None);
    }
    let bytes = std::fs::read(&png_path).map_err(|e| format!("读取皮肤失败: {e}"))?;
    let src = format!("data:image/png;base64,{}", STANDARD.encode(&bytes));

    // `None` when no meta file exists yet → frontend falls back to auto-detection.
    let variant = std::fs::read_to_string(skin_dir.join(format!("{uuid}.json")))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("variant").and_then(|s| s.as_str()).map(String::from))
        .filter(|v| v == "slim" || v == "classic");

    Ok(Some(serde_json::json!({ "src": src, "variant": variant })))
}

// ---------------------------------------------------------------------------
// Multiplayer servers
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn ping_mc_server(address: String) -> mcping::ServerStatus {
    mcping::ping_server(&address).await
}

#[tauri::command]
pub fn list_servers(state: State<AppState>, instance_id: String) -> Result<Value, String> {
    let dir = state.instances_dir().join(&instance_id);

    // 现代 Minecraft (1.20.5+) 使用 servers.json
    let json_path = dir.join("servers.json");
    if json_path.is_file() {
        if let Ok(text) = std::fs::read_to_string(&json_path) {
            if let Ok(v) = serde_json::from_str::<Value>(&text) {
                if let Some(servers) = v.get("servers").and_then(|x| x.as_array()) {
                    let list: Vec<Value> = servers
                        .iter()
                        .filter_map(|s| {
                            let name = s.get("name").and_then(|x| x.as_str())?.to_string();
                            let ip = s
                                .get("ip")
                                .and_then(|x| x.as_str())
                                .unwrap_or("")
                                .to_string();
                            if ip.is_empty() {
                                return None;
                            }
                            let icon = s.get("icon").and_then(|x| x.as_str()).map(|x| x.to_string());
                            Some(json!({ "name": name, "address": ip, "icon": icon }))
                        })
                        .collect();
                    return Ok(json!({ "servers": list }));
                }
            }
        }
    }

    // 旧版 Minecraft 使用 servers.dat (GZIP 压缩的 NBT)
    let dat_path = dir.join("servers.dat");
    if dat_path.is_file() {
        if let Ok(bytes) = std::fs::read(&dat_path) {
            use std::io::Read;
            // Minecraft 1.12 及更早的 servers.dat 是 gzip 压缩的 NBT，
            // 1.13+ 改为未压缩的纯 NBT。根据魔数判断是否需要先解压。
            let raw: Vec<u8> = if bytes.starts_with(&[0x1f, 0x8b]) {
                use flate2::read::GzDecoder;
                let mut decompressed = Vec::new();
                match GzDecoder::new(&bytes[..]).read_to_end(&mut decompressed) {
                    Ok(_) => decompressed,
                    Err(_) => bytes,
                }
            } else {
                bytes
            };
            if let Ok(root) = fastnbt::from_bytes::<ServersDat>(&raw) {
                let list: Vec<Value> = root
                    .servers
                    .into_iter()
                    .filter_map(|s| {
                        if s.ip.trim().is_empty() {
                            return None;
                        }
                        // NBT 中的 icon 是裸 base64（无 data: 前缀），补全以便前端渲染
                        let icon = s.icon.filter(|i| !i.trim().is_empty()).map(|i| {
                            if i.starts_with("data:") {
                                i
                            } else {
                                format!("data:image/png;base64,{}", i)
                            }
                        });
                        Some(json!({ "name": s.name, "address": s.ip, "icon": icon }))
                    })
                    .collect();
                return Ok(json!({ "servers": list }));
            }
        }
    }

    Ok(json!({ "servers": [] }))
}

#[derive(serde::Deserialize)]
struct ServersDat {
    servers: Vec<ServerNbt>,
}

#[derive(serde::Deserialize)]
struct ServerNbt {
    name: String,
    ip: String,
    #[serde(default)]
    icon: Option<String>,
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// 获取存储统计：优先返回上次扫描的缓存，无缓存时实时扫描
#[tauri::command]
pub fn get_storage_stats(state: State<AppState>) -> crate::storage::StorageStats {
    crate::storage::get_storage_stats(&state)
}

/// 强制重新扫描存储并返回最新统计
#[tauri::command]
pub fn refresh_storage_stats(state: State<AppState>) -> crate::storage::StorageStats {
    crate::storage::refresh_storage_stats(&state)
}

/// 清除可安全清理的缓存，返回释放的空间
#[tauri::command]
pub fn clear_cache(state: State<AppState>) -> Result<crate::storage::CacheClearResult, String> {
    crate::storage::clear_cache(&state)
}

// ---------------------------------------------------------------------------
// Hosted game servers
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_hosted_servers(state: State<AppState>) -> Result<Vec<ServerConfig>, String> {
    Ok(crate::servers::load_servers(&state))
}

#[tauri::command]
pub fn get_hosted_server(state: State<AppState>, id: String) -> Result<ServerConfig, String> {
    crate::servers::get_server(&state, &id)
}

#[tauri::command]
pub fn create_hosted_server(
    state: State<AppState>,
    name: String,
    core: ServerCore,
    mc_version: String,
) -> Result<ServerConfig, String> {
    crate::servers::create_server(&state, name, core, mc_version)
}

#[tauri::command]
pub fn update_hosted_server(
    state: State<AppState>,
    patch: Value,
) -> Result<ServerConfig, String> {
    crate::servers::update_server(&state, patch)
}

#[tauri::command]
pub fn delete_hosted_server(state: State<AppState>, id: String) -> Result<(), String> {
    crate::servers::delete_server(&state, &id)
}

#[tauri::command]
pub async fn install_hosted_server_core(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    crate::servers::install_server_core(app, &state, &id).await
}

#[tauri::command]
pub async fn start_hosted_server(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<u32, String> {
    crate::servers::start_server(app, &state, &id).await
}

#[tauri::command]
pub async fn stop_hosted_server(state: State<'_, AppState>, id: String) -> Result<(), String> {
    crate::servers::stop_server(&state, &id).await
}

#[tauri::command]
pub fn is_hosted_server_running(state: State<AppState>, id: String) -> Result<bool, String> {
    Ok(crate::servers::is_server_running(&state, &id))
}

#[tauri::command]
pub fn read_hosted_server_log(state: State<AppState>, id: String) -> Result<Vec<String>, String> {
    crate::servers::read_server_log(&state, &id)
}

#[tauri::command]
pub fn open_hosted_server_folder(
    app: tauri::AppHandle,
    state: State<AppState>,
    id: String,
    sub: Option<String>,
) -> Result<(), String> {
    let mut dir = crate::servers::server_dir(&state, &id);
    if let Some(s) = sub {
        if !crate::servers::SERVER_SUBFOLDERS.contains(&s.as_str()) {
            return Err("非法目录".into());
        }
        dir = dir.join(s);
    }
    if !dir.exists() {
        return Err("目录不存在".into());
    }
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// 在系统文件管理器中显示服务器目录下的任意文件/文件夹
#[tauri::command]
pub fn reveal_hosted_server_path(
    app: tauri::AppHandle,
    state: State<AppState>,
    id: String,
    rel: String,
) -> Result<(), String> {
    let path = crate::servers::resolve_server_path(&state, &id, &rel)?;
    let open_target = if path.is_file() {
        path.parent().map(|p| p.to_path_buf()).unwrap_or(path.clone())
    } else {
        path.clone()
    };
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(open_target.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_hosted_server_folders(
    state: State<AppState>,
    id: String,
) -> Result<Value, String> {
    let folders = crate::servers::list_server_folders(&state, &id);
    let arr: Vec<Value> = folders
        .into_iter()
        .map(|(name, exists)| json!({ "name": name, "exists": exists }))
        .collect();
    Ok(json!({ "folders": arr }))
}

#[tauri::command]
pub async fn list_hosted_server_files(
    state: State<'_, AppState>,
    id: String,
    sub: String,
) -> Result<Value, String> {
    if !crate::servers::SERVER_SUBFOLDERS.contains(&sub.as_str()) {
        return Err("非法目录".into());
    }
    let dir = crate::servers::server_dir(&state, &id).join(&sub);
    if !dir.exists() {
        return Ok(json!({ "files": [] }));
    }
    let files = tokio::task::spawn_blocking(move || {
        let mut files: Vec<Value> = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
            let e = entry.map_err(|e| e.to_string())?;
            let meta = e.metadata().map_err(|e| e.to_string())?;
            let path = e.path();
            files.push(json!({
                "name": e.file_name().to_string_lossy().to_string(),
                "path": path.to_string_lossy().to_string(),
                "size": meta.len(),
                "modified": meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0),
                "isDir": meta.is_dir(),
                "icon": null,
            }));
        }
        Ok::<Vec<Value>, String>(files)
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(json!({ "files": files }))
}

/// 列出服务器目录下任意相对路径的条目（与游戏实例的文件管理器一致），用于内置文件管理器
#[tauri::command]
pub async fn list_hosted_server_dir(
    state: State<'_, AppState>,
    id: String,
    rel: String,
) -> Result<Value, String> {
    let dir = crate::servers::resolve_server_path(&state, &id, &rel)?;
    if !dir.is_dir() {
        return Err("目标不是一个目录".into());
    }
    let rel_json = rel.clone();
    let entries = tokio::task::spawn_blocking(move || {
        let mut files: Vec<FsEntry> = Vec::new();
        let mut dirs: Vec<FsEntry> = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(|e| format!("读取目录失败: {e}"))? {
            let e = entry.map_err(|e| format!("读取目录失败: {e}"))?;
            let meta = e.metadata().map_err(|e| format!("读取元数据失败: {e}"))?;
            let name = e.file_name().to_string_lossy().to_string();
            let is_dir = meta.is_dir();
            let child_rel = if rel.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", rel.trim_end_matches('/'), name)
            };
            let fs = FsEntry {
                ext: if is_dir { String::new() } else { ext_of(&name) },
                name,
                rel: child_rel,
                size: if is_dir { 0 } else { meta.len() },
                modified: modified_secs(&meta),
                is_dir,
            };
            if is_dir {
                dirs.push(fs);
            } else {
                files.push(fs);
            }
        }
        dirs.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        dirs.extend(files);
        Ok::<Vec<FsEntry>, String>(dirs)
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(json!({ "rel": rel_json, "entries": entries }))
}

#[tauri::command]
pub fn read_hosted_server_file(
    state: State<AppState>,
    id: String,
    rel: String,
) -> Result<Value, String> {
    let path = crate::servers::resolve_server_path(&state, &id, &rel)?;
    if !path.is_file() {
        return Err("不是一个文件".into());
    }
    let meta = std::fs::metadata(&path).map_err(|e| format!("读取文件失败: {e}"))?;
    if meta.len() > MAX_EDIT_BYTES {
        return Err(format!(
            "文件过大（{}），内置编辑器最多支持 {}",
            fmt_bytes(meta.len()),
            fmt_bytes(MAX_EDIT_BYTES)
        ));
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("读取文件失败: {e}"))?;
    if bytes.iter().take(4096).any(|b| *b == 0) {
        return Err("这是二进制文件，无法在内置编辑器中打开".into());
    }
    let content = String::from_utf8(bytes)
        .map_err(|_| String::from("文件不是 UTF-8 编码，无法在内置编辑器中打开"))?;
    let meta2 = std::fs::metadata(&path).ok();
    Ok(json!({
        "rel": rel,
        "content": content,
        "size": meta.len(),
        "modified": meta2.as_ref().map(modified_secs).unwrap_or(0),
    }))
}

#[tauri::command]
pub fn write_hosted_server_file(
    state: State<AppState>,
    id: String,
    rel: String,
    content: String,
) -> Result<Value, String> {
    let path = crate::servers::resolve_server_path(&state, &id, &rel)?;
    if path.is_dir() {
        return Err("目标是一个目录".into());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let len = content.len() as u64;
    std::fs::write(&path, content).map_err(|e| format!("写入文件失败: {e}"))?;
    let meta = std::fs::metadata(&path).ok();
    Ok(json!({
        "rel": rel,
        "size": meta.as_ref().map(|m| m.len()).unwrap_or(len),
        "modified": meta.as_ref().map(modified_secs).unwrap_or(0),
    }))
}

#[tauri::command]
pub fn list_hosted_server_config_files(
    state: State<AppState>,
    id: String,
) -> Result<Vec<crate::servers::ServerConfigFile>, String> {
    crate::servers::list_server_config_files(&state, &id)
}

// ---------------------------------------------------------------------------
// Crash analysis
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
pub struct CrashLogEntry {
    pub filename: String,
    pub modified: u64,
    pub size: u64,
    pub kind: String,
}

/// List crash reports and JVM hs_err logs for an instance.
#[tauri::command]
pub fn list_crash_logs(state: State<AppState>, id: String) -> Result<Vec<CrashLogEntry>, String> {
    let inst_dir = state.root.join("instances").join(&id);
    let crash_dir = inst_dir.join("crash-reports");
    let mut out: Vec<CrashLogEntry> = Vec::new();

    if crash_dir.is_dir() {
        for entry in std::fs::read_dir(&crash_dir).map_err(|e| e.to_string())? {
            let e = entry.map_err(|e| e.to_string())?;
            let meta = e.metadata().map_err(|e| e.to_string())?;
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("crash-") && name.ends_with(".txt") {
                out.push(CrashLogEntry {
                    filename: name,
                    modified: meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0),
                    size: meta.len(),
                    kind: "crash".into(),
                });
            }
        }
    }

    for entry in std::fs::read_dir(&inst_dir).map_err(|e| e.to_string())? {
        let e = entry.map_err(|e| e.to_string())?;
        let meta = e.metadata().map_err(|e| e.to_string())?;
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with("hs_err_pid") && name.ends_with(".log") {
            out.push(CrashLogEntry {
                filename: name,
                modified: meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0),
                size: meta.len(),
                kind: "jvm".into(),
            });
        }
    }

    out.sort_by_key(|e| std::cmp::Reverse(e.modified));
    Ok(out)
}

/// Run the built-in crash diagnosis engine against a specific crash report.
///
/// 诊断逻辑（正则规则集 + 堆栈关键词反查模组）全部在 `crash` 模块中，
/// 这里只负责读取报告文本并回填报告文件名。
#[tauri::command]
pub fn analyze_crash_log(state: State<AppState>, id: String, filename: String) -> Result<CrashDiagnosis, String> {
    let inst_dir = state.root.join("instances").join(&id);
    let crash_path = if filename.starts_with("crash-") {
        inst_dir.join("crash-reports").join(&filename)
    } else {
        inst_dir.join(&filename)
    };
    if !crash_path.is_file() {
        return Err("崩溃报告文件不存在".into());
    }
    let content =
        String::from_utf8_lossy(&std::fs::read(&crash_path).map_err(|e| e.to_string())?).to_string();

    let mut diagnosis = crate::crash::analyze_text(&content, None);
    diagnosis.crash_report = Some(filename);
    Ok(diagnosis)
}


/// Read the raw content of a crash report or hs_err log.
#[tauri::command]
pub fn get_crash_report_content(state: State<AppState>, id: String, filename: String) -> Result<String, String> {
    let inst_dir = state.root.join("instances").join(&id);
    let path = if filename.starts_with("crash-") {
        inst_dir.join("crash-reports").join(&filename)
    } else {
        inst_dir.join(&filename)
    };
    if !path.is_file() {
        return Err("文件不存在".into());
    }
    Ok(String::from_utf8_lossy(&std::fs::read(&path).map_err(|e| e.to_string())?).to_string())
}
/// Minecraft 官方新闻搜索接口（minecraft.net 官网自用），按时间倒序取最新中文条目
const NEWS_API: &str = "https://net-secondary.web.minecraft-services.net/api/v1.0/zh-cn/search?pageSize=24&sortType=Recent&category=News&newsOnly=true&geography=CN";

#[derive(serde::Deserialize)]
struct NewsEntry {
    title: Option<String>,
    description: Option<String>,
    author: Option<String>,
    image: Option<String>,
    #[serde(rename = "imageAltText")]
    image_alt: Option<String>,
    url: Option<String>,
    /// Unix 时间戳（秒）
    time: Option<i64>,
}

#[derive(serde::Deserialize)]
struct NewsResult {
    results: Option<Vec<NewsEntry>>,
}

#[derive(serde::Deserialize)]
struct NewsResponse {
    result: Option<NewsResult>,
}

/// 官网原始数据里含有 `&amp;` 等 HTML 实体，直接展示会露出生标记
fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

/// 拉取 Minecraft 官方新闻。网络不可用时返回空数组，由前端展示「暂无新闻」。
#[tauri::command]
pub async fn fetch_news(state: State<'_, AppState>) -> Result<Vec<serde_json::Value>, String> {
    let resp = state
        .client
        .get(NEWS_API)
        .timeout(std::time::Duration::from_secs(15))
        .header("User-Agent", "QookiX-Launcher")
        .send()
        .await
        .map_err(|e| format!("获取新闻失败: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("获取新闻失败: HTTP {status}"));
    }
    let body: NewsResponse = resp
        .json()
        .await
        .map_err(|e| format!("解析新闻失败: {e}"))?;

    let items = body
        .result
        .and_then(|r| r.results)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|e| {
            let title = e.title.unwrap_or_default();
            if title.trim().is_empty() {
                return None;
            }
            Some(json!({
                "title": decode_entities(&title),
                "description": e.description.map(|d| decode_entities(&d)).unwrap_or_default(),
                "author": e.author.unwrap_or_default(),
                "time": e.time.unwrap_or(0),
                "image": e.image.unwrap_or_default(),
                "image_alt": e.image_alt.unwrap_or_default(),
                "url": e.url.unwrap_or_default(),
            }))
        })
        .collect();
    Ok(items)
}
