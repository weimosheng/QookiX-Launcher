// cubiomes FFI 绑定 + 安全封装 + 枚举映射。
// C 胶水层见 cubiomes_bridge.c，编译见 build.rs。
use std::os::raw::c_int;

/// Generator 标志位（见 vendor/cubiomes/generator.h）。
pub const LARGE_BIOMES: u32 = 0x1;

extern "C" {
    fn bridge_query_biome(
        seed: u64,
        mc: c_int,
        dim: c_int,
        scale: c_int,
        x: c_int,
        y: c_int,
        z: c_int,
        flags: u32,
    ) -> c_int;
    fn bridge_biome_depth(id: c_int, depth: *mut f32, scale: *mut f32) -> c_int;
    fn bridge_surface_height(seed: u64, mc: c_int, dim: c_int, x: c_int, z: c_int) -> c_int;
    fn bridge_get_spawn(
        seed: u64,
        mc: c_int,
        flags: u32,
        px: *mut c_int,
        pz: *mut c_int,
    ) -> c_int;
    fn bridge_get_structure_pos(
        stype: c_int,
        seed: u64,
        mc: c_int,
        reg_x: c_int,
        reg_z: c_int,
        px: *mut c_int,
        pz: *mut c_int,
    ) -> c_int;
    fn bridge_get_structure_info(
        stype: c_int,
        mc: c_int,
        region_size: *mut c_int,
        chunk_range: *mut c_int,
    ) -> c_int;
    fn bridge_is_slime_chunk(seed: u64, chunk_x: c_int, chunk_z: c_int) -> c_int;
    fn bridge_gen_biome_map(
        seed: u64,
        mc: c_int,
        dim: c_int,
        scale: c_int,
        x: c_int,
        y: c_int,
        z: c_int,
        sx: c_int,
        sz: c_int,
        flags: u32,
        out: *mut u8,
        out_shade: *mut u8,
        hs: c_int,
        hstep4: c_int,
        hx4: c_int,
        hz4: c_int,
        kstep: c_int,
    ) -> c_int;
}

pub fn query_biome(
    seed: u64,
    mc: i32,
    dim: i32,
    scale: i32,
    x: i32,
    y: i32,
    z: i32,
    large_biomes: bool,
) -> i32 {
    let flags = if large_biomes { LARGE_BIOMES } else { 0 };
    unsafe { bridge_query_biome(seed, mc, dim, scale, x, y, z, flags) as i32 }
}

/// 生物群系的基准高度（depth）与起伏（scale），用于前端绘制地貌阴影。
pub fn biome_depth(id: i32) -> (f32, f32) {
    let mut d: f32 = 0.0;
    let mut s: f32 = 0.0;
    unsafe {
        bridge_biome_depth(id, &mut d, &mut s);
    }
    (d, s)
}

/// 估算 (x,z) 处的地表高度（方块），用于生成 /tp 指令的 y。
pub fn surface_height(seed: u64, mc: i32, dim: i32, x: i32, z: i32) -> i32 {
    unsafe { bridge_surface_height(seed, mc, dim, x, z) as i32 }
}

/// 世界出生点（主世界）。计算较慢，调用方应放在阻塞线程里。
pub fn get_spawn(seed: u64, mc: i32, large_biomes: bool) -> Option<(i32, i32)> {
    let flags = if large_biomes { LARGE_BIOMES } else { 0 };
    let mut px: c_int = 0;
    let mut pz: c_int = 0;
    let ok = unsafe { bridge_get_spawn(seed, mc, flags, &mut px, &mut pz) };
    if ok != 0 {
        Some((px, pz))
    } else {
        None
    }
}

pub fn get_structure_pos(stype: i32, seed: u64, mc: i32, reg_x: i32, reg_z: i32) -> Option<(i32, i32)> {
    let mut px: c_int = 0;
    let mut pz: c_int = 0;
    let ok = unsafe { bridge_get_structure_pos(stype, seed, mc, reg_x, reg_z, &mut px, &mut pz) };
    if ok != 0 {
        Some((px, pz))
    } else {
        None
    }
}

pub fn get_structure_info(stype: i32, mc: i32) -> Option<(i32, i32)> {
    let mut rs: c_int = 0;
    let mut cr: c_int = 0;
    let ok = unsafe { bridge_get_structure_info(stype, mc, &mut rs, &mut cr) };
    if ok != 0 {
        Some((rs, cr))
    } else {
        None
    }
}

pub fn is_slime_chunk(seed: u64, chunk_x: i32, chunk_z: i32) -> bool {
    unsafe { bridge_is_slime_chunk(seed, chunk_x, chunk_z) != 0 }
}

/// 生物群系地图 + 山体阴影亮度（均为紧凑字节，便于直接以原始字节回传前端）。
pub struct BiomeMapData {
    /// sx*sz 个群系 id（1 字节；255 表示无群系）
    pub biomes: Vec<u8>,
    /// (hs-2)^2 个亮度字节（0..255）；未请求阴影时为空
    pub shade: Vec<u8>,
}

/// 批量生成 2D 生物群系地图（sy=1），并可选地附带山体阴影。
/// biomes 布局为 out[z*sx + x]；阴影基于独立的 1:4 世界栅格，
/// 因此地貌不会随显示缩放级别改变（只是采样更稀疏）。
#[allow(clippy::too_many_arguments)]
pub fn gen_biome_map(
    seed: u64,
    mc: i32,
    dim: i32,
    scale: i32,
    x: i32,
    y: i32,
    z: i32,
    sx: i32,
    sz: i32,
    large_biomes: bool,
    hx4: i32,
    hz4: i32,
    hstep4: i32,
    hs: i32,
    kstep: i32,
    want_shade: bool,
) -> Result<BiomeMapData, String> {
    if sx <= 0 || sz <= 0 {
        return Err("地图尺寸必须为正".into());
    }
    let mut out = vec![0u8; (sx as usize) * (sz as usize)];
    let slen = if want_shade && hs > 2 {
        ((hs - 2) as usize) * ((hs - 2) as usize)
    } else {
        0
    };
    let mut shade = vec![0u8; slen];
    let flags = if large_biomes { LARGE_BIOMES } else { 0 };
    let ret = unsafe {
        bridge_gen_biome_map(
            seed,
            mc,
            dim,
            scale,
            x,
            y,
            z,
            sx,
            sz,
            flags,
            out.as_mut_ptr(),
            if slen > 0 { shade.as_mut_ptr() } else { std::ptr::null_mut() },
            hs,
            hstep4,
            hx4,
            hz4,
            kstep,
        )
    };
    if ret != 0 {
        return Err(format!("gen_biome_map 失败: {ret}"));
    }
    Ok(BiomeMapData {
        biomes: out,
        shade,
    })
}

// MC 版本字符串 → cubiomes MCVersion 枚举值（见 vendor/cubiomes/biomes.h）。
// 接受 1.7 / 1.7.10 / 1.16.1 / b1.7 / 26.2 等写法；只有"大版本"时取该大版本的最新小版本。
pub fn parse_mc(ver: &str) -> Result<i32, String> {
    let v = ver.trim().to_lowercase();
    let mut it = v.split('.');
    let major = it.next().unwrap_or("");
    let minor = it.next().unwrap_or("");
    let patch = it.next().unwrap_or("");

    // 2025-12 起 Mojang 改用「年.版本.补丁」编号（26.1 / 26.1.2 / 26.2 / ...）。
    // cubiomes 建到的最后一个版本是 1.21.4（Winter Drop，枚举值 28），此后没有
    // 再建模新版本，所以 26.x 及以后统一按 28 处理：地表群系与结构模型一致。
    // 已知差异：26.2 新增了地下「硫确洞穴」群系，本工具无法生成。
    if let Ok(year) = major.parse::<u32>() {
        if year >= 26 {
            return Ok(28);
        }
    }

    let m = match (major, minor) {
        ("1", "21") => match patch {
            "" => 28,
            "0" | "1" => 26,
            "2" | "3" => 27,
            // 1.21.4（Winter Drop）及之后的 1.21.5 ~ 1.21.11 都沿用该模型
            _ => 28,
        },
        ("1", "20") => 25,
        ("1", "19") => match patch {
            "0" | "1" | "2" => 23,
            _ => 24,
        },
        ("1", "18") => 22,
        ("1", "17") => 21,
        ("1", "16") => {
            if patch == "1" {
                19
            } else {
                20
            }
        }
        ("1", "15") => 18,
        ("1", "14") => 17,
        ("1", "13") => 16,
        ("1", "12") => 15,
        ("1", "11") => 14,
        ("1", "10") => 13,
        ("1", "9") => 12,
        ("1", "8") => 11,
        ("1", "7") => 10,
        ("1", "6") => 9,
        ("1", "5") => 8,
        ("1", "4") => 7,
        ("1", "3") => 6,
        ("1", "2") => 5,
        ("1", "1") => 4,
        ("1", "0") => 3,
        ("b1", "8") => 2,
        ("b1", "7") => 1,
        _ => return Err(format!("不支持的 MC 版本: {ver}")),
    };
    Ok(m)
}

// —— 群系图采样高度的回归测试 ——
// 主世界的 2D 群系图必须按「世界顶端」（方块 y=320）采样：1.18+ 的主世界群系是 3D 的，
// 按 y=0 采样等于在地下取群系，海洋 / 山体会被洞穴群系盖住（看着像陆地）。
#[cfg(test)]
mod tests {
    use super::*;

    /// 滴水石洞穴 / 繁茂洞穴 / 深暗之域
    const CAVE_IDS: [i32; 3] = [174, 175, 183];
    /// 海洋（含各变体）/ 河流
    const WATER_IDS: [i32; 12] = [0, 7, 10, 11, 24, 44, 45, 46, 47, 48, 49, 50];

    /// 小范围扫一遍，返回（洞穴数, 水域数, 总数）
    fn scan(seed: u64, y4: i32) -> (usize, usize, usize) {
        let (mut cave, mut water, mut total) = (0, 0, 0);
        for iz in -40..40 {
            for ix in -40..40 {
                let id = query_biome(seed, 28, 0, 4, ix * 37, y4, iz * 37, false);
                total += 1;
                if CAVE_IDS.contains(&id) {
                    cave += 1;
                }
                if WATER_IDS.contains(&id) {
                    water += 1;
                }
            }
        }
        (cave, water, total)
    }

    #[test]
    fn surface_map_has_no_cave_biomes() {
        for seed in [0u64, 12345, 987654321, 42] {
            // y=80（1:4 单位）= 方块 320，即 cubiomes 测试里用的「世界顶端」
            let (cave, water, total) = scan(seed, 80);
            assert_eq!(cave, 0, "seed {seed}: 地表图不该出现洞穴群系（{cave}/{total}）");
            assert!(water * 10 > total, "seed {seed}: 应该能取到海洋/河流（{water}/{total}）");
        }
    }

    /// 反过来确认上面那条测试是有意义的：按 y=0（地下）采样确实会命中洞穴群系。
    #[test]
    fn underground_sampling_hits_cave_biomes() {
        let (cave, _, total) = scan(12345, 0);
        assert!(cave > 0, "y=0 本应落在地下并命中洞穴群系（{cave}/{total}）");
    }

    /// 地表高度估算：应落在合理范围，且地图上确实有高有低
    #[test]
    fn surface_height_is_plausible() {
        let mut hs = Vec::new();
        for i in 0..40 {
            for j in 0..40 {
                hs.push(surface_height(12345, 28, 0, (i - 20) * 200, (j - 20) * 200));
            }
        }
        assert!(hs.iter().all(|h| (10..=256).contains(h)), "高度超出合理区间: {hs:?}");
        assert!(hs.iter().any(|h| *h < 62), "应该有低于海平面的列");
        assert!(hs.iter().any(|h| *h > 70), "应该有高于海平面的列");
    }
}

// 结构类型字符串 → cubiomes StructureType 枚举值（见 vendor/cubiomes/finders.h）。
pub fn parse_structure_type(name: &str) -> Result<i32, String> {
    let s = name.trim().to_lowercase();
    let m = match s.as_str() {
        "village" => 5,
        "desert_pyramid" => 1,
        "jungle_temple" => 2,
        "swamp_hut" => 3,
        "igloo" => 4,
        "ocean_ruin" => 6,
        "shipwreck" => 7,
        "monument" => 8,
        "mansion" => 9,
        "outpost" => 10,
        "ruined_portal" => 11,
        "ruined_portal_n" => 12,
        "ancient_city" => 13,
        "treasure" => 14,
        "mineshaft" => 15,
        "desert_well" => 16,
        "geode" => 17,
        "fortress" => 18,
        "bastion" => 19,
        "end_city" => 20,
        "end_gateway" => 21,
        "end_island" => 22,
        "trail_ruins" => 23,
        "trial_chambers" => 24,
        _ => return Err(format!("未知结构类型: {name}")),
    };
    Ok(m)
}

pub fn biome_name(id: i32) -> String {
    match biome_name_opt(id) {
        Some(n) => n.to_string(),
        None => format!("unknown({id})"),
    }
}

fn biome_name_opt(id: i32) -> Option<&'static str> {
    let n = match id {
        -1 => "none",
        0 => "ocean",
        1 => "plains",
        2 => "desert",
        3 => "mountains",
        4 => "forest",
        5 => "taiga",
        6 => "swamp",
        7 => "river",
        8 => "nether_wastes",
        9 => "the_end",
        10 => "frozen_ocean",
        11 => "frozen_river",
        12 => "snowy_tundra",
        13 => "snowy_mountains",
        14 => "mushroom_fields",
        15 => "mushroom_field_shore",
        16 => "beach",
        17 => "desert_hills",
        18 => "wooded_hills",
        19 => "taiga_hills",
        20 => "mountain_edge",
        21 => "jungle",
        22 => "jungle_hills",
        23 => "jungle_edge",
        24 => "deep_ocean",
        25 => "stone_shore",
        26 => "snowy_beach",
        27 => "birch_forest",
        28 => "birch_forest_hills",
        29 => "dark_forest",
        30 => "snowy_taiga",
        31 => "snowy_taiga_hills",
        32 => "giant_tree_taiga",
        33 => "giant_tree_taiga_hills",
        34 => "wooded_mountains",
        35 => "savanna",
        36 => "savanna_plateau",
        37 => "badlands",
        38 => "wooded_badlands_plateau",
        39 => "badlands_plateau",
        40 => "small_end_islands",
        41 => "end_midlands",
        42 => "end_highlands",
        43 => "end_barrens",
        44 => "warm_ocean",
        45 => "lukewarm_ocean",
        46 => "cold_ocean",
        47 => "deep_warm_ocean",
        48 => "deep_lukewarm_ocean",
        49 => "deep_cold_ocean",
        50 => "deep_frozen_ocean",
        127 => "the_void",
        129 => "sunflower_plains",
        130 => "desert_lakes",
        131 => "gravelly_mountains",
        132 => "flower_forest",
        133 => "taiga_mountains",
        134 => "swamp_hills",
        140 => "ice_spikes",
        149 => "modified_jungle",
        151 => "modified_jungle_edge",
        155 => "tall_birch_forest",
        156 => "tall_birch_hills",
        157 => "dark_forest_hills",
        158 => "snowy_taiga_mountains",
        160 => "giant_spruce_taiga",
        161 => "giant_spruce_taiga_hills",
        162 => "modified_gravelly_mountains",
        163 => "shattered_savanna",
        164 => "shattered_savanna_plateau",
        165 => "eroded_badlands",
        166 => "modified_wooded_badlands_plateau",
        167 => "modified_badlands_plateau",
        168 => "bamboo_jungle",
        169 => "bamboo_jungle_hills",
        170 => "soul_sand_valley",
        171 => "crimson_forest",
        172 => "warped_forest",
        173 => "basalt_deltas",
        174 => "dripstone_caves",
        175 => "lush_caves",
        177 => "meadow",
        178 => "grove",
        179 => "snowy_slopes",
        180 => "jagged_peaks",
        181 => "frozen_peaks",
        182 => "stony_peaks",
        183 => "deep_dark",
        184 => "mangrove_swamp",
        185 => "cherry_grove",
        186 => "pale_garden",
        _ => return None,
    };
    Some(n)
}

/// 全部已知生物群系（id/名称/基准高度），供前端绘图例与地貌阴影使用。
pub fn biome_table() -> Vec<(i32, &'static str, f32, f32)> {
    let ids: Vec<i32> = (0..=50)
        .chain(std::iter::once(127))
        .chain(129..=175)
        .chain(177..=186)
        .collect();
    ids.into_iter()
        .filter_map(|id| {
            let name = biome_name_opt(id)?;
            let (d, s) = biome_depth(id);
            Some((id, name, d, s))
        })
        .collect()
}
