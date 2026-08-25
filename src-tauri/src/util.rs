use sha1::{Digest, Sha1};
use sha2::Sha512;
use std::io::Read;
use std::path::Path;

/// sha1 of a file (hex lowercase)
pub fn file_sha1(path: &Path) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut hasher = Sha1::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(format!("{:x}", hasher.finalize()))
}

/// sha512 of a file (hex lowercase)
pub fn file_sha512(path: &Path) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut hasher = Sha512::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(format!("{:x}", hasher.finalize()))
}

/// Read a text file fully.
#[allow(dead_code)]
pub fn read_text(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// Format a byte count for humans.
#[allow(dead_code)]
pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{} {}", n, UNITS[i])
    } else {
        format!("{:.1} {}", v, UNITS[i])
    }
}

// ---------------------------------------------------------------------------
// Mojang launch rules
// ---------------------------------------------------------------------------

pub fn os_name() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "osx",
        "linux" => "linux",
        _ => "unknown",
    }
}

pub fn os_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "x86" => "x86",
        "aarch64" => "arm64",
        "arm" => "arm",
        _ => "unknown",
    }
}

pub fn rule_matches(rule: &crate::models::Rule, features: &std::collections::HashMap<String, bool>) -> bool {
    if let Some(os) = &rule.os {
        if let Some(name) = &os.name {
            if name != os_name() {
                return false;
            }
        }
        if let Some(arch) = &os.arch {
            if arch != os_arch() {
                return false;
            }
        }
    }
    if let Some(fs) = &rule.features {
        for (k, v) in fs {
            // `false` means "feature not active"
            let active = features.get(k).copied().unwrap_or(false);
            if active != *v {
                return false;
            }
        }
    }
    true
}

/// Evaluate a rules list: no rules -> allowed; otherwise last match wins.
pub fn rules_allow(rules: &[crate::models::Rule], features: &std::collections::HashMap<String, bool>) -> bool {
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        if rule_matches(rule, features) {
            allowed = rule.action == "allow";
        }
    }
    allowed
}

// ---------------------------------------------------------------------------
// Zip helpers
// ---------------------------------------------------------------------------

/// Read the class-file major version of the first `.class` entry in a jar.
/// Java major = class_major - 44 (Java 8 = 52, 17 = 61, 21 = 65, 25 = 69).
pub fn jar_class_version(path: &Path) -> Option<u32> {
    let file = std::fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    for i in 0..archive.len() {
        let name = archive.by_index(i).ok()?.name().to_string();
        if !name.ends_with(".class") {
            continue;
        }
        let mut entry = archive.by_index(i).ok()?;
        let mut buf = [0u8; 8];
        std::io::Read::read_exact(&mut entry, &mut buf).ok()?;
        // CAFEBABE + minor(2) + major(2)
        if buf[0] == 0xCA && buf[1] == 0xFE && buf[2] == 0xBA && buf[3] == 0xBE {
            return Some(u16::from_be_bytes([buf[6], buf[7]]) as u32);
        }
    }
    None
}

/// Like `extract_zip`, but reports `(done, total)` through `on_file`
/// as files are written (used for install-phase progress).
pub fn extract_zip_progress(
    path: &Path,
    dest: &Path,
    skip_prefixes: &[&str],
    on_file: &mut dyn FnMut(usize, usize),
) -> Result<usize, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("打开 {} 失败: {e}", path.display()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("解析 zip 失败: {e}"))?;
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let mut count = 0usize;
    let total = archive.len();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_string();
        if entry.is_dir() {
            continue;
        }
        if skip_prefixes.iter().any(|p| name.starts_with(p)) {
            continue;
        }
        let rel = name.replace('\\', "/");
        let clean: Vec<&str> = rel.split('/').filter(|s| !s.is_empty() && *s != "..").collect();
        if clean.is_empty() {
            continue;
        }
        let out = dest.join(clean.join(std::path::MAIN_SEPARATOR_STR));
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        let mut f = std::fs::File::create(&out).map_err(|e| format!("创建 {} 失败: {e}", out.display()))?;
        std::io::copy(&mut entry, &mut f).map_err(|e| e.to_string())?;
        count += 1;
        if count % 25 == 0 || count == total {
            on_file(count, total);
        }
    }
    on_file(count, total);
    Ok(count)
}

/// Extract a zip archive into `dest`, skipping entries whose path starts with
/// any of `skip_prefixes`. Returns the number of files written.
pub fn extract_zip(path: &Path, dest: &Path, skip_prefixes: &[&str]) -> Result<usize, String> {
    extract_zip_inner(path, dest, skip_prefixes, None)
}

/// Like `extract_zip`, but also strips `strip` from each entry's path
/// (used to unpack an `overrides/` folder to the archive root).
pub fn extract_zip_strip(path: &Path, dest: &Path, strip: &str, skip_prefixes: &[&str]) -> Result<usize, String> {
    extract_zip_inner(path, dest, skip_prefixes, Some(strip))
}

fn extract_zip_inner(
    path: &Path,
    dest: &Path,
    skip_prefixes: &[&str],
    strip: Option<&str>,
) -> Result<usize, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("打开 {} 失败: {e}", path.display()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("解析 zip 失败: {e}"))?;
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let mut count = 0usize;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_string();
        if entry.is_dir() {
            continue;
        }
        if skip_prefixes.iter().any(|p| name.starts_with(p)) {
            continue;
        }
        // sanitize path
        let rel = name.replace('\\', "/");
        let rel = if let Some(strip) = strip {
            rel.strip_prefix(strip).unwrap_or(&rel).to_string()
        } else {
            rel
        };
        let clean: Vec<&str> = rel.split('/').filter(|s| !s.is_empty() && *s != "..").collect();
        if clean.is_empty() {
            continue;
        }
        let out = dest.join(clean.join(std::path::MAIN_SEPARATOR_STR));
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        let mut f = std::fs::File::create(&out).map_err(|e| format!("创建 {} 失败: {e}", out.display()))?;
        std::io::copy(&mut entry, &mut f).map_err(|e| e.to_string())?;
        count += 1;
    }
    Ok(count)
}

/// Read a single entry from a zip archive as bytes.
pub fn read_zip_entry(path: &Path, entry_name: &str) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("打开 {} 失败: {e}", path.display()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("解析 zip 失败: {e}"))?;
    let mut entry = archive
        .by_name(entry_name)
        .map_err(|e| format!("zip 内缺少 {entry_name}: {e}"))?;
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut entry, &mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

// ---------------------------------------------------------------------------
// Maven metadata helpers
// ---------------------------------------------------------------------------

/// Extract the modpack icon from a zip archive and save it to
/// `instances/{instance_id}/pack-icon.png`. Returns the absolute path on success.
pub fn extract_modpack_icon(pack_path: &Path, instance_dir: &Path) -> Option<String> {
    use std::io::Read;
    let file = std::fs::File::open(pack_path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    for name in ["pack.png", "icon.png", "logo.png", "modpack.png"] {
        if let Ok(mut entry) = archive.by_name(name) {
            if entry.is_dir() {
                continue;
            }
            let mut buf = Vec::new();
            if Read::read_to_end(&mut entry, &mut buf).is_err() || buf.len() < 8 {
                continue;
            }
            let out = instance_dir.join("pack-icon.png");
            if std::fs::write(&out, &buf).is_ok() {
                return Some(out.to_string_lossy().to_string());
            }
        }
    }
    None
}

/// Download a remote image (e.g. modpack project icon) and save it to
/// `instances/{instance_id}/pack-icon.png`. Returns the absolute path on success.
pub async fn download_icon(
    client: &reqwest::Client,
    url: &str,
    instance_dir: &std::path::Path,
) -> Option<String> {
    if url.trim().is_empty() {
        return None;
    }
    let resp = client.get(url).send().await.ok()?;
    let bytes = resp.bytes().await.ok()?;
    if bytes.len() < 8 {
        return None;
    }
    std::fs::create_dir_all(instance_dir).ok()?;
    let out = instance_dir.join("pack-icon.png");
    std::fs::write(&out, &bytes).ok()?;
    Some(out.to_string_lossy().to_string())
}

/// Best-effort: extract an icon embedded in a local mod / resource-pack / shader zip.
/// Looks for `pack.png`, `icon.png` or a Modrinth metadata icon; falls back to a generic icon name.
pub fn extract_archive_icon(path: &std::path::Path, kind: &str) -> Option<String> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let image_candidates: &[&str] = match kind {
        "resourcepack" => &["pack.png", "icon.png", "assets/minecraft/textures/gui/title/icon.png"],
        "shader" => &["icon.png", "pack.png"],
        _ => &["icon.png", "pack.png"],
    };
    let save_icon = |buf: Vec<u8>| -> Option<String> {
        if buf.len() < 8 {
            return None;
        }
        let f = std::env::temp_dir().join(format!("qookix-icon-{}.png", uuid::Uuid::new_v4().simple()));
        std::fs::write(&f, &buf).ok()?;
        Some(f.to_string_lossy().to_string())
    };
    for name in image_candidates {
        if let Ok(mut entry) = archive.by_name(name) {
            if entry.is_dir() {
                continue;
            }
            let mut buf = Vec::new();
            Read::read_to_end(&mut entry, &mut buf).ok()?;
            if let Some(p) = save_icon(buf) {
                return Some(p);
            }
        }
    }
    let mut icon_ref: Option<String> = None;
    for name in ["modrinth.mod.json", "fabric.mod.json"] {
        if icon_ref.is_some() {
            break;
        }
        if let Ok(mut entry) = archive.by_name(name) {
            let mut buf = Vec::new();
            Read::read_to_end(&mut entry, &mut buf).ok()?;
            let val: serde_json::Value = serde_json::from_slice(&buf).ok()?;
            if let Some(icon) = val.get("icon").and_then(|i| i.as_str()) {
                if !icon.is_empty() {
                    icon_ref = Some(icon.to_string());
                }
            }
        }
    }
    if let Some(icon) = icon_ref {
        if icon.starts_with("http://") || icon.starts_with("https://") {
            return Some(icon);
        }
        if let Ok(mut icon_entry) = archive.by_name(&icon) {
            let mut ibuf = Vec::new();
            Read::read_to_end(&mut icon_entry, &mut ibuf).ok()?;
            if let Some(p) = save_icon(ibuf) {
                return Some(p);
            }
        }
    }
    None
}

/// 从 jar 内部解析出的 mod 元数据。
#[derive(Default, Clone, Debug)]
pub struct ModJarMeta {
    pub mod_id: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub authors: Option<Vec<String>>,
    pub description: Option<String>,
    /// 已落地的 icon 本地路径或 http(s) URL
    pub icon: Option<String>,
    /// fabric / quilt / forge / neoforge
    pub loader: Option<String>,
}

fn save_icon_buf(buf: Vec<u8>) -> Option<String> {
    if buf.len() < 8 {
        return None;
    }
    let f = std::env::temp_dir().join(format!("qookix-icon-{}.png", uuid::Uuid::new_v4().simple()));
    std::fs::write(&f, &buf).ok()?;
    Some(f.to_string_lossy().to_string())
}

fn parse_json_authors(val: &serde_json::Value) -> Option<Vec<String>> {
    let a = val.get("authors")?;
    if let Some(arr) = a.as_array() {
        let v: Vec<String> = arr
            .iter()
            .filter_map(|x| {
                x.as_str()
                    .map(|s| s.to_string())
                    .or_else(|| x.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
            })
            .filter(|s| !s.is_empty())
            .collect();
        if v.is_empty() { None } else { Some(v) }
    } else if let Some(s) = a.as_str() {
        if s.is_empty() { None } else { Some(vec![s.to_string()]) }
    } else {
        None
    }
}

/// 极简 mods.toml 值解析：支持 "str" / 'str' / ["a","b"] / bare。
fn parse_toml_value(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if s.starts_with('[') && s.ends_with(']') && s.len() >= 2 {
        let inner = &s[1..s.len() - 1];
        let items: Vec<String> = inner.split(',').filter_map(|item| parse_toml_value(item)).collect();
        return Some(items.join(", "));
    }
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        return Some(s[1..s.len() - 1].to_string());
    }
    if s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2 {
        return Some(s[1..s.len() - 1].to_string());
    }
    Some(s.to_string())
}

fn split_authors(s: &str) -> Vec<String> {
    s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
}

/// 解析 Forge/NeoForge 的 mods.toml，只取第一个 [[mods]] 块。
fn parse_mods_toml(text: &str) -> Option<ModJarMeta> {
    let mut in_mods = false;
    let mut meta = ModJarMeta::default();
    let mut found_mod_id = false;
    let mut logo_file: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("[[") {
            if in_mods && found_mod_id {
                break;
            }
            in_mods = trimmed == "[[mods]]";
            continue;
        }
        if trimmed.starts_with('[') {
            in_mods = false;
            continue;
        }
        if !in_mods {
            continue;
        }
        let Some(eq) = trimmed.find('=') else { continue };
        let key = trimmed[..eq].trim();
        let value = trimmed[eq + 1..].trim();
        let Some(v) = parse_toml_value(value) else { continue };
        match key {
            "modId" => { meta.mod_id = Some(v); found_mod_id = true; }
            "displayName" => { meta.name = Some(v); }
            "version" => { meta.version = Some(v); }
            "authors" => { meta.authors = Some(split_authors(&v)); }
            "description" => { meta.description = Some(v); }
            "logoFile" | "logo" => { logo_file = Some(v); }
            _ => {}
        }
    }
    if found_mod_id {
        meta.icon = logo_file;
        Some(meta)
    } else {
        None
    }
}

/// 从 jar 内部解析 mod 元数据（fabric/quilt/forge/neoforge 通用）。
/// 优先级：fabric.mod.json → quilt.mod.json → META-INF/mods.toml → META-INF/neoforge.mods.toml
/// 一次 zip 读取同时提取元数据与内嵌 icon。
pub fn parse_mod_jar(path: &Path) -> Option<ModJarMeta> {
    let file = std::fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let mut meta = ModJarMeta::default();
    let mut icon_ref: Option<String> = None;
    let mut found = false;

    for name in ["fabric.mod.json", "quilt.mod.json"] {
        if found { break; }
        let Ok(mut entry) = archive.by_name(name) else { continue };
        let mut buf = Vec::new();
        if Read::read_to_end(&mut entry, &mut buf).is_err() { continue; }
        let val: serde_json::Value = match serde_json::from_slice(&buf) { Ok(v) => v, Err(_) => continue };
        meta.mod_id = val.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
        meta.name = val.get("name").and_then(|v| v.as_str()).map(|s| s.to_string());
        meta.version = val.get("version").and_then(|v| v.as_str()).map(|s| s.to_string());
        meta.description = val.get("description").and_then(|v| v.as_str()).map(|s| s.to_string());
        meta.authors = parse_json_authors(&val);
        icon_ref = val.get("icon").and_then(|v| v.as_str()).map(|s| s.to_string());
        meta.loader = if name == "fabric.mod.json" { Some("fabric".into()) } else { Some("quilt".into()) };
        found = true;
    }

    for name in ["META-INF/mods.toml", "META-INF/neoforge.mods.toml"] {
        if found { break; }
        let Ok(mut entry) = archive.by_name(name) else { continue };
        let mut buf = Vec::new();
        if Read::read_to_end(&mut entry, &mut buf).is_err() { continue; }
        let text = String::from_utf8_lossy(&buf).to_string();
        if let Some(m) = parse_mods_toml(&text) {
            icon_ref = m.icon.clone();
            meta = m;
            meta.loader = if name.contains("neoforge") { Some("neoforge".into()) } else { Some("forge".into()) };
            found = true;
        }
    }

    if !found { return None; }

    if let Some(icon) = icon_ref {
        if icon.starts_with("http://") || icon.starts_with("https://") {
            meta.icon = Some(icon);
        } else if let Ok(mut icon_entry) = archive.by_name(&icon) {
            let mut ibuf = Vec::new();
            if Read::read_to_end(&mut icon_entry, &mut ibuf).is_ok() {
                meta.icon = save_icon_buf(ibuf);
            }
        }
    }
    Some(meta)
}

/// 用 jar 内部元数据回填 `InstalledContent` 中缺失的字段。
/// 远程 API 元数据优先，jar 元数据作为回退；name 为文件名占位时也替换为真实名字。
pub fn fill_content_from_jar(rec: &mut crate::models::InstalledContent, jar_path: &Path) {
    let Some(meta) = parse_mod_jar(jar_path) else { return };
    if rec.mod_id.is_none() { rec.mod_id = meta.mod_id; }
    let name_is_placeholder = rec.name.as_deref() == Some(rec.filename.as_str());
    if rec.name.is_none() || name_is_placeholder {
        if let Some(n) = meta.name.filter(|n| !n.is_empty()) { rec.name = Some(n); }
    }
    if rec.version.is_none() { rec.version = meta.version; }
    if rec.authors.is_none() { rec.authors = meta.authors; }
    if rec.description.is_none() { rec.description = meta.description; }
    if rec.icon.is_none() { rec.icon = meta.icon; }
    if rec.slug.is_none() { rec.slug = rec.mod_id.clone(); }
}

/// Copy a file (creating parent dirs), or create a symlink to it when `link` is
/// true. On platforms where symlink creation is denied (e.g. Windows without
/// Developer Mode / admin), this silently falls back to a normal copy and sets
/// `*fallback = true` so the caller can warn the user.
pub fn copy_or_link(source: &Path, dest: &Path, link: bool, fallback: &mut bool) -> Result<u64, String> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if link {
        // On Windows, File::create_symlink_file requires the target not to exist.
        if dest.exists() {
            let _ = std::fs::remove_file(dest);
        }
        let link_result = {
            #[cfg(windows)]
            {
                std::os::windows::fs::symlink_file(source, dest)
            }
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(source, dest)
            }
        };
        match link_result {
            Ok(()) => Ok(0),
            Err(_) => {
                // privilege missing -> copy instead
                *fallback = true;
                std::fs::copy(source, dest).map_err(|e| e.to_string())
            }
        }
    } else {
        std::fs::copy(source, dest).map_err(|e| e.to_string())
    }
}

/// Parse a maven-metadata.xml `<versions>` block without an XML dependency.
pub fn parse_maven_versions(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<version>") {
        let after = &rest[start + "<version>".len()..];
        if let Some(end) = after.find("</version>") {
            out.push(after[..end].to_string());
            rest = &after[end + "</version>".len()..];
        } else {
            break;
        }
    }
    out
}

/// Sort version strings that look like `1.20.1-47.2.0` (mc-build) descending.
pub fn sort_mc_versions(mut versions: Vec<String>, mc_prefix: &str) -> Vec<String> {
    versions.retain(|v| v.starts_with(mc_prefix));
    versions.sort_by(|a, b| {
        let num = |s: &str| -> Vec<u64> {
            let part = s.split('-').last().unwrap_or(s);
            part.split('.')
                .filter_map(|p| p.parse::<u64>().ok())
                .collect()
        };
        num(b).partial_cmp(&num(a)).unwrap_or(std::cmp::Ordering::Equal)
    });
    versions
}

/// Sort arbitrary version strings (e.g. `0.15.11`, `20.4.237`) descending
/// by their numeric components.
pub fn sort_version_desc(mut versions: Vec<String>) -> Vec<String> {
    versions.sort_by(|a, b| {
        let num = |s: &str| -> Vec<u64> {
            s.split(|c: char| !c.is_ascii_digit())
                .filter(|p| !p.is_empty())
                .filter_map(|p| p.parse().ok())
                .collect()
        };
        num(b).partial_cmp(&num(a)).unwrap_or(std::cmp::Ordering::Equal)
    });
    versions
}
