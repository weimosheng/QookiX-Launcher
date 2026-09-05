use crate::models::*;
use crate::state::AppState;
use serde_json::{json, Value};
use tauri::State;

// 托管服务器面板复用文件管理器的 FsEntry / 列表助手
use super::files::{ext_of, fmt_bytes, modified_secs, FsEntry, MAX_EDIT_BYTES};

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

