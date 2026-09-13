use crate::cubiomes;
use serde::Serialize;

/// 前端把种子当**十进制字符串**传：Minecraft 种子是 64 位整数，很多存档的种子
/// 超出 JS 安全整数范围（2^53），走 JSON number 会被静默舍入成另一个世界。
fn parse_seed(seed: &str) -> Result<u64, String> {
    seed.trim()
        .parse::<i64>()
        .map(|v| v as u64)
        .map_err(|_| format!("种子必须是 64 位整数：{seed}"))
}

#[derive(Serialize)]
pub struct BiomeResult {
    pub id: i32,
    pub name: String,
}

#[derive(Serialize)]
pub struct StructureHit {
    pub r#type: String,
    pub x: i32,
    pub z: i32,
    pub region_x: i32,
    pub region_z: i32,
}

/// 查询指定种子下 (x,y,z) 处的生物群系（block 坐标，scale=1）。
#[tauri::command]
pub fn toolbox_query_biome(
    seed: String,
    mc: String,
    dim: i32,
    x: i32,
    y: i32,
    z: i32,
    large_biomes: Option<bool>,
) -> Result<BiomeResult, String> {
    let seed_u = parse_seed(&seed)?;
    let mc_id = cubiomes::parse_mc(&mc)?;
    let id = cubiomes::query_biome(
        seed_u,
        mc_id,
        dim,
        1,
        x,
        y,
        z,
        large_biomes.unwrap_or(false),
    );
    Ok(BiomeResult {
        id,
        name: cubiomes::biome_name(id),
    })
}

/// 估算 (x,z) 处的地表高度（方块），用于生成 /tp 指令的 y。
/// 1.18+ 主世界按生物群系深度噪声反推（误差几格）；旧版本 / 其它维度用群系基准高度。
#[tauri::command]
pub fn toolbox_surface_height(
    seed: String,
    mc: String,
    dim: i32,
    x: i32,
    z: i32,
) -> Result<i32, String> {
    let seed_u = parse_seed(&seed)?;
    let mc_id = cubiomes::parse_mc(&mc)?;
    Ok(cubiomes::surface_height(seed_u, mc_id, dim, x, z))
}

#[derive(Serialize)]
pub struct BiomeInfo {
    pub id: i32,
    pub name: String,
    /// 基准高度（-1..1 归一化），前端用于地貌阴影。
    pub depth: f32,
    /// 起伏强度。
    pub scale: f32,
}

/// 全部已知生物群系的 id / 名称 / 基准高度，供前端地图图例与地貌着色使用。
#[tauri::command]
pub fn toolbox_biome_table() -> Vec<BiomeInfo> {
    cubiomes::biome_table()
        .into_iter()
        .map(|(id, name, depth, scale)| BiomeInfo {
            id,
            name: name.to_string(),
            depth,
            scale,
        })
        .collect()
}

/// 查询中心 (center_x, center_z) 附近 radius_chunks 范围内的结构生成位置。
/// types 为结构类型字符串列表（village/monument/mansion/...）。
/// 计算量大，放到阻塞线程池执行，避免卡住主线程。
#[tauri::command]
pub async fn toolbox_query_structures(
    seed: String,
    mc: String,
    center_x: i32,
    center_z: i32,
    radius_chunks: i32,
    types: Vec<String>,
) -> Result<Vec<StructureHit>, String> {
    let seed_u = parse_seed(&seed)?;
    let mc_id = cubiomes::parse_mc(&mc)?;
    tokio::task::spawn_blocking(move || {
        let block_radius = radius_chunks.max(0) * 16;
        let mut out = Vec::new();
        for t in &types {
            let stype = match cubiomes::parse_structure_type(t) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let (region_size, _chunk_range) = match cubiomes::get_structure_info(stype, mc_id) {
                Some(info) => info,
                None => continue,
            };
            if region_size <= 0 {
                continue;
            }
            let region_block = region_size as i32 * 16;
            let reg_cx = center_x.div_euclid(region_block);
            let reg_cz = center_z.div_euclid(region_block);
            let reg_radius = block_radius / region_block + 1;
            for rx in (reg_cx - reg_radius)..=(reg_cx + reg_radius) {
                for rz in (reg_cz - reg_radius)..=(reg_cz + reg_radius) {
                    if let Some((px, pz)) = cubiomes::get_structure_pos(stype, seed_u, mc_id, rx, rz) {
                        if (px - center_x).abs() <= block_radius && (pz - center_z).abs() <= block_radius {
                            out.push(StructureHit {
                                r#type: t.clone(),
                                x: px,
                                z: pz,
                                region_x: rx,
                                region_z: rz,
                            });
                        }
                    }
                }
            }
        }
        out.sort_by_key(|h| (h.x - center_x).abs() + (h.z - center_z).abs());
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 查询指定区块是否为史莱姆区块。
#[tauri::command]
pub fn toolbox_slime_chunk(seed: String, chunk_x: i32, chunk_z: i32) -> Result<bool, String> {
    Ok(cubiomes::is_slime_chunk(parse_seed(&seed)?, chunk_x, chunk_z))
}

#[derive(Serialize)]
pub struct SpawnResult {
    pub x: i32,
    pub z: i32,
}

/// 主世界出生点（cubiomes getSpawn）。计算较重，放到阻塞线程池。
#[tauri::command]
pub async fn toolbox_world_spawn(
    seed: String,
    mc: String,
    large_biomes: Option<bool>,
) -> Result<SpawnResult, String> {
    let seed_u = parse_seed(&seed)?;
    let mc_id = cubiomes::parse_mc(&mc)?;
    tokio::task::spawn_blocking(move || {
        cubiomes::get_spawn(seed_u, mc_id, large_biomes.unwrap_or(false))
            .map(|(x, z)| SpawnResult { x, z })
            .ok_or_else(|| "无法计算出生点".to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 瓦片响应的二进制布局（小端）：
///   u32 width | u32 height | u32 shade_size | u32 reserved
///   width*height 字节群系 id（255 = 无）
///   shade_size*shade_size 字节山体阴影亮度（0..255）
const TILE_HEADER: usize = 16;

/// 批量生成以 (center_x, center_z) 为中心的生物群系地图（含 padding，供求阴影梯度）。
/// size 为每边单元数（正方形），scale 为 1 / 4 / 16 / 64（每格方块数）。
/// 直接返回原始字节（前端拿到 ArrayBuffer），避免大数组走 JSON 序列化 —— 这是
/// 地图流畅度的关键：单块瓦片几十 KB 而不是几百 KB，且前端无需解析数字数组。
#[tauri::command]
pub async fn toolbox_query_biome_map(
    seed: String,
    mc: String,
    dim: i32,
    center_x: i32,
    center_z: i32,
    size: i32,
    scale: i32,
    large_biomes: Option<bool>,
    want_shade: Option<bool>,
) -> Result<tauri::ipc::Response, String> {
    let seed_u = parse_seed(&seed)?;
    let mc_id = cubiomes::parse_mc(&mc)?;
    let want_shade = want_shade.unwrap_or(true);
    tokio::task::spawn_blocking(move || {
        let s = scale.max(1);
        let half = size / 2;
        let bx = center_x.div_euclid(s) - half;
        let bz = center_z.div_euclid(s) - half;
        // 采样高度：主世界取「世界顶端」（方块 y=320），这样拿到的才是地表群系。
        // 1.18+ 的主世界群系是 3D 的，按 y=0 采样等于在地下取：地表在 0 之上的
        // 海洋（海面 63、海底更低）和山体会被洞穴群系（繁茂洞穴 / 滴水石洞穴 /
        // 深暗之域）盖住，看着像陆地。实用上实测约 24% 的采样点会变成洞穴群系。
        // 注意 cubiomes 的 y 单位：scale==1 是方块坐标，其它 scale 是 1:4（方块/4）。
        // y=320 远高于所有地形，因此各缩放级别取到的地表群系一致，缩放不会变样。
        // 下界 / 末地的群系分布与 y 无关（已实测），沿用 0。
        let y = if dim == 0 {
            if s == 1 { 320 } else { 80 }
        } else {
            0
        };

        // 高程采样：每个样本固定跨 4 方块的整数倍，且在缩小时按 2 的倍数降采样
        // （depth 噪声本身很平滑，降采样几乎看不出差别，但能让瓦片快 4 倍）。
        let (k, step4) = if s >= 4 { (1, (s / 4).max(1)) } else { (4, 1) };
        let mult = if s >= 4 { 2 } else { 1 };
        let kstep = k * mult;
        if size % kstep != 0 {
            return Err("瓦片尺寸与采样步长不匹配".into());
        }
        let hs = size / kstep;
        let hstep4 = step4 * mult;
        let hx4 = center_x.div_euclid(4) - (hs * hstep4) / 2;
        let hz4 = center_z.div_euclid(4) - (hs * hstep4) / 2;

        let data = cubiomes::gen_biome_map(
            seed_u,
            mc_id,
            dim,
            s,
            bx,
            y,
            bz,
            size,
            size,
            large_biomes.unwrap_or(false),
            hx4,
            hz4,
            hstep4,
            hs,
            kstep,
            want_shade,
        )?;

        let shade_size = if data.shade.is_empty() { 0 } else { (hs - 2) as u32 };
        let mut buf = Vec::with_capacity(TILE_HEADER + data.biomes.len() + data.shade.len());
        buf.extend_from_slice(&(size as u32).to_le_bytes());
        buf.extend_from_slice(&(size as u32).to_le_bytes());
        buf.extend_from_slice(&shade_size.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&data.biomes);
        buf.extend_from_slice(&data.shade);
        Ok(tauri::ipc::Response::new(buf))
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---------------------------------------------------------------------------
// 从实例存档读取世界种子（level.dat）
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize, Default)]
struct LevelData {
    #[serde(default, rename = "RandomSeed")]
    random_seed: Option<i64>,
    #[serde(default, rename = "WorldGenSettings")]
    world_gen_settings: Option<WorldGenSettings>,
    /// 存档自己记的版本：`Data.Version.Name`，如 "26.2"（旧存档是 "1.12.2" 这种）
    #[serde(default, rename = "Version")]
    version: Option<LevelVersion>,
}

#[derive(serde::Deserialize, Default)]
struct LevelVersion {
    #[serde(default, rename = "Name")]
    name: Option<String>,
}

#[derive(serde::Deserialize, Default)]
struct WorldGenSettings {
    #[serde(default)]
    seed: Option<i64>,
}

/// 26.1 起 `WorldGenSettings` 从 `level.dat` 移到了存档里的
/// `data/minecraft/world_gen_settings.dat`。实测 26.2（DataVersion 4903）的结构是：
/// `{ DataVersion: int, data: { seed: long, generate_structures: byte, dimensions: {...}, bonus_chest: byte } }`
/// —— 种子在 **根下面的 `data` 复合标签**里，不在顶层。为了兼容不同快照，两种形状都认。
#[derive(serde::Deserialize, Default)]
struct WorldGenSettingsFile {
    #[serde(default)]
    seed: Option<i64>,
    #[serde(default, rename = "data")]
    data: Option<WorldGenSettingsInner>,
}

#[derive(serde::Deserialize, Default)]
struct WorldGenSettingsInner {
    #[serde(default)]
    seed: Option<i64>,
}

#[derive(serde::Deserialize, Default)]
struct LevelDat {
    #[serde(default, rename = "Data")]
    data: Option<LevelData>,
}

/// 读取并（按需）解压一个 NBT 文件：`level.dat` 可能是 gzip，也可能已经是裸 NBT。
fn read_nbt(path: &std::path::Path) -> Result<Vec<u8>, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("读取 {} 失败: {e}", path.display()))?;
    if !bytes.starts_with(&[0x1f, 0x8b]) {
        return Ok(bytes);
    }
    use flate2::read::GzDecoder;
    use std::io::Read;
    let mut out = Vec::new();
    GzDecoder::new(&bytes[..])
        .read_to_end(&mut out)
        .map_err(|e| format!("解压 {} 失败: {e}", path.display()))?;
    Ok(out)
}

/// 存档里能读到的信息：世界种子 + 存档版本。版本供导入时自动切 MC 版本用。
#[derive(serde::Serialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WorldInfo {
    /// 种子的十进制字符串（64 位种子走 JSON number 会被舍入；随机种子世界为 None）
    pub seed: Option<String>,
    /// `Data.Version.Name`，如 "26.2"；老存档或部分快照可能没有
    pub version: Option<String>,
}

/// 读取 `saves/<world>/level.dat`，给出世界种子与存档版本。
/// 未显式设置种子的世界（随机种子）`seed` 为 None。
#[tauri::command]
pub fn toolbox_read_world_info(
    state: tauri::State<'_, crate::state::AppState>,
    instance_id: String,
    world: String,
) -> Result<WorldInfo, String> {
    if !crate::util::is_safe_filename(&world) {
        return Err("非法存档名".into());
    }
    let dir = state
        .instances_dir()
        .join(&instance_id)
        .join("saves")
        .join(&world);
    read_world_info(&dir)
}

/// 从一个存档目录（`saves/<world>`）里读出世界种子与版本。
fn read_world_info(world_dir: &std::path::Path) -> Result<WorldInfo, String> {
    let level = world_dir.join("level.dat");
    if !level.is_file() {
        return Err("未找到 level.dat".into());
    }
    let root: LevelDat =
        fastnbt::from_bytes(&read_nbt(&level)?).map_err(|e| format!("解析 level.dat 失败: {e}"))?;
    let data = root.data.unwrap_or_default();
    let version = data.version.and_then(|v| v.name).filter(|s| !s.is_empty());
    if let Some(seed) = data.world_gen_settings.and_then(|w| w.seed).or(data.random_seed) {
        return Ok(WorldInfo { seed: Some(seed.to_string()), version });
    }

    // 1.21.11 及更早：种子在 level.dat 里，上面已经取到；26.1 起改存到
    // `data/minecraft/world_gen_settings.dat`（`data.seed`，兼容顶层 `seed`）。
    let settings = world_dir
        .join("data")
        .join("minecraft")
        .join("world_gen_settings.dat");
    if settings.is_file() {
        let bytes = read_nbt(&settings)?;
        let f: WorldGenSettingsFile = fastnbt::from_bytes(&bytes)
            .map_err(|e| format!("解析 world_gen_settings.dat 失败: {e}"))?;
        let seed = f.seed.or_else(|| f.data.and_then(|d| d.seed)).map(|s| s.to_string());
        return Ok(WorldInfo { seed, version });
    }
    Ok(WorldInfo { seed: None, version })
}

#[cfg(test)]
mod tests {
    use super::*;

    // —— 手写 NBT 字节，避免测试依赖序列化器 ——

    fn named(tag: u8, name: &str, payload: &[u8]) -> Vec<u8> {
        let mut v = vec![tag];
        v.extend_from_slice(&(name.len() as u16).to_be_bytes());
        v.extend_from_slice(name.as_bytes());
        v.extend_from_slice(payload);
        v
    }

    fn compound(children: &[Vec<u8>]) -> Vec<u8> {
        let mut v = Vec::new();
        for c in children {
            v.extend_from_slice(c);
        }
        v.push(0); // TAG_End
        v
    }

    /// 根 compound（名字为空）。
    fn root(children: &[Vec<u8>]) -> Vec<u8> {
        named(0x0a, "", &compound(children))
    }

    fn long_field(name: &str, v: i64) -> Vec<u8> {
        named(0x04, name, &v.to_be_bytes())
    }

    /// TAG_String：长度为 u16 前缀的 UTF-8。
    fn string_field(name: &str, v: &str) -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(&(v.len() as u16).to_be_bytes());
        p.extend_from_slice(v.as_bytes());
        named(0x08, name, &p)
    }

    fn version_field(name: &str) -> Vec<u8> {
        named(0x0a, "Version", &compound(&[string_field("Name", name)]))
    }

    fn gzip(data: &[u8]) -> Vec<u8> {
        use flate2::write::GzEncoder;
        use std::io::Write;
        let mut e = GzEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    fn tmp_world(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("qookix-seed-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 1.21.11 及更早：种子在 level.dat 的 `Data.WorldGenSettings.seed`（文件是 gzip）。
    #[test]
    fn reads_seed_from_level_dat() {
        let dir = tmp_world("legacy");
        let wgs = named(0x0a, "WorldGenSettings", &compound(&[long_field("seed", 1234)]));
        let data = named(0x0a, "Data", &compound(&[wgs, version_field("1.21.11")]));
        std::fs::write(dir.join("level.dat"), gzip(&root(&[data]))).unwrap();

        let info = read_world_info(&dir).unwrap();
        assert_eq!(info.seed.as_deref(), Some("1234"));
        assert_eq!(info.version.as_deref(), Some("1.21.11"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 26.1 起：level.dat 里不再有种子，改到
    /// `data/minecraft/world_gen_settings.dat`，种子在根下面的 `data` 复合标签里
    /// （实测 26.2：`{ DataVersion: 4903, data: { seed, generate_structures, dimensions, bonus_chest } }`）。
    #[test]
    fn reads_seed_from_world_gen_settings_dat() {
        let dir = tmp_world("new-gen-settings");
        let ver = version_field("26.2");
        let data = named(
            0x0a,
            "Data",
            &compound(&[named(0x01, "singleplayer_uuid", &[0]), ver]),
        );
        std::fs::write(dir.join("level.dat"), gzip(&root(&[data]))).unwrap();

        let sub = dir.join("data").join("minecraft");
        std::fs::create_dir_all(&sub).unwrap();
        let inner = named(
            0x0a,
            "data",
            &compound(&[
                long_field("seed", -9876543210),
                named(0x01, "generate_structures", &[1]),
                named(0x01, "bonus_chest", &[0]),
            ]),
        );
        let f = root(&[named(0x03, "DataVersion", &4903i32.to_be_bytes()), inner]);
        std::fs::write(sub.join("world_gen_settings.dat"), gzip(&f)).unwrap();

        let info = read_world_info(&dir).unwrap();
        assert_eq!(info.seed.as_deref(), Some("-9876543210"));
        assert_eq!(info.version.as_deref(), Some("26.2"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 兼容快照里出现过的另一种形状：`seed` 直接放在文件顶层。
    #[test]
    fn reads_seed_from_world_gen_settings_dat_top_level() {
        let dir = tmp_world("new-gen-settings-top");
        let data = named(0x0a, "Data", &compound(&[named(0x01, "singleplayer_uuid", &[0])]));
        std::fs::write(dir.join("level.dat"), root(&[data])).unwrap();

        let sub = dir.join("data").join("minecraft");
        std::fs::create_dir_all(&sub).unwrap();
        let f = root(&[long_field("seed", 4242)]);
        std::fs::write(sub.join("world_gen_settings.dat"), gzip(&f)).unwrap();

        assert_eq!(read_world_info(&dir).unwrap().seed.as_deref(), Some("4242"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 随机种子世界：level.dat 里没有种子，也没有新格式的 world_gen_settings.dat。
    #[test]
    fn random_seed_world_returns_none() {
        let dir = tmp_world("random");
        let data = named(0x0a, "Data", &compound(&[named(0x01, "hardcore", &[0])]));
        std::fs::write(dir.join("level.dat"), root(&[data])).unwrap();

        assert_eq!(read_world_info(&dir).unwrap().seed, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 非存档目录：没有 level.dat 时给出明确错误。
    #[test]
    fn missing_level_dat_errors() {
        let dir = tmp_world("no-level");
        let err = read_world_info(&dir).unwrap_err();
        assert!(err.contains("level.dat"), "unexpected error: {err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 种子用十进制字符串在各命令间传递：超出 JS 安全整数范围（2^53）的 64 位种子
    /// 也必须原样还原（走 JSON number 会被舍入成另一个世界）。
    #[test]
    fn parse_seed_keeps_full_i64_precision() {
        assert_eq!(parse_seed("140201723108850").unwrap(), 140201723108850);
        assert_eq!(
            parse_seed(" -5432548791662231456 ").unwrap(),
            (-5432548791662231456i64) as u64
        );
        assert_eq!(parse_seed("+7").unwrap(), 7);
        assert_eq!(parse_seed("9223372036854775807").unwrap(), i64::MAX as u64);
        assert_eq!(parse_seed("-9223372036854775808").unwrap(), i64::MIN as u64);
        // 越界 / 非整数都要报错，而不是悄悄取一个别的世界
        assert!(parse_seed("9223372036854775808").is_err());
        assert!(parse_seed("1.5").is_err());
        assert!(parse_seed("abc").is_err());
        assert!(parse_seed("").is_err());
    }
}
