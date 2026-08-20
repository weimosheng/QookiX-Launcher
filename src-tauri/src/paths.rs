use crate::state::AppState;
use std::path::PathBuf;

/// When version isolation is enabled, libraries/assets live inside each
/// instance folder, making every instance fully self-contained.
pub fn isolation_enabled(state: &AppState) -> bool {
    state.settings.read().unwrap().isolation
}

pub fn libraries_dir(state: &AppState, instance_id: &str) -> PathBuf {
    if isolation_enabled(state) {
        state.instances_dir().join(instance_id).join("libraries")
    } else {
        state.libraries_dir()
    }
}

pub fn assets_dir(state: &AppState, instance_id: &str) -> PathBuf {
    if isolation_enabled(state) {
        state.instances_dir().join(instance_id).join("assets")
    } else {
        state.assets_dir()
    }
}

pub fn assets_indexes_dir(state: &AppState, instance_id: &str) -> PathBuf {
    assets_dir(state, instance_id).join("indexes")
}

pub fn assets_objects_dir(state: &AppState, instance_id: &str) -> PathBuf {
    assets_dir(state, instance_id).join("objects")
}
