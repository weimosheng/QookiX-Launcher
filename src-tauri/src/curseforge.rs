use crate::models::{InstalledContent, Instance};
use crate::state::AppState;
use serde_json::{json, Value};
use tauri::Emitter;

const API: &str = "https://api.curseforge.com/v1";
const GAME_ID: u32 = 432; // Minecraft

/// CurseForge class ids for Minecraft content.
pub fn class_id_for(kind: &str) -> u32 {
    match kind {
        "modpack" => 4471,
        "resourcepack" => 12,
        "shader" => 6552,
        "datapack" => 6945,
        _ => 6, // mods
    }
}

/// Built-in CurseForge API Key. Injected at compile time via CURSEFORGE_API_KEY env var.
pub const BUILTIN_CF_API_KEY: &str = match option_env!("CURSEFORGE_API_KEY") {
    Some(k) => k,
    None => "",
};

fn api_key(state: &AppState) -> Result<String, String> {
    let s = state.settings.read().unwrap();
    if let Some(k) = s.curseforge_api_key.clone().filter(|k| !k.is_empty()) {
        return Ok(k);
    }
    if !BUILTIN_CF_API_KEY.is_empty() {
        return Ok(BUILTIN_CF_API_KEY.to_string());
    }
    Err("未配置 CurseForge API Key，请在设置中填写，或用 CURSEFORGE_API_KEY 环境变量重新构建（可前往 console.curseforge.com 免费申请）".into())
}

async fn get(state: &AppState, path: &str, params: &[(&str, String)]) -> Result<Value, String> {
    let key = api_key(state)?;
    let mut url = format!("{API}{path}");
    if !params.is_empty() {
        url.push('?');
        url.push_str(
            &params
                .iter()
                .map(|(k, v)| format!("{k}={}", crate::modrinth::urlencode(v)))
                .collect::<Vec<_>>()
                .join("&"),
        );
    }
    let resp = state
        .client
        .get(&url)
        .header("x-api-key", key)
        .send()
        .await
        .map_err(|e| format!("CurseForge 请求失败: {e}"))?;
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取 CurseForge 响应失败: {e}"))?;
    let body: Value = serde_json::from_str(&text).map_err(|e| {
        let snippet = text.chars().take(200).collect::<String>();
        format!("CurseForge 响应解析失败 (HTTP {status}): {e}\n响应内容: {snippet}")
    })?;
    if !status.is_success() {
        let msg = body
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let reason = if text.contains("API Key missing or invalid") || text.contains("Forbidden") {
            "CurseForge API Key 无效或未生效：请确认已在 console.curseforge.com 申请 API Key，并在「设置 → CurseForge API Key」中正确填写；新申请的 Key 可能需等待几分钟生效"
        } else if msg.is_empty() {
            "未知错误"
        } else {
            msg
        };
        return Err(format!("CurseForge API 错误 (HTTP {status}): {reason}"));
    }
    Ok(body)
}

/// Fetch a single mod file's metadata (fileName, fileLength, downloadUrl) by project + file id.
pub async fn file_info(state: &AppState, project_id: u64, file_id: u64) -> Result<Value, String> {
    get(state, &format!("/mods/{project_id}/files/{file_id}"), &[]).await
}

/// CurseForge relationType → dependency type string matching Modrinth format.
fn relation_type(rt: u64) -> &'static str {
    match rt {
        1 => "embedded",
        2 => "optional",
        3 => "required",
        5 => "incompatible",
        6 => "embedded",
        _ => "required",
    }
}

/// Dependencies of a CurseForge file: fetch file → extract dependencies → batch-fetch mod info.
pub async fn dependencies(state: &AppState, mod_id: &str, file_id: &str) -> Result<Vec<Value>, String> {
    let body = get(state, &format!("/mods/{mod_id}/files/{file_id}"), &[]).await?;
    let file = body.get("data").unwrap_or(&body);
    let deps = file.get("dependencies").and_then(|d| d.as_array()).cloned().unwrap_or_default();
    if deps.is_empty() {
        return Ok(vec![]);
    }
    // Collect mod IDs for batch lookup
    let mod_ids: Vec<u64> = deps
        .iter()
        .filter_map(|d| d.get("modId").and_then(|v| v.as_u64()))
        .collect();
    // Batch fetch mod info via POST /mods
    let mut info_map: std::collections::HashMap<u64, (String, String)> = std::collections::HashMap::new();
    if !mod_ids.is_empty() {
        let key = api_key(state)?;
        let url = format!("{API}/mods");
        let payload = json!({ "modIds": mod_ids });
        if let Ok(resp) = state
            .client
            .post(&url)
            .header("x-api-key", &key)
            .header("Content-Type", "application/json")
            .body(payload.to_string())
            .send()
            .await
        {
            if let Ok(body) = resp.json::<Value>().await {
                if let Some(data) = body.get("data").and_then(|d| d.as_array()) {
                    for m in data {
                        if let Some(id) = m.get("id").and_then(|v| v.as_u64()) {
                            let name = m.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            let slug = m.get("slug").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            info_map.insert(id, (name, slug));
                        }
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    for dep in &deps {
        let Some(mod_id) = dep.get("modId").and_then(|v| v.as_u64()) else { continue };
        let rt = dep.get("relationType").and_then(|v| v.as_u64()).unwrap_or(3);
        let (title, slug) = info_map.get(&mod_id).cloned().unwrap_or_default();
        out.push(json!({
            "projectId": mod_id.to_string(),
            "title": title,
            "slug": slug,
            "dependencyType": relation_type(rt),
        }));
    }
    Ok(out)
}

/// Search CurseForge mods/modpacks/resourcepacks/shaders.
pub async fn search(
    state: &AppState,
    query: &str,
    kind: &str,
    category_id: u32,
    page: usize,
    page_size: usize,
    game_version: &str,
    loader: &str,
    sort: &str,
) -> Result<Value, String> {
    let page_size = page_size.max(1);
    let index = page * page_size;
    // CurseForge API rejects index > 10000 with HTTP 400
    if index > 10000 {
        return Ok(json!({ "hits": [], "total": 0 }));
    }
    let class_id = class_id_for(kind);
    // CurseForge sortField: 2 = popularity, 3 = recently updated, 6 = total downloads
    let sort_field = match sort {
        "newest" | "updated" => "3",
        "downloads" => "6",
        _ => "2",
    };
    let mut params: Vec<(&str, String)> = vec![
        ("gameId", GAME_ID.to_string()),
        ("classId", class_id.to_string()),
        ("pageSize", page_size.to_string()),
        ("index", index.to_string()),
        ("sortField", sort_field.to_string()),
        ("sortOrder", "desc".to_string()),
    ];
    if !query.trim().is_empty() {
        params.push(("searchFilter", query.trim().to_string()));
    }
    if category_id > 0 {
        params.push(("categoryId", category_id.to_string()));
    }
    if !game_version.is_empty() {
        params.push(("gameVersion", game_version.to_string()));
    }
    let loader_id = match loader {
        "forge" => Some(1),
        "fabric" => Some(4),
        "quilt" => Some(5),
        "neoforge" => Some(6),
        _ => None,
    };
    if let Some(id) = loader_id {
        params.push(("modLoaderType", id.to_string()));
    }
    let body = get(state, "/mods/search", &params).await?;
    let data = body.get("data").and_then(|d| d.as_array()).cloned().unwrap_or_default();
    let out: Vec<Value> = data
        .iter()
        .map(|m| {
            let logo = m.get("logo").and_then(|l| l.get("url")).and_then(|v| v.as_str()).unwrap_or("");
            let featured = m.get("screenshots")
                .and_then(|s| s.as_array())
                .and_then(|s| s.first())
                .and_then(|x| x.get("url"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            json!({
                "provider": "curseforge",
                "id": m.get("id").and_then(|v| v.as_u64()).unwrap_or(0).to_string(),
                "slug": m.get("slug").and_then(|v| v.as_str()).unwrap_or(""),
                "title": m.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                "description": m.get("summary").and_then(|v| v.as_str()).unwrap_or(""),
                "author": m.get("authors").and_then(|a| a.as_array()).and_then(|a| a.first()).and_then(|x| x.get("name")).and_then(|v| v.as_str()).unwrap_or(""),
                "downloads": m.get("downloadCount").and_then(|v| v.as_u64()).unwrap_or(0),
                "follows": 0,
                "icon_url": logo,
                "project_type": kind,
                "categories": m.get("categories").and_then(|c| c.as_array()).map(|c| c.iter().filter_map(|x| x.get("name").and_then(|v| v.as_str()).map(|s| s.to_string())).collect::<Vec<_>>()).unwrap_or_default(),
                "latest_version": "",
                "game_versions": m.get("gameVersions").and_then(|v| v.as_array()).cloned().unwrap_or_default(),
                "updated": m.get("dateModified").and_then(|v| v.as_str()).unwrap_or(""),
                "featured_image": featured,
                "_sort_ts": m.get("dateModified").and_then(|v| v.as_str()).unwrap_or(""),
            })
        })
        .collect();
    let total = body.get("pagination").and_then(|p| p.get("totalCount")).and_then(|v| v.as_u64()).unwrap_or(0);
    Ok(json!({ "hits": out, "total": total }))
}

/// Categories for a content class (for the filter dropdown).
pub async fn categories(state: &AppState, kind: &str) -> Result<Vec<Value>, String> {
    let body = get(state, "/categories", &[("gameId", GAME_ID.to_string())]).await?;
    let class_id = class_id_for(kind);
    let data = body.get("data").and_then(|d| d.as_array()).cloned().unwrap_or_default();
    Ok(data
        .iter()
        .filter(|c| c.get("classId").and_then(|v| v.as_u64()).unwrap_or(0) == class_id as u64)
        .map(|c| {
            json!({
                "id": c.get("id").and_then(|v| v.as_u64()).unwrap_or(0),
                "name": c.get("name").and_then(|v| v.as_str()).unwrap_or(""),
            })
        })
        .collect())
}

/// Fetch a single CurseForge mod's full info (used when opening a dependency).
pub async fn project_info(state: &AppState, mod_id: &str) -> Result<Value, String> {
    let body = get(state, &format!("/mods/{mod_id}"), &[]).await?;
    let m = body.get("data").cloned().unwrap_or(body);
    let logo = m
        .get("logo")
        .and_then(|l| l.get("url"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let kind = match m.get("classId").and_then(|v| v.as_u64()).unwrap_or(6) {
        4471 => "modpack",
        12 => "resourcepack",
        6552 => "shader",
        6945 => "datapack",
        _ => "mod",
    };
    Ok(json!({
        "provider": "curseforge",
        "id": m.get("id").and_then(|v| v.as_u64()).unwrap_or(0).to_string(),
        "slug": m.get("slug").and_then(|v| v.as_str()).unwrap_or(""),
        "title": m.get("name").and_then(|v| v.as_str()).unwrap_or(""),
        "description": m.get("summary").and_then(|v| v.as_str()).unwrap_or(""),
        "author": m.get("authors").and_then(|a| a.as_array()).and_then(|a| a.first()).and_then(|x| x.get("name")).and_then(|v| v.as_str()).unwrap_or(""),
        "downloads": m.get("downloadCount").and_then(|v| v.as_u64()).unwrap_or(0),
        "follows": 0,
        "icon_url": logo,
        "project_type": kind,
        "categories": m.get("categories").and_then(|c| c.as_array()).map(|c| c.iter().filter_map(|x| x.get("name").and_then(|v| v.as_str()).map(|s| s.to_string())).collect::<Vec<_>>()).unwrap_or_default(),
        "latest_version": "",
        "game_versions": m.get("gameVersions").and_then(|v| v.as_array()).cloned().unwrap_or_default(),
    }))
}

/// Files of a mod, optionally filtered by game version.
pub async fn files(state: &AppState, mod_id: &str, mc_version: &str) -> Result<Vec<Value>, String> {
    let mut params: Vec<(&str, String)> = vec![("pageSize", "100".into())];
    if !mc_version.is_empty() {
        params.push(("gameVersion", mc_version.to_string()));
    }
    let body = get(state, &format!("/mods/{mod_id}/files"), &params).await?;
    let data = body.get("data").and_then(|d| d.as_array()).cloned().unwrap_or_default();
    Ok(data
        .iter()
        .map(|f| {
            json!({
                "id": f.get("id").and_then(|v| v.as_u64()).unwrap_or(0).to_string(),
                "name": f.get("displayName").and_then(|v| v.as_str()).unwrap_or(""),
                "version_number": f.get("displayName").and_then(|v| v.as_str()).unwrap_or(""),
                "date_published": f.get("fileDate").and_then(|v| v.as_str()).unwrap_or(""),
                "release_type": f.get("releaseType").and_then(|v| v.as_u64()).unwrap_or(0),
                "game_versions": f.get("gameVersions").and_then(|v| v.as_array()).cloned().unwrap_or_default()
                // 不返回 filename/size/download_url：安装时后端用文件 id 重新取文件
            })
        })
        .collect())
}

/// Check installed CurseForge content for available updates.
///
/// Returns the same shape as `modrinth::check_updates`:
/// `{filename, projectId, currentVersion, latestVersion, latestVersionId, projectTitle, kind}`.
///
/// CurseForge's file list is filtered by game version only; we additionally
/// prefer files whose declared game versions include the instance's loader
/// (case-insensitive) to avoid suggesting cross-loader updates.
pub async fn check_updates(
    state: &AppState,
    instance: &Instance,
    kind: &str,
) -> Result<Vec<Value>, String> {
    let list = crate::instances::list_content(state, &instance.id, kind);
    let loader = instance.loader.as_str().to_lowercase();
    let mut updates = Vec::new();
    for item in list {
        if item.source != "curseforge" {
            continue;
        }
        let (Some(project_id), Some(current_id)) =
            (item.project_id.as_deref(), item.version_id.as_deref())
        else {
            continue;
        };
        let flist = files(state, project_id, &instance.mc_version).await?;
        if flist.is_empty() {
            continue;
        }
        // Prefer files matching the instance loader, otherwise fall back to all.
        let mut candidates: Vec<&Value> = flist.iter().collect();
        if !loader.is_empty() {
            candidates = candidates
                .iter()
                .cloned()
                .filter(|f| {
                    f.get("game_versions")
                        .and_then(|g| g.as_array())
                        .map(|arr| {
                            arr.iter()
                                .any(|v| v.as_str().map(|s| s.to_lowercase() == loader).unwrap_or(false))
                        })
                        .unwrap_or(false)
                })
                .collect();
        }
        if candidates.is_empty() {
            candidates = flist.iter().collect();
        }
        // Pick the newest file by its publish date.
        let mut newest: Option<&Value> = None;
        for f in &candidates {
            match newest {
                None => newest = Some(f),
                Some(n) => {
                    let nd = n.get("date_published").and_then(|v| v.as_str()).unwrap_or("");
                    let fd = f.get("date_published").and_then(|v| v.as_str()).unwrap_or("");
                    if fd > nd {
                        newest = Some(f);
                    }
                }
            }
        }
        if let Some(latest) = newest {
            let latest_id = latest.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if !latest_id.is_empty() && latest_id != current_id {
                let current_version = flist
                    .iter()
                    .find(|f| f.get("id").and_then(|v| v.as_str()).unwrap_or("") == current_id)
                    .and_then(|f| f.get("version_number").and_then(|v| v.as_str()))
                    .unwrap_or(item.version.as_deref().unwrap_or(""))
                    .to_string();
                updates.push(json!({
                    "filename": item.filename,
                    "projectId": project_id,
                    "currentVersion": current_version,
                    "latestVersion": latest.get("version_number").and_then(|v| v.as_str()).unwrap_or(""),
                    "latestVersionId": latest_id,
                    "projectTitle": item.name,
                    "kind": kind,
                    "provider": "curseforge",
                }));
            }
        }
    }
    Ok(updates)
}

/// Download URL for a CurseForge file (edge CDN fallback when downloadUrl is absent).
fn file_download_url(file: &Value) -> String {
    if let Some(u) = file.get("downloadUrl").and_then(|v| v.as_str()) {
        if !u.is_empty() {
            return u.to_string();
        }
    }
    let id = file.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
    let filename = file.get("fileName").and_then(|v| v.as_str()).unwrap_or("file.jar");
    let a = id / 1000;
    let b = id % 1000;
    format!("https://edge.forgecdn.net/files/{a}/{b}/{filename}")
}

pub fn kind_folder(kind: &str) -> &'static str {
    crate::modrinth::kind_folder(kind)
}

/// Install a CurseForge file (mod / resourcepack / shader).
pub async fn install_file(
    app: tauri::AppHandle,
    state: &AppState,
    instance: &Instance,
    mod_id: &str,
    file_id: &str,
    kind: &str,
) -> Result<Value, String> {
    let body = get(state, &format!("/mods/{mod_id}/files/{file_id}"), &[]).await?;
    let file = body.get("data").ok_or("未找到该文件")?;
    let filename = file
        .get("fileName")
        .and_then(|v| v.as_str())
        .unwrap_or("file.jar")
        .to_string();
    // `fileName` comes from the CurseForge API (untrusted); it is joined onto
    // the instance content folder, so require a plain file name.
    if !crate::util::is_safe_filename(&filename) {
        return Err(format!("文件名为非法路径: {filename}"));
    }
    let url = file_download_url(file);
    let size = file.get("fileLength").and_then(|v| v.as_u64()).unwrap_or(0);
    let project_name = file
        .get("displayName")
        .and_then(|v| v.as_str())
        .unwrap_or(&filename)
        .to_string();

    let dest = state
        .instances_dir()
        .join(&instance.id)
        .join(kind_folder(kind))
        .join(&filename);
    let items = vec![crate::download::DownloadItem {
        url,
        dest: dest.clone(),
        sha1: None,
        sha512: None,
        size: if size > 0 { Some(size) } else { None },
        label: filename.clone(),
    }];
    let task_id = state.next_task_id();
    let source = format!("CurseForge：{project_name}");
    crate::install::emit_progress(
        &app,
        task_id,
        "content",
        &format!("正在下载 {filename}…"),
        0,
        1,
        instance,
        &source,
    );
    if let Err(e) = crate::download::download_many(app.clone(), state, task_id, "content", items).await {
        let _ = app.emit(
            "install://progress",
            serde_json::json!({
                "taskId": task_id,
                "stage": "done",
                "message": format!("安装失败：{e}"),
                "done": 1,
                "total": 1,
                "instanceId": &instance.id,
                "instanceName": &instance.name,
                "source": &source,
                "ok": false,
            }),
        );
        return Err(e);
    }

    let slug = get(state, &format!("/mods/{mod_id}"), &[])
        .await
        .ok()
        .and_then(|b| {
            b.get("data")
                .and_then(|d| d.get("slug"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .filter(|s| !s.is_empty());

    let mut record = InstalledContent {
        filename: filename.clone(),
        source: "curseforge".into(),
        project_id: Some(mod_id.to_string()),
        slug: slug.clone(),
        version_id: Some(file_id.to_string()),
        name: Some(project_name.clone()),
        version: None,
        mod_id: None,
        authors: None,
        description: None,
        installed_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        size,
        icon: cf_mod_logo(state, mod_id).await,
        enabled: true,
    };
    let jar_path = state
        .instances_dir()
        .join(&instance.id)
        .join(crate::modrinth::kind_folder(&kind))
        .join(&filename);
    crate::util::fill_content_from_jar(&mut record, &jar_path);
    crate::instances::add_content(state, &instance.id, kind, record)?;
    crate::install::emit_progress(&app, task_id, "done", "安装完成", 1, 1, instance, &format!("CurseForge：{project_name}"));
    Ok(json!({ "ok": true, "filename": filename }))
}

/// Best-effort CurseForge mod logo URL.
async fn cf_mod_logo(state: &AppState, mod_id: &str) -> Option<String> {
    let body = get(state, &format!("/mods/{mod_id}"), &[]).await.ok()?;
    body.get("data")
        .and_then(|d| d.get("logo"))
        .and_then(|l| l.get("url"))
        .and_then(|u| u.as_str())
        .map(|s| s.to_string())
}

/// Install a CurseForge modpack (Forge-style zip with manifest.json).
pub async fn install_modpack(
    app: tauri::AppHandle,
    state: &AppState,
    modpack_id: &str,
    file_id: &str,
) -> Result<Value, String> {
    let task_id = state.next_task_id();
    let app_err = app.clone();
    let result = install_modpack_inner(app, state, task_id, modpack_id, file_id).await;
    if let Err(ref e) = result {
        let _ = app_err.emit(
            "install://progress",
            serde_json::json!({
                "taskId": task_id,
                "stage": "done",
                "message": format!("安装失败：{e}"),
                "done": 0,
                "total": 0,
                "instanceId": "",
                "instanceName": "",
                "source": "整合包安装",
                "ok": false,
            }),
        );
    }
    result
}

async fn install_modpack_inner(
    app: tauri::AppHandle,
    state: &AppState,
    task_id: u64,
    modpack_id: &str,
    file_id: &str,
) -> Result<Value, String> {
    let body = get(state, &format!("/mods/{modpack_id}/files/{file_id}"), &[]).await?;
    let file = body.get("data").ok_or("未找到该文件")?;
    let filename = file
        .get("fileName")
        .and_then(|v| v.as_str())
        .unwrap_or("modpack.zip")
        .to_string();
    let url = file_download_url(file);
    let size = file.get("fileLength").and_then(|v| v.as_u64()).unwrap_or(0);

    let dl_dir = state.root.join("runtimes");
    std::fs::create_dir_all(&dl_dir).map_err(|e| e.to_string())?;
    let pack_path = dl_dir.join(&filename);
    crate::util::fs_best_effort("remove_file", &pack_path, std::fs::remove_file(&pack_path));
    let items = vec![crate::download::DownloadItem {
        url,
        dest: pack_path.clone(),
        sha1: None,
        sha512: None,
        size: if size > 0 { Some(size) } else { None },
        label: filename.clone(),
    }];
    let source = "整合包：CurseForge".to_string();
    let placeholder = Instance {
        name: "CurseForge 整合包".into(),
        ..Default::default()
    };
    crate::install::emit_progress(
        &app,
        task_id,
        "modpack",
        &format!("正在下载整合包 {filename}…"),
        0,
        1,
        &placeholder,
        &source,
    );
    crate::download::download_many(app.clone(), state, task_id, "modpack", vec![items[0].clone()]).await?;

    // detect pack metadata and create a new instance
    let (pack_name, mc_version, loader, loader_version) =
        crate::modpack::detect(&pack_path).await
            .map_err(|e| format!("解析整合包失败: {e}（文件: {}）", pack_path.display()))?;
    let instance = crate::instances::create_instance(
        state,
        pack_name.clone(),
        mc_version,
        loader,
        if loader_version.is_empty() { None } else { Some(loader_version) },
    )?;
    let source = format!("整合包：{pack_name}");

    // Extract modpack icon: first look inside the zip, then fall back to the
    // project logo from the CurseForge API (CF zips don't embed an icon).
    let instance_dir = state.instances_dir().join(&instance.id);
    let mut icon_path = crate::util::extract_modpack_icon(&pack_path, &instance_dir);
    if icon_path.is_none() {
        if let Ok(info) = crate::curseforge::project_info(state, modpack_id).await {
            if let Some(u) = info.get("icon_url").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                icon_path = crate::util::download_icon(&state.client, u, &instance_dir).await;
            }
        }
    }
    if let Some(icon_path) = icon_path {
        let mut inst = instance.clone();
        inst.icon = Some(format!("img:{icon_path}"));
        let _ = crate::util::log_best_effort("save_instance", crate::instances::save_instance(state, &inst));
    }

    let manifest_bytes = crate::util::read_zip_entry(&pack_path, "manifest.json")
        .map_err(|e| format!("整合包缺少 manifest.json: {e}"))?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes).map_err(|e| e.to_string())?;
    let files = manifest.get("files").and_then(|f| f.as_array()).cloned().unwrap_or_default();
    let total_files = files.len();

    // Phase 1: fetch metadata for all mods, collect download items + records
    let mut dl_items: Vec<crate::download::DownloadItem> = Vec::new();
    let mut mod_records: Vec<InstalledContent> = Vec::new();
    let mut fetched = 0usize;
    for f in &files {
        let Some(pid) = f.get("projectID").and_then(|v| v.as_u64()) else { continue };
        let Some(fid) = f.get("fileID").and_then(|v| v.as_u64()) else { continue };
        fetched += 1;
        let _ = crate::install::emit_progress(
            &app,
            task_id,
            "modpack",
            &format!("正在获取模组信息…（{}/{total_files}）", fetched),
            fetched,
            total_files,
            &instance,
            &source,
        );
        let fbody = get(state, &format!("/mods/{pid}/files/{fid}"), &[]).await?;
        let Some(fdata) = fbody.get("data") else { continue };
        let fname = fdata.get("fileName").and_then(|v| v.as_str()).unwrap_or("mod.jar").to_string();
        if !fname.ends_with(".jar") {
            continue;
        }
        let fsize = fdata.get("fileLength").and_then(|v| v.as_u64()).unwrap_or(0);
        dl_items.push(crate::download::DownloadItem {
            url: file_download_url(fdata),
            dest: state
                .instances_dir()
                .join(&instance.id)
                .join("mods")
                .join(&fname),
            sha1: None,
            sha512: None,
            size: if fsize > 0 { Some(fsize) } else { None },
            label: fname.clone(),
        });
        let mut rec = InstalledContent {
            filename: fname.clone(),
            source: "curseforge".into(),
            project_id: Some(pid.to_string()),
            slug: None,
            version_id: Some(fid.to_string()),
            name: Some(fname),
            version: None,
            mod_id: None,
            authors: None,
            description: None,
            installed_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            size: fsize,
            icon: None,
            enabled: true,
        };
        let jar_path = state
            .instances_dir()
            .join(&instance.id)
            .join("mods")
            .join(&rec.filename);
        crate::util::fill_content_from_jar(&mut rec, &jar_path);
        mod_records.push(rec);
    }

    // Phase 2: download all mods in one batch (progress bar won't reset)
    let _ = crate::install::emit_progress(
        &app,
        task_id,
        "modpack",
        &format!("正在下载 {} 个模组…", dl_items.len()),
        0,
        dl_items.len(),
        &instance,
        &source,
    );
    crate::download::download_many(app.clone(), state, task_id, "modpack", dl_items).await?;

    // batch-add all mod records in one save
    let mods_count = mod_records.len();
    if !mod_records.is_empty() {
        if let Ok(mut inst) = crate::instances::get_instance(state, &instance.id) {
            for rec in mod_records {
                inst.mods.retain(|c| c.filename != rec.filename);
                inst.mods.push(rec);
            }
            if let Err(e) = crate::instances::save_instance(state, &inst) {
                eprintln!("[modpack] save_instance failed: {e}");
            }
        }
    }

    // extract overrides (apply with prefix stripped) + progress
    crate::install::emit_progress(
        &app,
        task_id,
        "modpack-install",
        "正在写入整合包文件…",
        0,
        1,
        &instance,
        &source,
    );
    let _ = crate::util::extract_zip(
        &pack_path,
        &instance_dir,
        &["manifest.json", "META-INF/", "overrides/"],
    )
    .map_err(|e| format!("解压整合包失败: {e}"))?;
    let _ = crate::util::extract_zip_strip(
        &pack_path,
        &instance_dir,
        "overrides/",
        &["manifest.json", "META-INF/"],
    )?;
    let overrides_dir = instance_dir.join("overrides");
    crate::util::fs_best_effort("remove_dir_all", &overrides_dir, std::fs::remove_dir_all(&overrides_dir));
    crate::install::emit_progress(
        &app,
        task_id,
        "modpack-install",
        "整合包文件已写入",
        1,
        1,
        &instance,
        &source,
    );

    // auto-install game files (client jar, libraries, assets...)
    let _ = crate::install::install_game(app.clone(), state, &instance).await;
    let _ = crate::util::log_best_effort("mark_installed", crate::instances::mark_installed(state, &instance.id));

    crate::install::emit_progress(
        &app,
        task_id,
        "done",
        "整合包安装完成",
        1,
        1,
        &instance,
        &source,
    );

    Ok(json!({ "ok": true, "mods": mods_count, "instanceId": instance.id }))
}
