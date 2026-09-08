//! 世界存档备份的 Tauri 命令。

use crate::state::AppState;
use crate::world_backup::BackupInfo;
use tauri::State;

/// 列出某个世界的全部备份（修改时间倒序）
#[tauri::command]
pub fn list_world_backups(
    state: State<AppState>,
    instance_id: String,
    world: String,
) -> Result<Vec<BackupInfo>, String> {
    if !crate::util::is_safe_filename(&world) {
        return Err("非法的世界目录名".into());
    }
    Ok(crate::world_backup::list_backups(state.inner(), &instance_id, &world))
}

/// 创建备份：把 `saves/<world>` 打包为 zip
#[tauri::command]
pub fn create_world_backup(
    state: State<AppState>,
    instance_id: String,
    world: String,
) -> Result<BackupInfo, String> {
    crate::world_backup::create_backup(state.inner(), &instance_id, &world)
}

/// 恢复备份（恢复前自动为当前存档留一份安全快照）
#[tauri::command]
pub fn restore_world_backup(
    state: State<AppState>,
    instance_id: String,
    world: String,
    filename: String,
) -> Result<(), String> {
    crate::world_backup::restore_backup(state.inner(), &instance_id, &world, &filename)
}

/// 删除备份
#[tauri::command]
pub fn delete_world_backup(
    state: State<AppState>,
    instance_id: String,
    world: String,
    filename: String,
) -> Result<(), String> {
    crate::world_backup::delete_backup(state.inner(), &instance_id, &world, &filename)
}
