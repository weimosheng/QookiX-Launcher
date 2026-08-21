use crate::models::{InstalledContent, Instance, LoaderType};
use crate::state::AppState;
use crate::util::{extract_zip, extract_zip_strip, file_sha1};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tauri::Emitter;

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

/// Folder name inside an instance for a content kind.
pub fn kind_folder(kind: &str) -> &'static str {
    match kind {
        "resourcepack" => "resourcepacks",
        "shader" => "shaderpacks",
        "datapack" => "datapacks",
        _ => "mods",
    }
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
    let dest = state
        .instances_dir()
        .join(&instance.id)
        .join(kind_folder(kind))
        .join(&filename);

    let items = vec![crate::download::DownloadItem {
        url,
        dest: dest.clone(),
        sha1,
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
    crate::download::download_many(app.clone(), state, task_id, "content", items).await?;

    let record = InstalledContent {
        filename: filename.clone(),
        source: "modrinth".into(),
        project_id: Some(project_id.clone()),
        version_id: Some(version_id.to_string()),
        name: Some(project_name.clone()),
        version: Some(version_number.clone()),
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
        size: Some(size),
        label: filename.clone(),
    }];
    let pack_name = index_name_hint(&pack_path);
    let source = format!("整合包：{pack_name}");
    let placeholder = Instance {
        id: String::new(),
        name: pack_name.clone(),
        mc_version: String::new(),
        loader: LoaderType::Vanilla,
        loader_version: None,
        created: 0,
        last_played: None,
        installed: false,
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

    // read modrinth.index.json and detect pack metadata
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

    // Extract modpack icon if available
    let instance_dir = state.instances_dir().join(&instance.id);
    if let Some(icon_path) = crate::util::extract_modpack_icon(&pack_path, &instance_dir) {
        let mut inst = instance.clone();
        inst.icon = Some(format!("img:{icon_path}"));
        let _ = crate::instances::save_instance(state, &inst);
    }

    let index_bytes = crate::util::read_zip_entry(&pack_path, "modrinth.index.json")
        .map_err(|e| format!("整合包缺少 modrinth.index.json: {e}"))?;
    let index: Value = serde_json::from_slice(&index_bytes).map_err(|e| e.to_string())?;

    let files = index.get("files").and_then(|f| f.as_array()).cloned().unwrap_or_default();
    let mut items = Vec::new();
    for f in &files {
        let path = f.get("path").and_then(|p| p.as_str()).unwrap_or("").to_string();
        if path.is_empty() || path.ends_with('/') {
            continue;
        }
        let downloads = f.get("downloads").and_then(|d| d.as_array()).cloned().unwrap_or_default();
        let Some(first) = downloads.first().and_then(|d| d.as_str()) else { continue };
        let hashes = f.get("hashes").and_then(|h| h.as_object()).cloned().unwrap_or_default();
        let sha1 = hashes.get("sha1").and_then(|v| v.as_str()).map(|s| s.to_string());
        let sha512 = hashes.get("sha512").and_then(|v| v.as_str()).map(|s| s.to_string());
        let dest = state.instances_dir().join(&instance.id).join(&path);
        let size = f.get("fileSize").and_then(|v| v.as_u64()).unwrap_or(0);
        items.push(crate::download::DownloadItem {
            url: first.to_string(),
            dest,
            sha1: sha1.or(sha512),
            size: if size > 0 { Some(size) } else { None },
            label: path,
        });
    }
    let file_count = items.len();
    crate::install::emit_progress(
        &app,
        task_id,
        "modpack",
        &format!("正在下载 {} 个文件…", file_count),
        0,
        file_count,
        &instance,
        &source,
    );
    crate::download::download_many(app.clone(), state, task_id, "modpack", items).await?;

    // extract overrides
    let overrides = index
        .get("overrides")
        .and_then(|v| v.as_str())
        .unwrap_or("overrides")
        .to_string();
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
    // regular files (skip the pack metadata + the overrides prefix)
    let _ = extract_zip(
        &pack_path,
        &instance_dir,
        &["modrinth.index.json", "META-INF/", &format!("{overrides}/")],
    )
    .map_err(|e| format!("解压整合包失败: {e}"))?;
    // apply overrides with the prefix stripped into the instance root
    let _ = extract_zip_strip(
        &pack_path,
        &instance_dir,
        &format!("{overrides}/"),
        &["modrinth.index.json", "META-INF/"],
    )?;
    let _ = std::fs::remove_dir_all(instance_dir.join(&overrides));
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

    // record installed mods — scan disk and batch-add in one save
    let mods_dir = state.instances_dir().join(&instance.id).join("mods");
    let mut count = 0usize;
    if let Ok(mut inst) = crate::instances::get_instance(state, &instance.id) {
        if let Ok(entries) = std::fs::read_dir(&mods_dir) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if !name.ends_with(".jar") {
                    continue;
                }
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                let rec = InstalledContent {
                    filename: name.clone(),
                    source: "modrinth".into(),
                    project_id: None,
                    version_id: None,
                    name: Some(name),
                    version: None,
                    installed_at: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                    size,
                    icon: None,
                    enabled: true,
                };
                inst.mods.retain(|c| c.filename != rec.filename);
                inst.mods.push(rec);
                count += 1;
            }
        }
        if let Err(e) = crate::instances::save_instance(state, &inst) {
            eprintln!("[modpack] save_instance failed: {e}");
        }
    }
    // auto-install game files (client jar, libraries, assets...)
    let _ = crate::install::install_game(app.clone(), state, &instance).await;
    let _ = crate::instances::mark_installed(state, &instance.id);

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
