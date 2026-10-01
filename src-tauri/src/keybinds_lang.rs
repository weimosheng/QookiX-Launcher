//! 从游戏客户端、资源索引与模组 jar 中读取中文语言文件，取按键动作名。
//! 按游戏内优先级从低到高合并：资源索引（1.13+）→ 客户端 jar → 模组 jar
//! （`assets/<modid>/lang/zh_cn.json`）→ 资源包。
//! 只保留 `key.` 开头的词条，避免把几千条无关翻译传给前端。

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

/// 一次扫描同时产出「按键动作 → 中文名」与模组列表；模组 jar 只打开一遍，
/// 进度通过 `progress(done, total)` 逐个文件上报（前端显示进度条）。
pub fn collect_all(
    instance_dir: &Path,
    version_dir: &Path,
    assets_dir: &Path,
    progress: &(dyn Fn(usize, usize) + Send + Sync),
) -> (HashMap<String, String>, Vec<ModInfo>) {
    let mod_jars = archives_in(&instance_dir.join("mods"));
    let version_jars = archives_in(version_dir);
    let packs = archives_in(&instance_dir.join("resourcepacks"));
    // 资源索引算 1 步，其余每个 jar 一步
    let total = 1 + version_jars.len() + mod_jars.len() + packs.len();
    let mut done = 0usize;

    let mut map = HashMap::new();
    // 1) 资源索引里的原版语言文件
    merge_assets_index(assets_dir, &mut map);
    done += 1;
    progress(done, total);
    // 2) 版本 jar（老版本格式 / 或客户端自带 assets 的情况）
    for jar in &version_jars {
        merge_archive(jar, &mut map);
        done += 1;
        progress(done, total);
    }
    // 3) 模组（覆盖原版），顺手读模组元信息
    let mut mods: Vec<ModInfo> = Vec::new();
    for jar in &mod_jars {
        merge_archive(jar, &mut map);
        if let Some(info) = read_mod_meta(jar) {
            mods.push(info);
        }
        done += 1;
        progress(done, total);
    }
    // 4) 资源包（最后覆盖）
    for zip in &packs {
        merge_archive(zip, &mut map);
        done += 1;
        progress(done, total);
    }
    mods.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    mods.dedup_by(|a, b| a.id.eq_ignore_ascii_case(&b.id));
    (map, mods)
}

/// 模组元信息：id（用于匹配按键动作前缀）+ 显示名（卡片标题）
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ModInfo {
    pub id: String,
    pub name: String,
}

// ---- 扫描结果磁盘缓存：按"输入文件的指纹"命中，模组没变就不用再扫一遍 ----

/// 待扫描的输入文件清单（与 `collect_all` 的扫描范围一致）
fn scan_inputs(instance_dir: &Path, version_dir: &Path, assets_dir: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    v.extend(archives_in(version_dir));
    v.extend(archives_in(&instance_dir.join("mods")));
    v.extend(archives_in(&instance_dir.join("resourcepacks")));
    if let Ok(rd) = std::fs::read_dir(assets_dir.join("indexes")) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "json") {
                v.push(p);
            }
        }
    }
    v.sort();
    v
}

/// 输入指纹：全部待扫描文件的「路径 + 大小 + 修改时间」做 SHA-256。
/// 不用文件内容：jar 合起来几百 MB，读内容算哈希比扫描本身还慢。
pub fn scan_fingerprint(instance_dir: &Path, version_dir: &Path, assets_dir: &Path) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for p in scan_inputs(instance_dir, version_dir, assets_dir) {
        let meta = std::fs::metadata(&p).ok();
        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let mtime = meta
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        h.update(p.to_string_lossy().as_bytes());
        h.update(size.to_le_bytes());
        h.update(mtime.to_le_bytes());
    }
    format!("{:x}", h.finalize())
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct ScanCache {
    instances: HashMap<String, ScanEntry>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ScanEntry {
    fingerprint: String,
    ts: u64,
    labels: HashMap<String, String>,
    mods: Vec<ModInfo>,
}

fn cache_path(root: &Path) -> PathBuf {
    root.join("cache").join("keybind_scan.json")
}

/// 读缓存：实例 id 与指纹都匹配才命中。
pub fn load_cached(
    root: &Path,
    instance_id: &str,
    fingerprint: &str,
) -> Option<(HashMap<String, String>, Vec<ModInfo>)> {
    let text = std::fs::read_to_string(cache_path(root)).ok()?;
    let cache: ScanCache = serde_json::from_str(&text).ok()?;
    let e = cache.instances.get(instance_id)?;
    if e.fingerprint != fingerprint {
        return None;
    }
    Some((e.labels.clone(), e.mods.clone()))
}

/// 写缓存（保留最近 20 个实例，避免文件无限增长）。
pub fn save_cache(
    root: &Path,
    instance_id: &str,
    fingerprint: &str,
    labels: &HashMap<String, String>,
    mods: &[ModInfo],
) {
    let path = cache_path(root);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut cache: ScanCache = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    cache.instances.insert(
        instance_id.to_string(),
        ScanEntry {
            fingerprint: fingerprint.to_string(),
            ts,
            labels: labels.clone(),
            mods: mods.to_vec(),
        },
    );
    if cache.instances.len() > 20 {
        let mut order: Vec<(String, u64)> = cache
            .instances
            .iter()
            .map(|(k, e)| (k.clone(), e.ts))
            .collect();
        order.sort_by_key(|(_, t)| *t);
        for (k, _) in order.iter().take(order.len() - 20) {
            cache.instances.remove(k);
        }
    }
    if let Ok(s) = serde_json::to_string(&cache) {
        let _ = std::fs::write(&path, s);
    }
}

fn read_mod_meta(path: &Path) -> Option<ModInfo> {
    let file = std::fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;

    // Fabric
    if let Some(text) = read_archive_entry(&mut archive, "fabric.mod.json") {
        if let Ok(j) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(id) = j.get("id").and_then(|v| v.as_str()) {
                // ModMenu 里登记的名字最贴近用户在游戏里看到的称呼
                let name = j
                    .get("custom")
                    .and_then(|c| c.get("modmenu"))
                    .and_then(|m| m.get("name"))
                    .and_then(|v| v.as_str())
                    .or_else(|| j.get("name").and_then(|v| v.as_str()))
                    .unwrap_or(id);
                return Some(ModInfo {
                    id: id.to_string(),
                    name: clean_name(name),
                });
            }
        }
    }
    // Quilt
    if let Some(text) = read_archive_entry(&mut archive, "quilt.mod.json") {
        if let Ok(j) = serde_json::from_str::<serde_json::Value>(&text) {
            let loader = j.get("quilt_loader")?;
            let id = loader.get("id").and_then(|v| v.as_str())?;
            let name = loader
                .get("metadata")
                .and_then(|m| m.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or(id);
            return Some(ModInfo {
                id: id.to_string(),
                name: clean_name(name),
            });
        }
    }
    // Forge / NeoForge（TOML）
    if let Some(text) = read_archive_entry(&mut archive, "META-INF/mods.toml") {
        if let Some(id) = toml_value(&text, "modId") {
            let name = toml_value(&text, "displayName").unwrap_or_else(|| id.clone());
            return Some(ModInfo {
                id,
                name: clean_name(&name),
            });
        }
    }
    // Legacy Forge
    if let Some(text) = read_archive_entry(&mut archive, "mcmod.info") {
        if let Ok(j) = serde_json::from_str::<serde_json::Value>(&text) {
            let first = j.as_array().and_then(|a| a.first()).or(Some(&j))?;
            let id = first.get("modid").and_then(|v| v.as_str())?;
            let name = first
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(id);
            return Some(ModInfo {
                id: id.to_string(),
                name: clean_name(name),
            });
        }
    }
    None
}

fn read_archive_entry(
    archive: &mut zip::ZipArchive<std::fs::File>,
    name: &str,
) -> Option<String> {
    let mut entry = archive.by_name(name).ok()?;
    let mut text = String::new();
    entry.read_to_string(&mut text).ok()?;
    Some(text)
}

/// TOML 里取 `key = "value"` 形式的字符串（mods.toml 结构简单，不必上完整解析器）。
/// 只取第一个引号串，避免把行尾注释（如 `#mandatory`）带进来。
fn toml_value(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with(key) {
            continue;
        }
        let rest = line[key.len()..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(quote) = rest.chars().next() else {
            continue;
        };
        if quote != '"' && quote != '\'' {
            continue;
        }
        let body = &rest[1..];
        let Some(end) = body.find(quote) else {
            continue;
        };
        let v = &body[..end];
        // 跳过占位符（如 ${file.jarVersion}）
        if !v.is_empty() && !v.starts_with("${") {
            return Some(v.to_string());
        }
    }
    None
}

/// 去掉模组名里常见的颜色/格式标记与多余空白
fn clean_name(s: &str) -> String {
    let cleaned: String = s.chars().filter(|c| *c != '\u{00A7}' && !c.is_control()).collect();
    cleaned.trim().to_string()
}

fn archives_in(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in rd.flatten() {
        let p = e.path();
        let is_archive = p
            .extension()
            .and_then(|x| x.to_str())
            .map(|x| x.eq_ignore_ascii_case("jar") || x.eq_ignore_ascii_case("zip"))
            .unwrap_or(false);
        if is_archive && p.is_file() {
            out.push(p);
        }
    }
    out
}

/// 从 assets/indexes/*.json 找到中文语言文件的 hash 并读取。
/// 优先新版 `zh_cn.json`（词条最全），没有再退回老版 `zh_CN.lang`。
fn merge_assets_index(assets_dir: &Path, out: &mut HashMap<String, String>) {
    let idx_dir = assets_dir.join("indexes");
    for candidate in ["minecraft/lang/zh_cn.json", "minecraft/lang/zh_CN.lang"] {
        let Ok(rd) = std::fs::read_dir(&idx_dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else {
                continue;
            };
            let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
                continue;
            };
            let Some(objs) = json.get("objects").and_then(|o| o.as_object()) else {
                continue;
            };
            let Some(hash) = objs
                .get(candidate)
                .and_then(|v| v.get("hash"))
                .and_then(|v| v.as_str())
            else {
                continue;
            };
            if hash.len() < 2 {
                continue;
            }
            let file = assets_dir.join("objects").join(&hash[..2]).join(hash);
            if file.is_file() {
                let is_lang = candidate.ends_with(".lang");
                if let Ok(text) = std::fs::read_to_string(&file) {
                    parse_lang(&text, is_lang, out);
                    return;
                }
            }
        }
    }
}

fn merge_archive(path: &Path, out: &mut HashMap<String, String>) {
    let Ok(file) = std::fs::File::open(path) else {
        return;
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return;
    };
    // 先取条目名（避免与后续 by_name 的可变借用冲突）
    let names: Vec<String> = (0..archive.len())
        .filter_map(|i| archive.by_index(i).ok().map(|e| e.name().to_string()))
        .collect();
    for name in names {
        let lower = name.to_ascii_lowercase();
        if !lower.starts_with("assets/") {
            continue;
        }
        let is_json = lower.ends_with("/lang/zh_cn.json");
        let is_lang = lower.ends_with("/lang/zh_cn.lang");
        if !is_json && !is_lang {
            continue;
        }
        let Ok(mut entry) = archive.by_name(&name) else {
            continue;
        };
        let mut text = String::new();
        if entry.read_to_string(&mut text).is_err() {
            continue;
        }
        parse_lang(&text, is_lang, out);
    }
}

/// 解析语言文件文本：1.13+ 是 JSON；1.12 及以前是 `key=value` 行
fn parse_lang(text: &str, is_lang: bool, out: &mut HashMap<String, String>) {
    if is_lang {
        for line in text.lines() {
            if let Some((k, v)) = line.split_once('=') {
                if k.starts_with("key.") {
                    out.insert(k.to_string(), v.to_string());
                }
            }
        }
        return;
    }
    let Ok(json) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    let Some(obj) = json.as_object() else {
        return;
    };
    for (k, v) in obj {
        if k.starts_with("key.") {
            if let Some(s) = v.as_str() {
                out.insert(k.clone(), s.to_string());
            }
        }
    }
}
