use crate::models::Settings;
use crate::state::AppState;
use directories::BaseDirs;

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

pub fn load_settings(root: &std::path::Path) -> Settings {
    let path = root.join("settings.json");
    let mut settings = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
        .unwrap_or_default();
    if settings.data_dir.is_empty() {
        settings.data_dir = root.to_string_lossy().to_string();
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
        settings.curseforge_api_key = v.as_str().map(|s| s.to_string()).filter(|s| !s.is_empty());
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
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(format!(
            "QookiX-Launcher/{} (desktop)",
            env!("CARGO_PKG_VERSION")
        ))
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .expect("failed to build http client")
}

pub fn state_guard<'a>(state: &'a AppState) -> std::sync::RwLockReadGuard<'a, Settings> {
    state.settings.read().unwrap()
}

pub fn with_settings<T>(state: &AppState, f: impl FnOnce(&Settings) -> T) -> T {
    let s = state.settings.read().unwrap();
    f(&s)
}
