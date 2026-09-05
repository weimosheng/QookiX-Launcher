use crate::state::AppState;
use tauri::State;

// Storage
// ---------------------------------------------------------------------------

/// 获取存储统计：优先返回上次扫描的缓存，无缓存时实时扫描
#[tauri::command]
pub fn get_storage_stats(state: State<AppState>) -> crate::storage::StorageStats {
    crate::storage::get_storage_stats(&state)
}

/// 强制重新扫描存储并返回最新统计
#[tauri::command]
pub fn refresh_storage_stats(state: State<AppState>) -> crate::storage::StorageStats {
    crate::storage::refresh_storage_stats(&state)
}

/// 清除可安全清理的缓存，返回释放的空间
#[tauri::command]
pub fn clear_cache(state: State<AppState>) -> Result<crate::storage::CacheClearResult, String> {
    crate::storage::clear_cache(&state)
}

