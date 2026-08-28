use crate::state::AppState;
use std::path::PathBuf;

/// Libraries and assets live in the shared root directory (shared across instances).
pub fn libraries_dir(state: &AppState) -> PathBuf {
    state.libraries_dir()
}

pub fn assets_dir(state: &AppState) -> PathBuf {
    state.assets_dir()
}

pub fn assets_indexes_dir(state: &AppState) -> PathBuf {
    assets_dir(state).join("indexes")
}

pub fn assets_objects_dir(state: &AppState) -> PathBuf {
    assets_dir(state).join("objects")
}

/// Resolve the version directory for an instance. Prefers the self-contained
/// `instances/<id>/` (version json/jar live directly there for imported
/// instances), falling back to the shared global `versions/<id>/`.
pub fn resolve_version_dir(state: &AppState, instance_id: &str) -> PathBuf {
    let inst_dir = state.instances_dir().join(instance_id);
    if inst_dir.join(format!("{}.json", instance_id)).exists() {
        return inst_dir;
    }
    state.versions_dir().join(instance_id)
}
