use crate::models::{InstalledContent, Instance};
use crate::state::AppState;
use crate::util::file_sha1;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tauri::{Emitter, Manager};

/// Best-effort pack display name from the zip's index.json.
fn index_name_hint(pack_path: &Path) -> String {
    if let Ok(bytes) = crate::util::read_zip_entry(pack_path, "modrinth.index.json") {
        if let Ok(v) = serde_json::from_slice::<Value>(&bytes) {
            if let Some(n) = v.get("name").and_then(|n| n.as_str()) {
                return n.to_string();
            }
        }
    }
    pack_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "整合包".into())
}

const API: &str = "https://api.modrinth.com/v2";

/// Browse / search projects on Modrinth.
pub async fn search(
    state: &AppState,
    query: &str,
    project_type: &str,
    category: &str,
    index: &str,
    offset: usize,
    limit: usize,
    game_version: &str,
    loader: &str,
) -> Result<Value, String> {
    let mut facets: Vec<Value> = Vec::new();
    if !project_type.is_empty() {
        facets.push(json!(["project_type:".to_string() + project_type]));
    }
    if !category.is_empty() {
        facets.push(json!(["categories:".to_string() + category]));
    }
    if !game_version.is_empty() {
        facets.push(json!(["versions:".to_string() + game_version]));
    }
    if !loader.is_empty() {
        // Modrinth treats loaders as a category facet (e.g. categories:fabric)
        facets.push(json!(["categories:".to_string() + loader]));
    }
    // Modrinth expects `facets` to be a nested JSON array: [["project_type:mod"]]
    let facets_str = serde_json::to_string(&facets).map_err(|e| e.to_string())?;
    let url = format!(
        "{API}/search?query={}&limit={}&offset={}&index={}&facets={}",
        urlencode(query),
        limit,
        offset,
        index,
        urlencode(&facets_str)
    );
    let body: Value = crate::download::get_json(&state.client, &url).await?;
    let hits = body.get("hits").and_then(|h| h.as_array()).cloned().unwrap_or_default();
    let out: Vec<Value> = hits
        .iter()
        .map(|h| {
            let gallery = h.get("gallery").and_then(|v| v.as_array());
            let featured = gallery
                .and_then(|g| g.first())
                .and_then(|v| v.as_str())
                .unwrap_or("");
            json!({
                "provider": "modrinth",
                "id": h.get("project_id").and_then(|v| v.as_str()).unwrap_or(""),
                "slug": h.get("slug").and_then(|v| v.as_str()).unwrap_or(""),
                "title": h.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                "description": h.get("description").and_then(|v| v.as_str()).unwrap_or(""),
                "author": h.get("author").and_then(|v| v.as_str()).unwrap_or(""),
                "downloads": h.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0),
                "follows": h.get("follows").and_then(|v| v.as_u64()).unwrap_or(0),
                "icon_url": h.get("icon_url").and_then(|v| v.as_str()).unwrap_or(""),
                "project_type": h.get("project_type").and_then(|v| v.as_str()).unwrap_or("mod"),
                "categories": h.get("categories").and_then(|v| v.as_array()).cloned().unwrap_or_default(),
                "latest_version": h.get("latest_version").and_then(|v| v.as_str()).unwrap_or(""),
                "game_versions": h.get("game_versions").and_then(|v| v.as_array()).cloned().unwrap_or_default(),
                "updated": h.get("date_modified").and_then(|v| v.as_str()).unwrap_or(""),
                "featured_image": featured,
                "_sort_ts": h.get("date_modified").and_then(|v| v.as_str()).unwrap_or(""),
            })
        })
        .collect();
    Ok(json!({ "hits": out, "total": body.get("total_hits").and_then(|v| v.as_u64()).unwrap_or(0) }))
}

/// List versions for a project, optionally filtered by game version + loader.
pub async fn versions(
    state: &AppState,
    project_id: &str,
    mc_version: &str,
    loader: &str,
) -> Result<Vec<Value>, String> {
    let mut url = format!("{API}/project/{project_id}/version");
    let mut params: Vec<String> = Vec::new();
    if !mc_version.is_empty() {
        params.push(format!("game_versions={}", urlencode(&format!("[\"{mc_version}\"]"))));
    }
    if !loader.is_empty() {
        params.push(format!("loaders={}", urlencode(&format!("[\"{loader}\"]"))));
    }
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }
    let list: Vec<Value> = crate::download::get_json(&state.client, &url).await?;
    Ok(list
        .into_iter()
        .map(|v| {
            json!({
                "id": v.get("id").and_then(|x| x.as_str()).unwrap_or(""),
                "name": v.get("name").and_then(|x| x.as_str()).unwrap_or(""),
                "version_number": v.get("version_number").and_then(|x| x.as_str()).unwrap_or(""),
                "version_type": v.get("version_type").and_then(|x| x.as_str()).unwrap_or("release"),
                "date_published": v.get("date_published").and_then(|x| x.as_str()).unwrap_or(""),
                "game_versions": v.get("game_versions").and_then(|x| x.as_array()).cloned().unwrap_or_default(),
                "loaders": v.get("loaders").and_then(|x| x.as_array()).cloned().unwrap_or_default()
                // 不返回 files/dependencies：前端用版本 id 安装时后端会重新取文件，避免大量冗余传输
            })
        })
        .collect())
}

/// Enriched dependencies of a Modrinth version: project titles + slugs.
pub async fn dependencies(state: &AppState, version_id: &str) -> Result<Vec<Value>, String> {
    let ver = version(state, version_id).await?;
    let deps = ver
        .get("dependencies")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for dep in deps {
        let dtype = dep
            .get("dependency_type")
            .and_then(|d| d.as_str())
            .unwrap_or("required")
            .to_string();
        let fallback = dep
            .get("file_name")
            .and_then(|f| f.as_str())
            .unwrap_or("")
            .to_string();
        let Some(pid) = dep.get("project_id").and_then(|p| p.as_str()) else {
            continue;
        };
        // enrich with title + slug (best effort)
        let mut title = fallback;
        let mut slug = String::new();
        if let Ok(resp) = state.client.get(format!("{API}/project/{pid}")).send().await {
            if let Ok(p) = resp.json::<Value>().await {
                if let Some(t) = p.get("title").and_then(|t| t.as_str()) {
                    title = t.to_string();
                }
                if let Some(s) = p.get("slug").and_then(|s| s.as_str()) {
                    slug = s.to_string();
                }
            }
        }
        out.push(json!({
            "projectId": pid,
            "title": title,
            "slug": slug,
            "dependencyType": dtype,
        }));
    }
    Ok(out)
}

/// Get a single version by id.
/// 按文件哈希反查 Modrinth 上的版本（`GET /version_file/{hash}`）。
/// 同一个文件的哈希相同，所以这是**精确匹配**——不存在重名风险。
/// 返回 None 表示 Modrinth 上没有这个文件（本地改过的、只发布在 CurseForge 的、
/// 或作者自己编译的）。
pub async fn version_by_hash(state: &AppState, sha1: &str) -> Result<Option<Value>, String> {
    if sha1.is_empty() {
        return Ok(None);
    }
    let url = format!("{API}/version_file/{sha1}?algorithm=sha1");
    let res = state.client.get(&url).send().await;
    match res {
        Ok(resp) if resp.status().is_success() => {
            let v: Value = resp.json().await.map_err(|e| format!("解析响应失败: {e}"))?;
            Ok(Some(v))
        }
        Ok(resp) if resp.status().as_u16() == 404 => Ok(None),
        Ok(resp) => Err(format!("Modrinth 查询失败: HTTP {}", resp.status())),
        Err(e) => Err(format!("Modrinth 请求失败: {e}")),
    }
}

pub async fn version(state: &AppState, version_id: &str) -> Result<Value, String> {
    crate::download::get_json(&state.client, &format!("{API}/version/{version_id}")).await
}

/// Primary downloadable file of a version (prefer `primary`, fallback first).
pub fn primary_file(version: &Value) -> Option<(String, String, u64, Option<String>)> {
    let files = version.get("files").and_then(|f| f.as_array())?;
    let primary = files.iter().find(|f| f.get("primary").and_then(|p| p.as_bool()).unwrap_or(false));
    let chosen = primary.or_else(|| files.first())?;
    let url = chosen.get("url").and_then(|u| u.as_str())?.to_string();
    let filename = chosen.get("filename").and_then(|u| u.as_str())?.to_string();
    let size = chosen.get("size").and_then(|u| u.as_u64()).unwrap_or(0);
    let sha1 = chosen
        .get("hashes")
        .and_then(|h| h.get("sha1"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    Some((url, filename, size, sha1))
}

/// A path taken from a modpack index is only acceptable if it is a normal,
/// forward-slash relative path with no `.` / `..` segments, no absolute prefix
/// and no drive-letter prefix. Used to block zip-slip-style escapes before any
/// file is written from pack metadata.
pub fn safe_pack_rel_path(p: &str) -> bool {
    if p.starts_with('/') || p.starts_with('\\') {
        return false;
    }
    // drive letter prefix, e.g. `C:\...` or `C:/...`
    let bytes = p.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return false;
    }
    p.split(['/', '\\']).all(|seg| !seg.is_empty() && seg != "." && seg != "..")
}

/// Folder name inside an instance for a content kind.
pub fn kind_folder(kind: &str) -> &'static str {
    match kind {
        "resourcepack" => "resourcepacks",
        "shader" => "shaderpacks",
        "datapack" => "datapacks",
        _ => "mods",
    }
}

/// 解析 mrpack 的 `modrinth.index.json`：
/// - 产出下载项（目标路径 = `instance_dir/<path>`）
/// - 产出「`mods/` 下的文件名 → sha1」映射（供后续按哈希反查项目来源）
///
/// `path` 来自不可信的包索引，会做路径穿越校验（见 [`safe_pack_rel_path`]）。
pub fn parse_index_files(
    index: &Value,
    instance_dir: &std::path::Path,
) -> Result<(Vec<crate::download::DownloadItem>, std::collections::HashMap<String, String>), String> {
    let files = index.get("files").and_then(|f| f.as_array()).cloned().unwrap_or_default();
    let mut items = Vec::new();
    let mut hash_by_name: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for f in &files {
        let path = f.get("path").and_then(|p| p.as_str()).unwrap_or("").to_string();
        if path.is_empty() || path.ends_with('/') {
            continue;
        }
        // Path traversal guard: `path` comes from the (untrusted) pack index and
        // is joined onto the instance dir for downloads. Reject anything that is
        // absolute, drive-qualified, or contains `.` / `..` segments.
        if !safe_pack_rel_path(&path) {
            return Err(format!("整合包包含非法文件路径: {path}"));
        }
        let downloads = f.get("downloads").and_then(|d| d.as_array()).cloned().unwrap_or_default();
        let Some(first) = downloads.first().and_then(|d| d.as_str()) else { continue };
        let hashes = f.get("hashes").and_then(|h| h.as_object()).cloned().unwrap_or_default();
        let sha1 = hashes.get("sha1").and_then(|v| v.as_str()).map(|s| s.to_string());
        let sha512 = hashes.get("sha512").and_then(|v| v.as_str()).map(|s| s.to_string());
        if path.starts_with("mods/") {
            if let Some(h) = &sha1 {
                let fname = std::path::Path::new(&path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                if !fname.is_empty() {
                    hash_by_name.insert(fname, h.clone());
                }
            }
        }
        let size = f.get("fileSize").and_then(|v| v.as_u64()).unwrap_or(0);
        items.push(crate::download::DownloadItem {
            url: first.to_string(),
            dest: instance_dir.join(&path),
            sha1,
            sha512,
            size: if size > 0 { Some(size) } else { None },
            label: path,
        });
    }
    Ok((items, hash_by_name))
}

/// Install a project version (mod / resourcepack / shader / modpack) into an instance.
pub async fn install_version(
    app: tauri::AppHandle,
    state: &AppState,
    instance: &Instance,
    version_id: &str,
    kind: &str,
) -> Result<Value, String> {
    let ver = version(state, version_id).await?;
    let project_id = ver
        .get("project_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let project_name = ver
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let version_number = ver
        .get("version_number")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if kind == "modpack" {
        return install_modpack(app, state, &ver).await;
    }

    let (url, filename, size, sha1) = primary_file(&ver).ok_or("该版本没有可下载的文件")?;
    // `filename` comes from the Modrinth API (untrusted); ensure it is a plain
    // file name so it can never escape the content folder via traversal.
    if !crate::util::is_safe_filename(&filename) {
        return Err(format!("文件名为非法路径: {filename}"));
    }
    let dest = state
        .instances_dir()
        .join(&instance.id)
        .join(kind_folder(kind))
        .join(&filename);

    let items = vec![crate::download::DownloadItem {
        url,
        dest: dest.clone(),
        sha1,
        sha512: None,
        size: Some(size),
        label: filename.clone(),
    }];
    let task_id = state.next_task_id();
    let source = format!("Modrinth：{project_name}（{version_number}）");
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

    let slug = project_info(state, &project_id)
        .await
        .ok()
        .and_then(|p| {
            p.get("slug")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .filter(|s| !s.is_empty());

    let record = InstalledContent {
        filename: filename.clone(),
        source: "modrinth".into(),
        project_id: Some(project_id.clone()),
        slug: slug.clone(),
        version_id: Some(version_id.to_string()),
        name: Some(project_name.clone()),
        version: Some(version_number.clone()),
        mod_id: None,
        authors: None,
        description: None,
        installed_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        size,
        icon: project_icon(&state, &project_id).await,
        enabled: true,
    };
    crate::instances::add_content(state, &instance.id, kind, record)?;
    crate::install::emit_progress(&app, task_id, "done", "安装完成", 1, 1, instance, &format!("Modrinth：{project_name}（{version_number}）"));
    Ok(json!({ "ok": true, "filename": filename }))
}

/// Fabric API 在 Modrinth 上的项目 ID（全球唯一、长期不变）
pub const FABRIC_API_PROJECT: &str = "P7dR8mSH";

/// Fabric 实例创建后自动补装 Fabric API（best-effort，后台执行）。
///
/// 绝大多数 Fabric 模组都依赖 Fabric API，不装的话游戏内/加载时会成片报错，
/// 因此创建 Fabric（及兼容的 Quilt）实例时自动按 MC 版本匹配最新 release 安装。
/// 已手动装过（mods 里有 fabric-api 的任意形式）时不重复安装；
/// 失败只记录日志，不影响实例本身的使用。
pub async fn auto_install_fabric_api(
    app: &tauri::AppHandle,
    instance_id: &str,
    mc_version: &str,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let instance = crate::instances::get_instance(&state, instance_id)?;

    // 已有 Fabric API 就不再装（按项目 id / slug / 文件名多种形态匹配）
    let has_api = instance.mods.iter().any(|m| {
        m.project_id.as_deref() == Some(FABRIC_API_PROJECT)
            || m.slug.as_deref() == Some("fabric-api")
            || m.filename.to_lowercase().starts_with("fabric-api")
            || m.name
                .as_deref()
                .map(|n| n.to_lowercase().contains("fabric api"))
                .unwrap_or(false)
    });
    if has_api {
        return Ok(());
    }

    // 匹配当前 MC 版本、fabric 加载器的最新 release（没有 release 时退回第一个）
    let list = versions(&state, FABRIC_API_PROJECT, mc_version, "fabric").await?;
    let ver_id = list
        .iter()
        .find(|v| v.get("version_type").and_then(|x| x.as_str()) == Some("release"))
        .or_else(|| list.first())
        .and_then(|v| v.get("id").and_then(|x| x.as_str()))
        .ok_or_else(|| format!("未找到适配 Minecraft {mc_version} 的 Fabric API 版本"))?
        .to_string();

    install_version(app.clone(), &state, &instance, &ver_id, "mod").await?;
    Ok(())
}

/// Best-effort project icon URL (Modrinth).
async fn project_icon(state: &AppState, project_id: &str) -> Option<String> {
    let resp = state
        .client
        .get(format!("{API}/project/{project_id}"))
        .send()
        .await
        .ok()?;
    let body: Value = resp.json().await.ok()?;
    body.get("icon_url").and_then(|i| i.as_str()).map(|s| s.to_string())
}

/// Fetch a single project's full info (used when opening a dependency).
pub async fn project_info(state: &AppState, project_id: &str) -> Result<Value, String> {
    let url = format!("{API}/project/{project_id}");
    let body: Value = crate::download::get_json(&state.client, &url).await?;
    Ok(json!({
        "provider": "modrinth",
        "id": body.get("id").and_then(|v| v.as_str()).unwrap_or(project_id),
        "slug": body.get("slug").and_then(|v| v.as_str()).unwrap_or(""),
        "title": body.get("title").and_then(|v| v.as_str()).unwrap_or(""),
        "description": body.get("description").and_then(|v| v.as_str()).unwrap_or(""),
        "body": body.get("body").and_then(|v| v.as_str()).unwrap_or(""),
        "author": "",
        "downloads": body.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0),
        "follows": body.get("follows").and_then(|v| v.as_u64()).unwrap_or(0),
        "icon_url": body.get("icon_url").and_then(|v| v.as_str()).unwrap_or(""),
        "project_type": body.get("project_type").and_then(|v| v.as_str()).unwrap_or("mod"),
        "categories": body.get("categories").and_then(|v| v.as_array()).cloned().unwrap_or_default(),
        "latest_version": body.get("latest_version").and_then(|v| v.as_str()).unwrap_or(""),
        "game_versions": body.get("game_versions").and_then(|v| v.as_array()).cloned().unwrap_or_default(),
    }))
}

/// Fetch project members (authors). Returns a list of usernames.
pub async fn project_authors(state: &AppState, project_id: &str) -> Vec<String> {
    let url = format!("{API}/project/{project_id}/members");
    let resp = match state.client.get(&url).send().await {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let body: Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    if let Some(arr) = body.as_array() {
        for m in arr {
            if let Some(name) = m.get("user").and_then(|u| u.get("username")).and_then(|n| n.as_str()) {
                out.push(name.to_string());
            }
        }
    }
    out
}

/// Batch-resolve project_id/version_id from sha1 hashes via the version_files API.
/// Returns a map: sha1 -> (project_id, version_id). Best-effort, empty on failure.
pub async fn resolve_by_hashes(
    state: &AppState,
    hashes: &[String],
) -> std::collections::HashMap<String, (String, String)> {
    let mut out = std::collections::HashMap::new();
    if hashes.is_empty() {
        return out;
    }
    let body = json!({ "hashes": hashes, "algorithm": "sha1" });
    let resp = match state.client.post(format!("{API}/version_files")).json(&body).send().await {
        Ok(r) => r,
        Err(_) => return out,
    };
    let map: Value = match resp.json().await {
        Ok(m) => m,
        Err(_) => return out,
    };
    if let Some(obj) = map.as_object() {
        for (hash, ver) in obj {
            let pid = ver.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
            let vid = ver.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if !pid.is_empty() && !vid.is_empty() {
                out.insert(hash.clone(), (pid.to_string(), vid.to_string()));
            }
        }
    }
    out
}

/// Install a Modrinth modpack (.mrpack) into an instance.
pub async fn install_modpack(
    app: tauri::AppHandle,
    state: &AppState,
    ver: &Value,
) -> Result<Value, String> {
    let task_id = state.next_task_id();
    let app_err = app.clone();
    let result = install_modpack_inner(app, state, task_id, ver).await;
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
    ver: &Value,
) -> Result<Value, String> {
    let (url, filename, size, _) = primary_file(ver).ok_or("该版本没有可下载的文件")?;
    let dl_dir = state.root.join("runtimes");
    std::fs::create_dir_all(&dl_dir).map_err(|e| e.to_string())?;
    let pack_path = dl_dir.join(&filename);
    let _ = std::fs::remove_file(&pack_path);
    let items = vec![crate::download::DownloadItem {
        url,
        dest: pack_path.clone(),
        sha1: None,
        sha512: None,
        size: Some(size),
        label: filename.clone(),
    }];
    let pack_name = index_name_hint(&pack_path);
    let source = format!("整合包：{pack_name}");
    let placeholder = Instance {
        name: pack_name.clone(),
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

    // ---- 公共骨架第一步：建实例 + 图标（mrpack 通常不内嵌图标，用项目图标兜底）----
    let project_id = ver.get("project_id").and_then(|v| v.as_str()).unwrap_or("");
    let icon_fallback_url = if project_id.is_empty() {
        None
    } else {
        crate::modrinth::project_info(state, project_id)
            .await
            .ok()
            .and_then(|info| info.get("icon_url").and_then(|v| v.as_str()).map(|s| s.to_string()))
    };
    let (instance, source) = crate::modpack::prepare_pack(
        state,
        crate::modpack::PackPrepareSpec {
            pack_path: pack_path.clone(),
            icon_fallback_url,
        },
    )
    .await?;

    let index_bytes = crate::util::read_zip_entry(&pack_path, "modrinth.index.json")
        .map_err(|e| format!("整合包缺少 modrinth.index.json: {e}"))?;
    let index: Value = serde_json::from_slice(&index_bytes).map_err(|e| e.to_string())?;

    // 纯函数解析，便于单测（路径穿越防护、mods/ 归类等）
    let (items, hash_by_name) = parse_index_files(&index, &state.instances_dir().join(&instance.id))?;
    // ---- 公共骨架第二步：下载内容 → 扫盘登记 → 解压 overrides →
    //      安装游戏本体 → 标记已安装 ----
    // 先按文件哈希批量反查 Project/Version，供扫盘登记时补上在线来源信息
    let resolved = resolve_by_hashes(state, &hash_by_name.values().cloned().collect::<Vec<_>>()).await;
    let mut hash_meta: std::collections::HashMap<String, (String, String)> =
        std::collections::HashMap::new();
    for (fname, h) in &hash_by_name {
        if let Some((pid, vid)) = resolved.get(h) {
            hash_meta.insert(fname.clone(), (pid.clone(), vid.clone()));
        }
    }
    let overrides = index
        .get("overrides")
        .and_then(|v| v.as_str())
        .unwrap_or("overrides")
        .to_string();
    let file_count = items.len();
    let count = crate::modpack::install_pack_contents(
        &app,
        state,
        &instance,
        &pack_path,
        crate::modpack::PackContentsSpec {
            task_id,
            source: source.clone(),
            download_label: format!("正在下载 {file_count} 个文件…"),
            downloads: items,
            records: Vec::new(),
            hash_meta,
            skip_prefixes: vec!["modrinth.index.json".into(), "META-INF/".into()],
            overrides_prefix: overrides,
        },
    )
    .await?;

    crate::install::emit_progress(&app, task_id, "done", "整合包安装完成", 1, 1, &instance, &source);
    Ok(json!({ "ok": true, "files": file_count, "mods": count, "instanceId": instance.id }))
}

/// Check installed Modrinth content for updates. Returns a list of
/// `{filename, projectId, currentVersion, latestVersion, latestVersionId, projectTitle}`.
pub async fn check_updates(
    state: &AppState,
    instance: &Instance,
    kind: &str,
) -> Result<Vec<Value>, String> {
    let list = crate::instances::list_content(state, &instance.id, kind);
    let loader = instance.loader.as_str();
    let mut updates = Vec::new();
    for item in list {
        if item.source != "modrinth" {
            continue;
        }
        let (Some(project_id), Some(_current)) = (item.project_id.as_deref(), item.version_id.as_deref()) else {
            continue;
        };
        let vlist = versions(state, project_id, &instance.mc_version, loader).await?;
        if let Some(latest) = vlist.first() {
            let latest_id = latest.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if !latest_id.is_empty() && latest_id != item.version_id.as_deref().unwrap_or("") {
                updates.push(json!({
                    "filename": item.filename,
                    "projectId": project_id,
                    "currentVersion": item.version,
                    "latestVersion": latest.get("version_number").and_then(|v| v.as_str()).unwrap_or(""),
                    "latestVersionId": latest_id,
                    "projectTitle": item.name,
                    "kind": kind,
                    "provider": "modrinth",
                    }));
            }
        }
    }
    Ok(updates)
}

pub fn urlencode(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}

/// Remove an installed content file and its record.
pub fn uninstall(state: &AppState, instance: &Instance, kind: &str, filename: &str) -> Result<(), String> {
    // `filename` originates from the Modrinth API / pack metadata (untrusted);
    // block path traversal before it is joined onto the instance folder.
    if !crate::util::is_safe_filename(filename) {
        return Err("非法文件名".into());
    }
    let dir: PathBuf = state
        .instances_dir()
        .join(&instance.id)
        .join(kind_folder(kind));
    let _ = std::fs::remove_file(dir.join(filename));
    let _ = std::fs::remove_file(dir.join(format!("{filename}.disabled")));
    crate::instances::remove_content(state, &instance.id, kind, filename)
}

/// Verify a downloaded file matches expected sha1 if provided.
#[allow(dead_code)]
pub fn verify_sha1(path: &PathBuf, expected: Option<&str>) -> bool {
    match (expected, file_sha1(path)) {
        (Some(e), Some(a)) => a.eq_ignore_ascii_case(e),
        (None, _) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn index_with(files: serde_json::Value) -> Value {
        json!({ "formatVersion": 1, "name": "测试包", "files": files })
    }

    /// 正常解析：下载项目标路径、哈希、mods/ 归类
    #[test]
    fn parse_index_files_builds_downloads_and_hash_map() {
        let dir = std::path::Path::new("I:/instances/test01");
        let idx = index_with(json!([
            {
                "path": "mods/sodium.jar",
                "downloads": ["https://cdn.example/sodium.jar"],
                "hashes": { "sha1": "AAA111", "sha512": "BBB222" },
                "fileSize": 123456
            },
            {
                "path": "config/sodium-options.json",
                "downloads": ["https://cdn.example/opts.json"],
                "hashes": { "sha1": "CCC333" }
            }
        ]));
        let (items, hashes) = parse_index_files(&idx, dir).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(hashes.len(), 1, "只有 mods/ 下的文件才进哈希表");
        assert_eq!(hashes.get("sodium.jar").map(|s| s.as_str()), Some("AAA111"));

        let mod_item = items.iter().find(|i| i.label == "mods/sodium.jar").unwrap();
        assert!(mod_item.dest.ends_with("mods/sodium.jar"), "dest 拼错: {:?}", mod_item.dest);
        assert_eq!(mod_item.url, "https://cdn.example/sodium.jar");
        assert_eq!(mod_item.sha1.as_deref(), Some("AAA111"));
        assert_eq!(mod_item.sha512.as_deref(), Some("BBB222"));
        assert_eq!(mod_item.size, Some(123456));

        let cfg_item = items.iter().find(|i| i.label == "config/sodium-options.json").unwrap();
        assert!(cfg_item.dest.ends_with("config/sodium-options.json"));
        assert_eq!(cfg_item.size, None, "fileSize 缺失时不应写入 size");
    }

    /// 路径穿越必须被拒绝（这些 path 会被拼到实例目录下）
    #[test]
    fn parse_index_files_rejects_traversal_paths() {
        let dir = std::path::Path::new("I:/instances/test01");
        for bad in [
            "../evil.jar",
            "mods/../../evil.jar",
            "/etc/passwd",
            "\\windows\\system32\\evil.dll",
            "C:/windows/evil.dll",
            "mods/./evil.jar",
        ] {
            let idx = index_with(json!([{
                "path": bad,
                "downloads": ["https://cdn.example/x.jar"],
                "hashes": { "sha1": "X" }
            }]));
            let r = parse_index_files(&idx, dir);
            assert!(r.is_err(), "非法路径未被拒绝: {bad}");
        }
    }

    /// 目录条目、空路径、无下载地址的条目都应被跳过而不是报错
    #[test]
    fn parse_index_files_skips_unusable_entries() {
        let dir = std::path::Path::new("I:/instances/test01");
        let idx = index_with(json!([
            { "path": "mods/", "downloads": ["https://cdn.example/dir/"], "hashes": {} },
            { "path": "", "downloads": ["https://cdn.example/empty"], "hashes": {} },
            { "path": "mods/no-download.jar", "hashes": { "sha1": "D" } },
            {
                "path": "mods/ok.jar",
                "downloads": ["https://cdn.example/ok.jar"],
                "hashes": { "sha1": "E" }
            }
        ]));
        let (items, hashes) = parse_index_files(&idx, dir).unwrap();
        assert_eq!(items.len(), 1, "只应保留唯一可下载的条目");
        assert_eq!(items[0].label, "mods/ok.jar");
        assert!(hashes.contains_key("ok.jar"));
    }

    /// 空 files 不应报错
    #[test]
    fn parse_index_files_handles_missing_files() {
        let dir = std::path::Path::new("I:/instances/test01");
        let (items, hashes) = parse_index_files(&json!({ "name": "x" }), dir).unwrap();
        assert!(items.is_empty());
        assert!(hashes.is_empty());
    }
}
