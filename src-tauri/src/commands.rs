use crate::accounts;
use crate::curseforge;
use crate::install;
use crate::launch;
use crate::mcmeta;
use crate::models::*;
use crate::modrinth;
use crate::settings;
use crate::state::AppState;
use serde_json::{json, Value};
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

/// Auto-detect system memory and return (total, used, available) + recommended (max, min) in MB.
#[tauri::command]
pub fn auto_detect_memory() -> Result<Value, String> {
    let total = crate::settings::total_memory_mb().ok_or("无法检测系统内存")?;
    let used = crate::settings::used_memory_mb().unwrap_or(0);
    let available = crate::settings::available_memory_mb().unwrap_or(total.saturating_sub(used));
    let (mut max, mut min) = crate::settings::recommended_memory(total);
    // 自动推荐值不超过当前可用（剩余）内存，避免配置超出剩下空间
    if available >= 512 {
        max = max.min(available as u32).max(256);
    }
    min = min.min(max).max(64);
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
    let runtimes = state.root.join("runtimes");
    let candidates = crate::java::detect_java(None, Some(&runtimes));
    let selected = candidates.first().cloned();
    *state.java_cache.lock().unwrap() = Some((now, candidates.clone()));
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
    crate::instances::create_instance(&state, name, mc_version, l, loader_version)
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
) -> Result<LaunchResult, String> {
    if launch::is_running(&state) {
        return Err("已有游戏在运行中".into());
    }
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
    let account = accounts::refresh_microsoft(&state, &account).await?;
    let resolved = launch::ResolvedAccount {
        username: account.username().to_string(),
        uuid: account.uuid().to_string(),
        access_token: match &account {
            Account::Microsoft { msa_access_token, .. } => msa_access_token.clone(),
            Account::Offline { .. } => "0".into(),
        },
        user_type: if account.is_microsoft() { "msa".into() } else { "legacy".into() },
    };
    let result = launch::launch_game(app.clone(), &state, &instance, resolved, world).await?;
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
    let source = std::path::Path::new(&source_path);
    let ext = source
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_else(|| "png".into());
    let dir = state.root.join("icons");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(format!("{}.{}", uuid::Uuid::new_v4().simple(), ext.to_lowercase()));
    std::fs::copy(source, &dest).map_err(|e| format!("复制图片失败: {e}"))?;
    Ok(dest.to_string_lossy().to_string())
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
            modrinth::search(
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
            .await
        }
        "curseforge" => {
            let cat = category.parse::<u32>().unwrap_or(0);
            curseforge::search(&state, &query, &project_type, cat, page as usize, ps, &game_version, &loader, &sort).await
        }
        // "全部来源"：整合 Modrinth 与 CurseForge。每页各取一页，合并后统一按下载量
        // 降序排序并截断一页，让两平台结果混合排布。分类只作用于 Modrinth。
        "all" => {
            let m = modrinth::search(
                &state,
                &query,
                &project_type,
                &category,
                "downloads",
                (page as usize) * ps,
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
            let mut total = m.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
            if let Ok(c) = curseforge::search(
                &state,
                &query,
                &project_type,
                0,
                page as usize,
                ps,
                &game_version,
                &loader,
                "downloads",
            )
            .await
            {
                if let Some(ch) = c.get("hits").and_then(|v| v.as_array()) {
                    hits.extend(ch.iter().cloned());
                }
                total += c.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
            }
            hits.sort_by(|a, b| {
                let da = a.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0);
                let db = b.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0);
                db.cmp(&da)
            });
            hits.truncate(ps);
            Ok(json!({ "hits": hits, "total": total }))
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

/// Required/optional dependency projects of a project version (Modrinth only).
#[tauri::command]
pub async fn project_dependencies(
    state: State<'_, AppState>,
    provider: String,
    version_id: String,
) -> Result<Vec<Value>, String> {
    if provider == "modrinth" {
        modrinth::dependencies(&state, &version_id).await
    } else {
        Ok(vec![])
    }
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
    modrinth::check_updates(&state, &instance, &kind).await
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
    let instance = crate::instances::get_instance(&state, &instance_id)?;
    // install new version first (adds a new record)
    let result = match provider.as_str() {
        "modrinth" => modrinth::install_version(app, &state, &instance, &new_version_id, &kind).await?,
        "curseforge" => curseforge::install_file(app, &state, &instance, &project_id, &new_version_id, &kind).await?,
        _ => return Err("未知内容源".into()),
    };
    // remove the old file + record
    let _ = modrinth::uninstall(&state, &instance, &kind, &old_filename);
    Ok(result)
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
    let ext = if kind == "shader" { ".zip" } else { ".jar" };
    let mut new_records: Vec<InstalledContent> = Vec::new();
    for fname in &on_disk {
        if !fname.ends_with(ext) {
            continue;
        }
        if records.iter().any(|r: &InstalledContent| r.filename == *fname) {
            continue;
        }
        let size = std::fs::metadata(dir.join(fname)).map(|m| m.len()).unwrap_or(0);
        new_records.push(InstalledContent {
            filename: fname.clone(),
            source: "modpack".into(),
            project_id: None,
            version_id: None,
            name: Some(fname.clone()),
            version: None,
            installed_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            size,
            icon: None,
            enabled: true,
        });
    }
    if !new_records.is_empty() {
        let _ = crate::instances::add_content_batch(&state, &instance_id, &kind, new_records.clone());
        records.extend(new_records);
    }
    for rec in &mut records {
        let abs = dir.join(&rec.filename);
        if rec.icon.is_none() && abs.is_file() {
            rec.icon = crate::util::extract_archive_icon(&abs, &kind);
        }
    }
    let items: Vec<Value> = records
        .iter()
        .map(|r| {
            let exists = dir.join(&r.filename).is_file()
                || dir.join(format!("{}.disabled", r.filename)).is_file();
            json!({ "record": r, "exists": exists })
        })
        .collect();
    Ok(json!({ "items": items, "onDisk": on_disk }))
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
    let record = InstalledContent {
        filename,
        source: "manual".into(),
        project_id: None,
        version_id: None,
        name: None,
        version: None,
        installed_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        size,
        icon,
        enabled: true,
    };
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
