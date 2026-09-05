use crate::curseforge;
use crate::models::*;
use crate::modrinth;
use crate::state::AppState;
use serde_json::{json, Value};
use tauri::Emitter;
use tauri::Manager;
use tauri::State;

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

/// "全部来源"搜索：Modrinth 与 CurseForge 各自独立分页、各取当前页，
/// 合并后按所选排序维度统一排序并截取一页。CurseForge 失败不影响整体
/// 结果（仅第一页时上报 `cf_error` 供前端提示）。分类只作用于 Modrinth。
async fn browse_all_sources(
    state: &AppState,
    query: &str,
    project_type: &str,
    category: &str,
    page: u32,
    game_version: &str,
    loader: &str,
    sort: &str,
    ps: usize,
) -> Result<Value, String> {
    let offset = (page as usize) * ps;
    let m = modrinth::search(
        state,
        query,
        project_type,
        category,
        sort,
        offset,
        ps,
        game_version,
        loader,
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
        state,
        query,
        project_type,
        0,
        page as usize,
        ps,
        game_version,
        loader,
        sort,
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
    match sort {
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
        // "全部来源"：合并多源结果（见 browse_all_sources）
        "all" => {
            browse_all_sources(&state, &query, &project_type, &category, page, &game_version, &loader, &sort, ps).await
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
            let _ = crate::util::log_best_effort("uninstall_content", modrinth::uninstall(&state2, &instance, &kind2, &old_filename2));
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
        let _ = crate::util::log_best_effort("add_content_batch", crate::instances::add_content_batch(&state, &instance_id, &kind, new_records.clone()));
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
        let _ = crate::util::log_best_effort("add_content_batch", crate::instances::add_content_batch(&state, &instance_id, &kind, records.clone()));
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
                    let _ = crate::util::log_best_effort("add_content_batch", crate::instances::add_content_batch(&state, &instance_id, &kind, vec![rec.clone()]));
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
                    let _ = crate::util::log_best_effort("add_content_batch", crate::instances::add_content_batch(&state, &instance_id, &kind, vec![rec.clone()]));
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

