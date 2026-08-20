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

    // record staged mods
    let mods_dir = instance_dir.join("mods");
    if mods_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&mods_dir) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if !name.ends_with(".jar") {
                    continue;
                }
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                let rec = InstalledContent {
                    filename: name.clone(),
                    source: "modpack".into(),
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
                let _ = crate::instances::add_content(state, &instance.id, "mod", rec);
            }
        }
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
