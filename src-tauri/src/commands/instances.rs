use crate::accounts;
use crate::install;
use crate::launch;
use crate::models::*;
use crate::modrinth;
use crate::state::AppState;
use serde_json::{json, Value};
use tauri::Emitter;
use tauri::State;

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
    let l: LoaderType = loader.parse()?;
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

