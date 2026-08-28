use crate::models::{ManifestVersion, VersionJson, VersionManifest};
use crate::state::AppState;
use std::path::PathBuf;

const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

/// Fetch and cache the Mojang version manifest.
pub async fn fetch_manifest(state: &AppState) -> Result<VersionManifest, String> {
    let path = state.root.join("version_manifest.json");
    // refresh if older than 1 hour
    let stale = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .map(|t| t.elapsed().map(|e| e.as_secs() > 3600).unwrap_or(true))
        .unwrap_or(true);
    if path.exists() && !stale {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(m) = serde_json::from_str::<VersionManifest>(&text) {
                return Ok(m);
            }
        }
    }
    let text = crate::download::get_text(&state.client, MANIFEST_URL).await?;
    let manifest: VersionManifest = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(path.parent().unwrap()).ok();
    let _ = std::fs::write(&path, &text);
    Ok(manifest)
}

/// Find a manifest version entry by id.
pub fn find_version<'a>(manifest: &'a VersionManifest, id: &str) -> Option<&'a ManifestVersion> {
    manifest.versions.iter().find(|v| v.id == id)
}

/// Return the full (non-patched) Mojang version JSON for a version id,
/// following `inheritsFrom` chains.
pub async fn fetch_version_json(state: &AppState, id: &str) -> Result<VersionJson, String> {
    let manifest = fetch_manifest(state).await?;
    let entry = find_version(&manifest, id)
        .ok_or_else(|| format!("未找到 Minecraft 版本 {id}"))?;
    let json: VersionJson = crate::download::get_json(&state.client, &entry.url).await?;
    Ok(json)
}

/// Cache a version JSON at `versions/<id>/<id>.json` (used for vanilla).
pub async fn cache_version_json(state: &AppState, json: &VersionJson) -> Result<PathBuf, String> {
    let dir = crate::paths::resolve_version_dir(state, &json.id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.json", json.id));
    let text = serde_json::to_string_pretty(json).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Resolve a version json following the `inheritsFrom` chain and merging
/// parent data (libraries, asset index, downloads, java version...).
#[allow(dead_code)]
pub fn resolve_inheritance(root: &VersionJson, parents: &[VersionJson]) -> VersionJson {
    let mut merged = root.clone();
    for parent in parents.iter().rev() {
        if merged.asset_index.is_none() {
            merged.asset_index = parent.asset_index.clone();
        }
        if merged.downloads.client.is_none() {
            merged.downloads.client = parent.downloads.client.clone();
        }
        if merged.downloads.server.is_none() {
            merged.downloads.server = parent.downloads.server.clone();
        }
        if merged.java_version.is_none() {
            merged.java_version = parent.java_version.clone();
        }
        if merged.logging.is_none() {
            merged.logging = parent.logging.clone();
        }
        // prepend parent libraries that are not already present (by name)
        let existing: std::collections::HashSet<String> =
            merged.libraries.iter().map(|l| l.name.clone()).collect();
        let mut libs: Vec<_> = parent
            .libraries
            .iter()
            .filter(|l| !existing.contains(&l.name))
            .cloned()
            .collect();
        libs.extend(merged.libraries.clone());
        merged.libraries = libs;
        if merged.minecraft_arguments.is_none() {
            merged.minecraft_arguments = parent.minecraft_arguments.clone();
        }
        if merged.arguments.is_none() {
            merged.arguments = parent.arguments.clone();
        }
    }
    merged.inherits_from = None;
    merged
}
