//! 世界存档备份：把 `saves/<world>` 打包为 zip 快照，支持恢复与删除。
//! 备份存放于 `instances/<id>/backups/<world>/`，文件名形如 `<world>-20260906-120000.zip`。

use crate::state::AppState;
use serde::Serialize;
use std::io::Read;
use std::path::{Path, PathBuf};

/// 单个备份快照的元信息
#[derive(Serialize, Clone, Debug)]
pub struct BackupInfo {
    pub filename: String,
    pub size: u64,
    /// unix 秒
    pub modified: u64,
}

fn backups_dir(state: &AppState, instance_id: &str, world: &str) -> PathBuf {
    state
        .instances_dir()
        .join(instance_id)
        .join("backups")
        .join(world)
}

fn world_dir(state: &AppState, instance_id: &str, world: &str) -> PathBuf {
    state.instances_dir().join(instance_id).join("saves").join(world)
}

fn timestamp() -> String {
    // unix 秒作为唯一后缀；前端展示时用 fmtDate 转本地时间
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

/// 可读的备份文件名：<world>-<unix_secs>.zip
fn backup_filename(world: &str) -> String {
    format!("{}-{}.zip", world, timestamp())
}

fn info_from_file(p: &Path) -> Option<BackupInfo> {
    let meta = std::fs::metadata(p).ok()?;
    let modified = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(BackupInfo {
        filename: p.file_name()?.to_string_lossy().to_string(),
        size: meta.len(),
        modified,
    })
}

/// 列出某个世界的全部备份（按修改时间倒序）
pub fn list_backups(state: &AppState, instance_id: &str, world: &str) -> Vec<BackupInfo> {
    let dir = backups_dir(state, instance_id, world);
    let mut out: Vec<BackupInfo> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().map(|x| x == "zip").unwrap_or(false))
                .filter_map(|p| info_from_file(&p))
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| b.modified.cmp(&a.modified));
    out
}

/// 递归把 `dir` 下的文件写入 zip（zip 内路径相对 `dir`）
fn write_dir_to_zip(zw: &mut zip::ZipWriter<std::fs::File>, dir: &Path, prefix: &str) -> Result<(), String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("读取目录失败: {e}"))?;
    for e in rd.flatten() {
        let p = e.path();
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        if p.is_dir() {
            write_dir_to_zip(zw, &p, &rel)?;
        } else if p.is_file() {
            let mut f = std::fs::File::open(&p).map_err(|e| format!("打开 {} 失败: {e}", p.display()))?;
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zw.start_file(rel.clone(), opts)
                .map_err(|e| format!("写入 zip 条目 {rel} 失败: {e}"))?;
            std::io::copy(&mut f, zw).map_err(|e| format!("写入 {rel} 内容失败: {e}"))?;
        }
    }
    Ok(())
}

/// 创建备份：把 `saves/<world>` 打包为 zip。返回备份信息。
pub fn create_backup(state: &AppState, instance_id: &str, world: &str) -> Result<BackupInfo, String> {
    if !crate::util::is_safe_filename(world) {
        return Err("非法的世界目录名".into());
    }
    let src = world_dir(state, instance_id, world);
    if !src.is_dir() {
        return Err(format!("世界目录不存在: {}", src.display()));
    }
    let dir = backups_dir(state, instance_id, world);
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建备份目录失败: {e}"))?;
    let zip_path = dir.join(backup_filename(world));
    let file = std::fs::File::create(&zip_path).map_err(|e| format!("创建备份文件失败: {e}"))?;
    let mut zw = zip::ZipWriter::new(file);
    write_dir_to_zip(&mut zw, &src, "")?;
    zw.finish().map_err(|e| format!("完成 zip 失败: {e}"))?;
    info_from_file(&zip_path).ok_or_else(|| "读取备份信息失败".into())
}

/// 恢复备份：清空当前 `saves/<world>`，用快照内容替换。
pub fn restore_backup(state: &AppState, instance_id: &str, world: &str, filename: &str) -> Result<(), String> {
    if !crate::util::is_safe_filename(world) || !crate::util::is_safe_filename(filename) {
        return Err("非法的路径参数".into());
    }
    let zip_path = backups_dir(state, instance_id, world).join(filename);
    if !zip_path.is_file() {
        return Err("备份文件不存在".into());
    }
    let dest = world_dir(state, instance_id, world);
    // 恢复前先把当前存档留一份安全备份（防误恢复丢档）
    if dest.is_dir() {
        let safety = backups_dir(state, instance_id, world).join(backup_filename(world));
        if let Ok(f) = std::fs::File::create(&safety) {
            let mut zw = zip::ZipWriter::new(f);
            let _ = write_dir_to_zip(&mut zw, &dest, "");
            let _ = zw.finish();
        }
    }
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| format!("清空当前存档失败: {e}"))?;
    }
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    let n = crate::util::extract_zip(&zip_path, &dest, &[])?;
    if n == 0 {
        return Err("备份内容为空".into());
    }
    Ok(())
}

/// 删除备份
pub fn delete_backup(state: &AppState, instance_id: &str, world: &str, filename: &str) -> Result<(), String> {
    if !crate::util::is_safe_filename(world) || !crate::util::is_safe_filename(filename) {
        return Err("非法的路径参数".into());
    }
    let p = backups_dir(state, instance_id, world).join(filename);
    if !p.is_file() {
        return Err("备份文件不存在".into());
    }
    std::fs::remove_file(&p).map_err(|e| format!("删除失败: {e}"))
}

/// 读取 zip 中的单个条目（恢复流程之外的辅助）
#[allow(dead_code)]
fn read_entry(archive: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Result<Vec<u8>, String> {
    let mut e = archive
        .by_name(name)
        .map_err(|e| format!("zip 内缺少 {name}: {e}"))?;
    let mut buf = Vec::new();
    Read::read_to_end(&mut e, &mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}
