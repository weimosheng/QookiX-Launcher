use crate::models::*;
use crate::state::AppState;
use serde_json::{json, Value};
use tauri::Emitter;
use tauri::State;

// Instance file manager
// ---------------------------------------------------------------------------

/// Maximum file size (bytes) the built-in editor is willing to load.
pub(crate) const MAX_EDIT_BYTES: u64 = 4 * 1024 * 1024;

pub(crate) fn fmt_bytes(n: u64) -> String {
    if n >= 1024 * 1024 {
        format!("{:.1} MB", n as f64 / 1024.0 / 1024.0)
    } else if n >= 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

pub(crate) fn modified_secs(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(crate) fn ext_of(name: &str) -> String {
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

/// 把用户选择的图片复制到 `state.root/<dir>/` 下（uuid 命名），返回绝对路径。
/// `image_exts` 为允许的扩展名，`err_prefix` 用于拼接复制失败的错误信息。
fn import_image_into(
    state: &AppState,
    source_path: &str,
    dir_name: &str,
    image_exts: &[&str],
    err_prefix: &str,
) -> Result<String, String> {
    let source = std::path::Path::new(source_path);
    let ext = source
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_else(|| "png".into());
    if !image_exts.contains(&ext.as_str()) {
        return Err("不支持的图片格式".into());
    }
    let dir = state.root.join(dir_name);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(format!("{}.{}", uuid::Uuid::new_v4().simple(), ext));
    std::fs::copy(source, &dest).map_err(|e| format!("{err_prefix}失败: {e}"))?;
    Ok(dest.to_string_lossy().to_string())
}

/// Copy an image file into the launcher icons dir; returns the absolute path.
#[tauri::command]
pub fn import_instance_image(
    state: State<AppState>,
    source_path: String,
) -> Result<String, String> {
    import_image_into(&state, &source_path, "icons", &["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico"], "复制图片")
}

/// Copy a user-selected image into the launcher backgrounds dir; returns the absolute path.
#[tauri::command]
pub fn import_background_image(
    state: State<AppState>,
    source_path: String,
) -> Result<String, String> {
    import_image_into(&state, &source_path, "backgrounds", &["png", "jpg", "jpeg", "gif", "webp", "bmp"], "复制背景图片")
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

