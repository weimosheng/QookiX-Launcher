//! 从本地 MC 客户端 jar 提取投影预览所需资源（方块模型、方块状态、纹理图集）。
//! 扫描 .minecraft/versions/ 取最新 release 版本，从 jar 中提取并生成资源文件，按版本缓存。
#![allow(non_snake_case)]

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Cursor, Read};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use zip::ZipArchive;

/// 解析版本 ID（如 "1.21.4"）为可比较的数字元组。
fn parse_version_id(id: &str) -> Vec<u32> {
    id.split('.')
        .filter_map(|s| s.split(|c: char| !c.is_ascii_digit()).next())
        .filter_map(|s| s.parse::<u32>().ok())
        .collect()
}

/// "1.21.4" / "1.20" / "26.2" 这种像版本号的字符串。
/// 注意：整合包的版本目录名和 id 往往是哈希（如 `5046802d`），不算版本号。
fn looks_like_mc_version(s: &str) -> bool {
    let core = s.split('-').next().unwrap_or(s);
    core.contains('.') && core.chars().all(|c| c.is_ascii_digit() || c == '.') && core != "."
}

/// 读 jar 内部的 `version.json` 拿版本号（官方客户端 jar 里有；
/// Forge 的形如 "1.12.2-forge-14.23.5.2860"，取第一段）。
fn jar_version_id(jar_path: &Path) -> Option<String> {
    let file = File::open(jar_path).ok()?;
    let mut archive = ZipArchive::new(BufReader::new(file)).ok()?;
    let mut entry = archive.by_name("version.json").ok()?;
    let mut buf = String::new();
    entry.read_to_string(&mut buf).ok()?;
    let v: Value = serde_json::from_str(&buf).ok()?;
    let id = v.get("id").and_then(|x| x.as_str())?;
    let core = id.split('-').next().unwrap_or(id);
    looks_like_mc_version(core).then(|| core.to_string())
}

/// 扫描 versions/ 目录，找**最新且资源格式可用**的版本。
///
/// 两个坑：
/// 1. 整合包版本的目录名 / `id` 常是哈希，`parse_version_id` 会解析出空值 ——
///    直接按 id 排序等于随机挑一个。真实版本要从 `inheritsFrom` 或 jar 里的
///    `version.json` 拿。
/// 2. 投影预览用的是 deepslate，只认 1.13+ 的扁平化资源（`block/stone`）。
///    1.12 及更早是 `blocks/anvil_base` 这种旧命名，提出来也对不上，必须跳过。
fn find_latest_version(versions_dir: &Path) -> Result<(String, PathBuf), String> {
    let entries = std::fs::read_dir(versions_dir).map_err(|e| format!("读取版本目录失败: {e}"))?;
    // 排序键：(是否经典 1.x, 版本号)。渲染用的 deepslate 跟的是经典版本号，
    // 新年份版本（26.2 这类）先让位，只有没装 1.x 时才用它。
    let mut best: Option<((u8, Vec<u32>), String, PathBuf)> = None;
    let mut legacy_only = false;

    for entry in entries.flatten() {
        let dir = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let jar_path = dir.join(format!("{name}.jar"));
        if !jar_path.is_file() {
            continue;
        }

        // 真实 MC 版本：inheritsFrom（模组版） > id（官方版） > jar 内 version.json
        let mut version = None;
        if let Ok(content) = std::fs::read_to_string(dir.join(format!("{name}.json"))) {
            if let Ok(v) = serde_json::from_str::<Value>(&content) {
                version = v
                    .get("inheritsFrom")
                    .and_then(|x| x.as_str())
                    .or_else(|| v.get("id").and_then(|x| x.as_str()))
                    .filter(|s| looks_like_mc_version(s))
                    .map(|s| s.to_string());
            }
        }
        let Some(version) = version.or_else(|| jar_version_id(&jar_path)) else {
            continue;
        };
        let key = parse_version_id(&version);
        if key.is_empty() {
            continue;
        }
        if key < vec![1, 13] {
            legacy_only = true;
            continue;
        }
        let rank = (u8::from(key.first() == Some(&1)), key);
        if best.as_ref().is_none_or(|(bk, _, _)| rank > *bk) {
            best = Some((rank, version, jar_path));
        }
    }

    match best {
        Some((_, version, jar)) => Ok((version, jar)),
        None if legacy_only => Err(
            "投影预览需要 1.13 及以上的客户端资源，当前只找到更旧的版本，请先安装一个 1.13+ 的版本"
                .into(),
        ),
        None => Err("未找到可用的 MC 版本（需要 1.13+）".into()),
    }
}

/// 检查缓存目录是否已有完整资源。
fn is_cache_complete(cache_dir: &Path) -> bool {
    const CACHE_VERSION: &str = "v7-atlas-pad";
    let version_file = cache_dir.join("cache-version.txt");
    match std::fs::read_to_string(&version_file) {
        Ok(v) if v.trim() == CACHE_VERSION => {}
        _ => return false,
    }
    let files: &[(&str, u64)] = &[
        ("block-model-index.json", 1_000),
        ("block-state-index.json", 1_000),
        ("texture-layout.json", 1_000),
        ("texture-atlas.png", 10_000),
        ("block-property-defaults.json", 10),
        ("block-name-index.json", 10),
        ("_debug.json", 10),
    ];
    files.iter().all(|(f, min_size)| {
        let path = cache_dir.join(f);
        std::fs::metadata(&path).map(|m| m.len() >= *min_size).unwrap_or(false)
    })
}

struct TextureEntry {
    key: String,
    width: u32,
    height: u32,
    data: Vec<u8>,
}

/// 从 jar 提取所有资源并写入缓存目录。
fn extract_resources_from_jar(jar_path: &Path, cache_dir: &Path) -> Result<(), String> {
    let file = File::open(jar_path).map_err(|e| format!("打开 jar 失败: {e}"))?;
    let mut archive = ZipArchive::new(BufReader::new(file)).map_err(|e| format!("读取 jar 失败: {e}"))?;

    let mut model_index: BTreeMap<String, Value> = BTreeMap::new();
    let mut state_index: BTreeMap<String, Value> = BTreeMap::new();
    let mut textures: Vec<TextureEntry> = Vec::new();
    let mut lang_data: Option<Value> = None;

    let mut tex_fail = 0;
    let mut tex_fail_msg = String::new();

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("读取 jar 条目失败: {e}"))?;
        let name = entry.name().to_string();

        if name.ends_with('/') {
            continue;
        }

        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry
            .read_to_end(&mut buf)
            .map_err(|e| format!("读取 jar 内容失败: {e}"))?;

        if name.starts_with("assets/minecraft/models/block/") && name.ends_with(".json") {
            let key = name
                .strip_prefix("assets/minecraft/models/")
                .and_then(|s| s.strip_suffix(".json"))
                .unwrap_or("")
                .to_string();
            if let Ok(v) = serde_json::from_slice::<Value>(&buf) {
                model_index.insert(key, v);
            }
        } else if name.starts_with("assets/minecraft/blockstates/") && name.ends_with(".json") {
            let key = name
                .strip_prefix("assets/minecraft/blockstates/")
                .and_then(|s| s.strip_suffix(".json"))
                .unwrap_or("")
                .to_string();
            if let Ok(v) = serde_json::from_slice::<Value>(&buf) {
                state_index.insert(key, v);
            }
        } else if name.starts_with("assets/minecraft/textures/") && name.ends_with(".png") {
            let key = name
                .strip_prefix("assets/minecraft/textures/")
                .and_then(|s| s.strip_suffix(".png"))
                .unwrap_or("")
                .to_string();
            match decode_png(&buf) {
                Ok(tex) => {
                    textures.push(TextureEntry { key, width: tex.0, height: tex.1, data: tex.2 });
                }
                Err(e) if tex_fail < 3 => {
                    tex_fail += 1;
                    tex_fail_msg.push_str(&format!("{}: {} | ", key, e));
                }
                Err(_) => { tex_fail += 1; }
            }
        } else if name == "assets/minecraft/lang/en_us.json" {
            if let Ok(v) = serde_json::from_slice::<Value>(&buf) {
                lang_data = Some(v);
            }
        }
    }

    eprintln!("[schematic] 模型:{} 状态:{} 纹理:{} 失败:{}", model_index.len(), state_index.len(), textures.len(), tex_fail);
    if !tex_fail_msg.is_empty() {
        eprintln!("[schematic] 失败示例: {}", tex_fail_msg);
    }

    // 防呆：渲染器（deepslate）只认 1.13+ 的扁平化模型。旧格式（`blocks/anvil_base`
    // 这种）提出来两边键名对不上，所有方块都会退化到图集左上角一个像素，
    // 整张投影变成一坨同色块 —— 不如直接报错说清楚。
    if !model_index.contains_key("block/stone") {
        return Err(
            "该版本的资源是 1.13 之前的旧格式，投影预览需要 1.13 及以上的客户端资源".to_string(),
        );
    }

    std::fs::create_dir_all(cache_dir).map_err(|e| format!("创建缓存目录失败: {e}"))?;

    // _debug.json
    let debug = json!({
        "models": model_index.len(),
        "states": state_index.len(),
        "textures_ok": textures.len(),
        "textures_fail": tex_fail,
        "fail_samples": tex_fail_msg,
    });
    std::fs::write(cache_dir.join("_debug.json"), debug.to_string())
        .map_err(|e| format!("写入调试信息失败: {e}"))?;

    // block-model-index.json
    let model_json = serde_json::to_string(&model_index)
        .map_err(|e| format!("序列化模型索引失败: {e}"))?;
    std::fs::write(cache_dir.join("block-model-index.json"), model_json)
        .map_err(|e| format!("写入模型索引失败: {e}"))?;

    // block-state-index.json
    let state_json = serde_json::to_string(&state_index)
        .map_err(|e| format!("序列化状态索引失败: {e}"))?;
    std::fs::write(cache_dir.join("block-state-index.json"), state_json)
        .map_err(|e| format!("写入状态索引失败: {e}"))?;

    // texture-atlas.png + texture-layout.json
    let (atlas_w, atlas_h, atlas_data, layout) = pack_texture_atlas(&textures);
    eprintln!("[schematic] 图集尺寸:{}x{} 布局条目:{}", atlas_w, atlas_h, layout.len());
    eprintln!("[schematic] 样例键:{}", {
        let samples: Vec<&str> = layout.keys().take(5).map(|s| s.as_str()).collect();
        samples.join(", ")
    });
    encode_png(cache_dir.join("texture-atlas.png"), atlas_w, atlas_h, &atlas_data)
        .map_err(|e| format!("写入纹理图集失败: {e}"))?;
    let layout_json = serde_json::to_string(&layout)
        .map_err(|e| format!("序列化纹理布局失败: {e}"))?;
    std::fs::write(cache_dir.join("texture-layout.json"), layout_json)
        .map_err(|e| format!("写入纹理布局失败: {e}"))?;

    // block-property-defaults.json
    let defaults = generate_property_defaults(&state_index);
    let defaults_json = serde_json::to_string(&defaults)
        .map_err(|e| format!("序列化属性默认值失败: {e}"))?;
    std::fs::write(cache_dir.join("block-property-defaults.json"), defaults_json)
        .map_err(|e| format!("写入属性默认值失败: {e}"))?;

    // block-name-index.json
    let names = generate_name_index(lang_data);
    let names_json = serde_json::to_string(&names)
        .map_err(|e| format!("序列化方块名称索引失败: {e}"))?;
    std::fs::write(cache_dir.join("block-name-index.json"), names_json)
        .map_err(|e| format!("写入方块名称索引失败: {e}"))?;

    std::fs::write(cache_dir.join("cache-version.txt"), "v4-trns-alpha")
        .map_err(|e| format!("写入缓存版本失败: {e}"))?;

    Ok(())
}

/// 解码 PNG 为 RGBA 像素数据。
fn decode_png(data: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let decoder = png::Decoder::new(Cursor::new(data));
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("PNG 解码失败: {e}"))?;
    let info = reader.info().clone();
    let width = info.width;
    let height = info.height;

    let mut buf = vec![0u8; reader.output_buffer_size()];
    let frame = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("PNG 帧读取失败: {e}"))?;
    buf.truncate(frame.buffer_size());

    let expected = (width as usize) * (height as usize);
    if info.color_type == png::ColorType::Rgba {
        if buf.len() != expected * 4 {
            return Err(format!("RGBA 数据大小不匹配: {} != {}", buf.len(), expected * 4));
        }
        Ok((width, height, buf))
    } else if info.color_type == png::ColorType::Rgb {
        if buf.len() != expected * 3 {
            return Err(format!("RGB 数据大小不匹配: {} != {}", buf.len(), expected * 3));
        }
        let mut rgba = Vec::with_capacity(expected * 4);
        for chunk in buf.chunks_exact(3) {
            rgba.push(chunk[0]);
            rgba.push(chunk[1]);
            rgba.push(chunk[2]);
            rgba.push(255);
        }
        Ok((width, height, rgba))
    } else if info.color_type == png::ColorType::Grayscale {
        if buf.len() != expected {
            return Err(format!("灰度数据大小不匹配: {} != {}", buf.len(), expected));
        }
        let mut rgba = Vec::with_capacity(expected * 4);
        for &g in &buf {
            rgba.push(g);
            rgba.push(g);
            rgba.push(g);
            rgba.push(255);
        }
        Ok((width, height, rgba))
    } else if info.color_type == png::ColorType::GrayscaleAlpha {
        if buf.len() != expected * 2 {
            return Err(format!("灰度Alpha数据大小不匹配: {} != {}", buf.len(), expected * 2));
        }
        let mut rgba = Vec::with_capacity(expected * 4);
        for chunk in buf.chunks_exact(2) {
            rgba.push(chunk[0]);
            rgba.push(chunk[0]);
            rgba.push(chunk[0]);
            rgba.push(chunk[1]);
        }
        Ok((width, height, rgba))
    } else if info.color_type == png::ColorType::Indexed {
        let palette = info.palette.as_ref().ok_or("索引 PNG 缺少调色板")?;
        let trns = info.trns.as_deref().unwrap_or(&[]);
        let bd = info.bit_depth as u8 as usize;
        let mut rgba = Vec::with_capacity(expected * 4);
        if bd == 8 {
            if buf.len() < expected {
                return Err(format!("索引数据大小不匹配: {} < {}", buf.len(), expected));
            }
            for &idx in &buf[..expected] {
                let i = idx as usize * 3;
                rgba.push(palette.get(i).copied().unwrap_or(0));
                rgba.push(palette.get(i + 1).copied().unwrap_or(0));
                rgba.push(palette.get(i + 2).copied().unwrap_or(0));
                rgba.push(trns.get(idx as usize).copied().unwrap_or(255));
            }
        } else {
            let ppb = 8 / bd;
            let mask = (1u8 << bd) - 1;
            let row_bytes = (width as usize * bd + 7) / 8;
            for y in 0..height as usize {
                for x in 0..width as usize {
                    let bi = y * row_bytes + x / ppb;
                    let shift = 8 - bd - (x % ppb) * bd;
                    let idx = (buf.get(bi).copied().unwrap_or(0) >> shift) & mask;
                    let i = idx as usize * 3;
                    rgba.push(palette.get(i).copied().unwrap_or(0));
                    rgba.push(palette.get(i + 1).copied().unwrap_or(0));
                    rgba.push(palette.get(i + 2).copied().unwrap_or(0));
                    rgba.push(trns.get(idx as usize).copied().unwrap_or(255));
                }
            }
        }
        Ok((width, height, rgba))
    } else {
        Err(format!("不支持的 PNG 颜色类型: {:?}", info.color_type))
    }
}

/// 编码 RGBA 数据为 PNG 文件。
fn encode_png(path: PathBuf, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let file = File::create(path).map_err(|e| format!("创建 PNG 文件失败: {e}"))?;
    let w = BufWriter::new(file);
    let mut encoder = png::Encoder::new(w, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| format!("PNG 编码头失败: {e}"))?;
    writer
        .write_image_data(rgba)
        .map_err(|e| format!("PNG 编码数据失败: {e}"))?;
    Ok(())
}

/// 将纹理打包成图集（shelf-based packing）。
fn pack_texture_atlas(
    textures: &[TextureEntry],
) -> (u32, u32, Vec<u8>, BTreeMap<String, [u32; 4]>) {
    // 宽一点、矮一点：1024 宽会堆出两万多像素高，超过不少显卡的
    // MAX_TEXTURE_SIZE（常见上限 16384），贴着上限很危险。
    const MAX_WIDTH: u32 = 4096;
    /// 每张贴图四周留的边：内容用边缘像素扩展填充。
    /// 必须留 —— 掠射角下 mipmap 会越出贴图边界采样，紧挨着打包就会把隔壁
    /// 贴图（很多是透明的）混进来，alpha 掉到 alphaTest 阈值以下被抠掉，
    /// 每条方块边界就出现一条断续的暗线。
    const PAD: u32 = 2;

    let mut sorted: Vec<&TextureEntry> = textures.iter().collect();
    sorted.sort_by(|a, b| b.height.cmp(&a.height).then(b.width.cmp(&a.width)));

    let mut layout: BTreeMap<String, [u32; 4]> = BTreeMap::new();
    let mut atlas_w: u32 = 0;
    let mut atlas_h: u32 = 0;
    let mut shelf_x: u32 = 0;
    let mut shelf_y: u32 = 0;
    let mut shelf_h: u32 = 0;

    for tex in &sorted {
        let bw = tex.width + PAD * 2;
        let bh = tex.height + PAD * 2;
        if shelf_x + bw > MAX_WIDTH {
            shelf_y += shelf_h;
            shelf_x = 0;
            shelf_h = 0;
        }
        // 布局里只记内圈（真正贴图）的坐标，四周的边不参与 UV
        layout.insert(
            tex.key.clone(),
            [shelf_x + PAD, shelf_y + PAD, tex.width, tex.height],
        );
        shelf_x += bw;
        if bh > shelf_h {
            shelf_h = bh;
        }
        if shelf_x > atlas_w {
            atlas_w = shelf_x;
        }
        if shelf_y + shelf_h > atlas_h {
            atlas_h = shelf_y + shelf_h;
        }
    }

    if atlas_w == 0 {
        atlas_w = 1;
    }
    if atlas_h == 0 {
        atlas_h = 1;
    }

    let mut atlas_data = vec![0u8; (atlas_w * atlas_h * 4) as usize];
    for tex in &sorted {
        let &[x, y, w, h] = &layout[&tex.key];
        let expected_len = (w * h * 4) as usize;
        if tex.data.len() < expected_len || w == 0 || h == 0 {
            continue;
        }
        // 连四周的边一起写：源坐标 clamp 到贴图内 → 边缘像素向外扩展
        for row in 0..(h + PAD * 2) {
            let sy = row.saturating_sub(PAD).min(h - 1);
            let dst_y = (y - PAD) + row;
            for col in 0..(w + PAD * 2) {
                let sx = col.saturating_sub(PAD).min(w - 1);
                let src = ((sy * tex.width + sx) * 4) as usize;
                let dst = ((dst_y * atlas_w + ((x - PAD) + col)) * 4) as usize;
                if src + 4 > tex.data.len() || dst + 4 > atlas_data.len() {
                    continue;
                }
                atlas_data[dst..dst + 4].copy_from_slice(&tex.data[src..src + 4]);
            }
        }
    }

    (atlas_w, atlas_h, atlas_data, layout)
}

/// 从 blockstate JSON 提取默认属性值。
fn generate_property_defaults(
    state_index: &BTreeMap<String, Value>,
) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut defaults: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for (name, state) in state_index {
        let mut props: BTreeMap<String, String> = BTreeMap::new();
        if let Some(variants) = state.get("variants").and_then(|v| v.as_object()) {
            for key in variants.keys() {
                for part in key.split(',') {
                    if let Some(eq) = part.find('=') {
                        let k = &part[..eq];
                        let v = &part[eq + 1..];
                        props.entry(k.to_string()).or_insert_with(|| v.to_string());
                    }
                }
            }
        }
        if let Some(multipart) = state.get("multipart").and_then(|v| v.as_array()) {
            for part in multipart {
                if let Some(when) = part.get("when").and_then(|w| w.as_object()) {
                    for (k, v) in when {
                        if let Some(s) = v.as_str() {
                            props.entry(k.clone()).or_insert_with(|| s.to_string());
                        }
                    }
                }
            }
        }
        if !props.is_empty() {
            defaults.insert(name.clone(), props);
        }
    }
    defaults
}

/// 从语言文件生成方块名称索引。
fn generate_name_index(lang_data: Option<Value>) -> Value {
    let mut result = BTreeMap::new();
    if let Some(lang) = lang_data.and_then(|v| v.as_object().map(|o| o.clone())) {
        let mut block_names = BTreeMap::new();
        for (k, v) in &lang {
            if k.starts_with("block.minecraft.") {
                if let Some(s) = v.as_str() {
                    block_names.insert(k.clone(), s.to_string());
                }
            }
        }
        result.insert("en_us".to_string(), block_names);
    }
    json!(result)
}

/// 提取是重活（解码几千张贴图 + 打包图集 + 编码 PNG）。同时打开两个投影文件
/// 会并发提取、互相删掉对方的缓存目录，结果就是两边都拿到半成品 —— 串行化。
static EXTRACT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 提取本地 MC 资源，返回缓存目录路径。
#[tauri::command]
pub async fn schematic_extract_resources(
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<String, String> {
    let versions_dir = state.versions_dir();
    let root = state.root.clone();

    tokio::task::spawn_blocking(move || {
        // 按真实 MC 版本命名缓存目录：同一个版本的不同整合包共用一份，不重复提取
        let (version, jar_path) = find_latest_version(&versions_dir)?;

        let cache_dir = root.join("schematic-resources").join(&version);

        // 串行化；拿到锁后再查一次缓存，排在前面的任务可能刚提完
        let _guard = EXTRACT_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if is_cache_complete(&cache_dir) {
            return Ok(cache_dir.to_string_lossy().to_string());
        }

        if cache_dir.exists() {
            std::fs::remove_dir_all(&cache_dir)
                .map_err(|e| format!("清空旧缓存失败: {e}"))?;
        }

        extract_resources_from_jar(&jar_path, &cache_dir)?;

        Ok(cache_dir.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| format!("资源提取任务失败: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_version(dir: &Path, name: &str, json: &str) {
        let d = dir.join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(format!("{name}.jar")), b"stub").unwrap();
        std::fs::write(d.join(format!("{name}.json")), json).unwrap();
    }

    /// 整合包版本的目录名/id 是哈希，必须按 inheritsFrom 里的真实版本判定，
    /// 并且跳过 1.13 之前的旧资源格式
    #[test]
    fn picks_modern_version_not_modpack_hash() {
        let base = std::env::temp_dir().join(format!("qookix-ver-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        // 1.12.2 整合包：目录名是哈希，真实版本写在 inheritsFrom 里
        write_version(
            &base,
            "5046802d",
            r#"{"id":"5046802d","type":"release","inheritsFrom":"1.12.2"}"#,
        );
        // 1.21.3：id 本身就是版本号
        write_version(&base, "a4b7bb8b", r#"{"id":"1.21.3","type":"release"}"#);
        // 年份版本号（渲染库跟的是经典 1.x，让它让位）
        write_version(&base, "c2d82696", r#"{"id":"26.2","type":"release"}"#);

        let (version, jar) = find_latest_version(&base).expect("应挑到 1.21.3");
        assert_eq!(version, "1.21.3");
        assert_eq!(jar, base.join("a4b7bb8b").join("a4b7bb8b.jar"));

        // 只剩年份版本时，它也是可用的（1.13+ 的扁平化资源）
        std::fs::remove_dir_all(base.join("a4b7bb8b")).unwrap();
        assert_eq!(find_latest_version(&base).unwrap().0, "26.2");
        std::fs::remove_dir_all(base.join("c2d82696")).unwrap();

        // 只剩旧版本：明确报错，而不是提一堆用不了的资源出来
        let err = find_latest_version(&base).unwrap_err();
        assert!(err.contains("1.13"), "错误信息要说明需要 1.13+：{err}");

        let _ = std::fs::remove_dir_all(&base);
    }

    /// 图集里每张贴图四周要有边缘扩展的边（否则 mip 采样会串到隔壁贴图）
    #[test]
    fn atlas_pads_tiles_with_edge_replication() {
        let mk = |key: &str, color: [u8; 4]| {
            let mut data = Vec::new();
            for _ in 0..4 {
                data.extend_from_slice(&color);
            }
            TextureEntry {
                key: key.to_string(),
                width: 2,
                height: 2,
                data,
            }
        };
        let (w, _h, data, layout) = pack_texture_atlas(&[
            mk("a", [255, 0, 0, 255]),
            mk("b", [0, 0, 255, 128]),
        ]);
        let px = |x: u32, y: u32| {
            let i = ((y * w + x) * 4) as usize;
            [data[i], data[i + 1], data[i + 2], data[i + 3]]
        };
        let [ax, ay, aw, ah] = layout["a"];
        assert_eq!(px(ax, ay), [255, 0, 0, 255], "内圈是原像素");
        assert_eq!(px(ax - 1, ay), [255, 0, 0, 255], "左边要做边缘扩展");
        assert_eq!(px(ax + aw, ay + ah - 1), [255, 0, 0, 255], "右边要做边缘扩展");
        assert_eq!(px(ax, ay + ah), [255, 0, 0, 255], "下边要做边缘扩展");
        assert_eq!(px(ax + aw - 1, ay - 1), [255, 0, 0, 255], "上边要做边缘扩展");

        let [bx, ..] = layout["b"];
        assert!(bx >= ax + aw + 2, "两张贴图之间要隔开：{ax}+{aw} vs {bx}");
    }

    fn looks_like_version() {
        assert!(looks_like_mc_version("1.21.4"));
        assert!(looks_like_mc_version("26.2"));
        assert!(!looks_like_mc_version("5046802d"));
        assert!(!looks_like_mc_version("a4b7bb8b"));
    }
}
