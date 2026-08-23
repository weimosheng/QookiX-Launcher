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

/// Recommend (max, min) memory in MB based on total system memory.
/// - <=4 GB: total - 1 GB (min 1 GB)
/// - 4-8 GB: half
/// - >8 GB: two-thirds, capped at 8 GB
pub fn recommended_memory(total_mb: u64) -> (u32, u32) {
    let max = if total_mb <= 4096 {
        (total_mb.saturating_sub(1024)).max(1024)
    } else if total_mb <= 8192 {
        total_mb / 2
    } else {
        (total_mb * 2 / 3).min(8192)
    };
    let min = (max / 4).max(512);
    (max as u32, min as u32)
}

pub fn load_settings(root: &std::path::Path) -> Settings {
    let path = root.join("settings.json");
    let exists = path.exists();
    let mut settings = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
        .unwrap_or_default();
    if settings.data_dir.is_empty() {
        settings.data_dir = root.to_string_lossy().to_string();
    }
    // First launch: auto-detect memory
    if !exists {
        if let Some(total) = total_memory_mb() {
            let (max, min) = recommended_memory(total);
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
    if let Some(v) = patch.get("data_dir") {
        if let Some(s) = v.as_str() {
            settings.data_dir = s.to_string();
        }
    }
    if let Some(v) = patch.get("java_path") {
        settings.java_path = v.as_str().map(|s| s.to_string()).filter(|s| !s.is_empty());
    }
    if let Some(v) = patch.get("max_memory_mb").and_then(|v| v.as_u64()) {
        settings.max_memory_mb = v as u32;
    }
    if let Some(v) = patch.get("min_memory_mb").and_then(|v| v.as_u64()) {
        settings.min_memory_mb = v as u32;
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
    if let Some(v) = patch.get("curseforge_api_key") {
        let k = v.as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        settings.curseforge_api_key = k;
    }
    if let Some(v) = patch.get("theme").and_then(|v| v.as_str()) {
        settings.theme = v.to_string();
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
    if let Some(v) = patch.get("isolation").and_then(|v| v.as_bool()) {
        settings.isolation = v;
    }
    if let Some(v) = patch.get("proxy") {
        let p = v.as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        settings.proxy = p;
    }
    let cloned = settings.clone();
    drop(settings);
    persist(state)?;
    Ok(cloned)
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
    ] {
        std::fs::create_dir_all(root.join(sub))?;
    }
    Ok(())
}

/// Shared http client builder with a browser-ish UA.
pub fn http_client(proxy: Option<&str>) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .user_agent(format!(
            "QookiX-Launcher/{} (desktop)",
            env!("CARGO_PKG_VERSION")
        ))
        .gzip(true)
        .brotli(true)
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(60));
    if let Some(p) = proxy {
        if !p.is_empty() {
            if let Ok(proxy) = reqwest::Proxy::all(p) {
                builder = builder.proxy(proxy);
            }
        }
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
