//! NBT 编辑前的单文件备份。
//!
//! 备份与源文件**同目录**（`level.dat.qookix_backup.YYYYMMDD_HHMMSS`），
//! 这样实例内的世界与手动指定的世界目录走同一套逻辑。
//! 游戏不会读取这些文件：playerdata 只扫描 `*.dat`，存档目录名也固定。
//! 每个源文件保留最近 KEEP 份。

use serde_json::{json, Value as Json};
use std::path::{Path, PathBuf};

/// 每个源文件保留的备份份数
const KEEP: usize = 10;

/// 备份单个文件，返回备份文件名（不含目录）。
/// 命名：`<原文件名>.qookix_backup.YYYYMMDD_HHMMSS`
pub fn backup_file(src: &Path) -> Result<String, String> {
    backup_file_with(src, KEEP)
}

/// 指定保留份数的版本：region 文件（.mca）动辄好几 MB，一份能顶几十个
/// level.dat，所以区块编辑只留少数几份。
pub fn backup_file_with(src: &Path, keep: usize) -> Result<String, String> {
    let dir = src
        .parent()
        .ok_or_else(|| "无法确定备份目录".to_string())?
        .to_path_buf();
    let base = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| "无法确定文件名".to_string())?;
    // 同一秒内连续保存时时间戳会撞名，加序号避免覆盖上一份备份
    let stamp = stamp_now();
    let mut name = format!("{base}.qookix_backup.{stamp}");
    let mut seq = 1;
    while dir.join(&name).exists() {
        name = format!("{base}.qookix_backup.{stamp}_{seq:02}");
        seq += 1;
    }
    std::fs::copy(src, dir.join(&name)).map_err(|e| format!("备份文件失败: {e}"))?;
    prune(&dir, &base, keep);
    Ok(name)
}

// ------------------------------------------------------------ 备份管理 ----

/// 世界目录下所有可备份的目标（相对路径 → 绝对路径）：
/// `level.dat` 与 `playerdata/<uuid>.dat`
fn targets(world_dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out = vec![("level.dat".to_string(), world_dir.join("level.dat"))];
    if let Ok(rd) = std::fs::read_dir(world_dir.join("playerdata")) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("dat") {
                continue;
            }
            if let Some(name) = p.file_name().map(|n| n.to_string_lossy().to_string()) {
                out.push((format!("playerdata/{name}"), p));
            }
        }
    }
    out
}

/// 列出该世界下的全部单文件备份（时间倒序）。
pub fn list_backups(world_dir: &Path) -> Vec<Json> {
    let mut out = Vec::new();
    for (rel, path) in targets(world_dir) {
        let Some(dir) = path.parent() else { continue };
        let Some(base) = path.file_name().map(|n| n.to_string_lossy().to_string()) else {
            continue;
        };
        let prefix = format!("{base}.qookix_backup.");
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.starts_with(&prefix) {
                continue;
            }
            let (size, modified) = match e.metadata() {
                Ok(m) => (
                    m.len(),
                    m.modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                ),
                Err(_) => (0, 0),
            };
            out.push(json!({
                "file": rel,
                "name": name,
                "size": size,
                "modified": modified,
            }));
        }
    }
    out.sort_by(|a, b| {
        b.get("modified")
            .and_then(|x| x.as_u64())
            .unwrap_or(0)
            .cmp(&a.get("modified").and_then(|x| x.as_u64()).unwrap_or(0))
    });
    out
}

/// 定位一个备份文件，顺便挡掉路径穿越：
/// `file` 是相对世界目录的路径（`level.dat` / `playerdata/<uuid>.dat`），
/// `name` 必须确实是该文件的备份（前缀是 `<原文件名>.qookix_backup.`）。
fn backup_path(world_dir: &Path, file: &str, name: &str) -> Result<PathBuf, String> {
    if file.is_empty()
        || file.contains("..")
        || file.contains('\\')
        || file.contains(':')
        || file.starts_with('/')
    {
        return Err("非法的文件路径".into());
    }
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains(':') {
        return Err("非法的备份文件名".into());
    }
    let target = world_dir.join(file);
    let base = target
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| "无法确定文件名".to_string())?;
    if !name.starts_with(&format!("{base}.qookix_backup.")) {
        return Err("不是该文件的备份".into());
    }
    let dir = target
        .parent()
        .ok_or_else(|| "无法确定备份目录".to_string())?;
    let p = dir.join(name);
    if !p.starts_with(world_dir) {
        return Err("路径越界".into());
    }
    if !p.is_file() {
        return Err("备份文件不存在".into());
    }
    Ok(p)
}

/// 用备份覆盖回原文件。恢复前先给"当前内容"再留一份快照，
/// 避免恢复之后才发现选错了备份（返回这次安全快照的文件名）。
pub fn restore_backup(world_dir: &Path, file: &str, name: &str) -> Result<String, String> {
    let src = backup_path(world_dir, file, name)?;
    let target = world_dir.join(file);
    let safety = if target.is_file() {
        backup_file(&target)?
    } else {
        String::new()
    };
    std::fs::copy(&src, &target).map_err(|e| format!("恢复备份失败: {e}"))?;
    Ok(safety)
}

/// 删除一个备份文件
pub fn delete_backup(world_dir: &Path, file: &str, name: &str) -> Result<(), String> {
    let p = backup_path(world_dir, file, name)?;
    std::fs::remove_file(p).map_err(|e| format!("删除备份失败: {e}"))
}

/// 只保留该源文件最新的 `keep` 份备份，超出的最旧先删。
fn prune(dir: &Path, base: &str, keep: usize) {
    let prefix = format!("{base}.qookix_backup.");
    let mut entries: Vec<(String, std::time::SystemTime)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if !n.starts_with(&prefix) {
                continue;
            }
            let mtime = e.metadata().ok().and_then(|m| m.modified().ok());
            entries.push((n, mtime.unwrap_or(std::time::UNIX_EPOCH)));
        }
    }
    if entries.len() <= keep {
        return;
    }
    // 新的在前；时间戳精度只到秒，同秒时按文件名兜底（序号已补零，字典序即时间序）
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.cmp(&a.0)));
    for (n, _) in entries.iter().skip(keep) {
        let _ = std::fs::remove_file(dir.join(n));
    }
}

/// UTC 时间戳 `YYYYMMDD_HHMMSS`（不引入 chrono 依赖，够用于备份排序）
fn stamp_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86400;
    let rem = secs % 86400;
    let (y, m, d) = civil_from_days(days as i64);
    format!(
        "{:04}{:02}{:02}_{:02}{:02}{:02}",
        y,
        m,
        d,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// 天数 → 年月日（Howard Hinnant 的 civil_from_days 算法）
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_stays_next_to_source_and_prunes() {
        let dir = std::env::temp_dir().join(format!("qookix-nbt-bak-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("level.dat");
        std::fs::write(&src, b"payload").unwrap();

        for _ in 0..(KEEP + 3) {
            backup_file(&src).unwrap();
        }
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.contains(".qookix_backup."))
            .collect();
        assert_eq!(names.len(), KEEP, "超出上限的旧备份要被清掉");
        // 源文件本身不受影响
        assert_eq!(std::fs::read(&src).unwrap(), b"payload");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn seed_world(dir: &Path) {
        std::fs::create_dir_all(dir.join("playerdata")).unwrap();
        std::fs::write(dir.join("level.dat"), b"v1").unwrap();
        std::fs::write(dir.join("playerdata/abc.dat"), b"p1").unwrap();
    }

    #[test]
    fn list_covers_level_and_playerdata() {
        let dir = std::env::temp_dir().join(format!("qookix-nbt-list-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        seed_world(&dir);
        backup_file(&dir.join("level.dat")).unwrap();
        backup_file(&dir.join("playerdata/abc.dat")).unwrap();

        let list = list_backups(&dir);
        assert_eq!(list.len(), 2, "level.dat 与玩家数据各一份");
        let files: Vec<&str> = list
            .iter()
            .filter_map(|e| e.get("file").and_then(|x| x.as_str()))
            .collect();
        assert!(files.contains(&"level.dat"));
        assert!(files.contains(&"playerdata/abc.dat"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_overwrites_and_keeps_safety_copy() {
        let dir = std::env::temp_dir().join(format!("qookix-nbt-restore-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        seed_world(&dir);
        let src = dir.join("level.dat");
        let name = backup_file(&src).unwrap(); // 备份内容 = "v1"
        std::fs::write(&src, b"v2").unwrap();

        let safety = restore_backup(&dir, "level.dat", &name).unwrap();
        assert_eq!(std::fs::read(&src).unwrap(), b"v1", "备份内容要盖回去");
        assert!(!safety.is_empty(), "恢复前的当前内容要留一份");
        // 安全快照里存的是恢复前的 v2
        assert_eq!(
            std::fs::read(dir.join(&safety)).unwrap(),
            b"v2",
            "安全快照应保留恢复前的内容"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_and_path_traversal_guards() {
        let dir = std::env::temp_dir().join(format!("qookix-nbt-del-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        seed_world(&dir);
        let name = backup_file(&dir.join("level.dat")).unwrap();

        // 越界 / 伪造的路径一律拒绝
        assert!(backup_path(&dir, "../level.dat", &name).is_err());
        assert!(backup_path(&dir, "level.dat", "../level.dat.qookix_backup.x").is_err());
        assert!(backup_path(&dir, "C:\\x\\level.dat", &name).is_err());
        // 别的文件的备份不能张冠李戴
        assert!(backup_path(&dir, "playerdata/abc.dat", &name).is_err());

        delete_backup(&dir, "level.dat", &name).unwrap();
        assert!(list_backups(&dir).is_empty(), "删完就没有备份了");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
