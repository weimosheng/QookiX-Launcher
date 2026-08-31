use crate::models::Settings;
use crate::state::AppState;
use directories::BaseDirs;
use sysinfo::System;

pub fn default_root() -> String {
    if let Some(base) = BaseDirs::new() {
        base.data_dir()
            .join("QookiX-Launcher")
            .to_string_lossy()
            .to_string()
    } else {
        std::env::temp_dir()
            .join("QookiX-Launcher")
            .to_string_lossy()
            .to_string()
    }
}

/// Read (total, used, available) physical memory in MB with a single refresh.
/// Uses sysinfo: used = total - available, so cache that can be reclaimed
/// is NOT counted as "used" (more accurate than the old PowerShell approach).
fn memory_mb() -> Option<(u64, u64, u64)> {
    let sys = System::new_all();
    let total = sys.total_memory();
    let used = sys.used_memory();
    let available = sys.available_memory();
    if total == 0 {
        return None;
    }
    Some((total / 1024 / 1024, used / 1024 / 1024, available / 1024 / 1024))
}

/// Total physical memory in MB (cross-platform, via sysinfo).
pub fn total_memory_mb() -> Option<u64> {
    memory_mb().map(|(t, _, _)| t)
}

/// Currently used physical memory in MB (total - available, excludes reclaimable cache).
pub fn used_memory_mb() -> Option<u64> {
    memory_mb().map(|(_, u, _)| u)
}

/// Currently available physical memory in MB.
pub fn available_memory_mb() -> Option<u64> {
    memory_mb().map(|(_, _, a)| a)
}

/// Recommend (max, min) memory in MB based on available system memory and mod count.
/// - Base: 40% of available (min 2048 MB), so vanilla scales with free RAM
/// - Mods: +512 MB per 100 mods, capped at +4 GB
/// - Total capped at 75% of available (leave room for OS) and 8 GB absolute
pub fn recommended_memory(available_mb: u64, mod_count: usize) -> (u32, u32) {
    let base = (available_mb * 40 / 100).max(2048);
    let extra = (mod_count as u64 * 512 / 100).min(4096);
    let mut max = base + extra;
    let cap = (available_mb * 3 / 4).max(512);
    max = max.min(cap).min(8192).max(512);
    let min = (max / 4).max(256);
    (max as u32, min as u32)
}

pub fn load_settings(root: &std::path::Path) -> Settings {
    let path = root.join("settings.json");
    let exists = path.exists();
    let mut settings = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
        .unwrap_or_default();
    // 兼容迁移：旧版本只有 proxy 没有 proxy_mode。
    // 若缺少 proxy_mode 但配置了自定义 proxy，则视为自定义模式。
    if settings.proxy_mode.is_empty() {
        settings.proxy_mode = if settings.proxy.as_deref().map(|s| !s.is_empty()).unwrap_or(false) {
            "custom".into()
        } else {
            "system".into()
        };
    }
    if settings.data_dir.is_empty() {
        settings.data_dir = root.to_string_lossy().to_string();
    }
    // First launch: auto-detect memory
    if !exists {
        if let Some(total) = total_memory_mb() {
            let avail = crate::settings::available_memory_mb().unwrap_or(total);
            let (max, min) = recommended_memory(avail, 0);
            settings.max_memory_mb = max;
            settings.min_memory_mb = min;
        }
    }
    settings
}

pub fn save_settings(root: &std::path::Path, settings: &Settings) -> Result<(), String> {
    let path = root.join("settings.json");
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

/// Persist current settings from state.
pub fn persist(state: &AppState) -> Result<(), String> {
    let settings = state.settings.read().unwrap().clone();
    save_settings(&state.root, &settings)
}

/// Update settings with a partial patch (values from frontend).
pub fn update_settings(state: &AppState, patch: serde_json::Value) -> Result<Settings, String> {
    let mut settings = state.settings.write().unwrap();
    // NOTE: `data_dir` is intentionally ignored here. The data root is fixed at
    // startup (honoring the value the installer seeds into settings.json) and
    // existing data is not migrated; applying a runtime change would silently
    // make the app "lose" everything on the next launch.
    if let Some(v) = patch.get("java_path") {
        settings.java_path = v.as_str().map(|s| s.to_string()).filter(|s| !s.is_empty());
    }
    if let Some(v) = patch.get("max_memory_mb").and_then(|v| v.as_u64()) {
        settings.max_memory_mb = v as u32;
    }
    if let Some(v) = patch.get("min_memory_mb").and_then(|v| v.as_u64()) {
        settings.min_memory_mb = v as u32;
    }
    if let Some(v) = patch.get("memory_mode").and_then(|v| v.as_str()) {
        settings.memory_mode = v.to_string();
    }
    if let Some(v) = patch.get("jvm_args").and_then(|v| v.as_str()) {
        settings.jvm_args = v.to_string();
    }
    if let Some(v) = patch.get("game_args").and_then(|v| v.as_str()) {
        settings.game_args = v.to_string();
    }
    if let Some(v) = patch.get("download_threads").and_then(|v| v.as_u64()) {
        settings.download_threads = (v as usize).clamp(1, 64);
    }
    if let Some(v) = patch.get("download_chunk_threads").and_then(|v| v.as_u64()) {
        settings.download_chunk_threads = (v as usize).clamp(1, 16);
    }
    if let Some(v) = patch.get("curseforge_api_key") {
        let k = v.as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        settings.curseforge_api_key = k;
    }
    if let Some(v) = patch.get("theme").and_then(|v| v.as_str()) {
        settings.theme = v.to_string();
    }
    if let Some(v) = patch.get("theme_color").and_then(|v| v.as_str()) {
        if !v.trim().is_empty() {
            settings.theme_color = v.trim().to_string();
        }
    }
    if let Some(v) = patch.get("close_behavior").and_then(|v| v.as_str()) {
        settings.close_behavior = v.to_string();
    }
    if let Some(v) = patch.get("auto_launch").and_then(|v| v.as_bool()) {
        settings.auto_launch = v;
    }
    if let Some(v) = patch.get("keep_open").and_then(|v| v.as_bool()) {
        settings.keep_open = v;
    }
    if let Some(v) = patch.get("ms_client_id").and_then(|v| v.as_str()) {
        if !v.trim().is_empty() {
            settings.ms_client_id = v.trim().to_string();
        }
    }
    if let Some(v) = patch.get("selected_account") {
        settings.selected_account = v.as_str().map(|s| s.to_string()).filter(|s| !s.is_empty());
    }
    if let Some(v) = patch.get("proxy_mode").and_then(|v| v.as_str()) {
        let m = v.trim().to_string();
        if matches!(m.as_str(), "system" | "direct" | "custom") {
            settings.proxy_mode = m;
        }
    }
    if let Some(v) = patch.get("proxy") {
        let p = v.as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        settings.proxy = p;
    }
    if let Some(v) = patch.get("mirror").and_then(|v| v.as_str()) {
        settings.mirror = v.trim().to_string();
    }
    if let Some(v) = patch.get("mirror_custom").and_then(|v| v.as_str()) {
        settings.mirror_custom = v.trim().to_string();
    }
    if let Some(v) = patch.get("background_image") {
        settings.background_image = v.as_str().map(|s| s.to_string()).filter(|s| !s.is_empty());
    }
    if let Some(v) = patch.get("background_blur").and_then(|v| v.as_u64()) {
        settings.background_blur = (v as u32).min(50);
    }
    if let Some(v) = patch.get("background_dim").and_then(|v| v.as_u64()) {
        settings.background_dim = (v as u32).min(100);
    }
    if let Some(v) = patch.get("glass_blur").and_then(|v| v.as_u64()) {
        settings.glass_blur = (v as u32).min(30);
    }
    if let Some(v) = patch.get("show_home_hero").and_then(|v| v.as_bool()) {
        settings.show_home_hero = v;
    }
    if let Some(v) = patch.get("show_sidebar_collapse_btn").and_then(|v| v.as_bool()) {
        settings.show_sidebar_collapse_btn = v;
    }
    if let Some(v) = patch.get("dismissed_update_version") {
        settings.dismissed_update_version =
            v.as_str().map(|s| s.to_string()).filter(|s| !s.is_empty());
    }
    if let Some(v) = patch.get("auto_update").and_then(|v| v.as_bool()) {
        settings.auto_update = v;
    }
    if let Some(v) = patch.get("update_source").and_then(|v| v.as_str()) {
        settings.update_source = if v.trim() == "github" {
            "github".into()
        } else {
            "bucket".into()
        };
    }
    let cloned = settings.clone();
    drop(settings);
    persist(state)?;
    Ok(cloned)
}

/// Migrate the launcher data root to `new_dir`.
///
/// `mode`:
/// - `"move"`: move all data into the new dir, update the seed
/// - `"copy"`: copy all data into the new dir (keep old as backup), update seed
/// - `"pointer"`: only update the seed — the user is responsible for the data
///
/// Symlink-imported instances (junctions) are recreated at the destination
/// pointing at their original external `.minecraft` instead of being
/// dereferenced, so they keep working after the move.
///
/// On success the in-memory `settings.data_dir` is updated and a seed
/// `settings.json` is written into the default root so the next launch picks
/// up the new location. The caller must restart for the change to fully take
/// effect (the running process still holds the old root).
pub fn change_data_dir(state: &AppState, new_dir: &str, mode: &str) -> Result<String, String> {
    let new_root = std::path::PathBuf::from(new_dir);
    if new_root.as_os_str().is_empty() {
        return Err("新数据目录不能为空".into());
    }
    let _ = std::fs::create_dir_all(&new_root);
    let new_clean = new_root
        .canonicalize()
        .map_err(|e| format!("无法解析新目录路径: {e}"))?;
    let cur_clean = state
        .root
        .canonicalize()
        .map_err(|e| format!("无法解析当前目录路径: {e}"))?;
    if new_clean == cur_clean {
        return Err("新目录与当前数据目录相同".into());
    }
    if new_clean.starts_with(&cur_clean) {
        return Err("新目录不能位于当前数据目录内".into());
    }
    if cur_clean.starts_with(&new_clean) {
        return Err("新目录不能包含当前数据目录".into());
    }

    ensure_layout(&new_root).map_err(|e| format!("创建目录布局失败: {e}"))?;

    let instances_name = std::ffi::OsStr::new("instances");
    let skip = |name: &std::ffi::OsStr| name == "settings.json";

    let migrate_instance = |src: &std::path::Path, dst: &std::path::Path| -> Result<(), String> {
        let json_path = src.join("instance.json");
        if let Ok(text) = std::fs::read_to_string(&json_path) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
                if val.get("is_symlink").and_then(|v| v.as_bool()).unwrap_or(false) {
                    if let Some(source) = val.get("source_path").and_then(|v| v.as_str()) {
                        let target = std::path::PathBuf::from(source);
                        let mut fb = false;
                        crate::util::link_dir(&target, dst, &mut fb)?;
                        return Ok(());
                    }
                }
            }
        }
        match mode {
            "move" => crate::util::move_entry(src, dst),
            _ => crate::util::copy_tree(src, dst),
        }
    };

    for e in std::fs::read_dir(&state.root).map_err(|e| e.to_string())?.flatten() {
        let name = e.file_name();
        if skip(&name) {
            continue;
        }
        let src = e.path();
        let dst = new_root.join(&name);
        if name == instances_name && src.is_dir() {
            std::fs::create_dir_all(&dst).map_err(|e| e.to_string())?;
            for inst in std::fs::read_dir(&src).map_err(|e| e.to_string())?.flatten() {
                let iname = inst.file_name();
                let isrc = inst.path();
                let idst = dst.join(&iname);
                migrate_instance(&isrc, &idst)?;
            }
            if mode == "move" {
                let _ = std::fs::remove_dir_all(&src);
            }
            continue;
        }
        match mode {
            "move" => {
                if src.is_dir() {
                    crate::util::move_entry(&src, &dst)?;
                } else if src.is_file() {
                    std::fs::rename(&src, &dst)
                        .or_else(|_| {
                            std::fs::copy(&src, &dst).and_then(|_| std::fs::remove_file(&src))
                        })
                        .map_err(|e| format!("移动 {} 失败: {e}", name.to_string_lossy()))?;
                }
            }
            "copy" => {
                if src.is_dir() {
                    crate::util::copy_tree(&src, &dst)?;
                } else if src.is_file() {
                    std::fs::copy(&src, &dst).map_err(|e| e.to_string())?;
                }
            }
            "pointer" => {}
            other => return Err(format!("未知迁移模式: {other}")),
        }
    }

    // Write the full settings.json into the new root with data_dir updated.
    {
        let mut s = state.settings.read().unwrap().clone();
        s.data_dir = new_dir.to_string();
        save_settings(&new_root, &s)?;
    }
    // Seed the default root so the next launch honors the new location.
    let default_root = std::path::PathBuf::from(default_root());
    let _ = std::fs::create_dir_all(&default_root);
    let seed = serde_json::json!({ "data_dir": new_dir.replace('\\', "/") });
    let _ = std::fs::write(
        default_root.join("settings.json"),
        serde_json::to_string_pretty(&seed).unwrap_or_default(),
    );

    // Reflect the new path in memory immediately (UI); the actual file I/O
    // still uses the old root until restart.
    {
        let mut s = state.settings.write().unwrap();
        s.data_dir = new_dir.to_string();
    }

    Ok(new_dir.to_string())
}

/// Ensure the directory layout exists under the given root.
pub fn ensure_layout(root: &std::path::Path) -> std::io::Result<()> {
    for sub in [
        "instances",
        "libraries",
        "assets/indexes",
        "assets/objects",
        "versions",
        "logs",
        "runtimes",
        "skins",
    ] {
        std::fs::create_dir_all(root.join(sub))?;
    }
    Ok(())
}

/// Shared http client builder with a browser-ish UA.
pub fn http_client(proxy_mode: &str, proxy: Option<&str>) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .user_agent(format!(
            "QookiX-Launcher/{} (desktop)",
            env!("CARGO_PKG_VERSION")
        ))
        .gzip(true)
        .brotli(true)
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(60));
    match proxy_mode {
        // 直连：禁用所有代理（含系统代理）
        "direct" => {
            builder = builder.no_proxy();
        }
        // 自定义：使用显式配置的代理 URL
        "custom" => {
            if let Some(p) = proxy {
                if !p.is_empty() {
                    if let Ok(proxy) = reqwest::Proxy::all(p) {
                        builder = builder.proxy(proxy);
                    }
                }
            }
        }
        // "system" 及其它：不额外设置，走 reqwest 默认的系统代理
        _ => {}
    }
    builder.build().expect("failed to build http client")
}

#[allow(dead_code)]
pub fn state_guard<'a>(state: &'a AppState) -> std::sync::RwLockReadGuard<'a, Settings> {
    state.settings.read().unwrap()
}

#[allow(dead_code)]
pub fn with_settings<T>(state: &AppState, f: impl FnOnce(&Settings) -> T) -> T {
    let s = state.settings.read().unwrap();
    f(&s)
}
