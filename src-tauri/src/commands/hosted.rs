use crate::models::*;
use crate::state::AppState;
use serde_json::{json, Value};
use tauri::State;

// 托管服务器面板复用实例文件管理器那套文件操作助手
use crate::fsutil::{self, FsEntry};

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
    fsutil::reveal(&app, &dir)
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
    fsutil::reveal(&app, &path)
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
        return Err("不是一个目录".into());
    }
    let base = rel.clone();
    let entries: Vec<FsEntry> = tokio::task::spawn_blocking(move || fsutil::list_dir(&dir, &base))
        .await
        .map_err(|e| e.to_string())??;
    Ok(json!({ "rel": rel, "entries": entries }))
}

#[tauri::command]
pub fn read_hosted_server_file(
    state: State<AppState>,
    id: String,
    rel: String,
) -> Result<Value, String> {
    let path = crate::servers::resolve_server_path(&state, &id, &rel)?;
    fsutil::read_text(&path, &rel)
}

#[tauri::command]
pub fn write_hosted_server_file(
    state: State<AppState>,
    id: String,
    rel: String,
    content: String,
) -> Result<Value, String> {
    let path = crate::servers::resolve_server_path(&state, &id, &rel)?;
    fsutil::write_text(&path, &rel, content)
}

#[tauri::command]
pub fn list_hosted_server_config_files(
    state: State<AppState>,
    id: String,
) -> Result<Vec<crate::servers::ServerConfigFile>, String> {
    crate::servers::list_server_config_files(&state, &id)
}

