//! 实例导出 / 分享：
//! - 导出：把实例元信息（MC 版本、加载器）+ 全部内容记录 + 内容文件
//!   （mods / 材质包 / 光影，含禁用态）打包为一个 `.qkxinst`（zip）分享包。
//! - 导入：解析分享包 → 新建实例 → 还原内容文件与记录。
//!   游戏本体（libraries/assets）不在包内，导入后需在实例里点「安装游戏」。

use crate::models::InstalledContent;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;

pub const PACK_EXT: &str = "qkxinst";

/// 导出选项：勾选哪些内容进分享包
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ExportOptions {
    #[serde(default = "default_true")]
    pub mods: bool,
    #[serde(default = "default_true")]
    pub resourcepacks: bool,
    #[serde(default = "default_true")]
    pub shaders: bool,
    /// 世界存档（saves/ 全部）
    #[serde(default)]
    pub worlds: bool,
    /// 配置文件（config/ 目录 + options.txt）
    #[serde(default)]
    pub config: bool,
}
fn default_true() -> bool { true }

impl Default for ExportOptions {
    fn default() -> Self {
        Self { mods: true, resourcepacks: true, shaders: true, worlds: false, config: false }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ShareItem {
    /// mod | resourcepack | shader
    kind: String,
    record: InstalledContent,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ShareMeta {
    format: u32,
    name: String,
    mc_version: String,
    loader: String,
    loader_version: Option<String>,
    /// 导出时间（unix 秒）
    exported_at: u64,
    #[serde(default)]
    count: usize,
    /// 本次导出的勾选项（导入端按它还原存档/配置）
    #[serde(default)]
    options: ExportOptions,
    items: Vec<ShareItem>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 导出实例到 `dest`（由前端保存对话框给出）。返回打包的内容条数。
/// 递归把 `dir` 下所有文件写进 zip（相对 `dir` 的路径，前缀 `prefix`）
fn write_dir_to_zip(zw: &mut zip::ZipWriter<std::fs::File>, dir: &Path, prefix: &str) -> Result<(), String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("读取目录失败: {e}"))?;
    for e in rd.flatten() {
        let p = e.path();
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        if p.is_dir() {
            write_dir_to_zip(zw, &p, &rel)?;
        } else if p.is_file() {
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zw.start_file(rel.clone(), opts).map_err(|e| e.to_string())?;
            let mut f = std::fs::File::open(&p).map_err(|e| format!("打开 {} 失败: {e}", p.display()))?;
            std::io::copy(&mut f, zw).map_err(|e| format!("写入 {rel} 失败: {e}"))?;
        }
    }
    Ok(())
}

pub fn export_pack(
    state: &AppState,
    instance_id: &str,
    dest: &Path,
    options: ExportOptions,
) -> Result<usize, String> {
    let inst = crate::instances::get_instance(state, instance_id)?;
    let instance_dir = state.instances_dir().join(&inst.id);

    let mut items: Vec<ShareItem> = Vec::new();
    let file = std::fs::File::create(dest).map_err(|e| format!("创建分享包失败: {e}"))?;
    let mut zw = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let kind_enabled = |k: &str| -> bool {
        match k {
            "resourcepack" => options.resourcepacks,
            "shader" => options.shaders,
            _ => options.mods,
        }
    };

    for kind in ["mod", "resourcepack", "shader"] {
        if !kind_enabled(kind) {
            continue;
        }
        for rec in crate::instances::list_content(state, instance_id, kind) {
            // 在线来源（有项目+版本 id）不打包文件，只记录元信息，
            // 导入端自动从 Modrinth/CurseForge 重新下载——分享包体积小一个量级
            let is_remote = (rec.source == "modrinth" || rec.source == "curseforge")
                && rec.project_id.is_some()
                && rec.version_id.is_some();
            if is_remote {
                items.push(ShareItem {
                    kind: kind.to_string(),
                    record: rec,
                });
                continue;
            }
            let folder = crate::modrinth::kind_folder(kind);
            // 禁用态文件实际名带 .disabled 后缀
            let active = instance_dir.join(&folder).join(&rec.filename);
            let disabled = instance_dir.join(&folder).join(format!("{}.disabled", rec.filename));
            let (src_path, zip_name) = if active.is_file() {
                (active, format!("files/{folder}/{}", rec.filename))
            } else if disabled.is_file() {
                (disabled, format!("files/{folder}/{}.disabled", rec.filename))
            } else {
                continue; // 磁盘上已不存在的记录（文件缺失）不打包
            };
            let mut f = std::fs::File::open(&src_path)
                .map_err(|e| format!("打开 {} 失败: {e}", src_path.display()))?;
            zw.start_file(zip_name, opts).map_err(|e| e.to_string())?;
            std::io::copy(&mut f, &mut zw).map_err(|e| e.to_string())?;
            items.push(ShareItem {
                kind: kind.to_string(),
                record: rec,
            });
        }
    }

    // 世界存档：saves/ 下每个世界整体打包（worlds/<世界名>/...）
    let mut world_count = 0usize;
    if options.worlds {
        let saves_dir = instance_dir.join("saves");
        if let Ok(rd) = std::fs::read_dir(&saves_dir) {
            for e in rd.flatten() {
                let p = e.path();
                if !p.is_dir() {
                    continue;
                }
                let world = e.file_name().to_string_lossy().to_string();
                write_dir_to_zip(&mut zw, &p, &format!("worlds/{world}"))?;
                world_count += 1;
            }
        }
    }

    // 配置文件：config/ 目录 + options.txt
    if options.config {
        let config_dir = instance_dir.join("config");
        if config_dir.is_dir() {
            write_dir_to_zip(&mut zw, &config_dir, "config")?;
        }
        let options_txt = instance_dir.join("options.txt");
        if options_txt.is_file() {
            zw.start_file("instance/options.txt", opts).map_err(|e| e.to_string())?;
            let mut f = std::fs::File::open(&options_txt).map_err(|e| e.to_string())?;
            std::io::copy(&mut f, &mut zw).map_err(|e| e.to_string())?;
        }
    }

    let meta = ShareMeta {
        format: 2,
        name: inst.name.clone(),
        mc_version: inst.mc_version.clone(),
        loader: inst.loader.as_str().to_string(),
        loader_version: inst.loader_version.clone(),
        exported_at: now_secs(),
        count: items.len(),
        options: options.clone(),
        items,
    };
    let meta_json = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?;
    zw.start_file("qookix-instance.json", opts).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut zw, meta_json.as_bytes()).map_err(|e| e.to_string())?;
    zw.finish().map_err(|e| format!("完成分享包失败: {e}"))?;
    let _ = world_count;
    Ok(meta.count)
}

/// 导入时需要在线补下载的条目（remote 来源且包内无文件）
#[derive(Serialize, Clone, Debug)]
pub struct PendingDownload {
    pub kind: String,
    pub provider: String,
    pub project_id: String,
    pub version_id: String,
    pub name: Option<String>,
}

/// 从分享包导入：新建实例、还原包内内容文件与记录。
/// 返回 (新实例 id, 待在线补下载的条目)。游戏本体未安装，由用户自行「安装游戏」。
pub fn import_pack(state: &AppState, src: &Path) -> Result<(String, Vec<PendingDownload>), String> {
    let file = std::fs::File::open(src).map_err(|e| format!("打开分享包失败: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("解析分享包失败: {e}"))?;

    let mut meta_buf = Vec::new();
    {
        let mut entry = archive
            .by_name("qookix-instance.json")
            .map_err(|_| "不是有效的 QookiX 实例分享包（缺少 qookix-instance.json）".to_string())?;
        entry.read_to_end(&mut meta_buf).map_err(|e| e.to_string())?;
    }
    let meta: ShareMeta = serde_json::from_slice(&meta_buf).map_err(|e| format!("分享包元信息解析失败: {e}"))?;
    if meta.format > 2 {
        return Err(format!("不支持的分享包格式版本: {}（请升级启动器）", meta.format));
    }

    let loader: crate::models::LoaderType = meta.loader.parse()?;
    let inst = crate::instances::create_instance(
        state,
        meta.name.clone(),
        meta.mc_version.clone(),
        loader,
        meta.loader_version.clone(),
    )?;

    // 还原内容：包内有文件直接解出；在线来源且包内无文件 → 待下载清单
    let mut by_kind: std::collections::HashMap<String, Vec<InstalledContent>> =
        std::collections::HashMap::new();
    let mut pending: Vec<PendingDownload> = Vec::new();
    for item in &meta.items {
        let folder = crate::modrinth::kind_folder(&item.kind);
        let enabled = item.record.enabled;
        let disk_name = if enabled {
            item.record.filename.clone()
        } else {
            format!("{}.disabled", item.record.filename)
        };
        let zip_name = format!("files/{folder}/{disk_name}");
        if archive.by_name(&zip_name).is_err() {
            // 包内没有文件：format 2 的在线来源走重新下载；
            // 其余情况（文件缺失）跳过并记录
            let is_remote = (item.record.source == "modrinth" || item.record.source == "curseforge")
                && item.record.project_id.is_some()
                && item.record.version_id.is_some();
            if is_remote {
                pending.push(PendingDownload {
                    kind: item.kind.clone(),
                    provider: item.record.source.clone(),
                    project_id: item.record.project_id.clone().unwrap_or_default(),
                    version_id: item.record.version_id.clone().unwrap_or_default(),
                    name: item.record.name.clone(),
                });
            }
            continue;
        }
        let dest_dir = state.instances_dir().join(&inst.id).join(&folder);
        std::fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
        let mut out = std::fs::File::create(dest_dir.join(&disk_name))
            .map_err(|e| format!("写入 {disk_name} 失败: {e}"))?;
        let mut entry = archive.by_name(&zip_name).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("复制 {disk_name} 失败: {e}"))?;
        by_kind
            .entry(item.kind.clone())
            .or_default()
            .push(item.record.clone());
    }
    for (kind, records) in by_kind {
        crate::instances::add_content_batch(state, &inst.id, &kind, records)?;
    }

    // 还原世界存档与配置（worlds/<name>/... → saves/<name>/；config/ → config/；
    // instance/options.txt → options.txt）。逐条目写入，安全清洗路径。
    let inst_dir = state.instances_dir().join(&inst.id);
    for i in 0..archive.len() {
        let name = archive.by_index(i).map(|e| e.name().to_string()).unwrap_or_default();
        let (dest_root, rel) = if let Some(rest) = name.strip_prefix("worlds/") {
            // worlds/<世界名>/<剩余>，世界名不能含路径分隔
            let Some(slash) = rest.find('/') else { continue };
            let world = &rest[..slash];
            if world.is_empty() || world.contains("..") || world.contains('/') {
                continue;
            }
            (inst_dir.join("saves").join(world), rest[slash + 1..].to_string())
        } else if let Some(rest) = name.strip_prefix("config/") {
            if rest.is_empty() { continue; }
            (inst_dir.join("config"), rest.to_string())
        } else if name == "instance/options.txt" {
            (inst_dir.clone(), "options.txt".to_string())
        } else {
            continue;
        };
        let mut clean: Vec<&str> = rel.split('/').filter(|s| !s.is_empty() && *s != "..").collect();
        if clean.is_empty() {
            continue;
        }
        let file_name = clean.pop().unwrap_or_default();
        let mut dest = dest_root.clone();
        for seg in &clean {
            dest.push(seg);
        }
        std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
        dest.push(file_name);
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
            continue;
        }
        let mut out = std::fs::File::create(&dest).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("还原 {} 失败: {e}", name))?;
    }

    Ok((inst.id, pending))
}
