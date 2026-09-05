use crate::install;
use crate::mcmeta;
use crate::models::*;
use crate::state::AppState;
use serde_json::{json, Value};
use tauri::State;

// Version metadata
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_version_manifest(state: State<'_, AppState>) -> Result<Value, String> {
    let manifest = mcmeta::fetch_manifest(&state).await?;
    // the Mojang manifest is already ordered newest -> oldest
    let versions: Vec<Value> = manifest
        .versions
        .iter()
        .map(|v| {
            json!({
                "id": v.id,
                "type": v.kind,
                "releaseTime": v.release_time,
            })
        })
        .collect();
    Ok(json!({
        "versions": versions,
        "latest": { "release": manifest.latest.release, "snapshot": manifest.latest.snapshot }
    }))
}

#[tauri::command]
pub async fn get_loader_versions(
    state: State<'_, AppState>,
    loader: String,
    mc_version: String,
) -> Result<Vec<String>, String> {
    let l: LoaderType = loader.parse()?;
    // vanilla 没有独立版本列表，保持与旧实现一致返回错误
    if matches!(l, LoaderType::Vanilla) {
        return Err("未知加载器".into());
    }
    install::loader_versions(&state, l, &mc_version).await
}

