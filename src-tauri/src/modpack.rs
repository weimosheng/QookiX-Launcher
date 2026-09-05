use crate::models::{Instance, InstalledContent, LoaderType};
use crate::state::AppState;
use serde_json::Value;

/// Read the pack metadata (name, mc version, loader) from a local
/// `.mrpack` or CurseForge modpack zip.
pub async fn detect(path: &std::path::Path) -> Result<(String, String, LoaderType, String), String> {
    if let Ok(bytes) = crate::util::read_zip_entry(path, "modrinth.index.json") {
        let index: Value = serde_json::from_slice(&bytes).map_err(|e| format!("modrinth.index.json 解析失败: {e}"))?;
        let name = index
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("导入的整合包")
            .to_string();
        let mc = index
            .get("dependencies")
            .and_then(|d| d.get("minecraft"))
            .and_then(|m| m.as_str())
            .ok_or("缺少 minecraft 版本依赖")?
            .to_string();
        let (loader, lv) = detect_mrpack_loader(&index);
        Ok((name, mc, loader, lv))
    } else if let Ok(bytes) = crate::util::read_zip_entry(path, "manifest.json") {
        let manifest: Value = serde_json::from_slice(&bytes).map_err(|e| format!("manifest.json 解析失败: {e}"))?;
        let name = manifest
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("导入的整合包")
            .to_string();
        let mc = manifest
            .get("minecraft")
            .and_then(|m| m.get("version"))
            .and_then(|v| v.as_str())
            .ok_or("缺少 minecraft 版本")?
            .to_string();
        let loader_id = manifest
            .get("minecraft")
            .and_then(|m| m.get("modLoaders"))
            .and_then(|l| l.as_array())
            .and_then(|a| a.first())
            .and_then(|f| f.get("id"))
            .and_then(|i| i.as_str())
            .unwrap_or("");
        let (loader, lv) = parse_cf_loader(loader_id);
        Ok((name, mc, loader, lv))
    } else {
        Err("无法识别的整合包格式（需要 modrinth.index.json 或 manifest.json）".into())
    }
}

fn detect_mrpack_loader(index: &Value) -> (LoaderType, String) {
    let deps = &index["dependencies"];
    for (key, lt) in [
        ("fabric-loader", LoaderType::Fabric),
        ("quilt-loader", LoaderType::Quilt),
        ("neoforge", LoaderType::NeoForge),
        ("forge", LoaderType::Forge),
    ] {
        if let Some(v) = deps.get(key).and_then(|v| v.as_str()) {
            return (lt, v.to_string());
        }
    }
    (LoaderType::Vanilla, String::new())
}

fn parse_cf_loader(id: &str) -> (LoaderType, String) {
    for (prefix, lt) in [
        ("fabric-", LoaderType::Fabric),
        ("quilt-", LoaderType::Quilt),
        ("neoforge-", LoaderType::NeoForge),
        ("forge-", LoaderType::Forge),
    ] {
        if let Some(rest) = id.strip_prefix(prefix) {
            return (lt, rest.to_string());
        }
    }
    (LoaderType::Vanilla, id.to_string())
}

/// Stage the pack's files (mods, overrides, config...) into the instance.
pub async fn apply(
    app: &tauri::AppHandle,
    state: &AppState,
    instance: &Instance,
    path: &std::path::Path,
) -> Result<(), String> {
    let instance_dir = state.instances_dir().join(&instance.id);
    std::fs::create_dir_all(&instance_dir).map_err(|e| e.to_string())?;

    let task_id = state.next_task_id();
    crate::install::emit_progress(
        app,
        task_id,
        "modpack",
        "正在解压整合包文件…",
        0,
        1,
        instance,
        "导入整合包",
    );

    // regular files (mods/, config/...), skipping the pack metadata + overrides
    let _ = crate::util::extract_zip_progress(
        path,
        &instance_dir,
        &["modrinth.index.json", "manifest.json", "overrides/", "META-INF/"],
        &mut |done, total| {
            crate::install::emit_progress(
                app,
                task_id,
                "modpack-install",
                &format!("正在解压整合包文件…（{done}/{total}）"),
                done,
                total,
                instance,
                "导入整合包",
            );
        },
    )
    .map_err(|e| format!("解压整合包失败: {e}"))?;
    // overrides -> instance root
    let _ = crate::util::extract_zip_strip(
        path,
        &instance_dir,
        "overrides/",
        &["modrinth.index.json", "manifest.json", "META-INF/"],
    )?;

    // record staged mods — prefer pack index metadata, fall back to disk scan
    let mods_dir = instance_dir.join("mods");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut records: Vec<InstalledContent> = Vec::new();
    let mut handled = false;

    // Modrinth .mrpack: resolve project_id/version_id from sha1 hashes
    if let Ok(bytes) = crate::util::read_zip_entry(path, "modrinth.index.json") {
        handled = true;
        if let Ok(index) = serde_json::from_slice::<Value>(&bytes) {
            let files = index.get("files").and_then(|f| f.as_array()).cloned().unwrap_or_default();
            let mut hash_by_name: std::collections::HashMap<String, String> = std::collections::HashMap::new();
            for f in &files {
                let p = f.get("path").and_then(|p| p.as_str()).unwrap_or("");
                if !p.starts_with("mods/") || p.ends_with('/') {
                    continue;
                }
                let fname = std::path::Path::new(p)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                if fname.is_empty() {
                    continue;
                }
                if let Some(h) = f.get("hashes").and_then(|h| h.get("sha1")).and_then(|v| v.as_str()) {
                    hash_by_name.insert(fname, h.to_string());
                }
            }
            let resolved =
                crate::modrinth::resolve_by_hashes(state, &hash_by_name.values().cloned().collect::<Vec<_>>()).await;
            for (fname, h) in &hash_by_name {
                if !mods_dir.join(fname).exists() {
                    continue;
                }
                let size = std::fs::metadata(mods_dir.join(fname)).map(|m| m.len()).unwrap_or(0);
                let (pid, vid) = resolved
                    .get(h)
                    .map(|(p, v)| (Some(p.clone()), Some(v.clone())))
                    .unwrap_or((None, None));
                let mut rec = InstalledContent {
                    filename: fname.clone(),
                    source: "modrinth".into(),
                    project_id: pid,
                    slug: None,
                    version_id: vid,
                    name: Some(fname.clone()),
                    version: None,
                    mod_id: None,
                    authors: None,
                    description: None,
                    installed_at: now,
                    size,
                    icon: None,
                    enabled: true,
                };
                crate::util::fill_content_from_jar(&mut rec, &mods_dir.join(fname));
                records.push(rec);
            }
        }
    }

    // CurseForge: resolve fileName from manifest projectID/fileID via API
    if !handled {
        if let Ok(bytes) = crate::util::read_zip_entry(path, "manifest.json") {
            handled = true;
            if let Ok(manifest) = serde_json::from_slice::<Value>(&bytes) {
                let files = manifest.get("files").and_then(|f| f.as_array()).cloned().unwrap_or_default();
                for f in &files {
                    let Some(pid) = f.get("projectID").and_then(|v| v.as_u64()) else { continue };
                    let Some(fid) = f.get("fileID").and_then(|v| v.as_u64()) else { continue };
                    let fname = match crate::curseforge::file_info(state, pid, fid).await {
                        Ok(info) => info
                            .get("data")
                            .and_then(|d| d.get("fileName"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        Err(_) => continue,
                    };
                    if fname.is_empty() || !fname.ends_with(".jar") {
                        continue;
                    }
                    if !mods_dir.join(&fname).exists() {
                        continue;
                    }
                    let size = std::fs::metadata(mods_dir.join(&fname)).map(|m| m.len()).unwrap_or(0);
                    let mut rec = InstalledContent {
                        filename: fname.clone(),
                        source: "curseforge".into(),
                        project_id: Some(pid.to_string()),
                        slug: None,
                        version_id: Some(fid.to_string()),
                        name: Some(fname.clone()),
                        version: None,
                        mod_id: None,
                        authors: None,
                        description: None,
                        installed_at: now,
                        size,
                        icon: None,
                        enabled: true,
                    };
                    crate::util::fill_content_from_jar(&mut rec, &mods_dir.join(&fname));
                    records.push(rec);
                }
            }
            if records.is_empty() {
                handled = false;
            }
        }
    }

    // Fallback: scan disk (offline import or unrecognized format)
    if !handled && mods_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&mods_dir) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if !name.ends_with(".jar") {
                    continue;
                }
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                let mut rec = InstalledContent {
                    filename: name.clone(),
                    source: "modpack".into(),
                    project_id: None,
                    slug: None,
                    version_id: None,
                    name: Some(name),
                    version: None,
                    mod_id: None,
                    authors: None,
                    description: None,
                    installed_at: now,
                    size,
                    icon: None,
                    enabled: true,
                };
                crate::util::fill_content_from_jar(&mut rec, &e.path());
                records.push(rec);
            }
        }
    }

    for rec in records {
        let _ = crate::util::log_best_effort("add_content", crate::instances::add_content(state, &instance.id, "mod", rec));
    }
    crate::install::emit_progress(
        app,
        task_id,
        "done",
        "整合包已导入",
        1,
        1,
        instance,
        "导入整合包",
    );
    Ok(())
}
