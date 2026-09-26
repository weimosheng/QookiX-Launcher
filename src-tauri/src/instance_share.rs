//! 实例导出 / 分享：
//! - 预览：扫描实例里可导出的全部条目（模组 / 资源包 / 光影 / 截图 / 存档 /
//!   附属文件夹 / 设置文件），供前端做逐项勾选。
//! - 导出：按勾选项打包为一个 `.qkxinst`（zip）。
//!   在线来源的模组只记录版本 ID（体积小一个量级），导入时自动重新下载。
//! - 导入：解析分享包 → 新建实例 → 还原文件与内容记录。
//!   游戏本体（libraries/assets）不在包内，导入后需在实例里点「安装游戏」。

use crate::models::InstalledContent;
use crate::state::AppState;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;
use std::sync::atomic::Ordering;
use tauri::Emitter;

#[allow(dead_code)]
pub const PACK_EXT: &str = "qkxinst";

// ---------------------------------------------------------------------------
// 预览
// ---------------------------------------------------------------------------

/// 一个可勾选条目（文件 / 世界 / 文件夹）
#[derive(Serialize, Clone, Debug)]
pub struct ExportItem {
    /// 唯一 key：文件名 / 世界名 / 文件夹名
    pub key: String,
    pub label: String,
    /// 字节大小（目录为递归合计）
    pub size: u64,
    /// 右侧灰色补充说明
    pub hint: Option<String>,
}

/// 一组勾选项
#[derive(Serialize, Clone, Debug)]
pub struct ExportGroup {
    pub key: String,
    pub label: String,
    /// 必含项（游戏本体信息，灰显不可取消）
    pub required: bool,
    pub hint: Option<String>,
    pub items: Vec<ExportItem>,
}

#[derive(Serialize, Clone, Debug)]
pub struct ExportPreview {
    pub name: String,
    pub mc_version: String,
    pub loader: String,
    pub loader_version: Option<String>,
    pub groups: Vec<ExportGroup>,
}

/// 导出选择：前端逐项勾选的结果。数组为空即该类不选。
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ExportSelection {
    /// 导出包的整合包名称（默认实例名）
    pub name: Option<String>,
    /// 整合包版本（仅作记录）
    pub version: Option<String>,
    /// 选中的模组文件名（含 .disabled 后缀的禁用态）
    pub mods: Vec<String>,
    /// 是否连带导出已禁用的模组
    pub include_disabled_mods: bool,
    pub resourcepacks: Vec<String>,
    pub shaders: Vec<String>,
    pub screenshots: Vec<String>,
    /// 选中的世界目录名
    pub worlds: Vec<String>,
    /// 选中的实例子目录（config、kubejs、tacz 等）
    pub folders: Vec<String>,
    /// options.txt（键位、音量、视频等游戏设置）
    pub options_txt: bool,
    /// servers.dat（多人服务器列表）
    pub servers_dat: bool,
    /// 在线来源的模组也直接打包文件（默认 false=只记引用，导入时重新下载）。
    /// 适合无法稳定连接 CurseForge / Modrinth 的接收方。
    #[serde(default)]
    pub bundle_online_files: bool,
    /// 打包在线来源文件时仅限 Modrinth 来源——CurseForge 的分发协议禁止
    /// 第三方整合包转打包其文件，Modrinth 无此限制。
    #[serde(default)]
    pub modrinth_only: bool,
    /// 用户采纳的「未登记模组 → Modrinth 来源」识别结果：
    /// 这些文件改为按引用导出（导入时重新下载），不再打包实体文件。
    #[serde(default)]
    pub identified: Vec<IdentifiedMod>,
}

// ---------------------------------------------------------------------------
// 附属文件夹识别：常见 mod → 它在实例目录下额外产生的数据文件夹
// ---------------------------------------------------------------------------

/// (mod 文件名关键字, 该 mod 会在实例根目录下产生的文件夹)
const MOD_FOLDER_RULES: &[(&str, &[&str])] = &[
    ("tacz", &["tacz"]),
    ("patchouli", &["patchouli_books"]),
    ("kubejs", &["kubejs"]),
    ("openloader", &["openloader"]),
    ("jei", &["jei"]),
    ("xaero", &["XaeroWorldMap", "XaeroWaypoints"]),
    ("ftbquests", &["ftbquests"]),
    ("computercraft", &["computercraft"]),
    ("apotheosis", &["apotheosis"]),
    ("waystones", &["waystones"]),
    ("ironchest", &["ironchest"]),
    ("sophisticated", &["sophisticatedbackpacks"]),
    ("slightguimodifications", &["slightguimodifications"]),
    ("botania", &["botania"]),
];

/// 已知的“重要数据”文件夹：无论有没有对应 mod，存在就列出供勾选
const KNOWN_DATA_FOLDERS: &[&str] = &[
    "kubejs",           // 脚本
    "openloader",       // 数据包 / 资源包加载器
    "config",           // mod 配置
    "scripts",          // 其它脚本加载器
    "patchouli_books",  // 帕秋莉手册
    "resourcepacks_extra",
];

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    let Ok(rd) = std::fs::read_dir(path) else { return 0 };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            total += dir_size(&p);
        } else if let Ok(m) = p.metadata() {
            total += m.len();
        }
    }
    total
}

fn dir_size_cached(path: &Path) -> u64 {
    // 目录大小可能很耗时（大存档），这里直接算；存档目录通常几十 MB 量级可接受
    dir_size(path)
}

fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains("..")
}

/// 列出目录下的一级条目（文件+文件夹），key 为名称
fn list_entries(dir: &Path) -> Vec<ExportItem> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if !is_safe_name(&name) {
            continue;
        }
        let size = if p.is_dir() { dir_size_cached(&p) } else { p.metadata().map(|m| m.len()).unwrap_or(0) };
        out.push(ExportItem {
            key: name.clone(),
            label: name,
            size,
            hint: None,
        });
    }
    out.sort_by(|a, b| b.size.cmp(&a.size));
    out
}

/// 扫描实例目录，生成可勾选清单
pub fn preview(state: &AppState, instance_id: &str) -> Result<ExportPreview, String> {
    let inst = crate::instances::get_instance(state, instance_id)?;
    let dir = state.instances_dir().join(&inst.id);
    let mut groups: Vec<ExportGroup> = Vec::new();

    // 游戏本体（必含，只展示信息）
    groups.push(ExportGroup {
        key: "game".into(),
        label: "游戏本体".into(),
        required: true,
        hint: Some(format!(
            "Minecraft {} · {}",
            inst.mc_version,
            loader_label(&inst)
        )),
        items: vec![ExportItem {
            key: "game".into(),
            label: format!("Minecraft {}（{}）", inst.mc_version, loader_label(&inst)),
            size: 0,
            hint: Some("导入后需联网安装，不计入包内".into()),
        }],
    });

    // 模组：以磁盘 mods/ 目录为准（分享包导入、手动迁移的实例可能没有内容
    // 记录），记录仅用于补充元信息（在线来源标记、显示名）。
    let mut mod_items: Vec<ExportItem> = Vec::new();
    {
        let mut rec_by_disk: std::collections::HashMap<String, InstalledContent> =
            std::collections::HashMap::new();
        for rec in crate::instances::list_content(state, instance_id, "mod") {
            let disk_name = if rec.enabled {
                rec.filename.clone()
            } else {
                format!("{}.disabled", rec.filename)
            };
            rec_by_disk.insert(disk_name, rec);
        }
        if let Ok(rd) = std::fs::read_dir(dir.join("mods")) {
            for e in rd.flatten() {
                let p = e.path();
                if !p.is_file() {
                    continue;
                }
                let disk_name = e.file_name().to_string_lossy().to_string();
                if !is_safe_name(&disk_name) || !disk_name.to_lowercase().ends_with(".jar") {
                    continue;
                }
                let size = p.metadata().map(|m| m.len()).unwrap_or(0);
                let rec = rec_by_disk.get(&disk_name);
                let remote = rec
                    .map(|r| {
                        (r.source == "modrinth" || r.source == "curseforge")
                            && r.project_id.is_some()
                            && r.version_id.is_some()
                    })
                    .unwrap_or(false);
                let disabled = disk_name.to_lowercase().ends_with(".jar.disabled");
                let mut hints: Vec<String> = Vec::new();
                if disabled {
                    hints.push("已禁用".into());
                }
                hints.push(if remote {
                    "在线来源，导入时重新下载".into()
                } else if rec.is_some() {
                    "本地文件，直接打包".into()
                } else {
                    "未登记，直接打包".into()
                });
                mod_items.push(ExportItem {
                    key: disk_name.clone(),
                    label: rec
                        .and_then(|r| r.name.clone())
                        .unwrap_or_else(|| disk_name.trim_end_matches(".disabled").to_string()),
                    size,
                    hint: Some(hints.join(" · ")),
                });
            }
        }
    }
    groups.push(ExportGroup {
        key: "mods".into(),
        label: "模组".into(),
        required: false,
        hint: Some(format!("{} 个", mod_items.len())),
        items: mod_items,
    });

    // 资源包 / 光影 / 截图：直接列目录内容
    for (key, folder, label) in [
        ("resourcepacks", "resourcepacks", "资源包"),
        ("shaders", "shaderpacks", "光影包"),
        ("screenshots", "screenshots", "截图"),
    ] {
        let items = list_entries(&dir.join(folder));
        if items.is_empty() {
            continue;
        }
        groups.push(ExportGroup {
            key: key.into(),
            label: label.into(),
            required: false,
            hint: Some(format!("{} 个", items.len())),
            items,
        });
    }

    // 世界存档（每个世界单独勾选）
    let mut world_items: Vec<ExportItem> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir.join("saves")) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if !is_safe_name(&name) {
                continue;
            }
            world_items.push(ExportItem {
                key: name.clone(),
                label: name,
                size: dir_size_cached(&p),
                hint: None,
            });
        }
    }
    if !world_items.is_empty() {
        world_items.sort_by(|a, b| b.size.cmp(&a.size));
        groups.push(ExportGroup {
            key: "worlds".into(),
            label: "单人游戏存档".into(),
            required: false,
            hint: Some(format!("{} 个世界", world_items.len())),
            items: world_items,
        });
    }

    // 附属文件夹：已知数据文件夹 + 按已装 mod 规则识别
    let mut folder_names: Vec<String> = Vec::new();
    let mod_names: Vec<String> = crate::instances::list_content(state, instance_id, "mod")
        .iter()
        .map(|r| r.filename.to_lowercase())
        .collect();
    for (kw, folders) in MOD_FOLDER_RULES {
        if mod_names.iter().any(|n| n.contains(kw)) {
            for f in *folders {
                let p = dir.join(f);
                if p.is_dir() && !folder_names.iter().any(|x| x == f) {
                    folder_names.push((*f).to_string());
                }
            }
        }
    }
    for f in KNOWN_DATA_FOLDERS {
        let p = dir.join(f);
        if p.is_dir() && !folder_names.iter().any(|x| x == *f) {
            folder_names.push((*f).to_string());
        }
    }
    let folder_items: Vec<ExportItem> = folder_names
        .iter()
        .map(|f| ExportItem {
            key: f.clone(),
            label: f.clone(),
            size: dir_size_cached(&dir.join(f)),
            hint: Some("实例子目录".into()),
        })
        .collect();
    if !folder_items.is_empty() {
        groups.push(ExportGroup {
            key: "folders".into(),
            label: "整合包重要数据".into(),
            required: false,
            hint: Some("脚本、配置、mod 附属数据".into()),
            items: folder_items,
        });
    }

    // 其它目录：实例根下不属于标准运行时目录、也不在识别规则里的，
    // 很可能是某个 mod 的附属数据（如 custommpcs、controllable_natives），
    // 一并列出让用户自行决定。
    const STANDARD_DIRS: &[&str] = &[
        "mods", "resourcepacks", "shaderpacks", "screenshots", "saves", "config",
        "logs", "crash-reports", "versions", "libraries", "assets", "natives",
        "bin", "patchouli_books", "kubejs", "openloader", "jei",
        "XaeroWorldMap", "XaeroWaypoints", "ftbquests", "computercraft",
        "tacz", "waystones", "ironchest", "sophisticatedbackpacks", "botania",
        "scripts", "config_backup",
    ];
    let mut other_items: Vec<ExportItem> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if !is_safe_name(&name) || STANDARD_DIRS.contains(&name.as_str()) {
                continue;
            }
            other_items.push(ExportItem {
                key: name.clone(),
                label: name,
                size: dir_size_cached(&p),
                hint: Some("未识别用途，确认需要再勾选".into()),
            });
        }
    }
    if !other_items.is_empty() {
        other_items.sort_by(|a, b| a.label.cmp(&b.label));
        groups.push(ExportGroup {
            key: "others".into(),
            label: "其它目录".into(),
            required: false,
            hint: Some("mod 附属数据或来源不明的目录".into()),
            items: other_items,
        });
    }

    // 设置文件（单文件级）
    let mut setting_items: Vec<ExportItem> = Vec::new();
    let options_txt = dir.join("options.txt");
    if options_txt.is_file() {
        setting_items.push(ExportItem {
            key: "options_txt".into(),
            label: "游戏设置（键位、音量、视频）".into(),
            size: options_txt.metadata().map(|m| m.len()).unwrap_or(0),
            hint: Some("options.txt".into()),
        });
    }
    let servers_dat = dir.join("servers.dat");
    if servers_dat.is_file() {
        setting_items.push(ExportItem {
            key: "servers_dat".into(),
            label: "多人游戏服务器列表".into(),
            size: servers_dat.metadata().map(|m| m.len()).unwrap_or(0),
            hint: Some("servers.dat".into()),
        });
    }
    if !setting_items.is_empty() {
        groups.push(ExportGroup {
            key: "settings".into(),
            label: "游戏本体设置".into(),
            required: false,
            hint: None,
            items: setting_items,
        });
    }

    Ok(ExportPreview {
        name: inst.name.clone(),
        mc_version: inst.mc_version.clone(),
        loader: inst.loader.as_str().to_string(),
        loader_version: inst.loader_version.clone(),
        groups,
    })
}

fn loader_label(inst: &crate::models::Instance) -> String {
    let l = inst.loader.as_str();
    if l == "vanilla" {
        return "原版".into();
    }
    match &inst.loader_version {
        Some(v) if !v.is_empty() => format!("{l} {v}"),
        _ => l.to_string(),
    }
}

// ---------------------------------------------------------------------------
// 打包内部格式（format 3）
//
// qookix-instance.json      元信息（含名称、版本、勾选项）
// files/mods/...            内容文件（与 format 2 同）
// files/resourcepacks/...
// files/shaderpacks/...
// files/screenshots/...
// worlds/<世界名>/...       存档
// folders/<目录名>/...       实例子目录（config / kubejs / tacz ...）
// instance/options.txt
// instance/servers.dat
// ---------------------------------------------------------------------------

/// 手动放入、未登记的模组在 Modrinth 上的识别结果。
/// `confidence` 决定可信度：
///   - "hash"：按文件 SHA1 精确反查（同一文件哈希相同，不会错）
///   - "name"：按文件名搜索猜测（**可能重名/错配**，需用户确认）
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct IdentifiedMod {
    pub filename: String,
    pub project_id: String,
    pub version_id: String,
    pub name: String,
    /// "hash" | "name"
    pub confidence: String,
}

/// 扫描实例里“未登记”的模组，尝试在 Modrinth 上定位来源，使其可以按
/// 引用（而非文件）导出，从而减小包体积。
/// 哈希命中的直接给出；哈希未命中的按文件名搜索兜底，并标为低置信度。
pub async fn identify_manual_mods(
    app: &tauri::AppHandle,
    state: &AppState,
    instance_id: &str,
) -> Result<Vec<IdentifiedMod>, String> {
    let inst = crate::instances::get_instance(state, instance_id)?;
    let dir = state.instances_dir().join(&inst.id).join("mods");

    let mut known: std::collections::HashSet<String> = std::collections::HashSet::new();
    for rec in crate::instances::list_content(state, instance_id, "mod") {
        known.insert(rec.filename.clone());
        known.insert(format!("{}.disabled", rec.filename));
    }

    let mut targets: Vec<String> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if !is_safe_name(&name) || known.contains(&name) {
                continue;
            }
            targets.push(name);
        }
    }
    targets.sort();

    // 并发识别（6 路，兼顾速度与 Modrinth 限流），每个文件完成即推送进度
    let total = targets.len();
    let done = std::sync::atomic::AtomicUsize::new(0);
    let current = std::sync::Mutex::new(String::new());
    let mut out: Vec<IdentifiedMod> = Vec::new();

    let push_progress = |d: usize, name: &str| {
        let _ = app.emit(
            "share://identify",
            serde_json::json!({ "done": d, "total": total, "current": name }),
        );
    };
    push_progress(0, targets.first().map(|s| s.as_str()).unwrap_or(""));

    let dir_ref = &dir;
    let results: Vec<Option<IdentifiedMod>> = futures_util::stream::iter(targets)
        .map(|name| {
            let done = &done;
            let current = &current;
            async move {
                {
                    let mut c = current.lock().unwrap();
                    *c = name.clone();
                }
                let r = identify_one(state, dir_ref, &name).await;
                let d = done.fetch_add(1, Ordering::Relaxed) + 1;
                let cur = current.lock().unwrap().clone();
                push_progress(d, &cur);
                r.map(|mut m| {
                    m.filename = name;
                    m
                })
            }
        })
        .buffered(6)
        .collect::<Vec<_>>()
        .await;
    for r in results.into_iter().flatten() {
        out.push(r);
    }
    Ok(out)
}

/// 识别单个未登记模组的 Modrinth 来源：
/// 1) 哈希精确反查（同一文件哈希相同，不会错）
/// 2) 文件名搜索兜底（低置信度，可能重名错配）
async fn identify_one(
    state: &AppState,
    dir: &Path,
    name: &str,
) -> Option<IdentifiedMod> {
    let path = dir.join(name);
    if let Some(sha1) = crate::util::file_sha1(&path) {
        if let Ok(Some(v)) = crate::modrinth::version_by_hash(state, &sha1).await {
            let project_id = v.get("project_id").and_then(|x| x.as_str()).unwrap_or_default();
            let version_id = v.get("id").and_then(|x| x.as_str()).unwrap_or_default();
            if !project_id.is_empty() && !version_id.is_empty() {
                let title = crate::modrinth::project_info(state, project_id)
                    .await
                    .ok()
                    .and_then(|p| p.get("title").and_then(|t| t.as_str()).map(|s| s.to_string()))
                    .unwrap_or_else(|| name.trim_end_matches(".disabled").to_string());
                return Some(IdentifiedMod {
                    filename: name.to_string(),
                    project_id: project_id.to_string(),
                    version_id: version_id.to_string(),
                    name: title,
                    confidence: "hash".into(),
                });
            }
        }
    }
    // 文件名兜底搜索（低置信度）
    let query = name
        .trim_end_matches(".disabled")
        .trim_end_matches(".jar")
        .to_string();
    if query.is_empty() {
        return None;
    }
    let res = crate::modrinth::search(state, &query, "mod", "", "relevance", 0, 1, "", "").await.ok()?;
    let hit = res
        .get("hits")
        .and_then(|h| h.as_array())
        .and_then(|a| a.first())?;
    let project_id = hit.get("project_id").and_then(|x| x.as_str()).unwrap_or_default();
    let version_id = hit
        .get("latest_version")
        .and_then(|x| x.as_str())
        .unwrap_or_default();
    let title = hit.get("title").and_then(|x| x.as_str()).unwrap_or(&query);
    if project_id.is_empty() || version_id.is_empty() {
        return None;
    }
    Some(IdentifiedMod {
        filename: name.to_string(),
        project_id: project_id.to_string(),
        version_id: version_id.to_string(),
        name: title.to_string(),
        confidence: "name".into(),
    })
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
    #[serde(default)]
    version: String,
    mc_version: String,
    loader: String,
    loader_version: Option<String>,
    exported_at: u64,
    #[serde(default)]
    count: usize,
    items: Vec<ShareItem>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 递归把 `dir` 下所有文件写进 zip（相对 `dir` 的路径，前缀 `prefix`）
fn write_dir_to_zip(
    zw: &mut zip::ZipWriter<std::fs::File>,
    dir: &Path,
    prefix: &str,
) -> Result<(), String> {
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

/// 导出实例到 `dest`。返回打包的内容条数。
pub fn export_pack(
    state: &AppState,
    instance_id: &str,
    dest: &Path,
    selection: ExportSelection,
) -> Result<usize, String> {
    let inst = crate::instances::get_instance(state, instance_id)?;
    let instance_dir = state.instances_dir().join(&inst.id);

    let mut items: Vec<ShareItem> = Vec::new();
    let file = std::fs::File::create(dest).map_err(|e| format!("创建分享包失败: {e}"))?;
    let mut zw = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    // ---- 模组：以勾选的磁盘文件名为准打包（记录仅补充元信息）。
    //      有在线来源记录的仍优先记引用（导入端重新下载，体积小一个量级）；
    //      没有记录的（手动迁移 / 未登记）直接打包文件本身。----
    let mut rec_by_disk: std::collections::HashMap<String, InstalledContent> =
        std::collections::HashMap::new();
    for rec in crate::instances::list_content(state, instance_id, "mod") {
        let disk_name = if rec.enabled {
            rec.filename.clone()
        } else {
            format!("{}.disabled", rec.filename)
        };
        rec_by_disk.insert(disk_name, rec);
    }
    // 用户采纳的识别结果：文件名 → 远程来源（按引用导出）
    let identified: std::collections::HashMap<&str, &IdentifiedMod> = selection
        .identified
        .iter()
        .map(|m| (m.filename.as_str(), m))
        .collect();
    for disk_name in &selection.mods {
        if !is_safe_name(disk_name) {
            continue;
        }
        let rec = rec_by_disk.get(disk_name);
        let disabled = disk_name.to_lowercase().ends_with(".jar.disabled");
        if disabled && !selection.include_disabled_mods {
            continue;
        }
        // 未登记但被识别为 Modrinth 来源：按引用记一条记录，不打实体文件
        if rec.is_none() {
            if let Some(idm) = identified.get(disk_name.as_str()) {
                if !disabled {
                    let mut r: InstalledContent = serde_json::from_value(serde_json::json!({
                        "filename": disk_name,
                        "source": "modrinth",
                        "project_id": idm.project_id,
                        "version_id": idm.version_id,
                        "name": idm.name,
                        "installed_at": now_secs(),
                        "size": 0,
                        "enabled": true,
                    }))
                    .map_err(|e| format!("构造识别记录失败: {e}"))?;
                    r.slug = None;
                    items.push(ShareItem { kind: "mod".into(), record: r });
                    continue;
                }
            }
        }
        if let Some(rec) = rec {
            let is_remote = (rec.source == "modrinth" || rec.source == "curseforge")
                && rec.project_id.is_some()
                && rec.version_id.is_some();
            // 在线来源默认只记引用；「打包资源文件」开启时才直接入包，
            // 「仅 Modrinth」进一步排除 CurseForge（分发协议限制）。
            let bundle = selection.bundle_online_files
                && (!selection.modrinth_only || rec.source == "modrinth");
            if is_remote && !disabled && !bundle {
                items.push(ShareItem { kind: "mod".into(), record: rec.clone() });
                continue;
            }
        }
        let src = instance_dir.join("mods").join(disk_name);
        if !src.is_file() {
            // 文件缺失（或选择不打包在线文件）：有在线来源记录的走重新下载
            if let Some(rec) = rec {
                let is_remote = (rec.source == "modrinth" || rec.source == "curseforge")
                    && rec.project_id.is_some()
                    && rec.version_id.is_some();
                let bundle = selection.bundle_online_files
                    && (!selection.modrinth_only || rec.source == "modrinth");
                if is_remote && !bundle {
                    items.push(ShareItem { kind: "mod".into(), record: rec.clone() });
                }
            }
            continue;
        }
        let zip_name = format!("files/mods/{disk_name}");
        zw.start_file(zip_name, opts).map_err(|e| e.to_string())?;
        let mut f = std::fs::File::open(&src).map_err(|e| e.to_string())?;
        std::io::copy(&mut f, &mut zw).map_err(|e| e.to_string())?;
        if let Some(rec) = rec {
            items.push(ShareItem { kind: "mod".into(), record: rec.clone() });
        }
    }

    // ---- 资源包 / 光影 / 截图：按勾选的目录条目（文件或文件夹）打包 ----
    for (folder, sel) in [
        ("resourcepacks", &selection.resourcepacks),
        ("shaderpacks", &selection.shaders),
        ("screenshots", &selection.screenshots),
    ] {
        for name in sel {
            if !is_safe_name(name) {
                continue;
            }
            let src = instance_dir.join(folder).join(name);
            if !src.exists() {
                continue;
            }
            if src.is_dir() {
                write_dir_to_zip(&mut zw, &src, &format!("files/{folder}/{name}"))?;
            } else {
                zw.start_file(format!("files/{folder}/{name}"), opts).map_err(|e| e.to_string())?;
                let mut f = std::fs::File::open(&src).map_err(|e| e.to_string())?;
                std::io::copy(&mut f, &mut zw).map_err(|e| e.to_string())?;
            }
        }
    }

    // ---- 世界存档 ----
    for world in &selection.worlds {
        if !is_safe_name(world) {
            continue;
        }
        let src = instance_dir.join("saves").join(world);
        if src.is_dir() {
            write_dir_to_zip(&mut zw, &src, &format!("worlds/{world}"))?;
        }
    }

    // ---- 实例子目录（config / kubejs / tacz ...）----
    for folder in &selection.folders {
        if !is_safe_name(folder) {
            continue;
        }
        let src = instance_dir.join(folder);
        if src.is_dir() {
            write_dir_to_zip(&mut zw, &src, &format!("folders/{folder}"))?;
        }
    }

    // ---- 单文件设置 ----
    if selection.options_txt {
        let src = instance_dir.join("options.txt");
        if src.is_file() {
            zw.start_file("instance/options.txt", opts).map_err(|e| e.to_string())?;
            let mut f = std::fs::File::open(&src).map_err(|e| e.to_string())?;
            std::io::copy(&mut f, &mut zw).map_err(|e| e.to_string())?;
        }
    }
    if selection.servers_dat {
        let src = instance_dir.join("servers.dat");
        if src.is_file() {
            zw.start_file("instance/servers.dat", opts).map_err(|e| e.to_string())?;
            let mut f = std::fs::File::open(&src).map_err(|e| e.to_string())?;
            std::io::copy(&mut f, &mut zw).map_err(|e| e.to_string())?;
        }
    }

    let count = items.len();
    let meta = ShareMeta {
        format: 3,
        name: selection.name.clone().unwrap_or_else(|| inst.name.clone()),
        version: selection.version.clone().unwrap_or_default(),
        mc_version: inst.mc_version.clone(),
        loader: inst.loader.as_str().to_string(),
        loader_version: inst.loader_version.clone(),
        exported_at: now_secs(),
        count,
        items,
    };
    let meta_json = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?;
    zw.start_file("qookix-instance.json", opts).map_err(|e| e.to_string())?;
    std::io::Write::write_all(&mut zw, meta_json.as_bytes()).map_err(|e| e.to_string())?;
    zw.finish().map_err(|e| format!("完成分享包失败: {e}"))?;
    Ok(count)
}

/// 导入时需要在线补下载的条目
#[derive(Serialize, Clone, Debug)]
pub struct PendingDownload {
    pub kind: String,
    pub provider: String,
    pub project_id: String,
    pub version_id: String,
    pub name: Option<String>,
}

/// 从分享包导入：新建实例、还原包内内容文件与记录。
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
    if meta.format > 3 {
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
    let inst_dir = state.instances_dir().join(&inst.id);

    // 先按内容记录还原（保持启用/禁用态），在线来源缺失的列入待下载
    let mut by_kind: std::collections::HashMap<String, Vec<InstalledContent>> =
        std::collections::HashMap::new();
    let mut pending: Vec<PendingDownload> = Vec::new();
    let mut restored: std::collections::HashSet<String> = std::collections::HashSet::new();
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
        let dest_dir = inst_dir.join(&folder);
        std::fs::create_dir_all(&dest_dir).map_err(|e| e.to_string())?;
        let mut out = std::fs::File::create(dest_dir.join(&disk_name))
            .map_err(|e| format!("写入 {disk_name} 失败: {e}"))?;
        let mut entry = archive.by_name(&zip_name).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("复制 {disk_name} 失败: {e}"))?;
        restored.insert(zip_name);
        by_kind
            .entry(item.kind.clone())
            .or_default()
            .push(item.record.clone());
    }
    for (kind, records) in by_kind {
        crate::instances::add_content_batch(state, &inst.id, &kind, records)?;
    }

    // 再遍历包内其余条目：目录式资源包/光影、截图、存档、实例子目录、设置文件
    for i in 0..archive.len() {
        let name = archive.by_index(i).map(|e| e.name().to_string()).unwrap_or_default();
        if name == "qookix-instance.json" || restored.contains(&name) || name.ends_with('/') {
            continue;
        }
        let (dest_root, rel) = if let Some(rest) = name.strip_prefix("files/") {
            // files/<folder>/<...> → <folder>/<...>
            (inst_dir.clone(), rest.to_string())
        } else if let Some(rest) = name.strip_prefix("worlds/") {
            let Some(slash) = rest.find('/') else { continue };
            let world = &rest[..slash];
            if !is_safe_name(world) {
                continue;
            }
            (inst_dir.join("saves").join(world), rest[slash + 1..].to_string())
        } else if let Some(rest) = name.strip_prefix("folders/") {
            if rest.is_empty() {
                continue;
            }
            (inst_dir.clone(), rest.to_string())
        } else if let Some(rest) = name.strip_prefix("config/") {
            // 旧包（format ≤2）的 config 布局
            if rest.is_empty() {
                continue;
            }
            (inst_dir.join("config"), rest.to_string())
        } else if name == "instance/options.txt" {
            (inst_dir.clone(), "options.txt".to_string())
        } else if name == "instance/servers.dat" {
            (inst_dir.clone(), "servers.dat".to_string())
        } else {
            continue;
        };
        let mut clean: Vec<&str> = rel.split('/').filter(|s| !s.is_empty() && *s != "." && *s != "..").collect();
        if clean.is_empty() {
            continue;
        }
        let file_name = clean.pop().unwrap_or_default();
        let mut dest = dest_root;
        for seg in &clean {
            dest.push(seg);
        }
        std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
        dest.push(file_name);
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(dest.parent().unwrap_or(&inst_dir)).ok();
        let mut out = std::fs::File::create(&dest).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("还原 {} 失败: {e}", name))?;
    }

    Ok((inst.id, pending))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::LoaderType;
    use std::sync::{Arc, Mutex, RwLock};

    fn test_state(root: &Path) -> AppState {
        AppState {
            root: root.to_path_buf(),
            settings: RwLock::new(Default::default()),
            db: Mutex::new(rusqlite::Connection::open_in_memory().unwrap()),
            client: crate::settings::http_client("system", None),
            semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
            game_pids: Arc::new(Mutex::new(std::collections::HashMap::new())),
            server_pids: Arc::new(Mutex::new(std::collections::HashMap::new())),
            server_senders: Arc::new(Mutex::new(std::collections::HashMap::new())),
            task_counter: std::sync::atomic::AtomicU64::new(1),
            install_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            ms_flow: Arc::new(Mutex::new(None)),
            java_cache: Mutex::new(None),
            terracotta: Mutex::new(None),
            pending_update: Mutex::new(None),
        }
    }

    fn put(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn zip_entries(pack: &Path) -> Vec<String> {
        let f = std::fs::File::open(pack).unwrap();
        let mut z = zip::ZipArchive::new(f).unwrap();
        (0..z.len())
            .map(|i| z.by_index(i).unwrap().name().to_string())
            .collect()
    }

    fn fresh(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// 完整往返：造实例内容 → 预览 → 全量导出 → 导入 → 校验每类都还原
    #[test]
    fn export_import_roundtrip_keeps_every_category() {
        let root = fresh("qookix-share-roundtrip");
        let state = test_state(&root);
        let inst = crate::instances::create_instance(
            &state,
            "原始实例".into(),
            "1.20.1".into(),
            LoaderType::Fabric,
            Some("0.15.11".into()),
        )
        .unwrap();
        let dir = state.instances_dir().join(&inst.id);

        put(&dir.join("mods/jest.jar"), "mod-bytes");
        // 未登记的 mod 文件（手动迁移的实例没有内容记录）也必须能导出
        put(&dir.join("mods/unregistered.jar"), "unregistered");
        put(&dir.join("saves/World1/level.dat"), "level");
        put(&dir.join("saves/World1/region/r.0.0.mca"), "region");
        put(&dir.join("config/jest.cfg"), "cfg");
        put(&dir.join("kubejs/server_scripts/a.js"), "js");
        put(&dir.join("screenshots/shot1.png"), "png");
        put(&dir.join("resourcepacks/rp.zip"), "rp");
        put(&dir.join("options.txt"), "options");
        put(&dir.join("servers.dat"), "servers");

        // 一条本地来源的模组记录（在线来源会被记成引用而非打包文件）
        let rec: InstalledContent = serde_json::from_value(serde_json::json!({
            "filename": "jest.jar", "source": "manual", "name": "Jest",
            "installed_at": 0, "size": 9, "enabled": true
        }))
        .unwrap();
        crate::instances::add_content_batch(&state, &inst.id, "mod", vec![rec]).unwrap();

        // ---- 预览：各类别都应出现 ----
        let pv = preview(&state, &inst.id).unwrap();
        let keys: Vec<&str> = pv.groups.iter().map(|g| g.key.as_str()).collect();
        for k in ["mods", "resourcepacks", "screenshots", "worlds", "folders", "settings"] {
            assert!(keys.contains(&k), "预览缺少分组 {k}，实际: {keys:?}");
        }
        let worlds = pv.groups.iter().find(|g| g.key == "worlds").unwrap();
        assert!(worlds.items.iter().any(|i| i.key == "World1"));
        let folders = pv.groups.iter().find(|g| g.key == "folders").unwrap();
        assert!(folders.items.iter().any(|i| i.key == "config"), "未识别出 config");
        assert!(folders.items.iter().any(|i| i.key == "kubejs"), "未识别出 kubejs");

        // ---- 导出：全量勾选 ----
        let selection = ExportSelection {
            name: Some("导出测试包".into()),
            version: Some("1.2.3".into()),
            mods: vec!["jest.jar".into(), "unregistered.jar".into()],
            include_disabled_mods: true,
            resourcepacks: vec!["rp.zip".into()],
            shaders: vec![],
            screenshots: vec!["shot1.png".into()],
            worlds: vec!["World1".into()],
            folders: vec!["config".into(), "kubejs".into()],
            options_txt: true,
            servers_dat: true,
            ..Default::default()
        };
        let pack = root.join("out.qkxinst");
        let n = export_pack(&state, &inst.id, &pack, selection).unwrap();
        assert_eq!(n, 1, "应打包 1 条模组记录");

        let names = zip_entries(&pack);
        assert!(names.iter().any(|x| x == "qookix-instance.json"));
        assert!(names.iter().any(|x| x == "files/mods/jest.jar"), "模组文件未入包: {names:?}");
        assert!(
            names.iter().any(|x| x == "files/mods/unregistered.jar"),
            "未登记的模组文件未入包: {names:?}"
        );
        assert!(names.iter().any(|x| x == "files/resourcepacks/rp.zip"));
        assert!(names.iter().any(|x| x == "files/screenshots/shot1.png"));
        assert!(names.iter().any(|x| x.starts_with("worlds/World1/")), "存档未入包: {names:?}");
        assert!(names.iter().any(|x| x == "folders/config/jest.cfg"));
        assert!(names.iter().any(|x| x == "folders/kubejs/server_scripts/a.js"));
        assert!(names.iter().any(|x| x == "instance/options.txt"));
        assert!(names.iter().any(|x| x == "instance/servers.dat"));

        // ---- 导入到全新数据目录 ----
        let root2 = fresh("qookix-share-import");
        let state2 = test_state(&root2);
        let (new_id, pending) = import_pack(&state2, &pack).unwrap();
        assert!(pending.is_empty(), "本地来源不应有待下载项");
        let d2 = state2.instances_dir().join(&new_id);
        assert!(d2.join("mods/jest.jar").is_file(), "模组未还原");
        assert!(d2.join("mods/unregistered.jar").is_file(), "未登记模组未还原");
        assert!(d2.join("saves/World1/level.dat").is_file(), "存档未还原");
        assert!(d2.join("saves/World1/region/r.0.0.mca").is_file(), "存档子目录未还原");
        assert!(d2.join("config/jest.cfg").is_file(), "config 未还原");
        assert!(d2.join("kubejs/server_scripts/a.js").is_file(), "附属文件夹未还原");
        assert!(d2.join("screenshots/shot1.png").is_file(), "截图未还原");
        assert!(d2.join("resourcepacks/rp.zip").is_file(), "资源包未还原");
        assert!(d2.join("options.txt").is_file(), "options.txt 未还原");
        assert!(d2.join("servers.dat").is_file(), "servers.dat 未还原");

        let inst2 = crate::instances::get_instance(&state2, &new_id).unwrap();
        assert_eq!(inst2.name, "导出测试包", "导出时自定义的名称未生效");
        assert_eq!(inst2.mods.len(), 1, "模组记录未还原");
        assert_eq!(inst2.mc_version, "1.20.1");
    }

    /// 附属文件夹识别：装了 tacz 的模组时，tacz/ 目录应被自动列出
    #[test]
    fn detects_mod_owned_folders() {
        let root = fresh("qookix-share-detect");
        let state = test_state(&root);
        let inst = crate::instances::create_instance(
            &state,
            "tacz 实例".into(),
            "1.20.1".into(),
            LoaderType::Forge,
            None,
        )
        .unwrap();
        let dir = state.instances_dir().join(&inst.id);
        put(&dir.join("mods/tacz-1.20.1-1.0.2.jar"), "mod");
        put(&dir.join("tacz/default_guns/a.json"), "{}");
        let rec: InstalledContent = serde_json::from_value(serde_json::json!({
            "filename": "tacz-1.20.1-1.0.2.jar", "source": "manual",
            "installed_at": 0, "size": 3, "enabled": true
        }))
        .unwrap();
        crate::instances::add_content_batch(&state, &inst.id, "mod", vec![rec]).unwrap();

        let pv = preview(&state, &inst.id).unwrap();
        let folders = pv.groups.iter().find(|g| g.key == "folders").expect("应识别出附属文件夹");
        assert!(
            folders.items.iter().any(|i| i.key == "tacz"),
            "tacz 目录未被识别，实际: {:?}",
            folders.items.iter().map(|i| &i.key).collect::<Vec<_>>()
        );
    }
}
