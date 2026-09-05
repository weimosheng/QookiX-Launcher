use crate::launch;
use crate::models::*;
use crate::settings;
use crate::state::AppState;
use serde_json::{json, Value};
use tauri::State;

// ---------------------------------------------------------------------------
// Settings & Java
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Result<Settings, String> {
    Ok(state.settings.read().unwrap().clone())
}

#[tauri::command]
pub fn set_settings(state: State<AppState>, patch: Value) -> Result<Settings, String> {
    settings::update_settings(&state, patch)
}

// ---------------------------------------------------------------------------
/// 可用的下载镜像源预设列表。
#[tauri::command]
pub fn list_mirrors() -> Value {
    crate::mirror::presets()
}

/// 测试镜像源连通性并返回首字节耗时（毫秒）。
/// `base` 为空串表示测试官方源。仅发一个 GET 并读取状态，不下载正文。
#[tauri::command]
pub async fn test_mirror(state: State<'_, AppState>, base: String) -> Result<Value, String> {
    let base = base.trim().trim_end_matches('/').to_string();
    let url = if base.is_empty() {
        crate::mirror::OFFICIAL_MANIFEST.to_string()
    } else {
        format!("{base}/mc/game/version_manifest_v2.json")
    };
    let start = std::time::Instant::now();
    let resp = state
        .client
        .get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("连接失败: {e}"))?;
    let ms = start.elapsed().as_millis() as u64;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    Ok(json!({ "ok": true, "ms": ms, "url": url }))
}

/// 测试下载代理配置是否可用。
/// 用传入的 `proxy_mode`/`proxy` 临时构建客户端访问官方 manifest，
/// 返回首字节耗时（毫秒）。`proxy` 仅在 `proxy_mode == "custom"` 时生效。
#[tauri::command]
pub async fn test_proxy(proxy_mode: String, proxy: Option<String>) -> Result<Value, String> {
    // 自定义模式必须给出合法的代理地址，否则 http_client 会退化为直连，
    // 导致「没填地址也测试成功」——这里先校验，避免误报成功。
    if proxy_mode == "custom" {
        let addr = proxy.as_deref().unwrap_or("").trim();
        if addr.is_empty() {
            return Err("请先填写代理地址".to_string());
        }
        if reqwest::Proxy::all(addr).is_err() {
            return Err("代理地址格式不正确，应类似 http://127.0.0.1:7890 或 socks5://127.0.0.1:1080".to_string());
        }
    }
    let client = settings::http_client(&proxy_mode, proxy.as_deref());
    let url = crate::mirror::OFFICIAL_MANIFEST.to_string();
    let start = std::time::Instant::now();
    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| format!("连接失败: {e}"))?;
    let ms = start.elapsed().as_millis() as u64;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    Ok(json!({ "ok": true, "ms": ms, "url": url }))
}

/// Move/copy the launcher data root to a new directory.
/// `mode`: "move" | "copy" | "pointer". Returns the new data dir; the caller
/// should restart for the change to fully take effect.
#[tauri::command]
pub fn change_data_dir(
    state: State<AppState>,
    new_dir: String,
    mode: String,
) -> Result<Value, String> {
    let new_dir = settings::change_data_dir(&state, &new_dir, &mode)?;
    Ok(json!({ "ok": true, "new_dir": new_dir, "need_restart": true }))
}

/// Auto-detect system memory and return (total, used, available) + recommended (max, min) in MB.
#[tauri::command]
pub fn auto_detect_memory() -> Result<Value, String> {
    let total = crate::settings::total_memory_mb().ok_or("无法检测系统内存")?;
    let used = crate::settings::used_memory_mb().unwrap_or(0);
    let available = crate::settings::available_memory_mb().unwrap_or(total.saturating_sub(used));
    let (max, min) = crate::settings::recommended_memory(available, 0);
    Ok(json!({
        "total_mb": total,
        "used_mb": used,
        "available_mb": available,
        "max_mb": max,
        "min_mb": min
    }))
}

#[tauri::command]
pub fn detect_java(
    state: State<AppState>,
    refresh: Option<bool>,
) -> Result<JavaDetection, String> {
    let force = refresh.unwrap_or(false);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Use cache unless manually forced to refresh
    if !force {
        if let Some((_ts, list)) = state.java_cache.lock().unwrap().as_ref() {
            return Ok(JavaDetection {
                candidates: list.clone(),
                selected: list.first().cloned(),
            });
        }
    }
    let root = state.root.clone();
    let runtimes = root.join("runtimes");
    // Reuse the persisted cache when possible; `force` re-scans and updates it.
    let (ts, candidates) = crate::java::cached_detect(&root, &runtimes, now, force);
    let selected = candidates.first().cloned();
    *state.java_cache.lock().unwrap() = Some((ts, candidates.clone()));
    Ok(JavaDetection { candidates, selected })
}

/// Download a Java runtime of the given major version (if not already present).
#[tauri::command]
pub async fn download_java(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    major: u32,
) -> Result<JavaInfo, String> {
    crate::java::download_java_runtime(app, &state, major).await
}

/// Recommended Java for an instance: required major + best available match.
#[tauri::command]
pub async fn recommend_java(state: State<'_, AppState>, instance_id: String) -> Result<Value, String> {
    let instance = crate::instances::get_instance(&state, &instance_id)?;
    let required = launch::required_java_for(&state, &instance);
    let best = launch::find_best_java(&state, required).await;
    Ok(json!({
        "required": required,
        "java": best,
        "needDownload": best.as_ref().map(|j| j.major < required).unwrap_or(true),
    }))
}
