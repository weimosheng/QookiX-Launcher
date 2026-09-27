//! 区块文件（`.mca` / Anvil region）读写。
//!
//! 文件结构：
//! - 8 KiB 头：1024 个 4 字节条目（高 3 字节 = 起始扇区号，低 1 字节 = 扇区数）
//!   + 1024 个 4 字节时间戳；条目顺序 = `(cx & 31) + (cz & 31) * 32`
//! - 数据区：`4 字节大端长度（含压缩类型字节） + 1 字节压缩类型 + NBT`，
//!   按 4 KiB 扇区对齐，尾部补零。压缩类型 1=gzip、2=zlib、3=不压缩。
//!
//! 写策略（安全优先）：把整个 region 文件读进内存，改完整体写回
//! （先写临时文件再替换，见 `level::write_bytes`）。新区块或放不下原槽位时
//! 追加到文件末尾并更新头部，旧槽位留空——游戏按头部表寻址，空洞无害。

use crate::nbt::level;
use fastnbt::Value;
use flate2::read::{GzDecoder, ZlibDecoder};
use flate2::write::ZlibEncoder;
use flate2::Compression;
use serde_json::{json, Value as Json};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const SECTOR: usize = 4096;
const HEADER_LEN: usize = SECTOR * 2;
/// 单个条目最多 255 个扇区（≈1 MiB），超过写不下
const MAX_SECTORS: usize = 255;

/// 维度对应的 region 目录
pub fn region_dir(world_dir: &Path, dim: &str) -> PathBuf {
    match dim {
        "nether" => world_dir.join("DIM-1").join("region"),
        "end" => world_dir.join("DIM1").join("region"),
        _ => world_dir.join("region"),
    }
}

/// 某个区块所在的 region 文件路径
pub fn region_file(world_dir: &Path, dim: &str, cx: i32, cz: i32) -> PathBuf {
    region_dir(world_dir, dim).join(format!("r.{}.{}.mca", cx >> 5, cz >> 5))
}

/// 条目下标：区块坐标在 region 内的相对位置
fn entry_index(cx: i32, cz: i32) -> usize {
    (cx.rem_euclid(32) + cz.rem_euclid(32) * 32) as usize
}

/// 读条目 → (起始字节偏移, 扇区数)；偏移 0 表示该区块未生成
fn read_entry(bytes: &[u8], index: usize) -> (usize, usize) {
    let p = index * 4;
    if bytes.len() < p + 4 {
        return (0, 0);
    }
    let raw = u32::from_be_bytes([bytes[p], bytes[p + 1], bytes[p + 2], bytes[p + 3]]);
    (((raw >> 8) as usize) * SECTOR, (raw & 0xFF) as usize)
}

fn parse_region_name(name: &str) -> Option<(i32, i32)> {
    let rest = name.strip_prefix("r.")?.strip_suffix(".mca")?;
    let (x, z) = rest.split_once('.')?;
    Some((x.parse().ok()?, z.parse().ok()?))
}

/// 列出该维度下所有已生成的区块（按坐标排序）
pub fn list_chunks(world_dir: &Path, dim: &str) -> Result<Vec<Json>, String> {
    let dir = region_dir(world_dir, dim);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for e in std::fs::read_dir(&dir).map_err(|e| format!("读取 region 目录失败: {e}"))?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some((rx, rz)) = parse_region_name(&name) else {
            continue;
        };
        // 只读 8KiB 头部：region 文件动辄几十 MB，整份读进来纯属浪费
        let Ok(mut f) = std::fs::File::open(e.path()) else {
            continue;
        };
        let mut header = vec![0u8; HEADER_LEN];
        let n = f.read(&mut header).unwrap_or(0);
        if n < SECTOR + 4 {
            continue;
        }
        header.truncate(n);
        for i in 0..1024usize {
            let (offset, sectors) = read_entry(&header, i);
            if offset == 0 || sectors == 0 {
                continue;
            }
            let p = SECTOR + i * 4;
            let modified = u32::from_be_bytes([header[p], header[p + 1], header[p + 2], header[p + 3]]);
            out.push(json!({
                "cx": rx * 32 + (i % 32) as i32,
                "cz": rz * 32 + (i / 32) as i32,
                "size": sectors * SECTOR,
                "modified": modified,
            }));
        }
    }
    out.sort_by(|a, b| {
        let (ax, az) = (
            a.get("cx").and_then(|x| x.as_i64()).unwrap_or(0),
            a.get("cz").and_then(|x| x.as_i64()).unwrap_or(0),
        );
        let (bx, bz) = (
            b.get("cx").and_then(|x| x.as_i64()).unwrap_or(0),
            b.get("cz").and_then(|x| x.as_i64()).unwrap_or(0),
        );
        (az, ax).cmp(&(bz, bx))
    });
    Ok(out)
}

/// 读一个区块的 NBT（根复合节点就是区块数据，没有 level.dat 那种 Data 包装层）
pub fn read_chunk(world_dir: &Path, dim: &str, cx: i32, cz: i32) -> Result<Value, String> {
    let path = region_file(world_dir, dim, cx, cz);
    let bytes = std::fs::read(&path)
        .map_err(|_| format!("区域文件不存在：{}（该区块尚未生成）", path.display()))?;
    decode_chunk(&bytes, cx, cz)
}

/// 从 region 文件内容里解出一个区块（地图渲染要批量用，所以开放给 crate 内）
pub(crate) fn decode_chunk(bytes: &[u8], cx: i32, cz: i32) -> Result<Value, String> {
    let (offset, sectors) = read_entry(bytes, entry_index(cx, cz));
    if offset == 0 {
        return Err("该区块尚未生成".into());
    }
    let end = (offset + sectors * SECTOR).min(bytes.len());
    let slot = bytes
        .get(offset..end)
        .filter(|s| s.len() >= 5)
        .ok_or_else(|| "区块数据不完整".to_string())?;
    let len = u32::from_be_bytes([slot[0], slot[1], slot[2], slot[3]]) as usize;
    let ctype = slot[4];
    let payload = slot
        .get(5..5 + len)
        .ok_or_else(|| "区块数据长度越界".to_string())?;
    let raw = match ctype {
        1 => {
            let mut d = GzDecoder::new(payload);
            let mut v = Vec::new();
            d.read_to_end(&mut v).map_err(|e| format!("解压区块失败(gzip): {e}"))?;
            v
        }
        2 => {
            let mut d = ZlibDecoder::new(payload);
            let mut v = Vec::new();
            d.read_to_end(&mut v).map_err(|e| format!("解压区块失败(zlib): {e}"))?;
            v
        }
        3 => payload.to_vec(),
        4 => return Err("该区块用了 LZ4 压缩（模组格式），暂不支持".into()),
        other => return Err(format!("未知的区块压缩类型: {other}")),
    };
    fastnbt::from_bytes(&raw).map_err(|e| format!("解析区块 NBT 失败: {e}"))
}

/// 写回一个区块（新增或覆盖）。返回是否需要在写盘前备份——调用方负责备份。
pub fn write_chunk(world_dir: &Path, dim: &str, cx: i32, cz: i32, data: &Value) -> Result<(), String> {
    let path = region_file(world_dir, dim, cx, cz);
    let mut bytes = std::fs::read(&path).unwrap_or_default();
    if bytes.len() < HEADER_LEN {
        bytes.resize(HEADER_LEN, 0);
    }

    let raw = fastnbt::to_bytes_with_opts(data, fastnbt::SerOpts::new())
        .map_err(|e: fastnbt::error::Error| format!("序列化区块失败: {e}"))?;
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(&raw).map_err(|e| format!("压缩区块失败: {e}"))?;
    let payload = enc.finish().map_err(|e| format!("压缩区块失败: {e}"))?;

    // 长度字段 = 压缩类型字节 + 数据
    let need = 5 + payload.len();
    let sectors_needed = need.div_ceil(SECTOR);
    if sectors_needed > MAX_SECTORS {
        return Err("区块数据过大，超出单个条目的上限".into());
    }

    let index = entry_index(cx, cz);
    let (old_offset, old_sectors) = read_entry(&bytes, index);
    // 原地覆盖（放得下）或追加到末尾（放不下 / 新区块）
    let offset = if old_offset != 0 && old_sectors >= sectors_needed {
        old_offset
    } else {
        let new_offset = bytes.len().div_ceil(SECTOR) * SECTOR;
        bytes.resize(new_offset, 0);
        new_offset
    };

    // 注意只能变长：原地覆盖时如果 resize 变小会把后面的区块整段切掉
    let slot_end = offset + sectors_needed * SECTOR;
    if bytes.len() < slot_end {
        bytes.resize(slot_end, 0);
    }
    bytes[offset..offset + 4].copy_from_slice(&((payload.len() + 1) as u32).to_be_bytes());
    bytes[offset + 4] = 2; // zlib
    bytes[offset + 5..offset + 5 + payload.len()].copy_from_slice(&payload);
    for b in &mut bytes[offset + 5 + payload.len()..offset + sectors_needed * SECTOR] {
        *b = 0;
    }

    // 更新头部条目与时间戳
    let entry = (((offset / SECTOR) as u32) << 8) | sectors_needed as u32;
    bytes[index * 4..index * 4 + 4].copy_from_slice(&entry.to_be_bytes());
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as u32)
        .unwrap_or(0);
    let tp = SECTOR + index * 4;
    bytes[tp..tp + 4].copy_from_slice(&ts.to_be_bytes());

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建区域目录失败: {e}"))?;
    }
    level::write_bytes(&path, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn chunk_with(seed: &str) -> Value {
        let mut m = HashMap::new();
        m.insert("DataVersion".into(), Value::Int(3465));
        m.insert("xPos".into(), Value::Int(0));
        m.insert("zPos".into(), Value::Int(0));
        m.insert("Status".into(), Value::String("full".into()));
        m.insert("Note".into(), Value::String(seed.into()));
        Value::Compound(m)
    }

    fn tmp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("qookix-region-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn write_then_read_roundtrip() {
        let world = tmp_dir("roundtrip");
        write_chunk(&world, "overworld", 0, 0, &chunk_with("a")).unwrap();
        write_chunk(&world, "overworld", 33, -1, &chunk_with("b")).unwrap();

        let a = read_chunk(&world, "overworld", 0, 0).unwrap();
        assert_eq!(level::get_string(&a, "Note").as_deref(), Some("a"));
        let b = read_chunk(&world, "overworld", 33, -1).unwrap();
        assert_eq!(level::get_string(&b, "Note").as_deref(), Some("b"));
        assert_eq!(level::get_int(&b, "xPos"), Some(0));

        let list = list_chunks(&world, "overworld").unwrap();
        assert_eq!(list.len(), 2);
        // 两个区块落在同一个 region 文件里（33 与 -1 属于 r.1.-1）
        assert!(world.join("region/r.0.0.mca").is_file());
        assert!(world.join("region/r.1.-1.mca").is_file());
        let _ = std::fs::remove_dir_all(&world);
    }

    /// 放得下就原地覆盖：文件长度不变，后面的区块不受影响
    #[test]
    fn in_place_overwrite_keeps_file_tail() {
        let world = tmp_dir("inplace");
        write_chunk(&world, "overworld", 0, 0, &chunk_with("aaaaaaaa")).unwrap();
        write_chunk(&world, "overworld", 1, 0, &chunk_with("neighbour")).unwrap();
        let path = world.join("region/r.0.0.mca");
        let before = std::fs::metadata(&path).unwrap().len();

        // 改成更短的内容 → 仍是原地覆盖，绝不能把文件截断
        write_chunk(&world, "overworld", 0, 0, &chunk_with("b")).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().len(),
            before,
            "原地覆盖不该改变文件长度"
        );
        let back = read_chunk(&world, "overworld", 0, 0).unwrap();
        assert_eq!(level::get_string(&back, "Note").as_deref(), Some("b"));
        let neighbour = read_chunk(&world, "overworld", 1, 0).unwrap();
        assert_eq!(
            level::get_string(&neighbour, "Note").as_deref(),
            Some("neighbour"),
            "原地覆盖不能切掉后面的区块"
        );
        let _ = std::fs::remove_dir_all(&world);
    }

    /// 放不下就迁移到文件末尾，隔壁区块同样必须完好
    #[test]
    fn larger_edit_relocates_and_keeps_neighbour() {
        let world = tmp_dir("grow");
        write_chunk(&world, "overworld", 0, 0, &chunk_with("keep-me")).unwrap();
        write_chunk(&world, "overworld", 1, 0, &chunk_with("x")).unwrap();
        let path = world.join("region/r.0.0.mca");
        let before = std::fs::metadata(&path).unwrap().len();

        // 用不可压缩的随机字节，确保放不下原槽位、必须迁移
        let mut x: u32 = 0x9E37_79B9;
        let noise: Vec<i8> = (0..20_000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x >> 24) as i8
            })
            .collect();
        let mut big = chunk_with("grown");
        if let Value::Compound(m) = &mut big {
            m.insert("Noise".into(), Value::ByteArray(fastnbt::ByteArray::new(noise)));
        }
        write_chunk(&world, "overworld", 0, 0, &big).unwrap();
        assert!(
            std::fs::metadata(&path).unwrap().len() > before,
            "放不下时应追加到文件末尾"
        );

        let grown = read_chunk(&world, "overworld", 0, 0).unwrap();
        assert_eq!(level::get_string(&grown, "Note").as_deref(), Some("grown"));
        assert!(level::get(&grown, "Noise").is_some());
        let neighbour = read_chunk(&world, "overworld", 1, 0).unwrap();
        assert_eq!(level::get_string(&neighbour, "Note").as_deref(), Some("x"));
        let _ = std::fs::remove_dir_all(&world);
    }

    #[test]
    fn missing_chunk_reports_clearly() {
        let world = tmp_dir("missing");
        let err = read_chunk(&world, "overworld", 5, 5).unwrap_err();
        assert!(err.contains("尚未生成"), "实际报错: {err}");
        let _ = std::fs::remove_dir_all(&world);
    }

    #[test]
    fn dimension_dirs_differ() {
        let world = Path::new("/tmp/w");
        assert!(region_dir(world, "overworld").ends_with("region"));
        assert!(region_dir(world, "nether").ends_with("DIM-1/region"));
        assert!(region_dir(world, "end").ends_with("DIM1/region"));
        assert_eq!(entry_index(0, 0), 0);
        // 33 → 相对 1；-1 → 相对 31
        assert_eq!(entry_index(33, -1), 1 + 31 * 32);
        assert_eq!(entry_index(-1, -1), 31 + 31 * 32);
    }
}
