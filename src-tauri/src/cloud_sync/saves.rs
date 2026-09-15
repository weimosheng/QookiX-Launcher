//! 世界存档打包与恢复。
//!
//! - 打包：`saves/<world>` → zip（排除 `session.lock` 等运行时文件）
//! - 恢复：先备份现有世界目录，再解压覆盖（不做静默覆盖）
//! - 世界标识：本地 (实例, 目录名) ↔ 云端 world_id 的映射，首次自动分配

use super::store::{self, WorldLink};
use crate::state::AppState;
use sha2::Digest;
use std::io::Read;
use std::path::{Path, PathBuf};

/// 打包时排除的运行时文件（游戏运行中这些文件不可靠/无意义）
fn is_excluded(name: &str) -> bool {
    matches!(name, "session.lock")
}

fn world_dir(root: &Path, instance_id: &str, world: &str) -> PathBuf {
    root.join("instances")
        .join(instance_id)
        .join("saves")
        .join(world)
}

/// 取本地世界对应的云端 world_id；没有则分配一个并持久化。
pub fn world_id_for(state: &AppState, instance_id: &str, world: &str) -> String {
    let mut cs = store::load(state);
    if let Some(link) = cs
        .worlds
        .iter()
        .find(|w| w.instance_id == instance_id && w.world_dir == world)
    {
        return link.world_id.clone();
    }
    let id = uuid::Uuid::new_v4().simple().to_string();
    cs.worlds.push(WorldLink {
        instance_id: instance_id.to_string(),
        world_dir: world.to_string(),
        world_id: id.clone(),
        auto_sync: false,
    });
    let _ = store::save(state, &cs);
    id
}

/// 设置某个世界的自动同步开关。
pub fn set_auto_sync(
    state: &AppState,
    instance_id: &str,
    world: &str,
    enabled: bool,
) -> Result<(), String> {
    let mut cs = store::load(state);
    if let Some(link) = cs
        .worlds
        .iter_mut()
        .find(|w| w.instance_id == instance_id && w.world_dir == world)
    {
        link.auto_sync = enabled;
    } else {
        cs.worlds.push(WorldLink {
            instance_id: instance_id.to_string(),
            world_dir: world.to_string(),
            world_id: uuid::Uuid::new_v4().simple().to_string(),
            auto_sync: enabled,
        });
    }
    store::save(state, &cs)
}

/// 兼容保留（旧单卷打包路径，暂未使用）
#[allow(dead_code)]
fn write_dir_to_zip(
    zw: &mut zip::ZipWriter<std::fs::File>,
    dir: &Path,
    prefix: &str,
    done: &mut u64,
    total: u64,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<(), String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("读取目录失败: {e}"))?;
    for e in rd.flatten() {
        let p = e.path();
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if is_excluded(&name) {
            continue;
        }
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if p.is_dir() {
            write_dir_to_zip(zw, &p, &rel, done, total, progress)?;
        } else if p.is_file() {
            let mut f =
                std::fs::File::open(&p).map_err(|e| format!("打开 {} 失败: {e}", p.display()))?;
            // 仅存储不压缩：MC 存档的 region 文件内部已 zlib 压缩，再压缩
            // 体积收益 <5% 却慢一个数量级；备份场景优先速度。
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zw.start_file(rel.clone(), opts)
                .map_err(|e| format!("写入 zip 条目 {rel} 失败: {e}"))?;
            let before = *done;
            std::io::copy(&mut f, zw).map_err(|e| format!("写入 {rel} 内容失败: {e}"))?;
            *done = before + p.metadata().map(|m| m.len()).unwrap_or(0);
            progress((*done).min(total), total);
        }
    }
    Ok(())
}

/// 递归统计目录字节数（排除 session.lock）
fn dir_size(dir: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                total += dir_size(&p);
            } else if let Ok(m) = p.metadata() {
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if !is_excluded(&name) {
                    total += m.len();
                }
            }
        }
    }
    total
}

/// GitHub Release 单个附件上限 2 GiB；分卷按 1.9 GiB 切，留安全余量
pub const MAX_ASSET_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const PART_TARGET_BYTES: u64 = 1900 * 1024 * 1024;

/// 一个分卷：临时 zip 路径 + 上传附件名
pub struct PackedPart {
    pub path: PathBuf,
    pub asset_name: String,
    pub size: u64,
}

/// 打包世界到临时文件（超 2GB 时自动按文件边界分卷）。
/// 返回全部分卷与 zip 总字节数。每个卷都是独立 zip（条目路径相对存档根），
/// 恢复时按顺序解压到同一目录即合并还原。
/// `progress(done, total)` 回调打包进度（总字节数按源目录预扫描）。
pub fn pack_world(
    root: &Path,
    instance_id: &str,
    world: &str,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<(Vec<PackedPart>, u64), String> {
    if !crate::util::is_safe_filename(world) {
        return Err("非法的世界目录名".into());
    }
    let src = world_dir(root, instance_id, world);
    if !src.is_dir() {
        return Err(format!("世界目录不存在: {}", src.display()));
    }
    let total = dir_size(&src);
    let tmp_dir = root.join("cloud_sync").join("tmp");
    std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("创建临时目录失败: {e}"))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // 收集所有文件（相对路径 + 大小），按大小降序排（大文件先进卷，减少碎片空间浪费）
    let mut files: Vec<(PathBuf, String, u64)> = Vec::new();
    collect_files(&src, "", &mut files)?;
    files.sort_by(|a, b| b.2.cmp(&a.2));
    // 单文件本身就超上限则无法分卷
    if let Some((p, _, sz)) = files.iter().find(|(_, _, s)| *s > MAX_ASSET_BYTES) {
        return Err(format!(
            "存档内单文件 {}（{}）超过 GitHub 单文件上限 2GB，无法上传",
            p.display(),
            fmt_size_lossy(*sz)
        ));
    }

    // 分配卷：贪心装箱（逐个放入当前未满的卷）
    let need_parts = if total > MAX_ASSET_BYTES {
        (total / PART_TARGET_BYTES + 1).max(2) as usize
    } else {
        1
    };
    let mut parts: Vec<(Vec<(PathBuf, String, u64)>, u64)> = vec![(Vec::new(), 0u64); need_parts];
    for f in &files {
        // 找第一个放得下的卷（否则开新卷）
        let placed = parts
            .iter_mut()
            .find(|(_, sz)| sz + f.2 <= PART_TARGET_BYTES)
            .map(|(list, sz)| {
                list.push(f.clone());
                *sz += f.2;
                true
            })
            .is_some();
        if !placed {
            parts.push((vec![f.clone()], f.2));
        }
    }

    // 逐卷写 zip
    let mut out = Vec::new();
    let mut done = 0u64;
    for (pi, (list, _)) in parts.iter().enumerate() {
        if list.is_empty() {
            continue;
        }
        let suffix = if parts.len() > 1 {
            format!("-part{}", pi + 1)
        } else {
            String::new()
        };
        let zip_path = tmp_dir.join(format!("{world}-{stamp}{suffix}.zip"));
        let file = std::fs::File::create(&zip_path).map_err(|e| format!("创建压缩包失败: {e}"))?;
        let mut zw = zip::ZipWriter::new(file);
        for (abs, rel, _sz) in list {
            let mut f = std::fs::File::open(abs)
                .map_err(|e| format!("打开 {} 失败: {e}", abs.display()))?;
            // 仅存储不压缩：MC 存档 region 文件内部已 zlib 压缩，再压缩收益极小
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zw.start_file(rel.clone(), opts)
                .map_err(|e| format!("写入 zip 条目 {rel} 失败: {e}"))?;
            std::io::copy(&mut f, &mut zw).map_err(|e| format!("写入 {rel} 内容失败: {e}"))?;
            done += _sz;
            progress(done.min(total), total);
        }
        zw.finish().map_err(|e| format!("完成压缩失败: {e}"))?;
        let size = std::fs::metadata(&zip_path).map(|m| m.len()).unwrap_or(0);
        out.push(PackedPart {
            path: zip_path,
            asset_name: format!("save{suffix}.zip"),
            size,
        });
    }
    let total_zipped = out.iter().map(|p| p.size).sum();
    Ok((out, total_zipped))
}

fn collect_files(dir: &Path, prefix: &str, out: &mut Vec<(PathBuf, String, u64)>) -> Result<(), String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("读取目录失败: {e}"))?;
    for e in rd.flatten() {
        let p = e.path();
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if is_excluded(&name) {
            continue;
        }
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if p.is_dir() {
            collect_files(&p, &rel, out)?;
        } else if p.is_file() {
            let sz = p.metadata().map(|m| m.len()).unwrap_or(0);
            out.push((p, rel, sz));
        }
    }
    Ok(())
}

fn fmt_size_lossy(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1}GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else {
        format!("{:.0}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// 计算文件 sha256（小写十六进制）。
pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("打开文件失败: {e}"))?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|e| format!("读取失败: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// 恢复：把一个或多个分卷 zip 解压到 `saves/<world>`。
/// 分卷条目路径相对存档根，按顺序解压到同一目录即合并还原。
/// 若该世界已存在，先整体改名为 `saves/<world>.bak-<时间戳>`（绝不静默覆盖）。
pub fn unpack_to_world(
    root: &Path,
    instance_id: &str,
    world: &str,
    zip_paths: &[PathBuf],
) -> Result<Option<String>, String> {
    if !crate::util::is_safe_filename(world) {
        return Err("非法的世界目录名".into());
    }
    let dest = world_dir(root, instance_id, world);
    let mut backup_name: Option<String> = None;
    if dest.exists() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let bak = format!("{world}.bak-{stamp}");
        let bak_path = dest
            .parent()
            .ok_or("路径异常")?
            .join(&bak);
        std::fs::rename(&dest, &bak_path).map_err(|e| format!("备份现有世界失败: {e}"))?;
        backup_name = Some(bak);
    }
    std::fs::create_dir_all(&dest).map_err(|e| format!("创建世界目录失败: {e}"))?;
    for zip_path in zip_paths {
        let file = std::fs::File::open(zip_path).map_err(|e| format!("打开压缩包失败: {e}"))?;
        let mut archive =
            zip::ZipArchive::new(file).map_err(|e| format!("解析压缩包失败: {e}"))?;
        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|e| format!("读取压缩条目失败: {e}"))?;
            let Some(rel) = entry.enclosed_name().map(|p| p.to_path_buf()) else {
                continue; // 拒绝越界路径
            };
            let out_path = dest.join(&rel);
            if entry.is_dir() {
                std::fs::create_dir_all(&out_path).ok();
                continue;
            }
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            let mut out = std::fs::File::create(&out_path)
                .map_err(|e| format!("写入 {} 失败: {e}", out_path.display()))?;
            std::io::copy(&mut entry, &mut out).map_err(|e| format!("解压失败: {e}"))?;
        }
    }
    Ok(backup_name)
}
