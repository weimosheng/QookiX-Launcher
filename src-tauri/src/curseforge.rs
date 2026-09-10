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
/// 从 CurseForge `manifest.json` 提取去重后的 (projectID, fileID) 列表。
/// 缺失 id 的条目直接跳过——整合包里偶尔会混入格式不完整的条目。
pub fn manifest_file_pairs(manifest: &Value) -> Vec<(u64, u64)> {
    let files = manifest.get("files").and_then(|f| f.as_array()).cloned().unwrap_or_default();
    let mut seen = std::collections::HashSet::<(u64, u64)>::new();
    let mut out = Vec::new();
    for f in &files {
        let Some(pid) = f.get("projectID").and_then(|v| v.as_u64()) else { continue };
        let Some(fid) = f.get("fileID").and_then(|v| v.as_u64()) else { continue };
        if seen.insert((pid, fid)) {
            out.push((pid, fid));
        }
    }
    out
}

/// 把已取到的文件元数据构造成下载项与内容记录（纯函数，便于单测）。
/// - 只处理 `.jar`（CF 整合包 manifest 里偶尔会有非 jar 条目）
/// - 下载地址走 [`file_download_url`]（downloadUrl 为空时按 fileId 拼 CDN 地址）
pub fn build_manifest_contents(
    metas: &[(u64, u64, Value)],
    mods_dir: &std::path::Path,
) -> (Vec<crate::download::DownloadItem>, Vec<InstalledContent>) {
    let mut items = Vec::new();
    let mut records = Vec::new();
    for (pid, fid, fdata) in metas {
        let fname = fdata.get("fileName").and_then(|v| v.as_str()).unwrap_or("mod.jar").to_string();
        if !fname.ends_with(".jar") {
            continue;
        }
        let fsize = fdata.get("fileLength").and_then(|v| v.as_u64()).unwrap_or(0);
        items.push(crate::download::DownloadItem {
            url: file_download_url(fdata),
            dest: mods_dir.join(&fname),
            sha1: None,
            sha512: None,
            size: if fsize > 0 { Some(fsize) } else { None },
            label: fname.clone(),
        });
        records.push(InstalledContent {
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
        });
    }
    (items, records)
}

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
    crate::util::log_line(&format!("[cf_modpack] 开始安装 modpack={modpack_id} file={file_id} task={task_id}"));
    let result = install_modpack_inner(app, state, task_id, modpack_id, file_id).await;
    crate::util::log_line(&format!("[cf_modpack] 结束 task={task_id}，结果: {:?}", result.as_ref().map(|_| "Ok")));
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
    crate::util::log_line(&format!("[cf_modpack] 文件元数据 OK: {filename} url={url} size={size}"));

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
    crate::util::log_line("[cf_modpack] 整包 zip 下载完成");

    // ---- 公共骨架第一步：建实例 + 图标 ----
    let icon_fallback_url = crate::curseforge::project_info(state, modpack_id)
        .await
        .ok()
        .and_then(|info| info.get("icon_url").and_then(|v| v.as_str()).map(|s| s.to_string()));
    let (instance, source) = crate::modpack::prepare_pack(
        state,
        crate::modpack::PackPrepareSpec {
            pack_path: pack_path.clone(),
            icon_fallback_url,
        },
    )
    .await?;

    // ---- 数据源差异部分：解析 CurseForge manifest，收集下载项与内容记录 ----
    let manifest_bytes = crate::util::read_zip_entry(&pack_path, "manifest.json")
        .map_err(|e| format!("整合包缺少 manifest.json: {e}"))?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes).map_err(|e| e.to_string())?;
    let files = manifest.get("files").and_then(|f| f.as_array()).cloned().unwrap_or_default();
    let total_files = files.len();
    crate::util::log_line(&format!("[cf_modpack] manifest 含 {total_files} 个文件"));

    // Phase 1: 逐个文件取元数据（网络部分），再交给纯函数构造清单
    let pairs = manifest_file_pairs(&manifest);
    let mut metas: Vec<(u64, u64, Value)> = Vec::with_capacity(pairs.len());
    for (idx, (pid, fid)) in pairs.iter().enumerate() {
        let _ = crate::install::emit_progress(
            &app,
            task_id,
            "modpack",
            &format!("正在获取模组信息…（{}/{total_files}）", idx + 1),
            idx + 1,
            total_files,
            &instance,
            &source,
        );
        let fbody = get(state, &format!("/mods/{pid}/files/{fid}"), &[]).await?;
        if let Some(fdata) = fbody.get("data") {
            metas.push((*pid, *fid, fdata.clone()));
        }
    }
    let mods_dir = state.instances_dir().join(&instance.id).join("mods");
    let (dl_items, mod_records) = build_manifest_contents(&metas, &mods_dir);

    // ---- 公共骨架第二步：下载内容 → 登记记录 → 解压 overrides →
    //      安装游戏本体 → 标记已安装（与 Modrinth 整合包共用同一实现）----
    let mods_count = mod_records.len();
    let _ = crate::modpack::install_pack_contents(
        &app,
        state,
        &instance,
        &pack_path,
        crate::modpack::PackContentsSpec {
            task_id,
            source: source.clone(),
            download_label: format!("正在下载 {} 个模组…", dl_items.len()),
            downloads: dl_items,
            records: mod_records.into_iter().map(|r| ("mod".to_string(), r)).collect(),
            hash_meta: Default::default(),
            skip_prefixes: vec!["manifest.json".into(), "META-INF/".into()],
            overrides_prefix: "overrides".into(),
        },
    )
    .await?;
    crate::util::log_line(&format!("[cf_modpack] 整合包安装完成，实例 {}", instance.id));

    crate::install::emit_progress(&app, task_id, "done", "整合包安装完成", 1, 1, &instance, &source);
    Ok(json!({ "ok": true, "mods": mods_count, "instanceId": instance.id }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn manifest_pairs_dedupes_and_skips_incomplete() {
        let manifest = json!({
            "name": "测试包",
            "files": [
                { "projectID": 238222, "fileID": 1234567 },
                { "projectID": 238222, "fileID": 1234567 },   // 重复
                { "projectID": 394468, "fileID": 7654321, "required": true },
                { "projectID": 999999 },                       // 缺 fileID
                { "fileID": 111111 },                          // 缺 projectID
                { "projectID": 306770, "fileID": 2222222 }
            ]
        });
        let pairs = manifest_file_pairs(&manifest);
        assert_eq!(pairs.len(), 3, "应去重且跳过不完整条目: {pairs:?}");
        assert_eq!(pairs[0], (238222, 1234567));
        assert_eq!(pairs[1], (394468, 7654321));
        assert_eq!(pairs[2], (306770, 2222222));
    }

    #[test]
    fn manifest_pairs_handles_missing_files_key() {
        assert!(manifest_file_pairs(&json!({ "name": "x" })).is_empty());
        assert!(manifest_file_pairs(&json!({ "files": [] })).is_empty());
    }

    #[test]
    fn build_contents_filters_non_jar_and_fills_records() {
        let mods_dir = std::path::Path::new("I:/instances/test01/mods");
        let metas = vec![
            (
                238222u64,
                1234567u64,
                json!({
                    "id": 1234567,
                    "fileName": "jei-1.20.1.jar",
                    "fileLength": 2048,
                    "downloadUrl": "https://edge.forgecdn.net/files/1205/567/jei.jar"
                }),
            ),
            // 非 jar（配置文件等）：不该进入下载项
            (
                111u64,
                222u64,
                json!({ "id": 222, "fileName": "readme.txt", "fileLength": 10 }),
            ),
            // downloadUrl 为空：应按 fileId 拼出 CDN 地址
            (
                333u64,
                4444444u64,
                json!({ "id": 4444444, "fileName": "fallback.jar", "fileLength": 0 }),
            ),
        ];
        let (items, records) = build_manifest_contents(&metas, mods_dir);
        assert_eq!(items.len(), 2, "非 jar 应被过滤: {:?}", items.iter().map(|i| &i.label).collect::<Vec<_>>());
        assert_eq!(records.len(), 2);

        let jei = items.iter().find(|i| i.label == "jei-1.20.1.jar").unwrap();
        assert_eq!(jei.url, "https://edge.forgecdn.net/files/1205/567/jei.jar");
        assert_eq!(jei.size, Some(2048));
        assert!(jei.dest.ends_with("mods/jei-1.20.1.jar"), "dest 拼错: {:?}", jei.dest);

        let fb = items.iter().find(|i| i.label == "fallback.jar").unwrap();
        assert!(
            fb.url.starts_with("https://edge.forgecdn.net/files/4444/444/"),
            "downloadUrl 为空时应按 fileId 拼 CDN 地址，实际: {}",
            fb.url
        );
        assert_eq!(fb.size, None, "fileLength 为 0 时不应写入 size");

        let rec = records.iter().find(|r| r.filename == "jei-1.20.1.jar").unwrap();
        assert_eq!(rec.source, "curseforge");
        assert_eq!(rec.project_id.as_deref(), Some("238222"));
        assert_eq!(rec.version_id.as_deref(), Some("1234567"));
        assert_eq!(rec.size, 2048);
        assert!(rec.enabled);
    }
}

