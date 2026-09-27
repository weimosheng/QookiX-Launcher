//! 区块地图：把存档里**已生成**的区块渲染成平面图。
//!
//! 与种子地图的区别：种子地图用 cubiomes 按种子推算地形，这里读的是存档里
//! 真实的区块数据——玩家实际去过、生成过的地方才有像素，没生成的区块留空。
//!
//! 四个关键点：
//! 1. **一格一格投票**，不是"取左上角那个方块"。一个像素可能覆盖几十个方块
//!    （城市、建筑、混交林），只取一个方块会让整张图像随机色块；这里对该像素
//!    覆盖的方块做多数表决，取出现最多的那种。
//! 2. **点名解码**。渲染先算采样点，只解压"采样点落在里面"的区块。
//!    缩得很小时，一个像素覆盖几十格，没必要把整个存档都解出来。
//! 3. **并行解码**。区块解压是大头（一个区块约 2ms），按 region 文件分给
//!    多个线程跑，整张全存档图从 37s 降到几秒。
//! 4. 地表列按 (维度, 区块) 缓存成 u16 方块 id（256×2 字节一个区块），
//!    平移重画不用重新解压解析。

use crate::nbt::{level, region};
use fastnbt::Value;
use serde_json::{json, Value as Json};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{LazyLock, Mutex};

/// 单次渲染的最大输出边长（像素）
pub const MAX_SIDE: i32 = 512;
/// 调色板上限：0 号固定是"未生成"
const PALETTE_MAX: usize = 255;
/// 未生成 / 无数据
const UNKNOWN: u8 = 0;
/// 调色板塞满后的兜底色
const FALLBACK: [u8; 3] = [128, 128, 134];
/// 一格最多记几个候选方块（4 个槽足够抓住占比 >25% 的那一种）
const SLOTS: usize = 4;

/// 要渲染的范围：方块坐标左闭右开，`step` = 每个像素代表多少方块
#[derive(Clone, Copy)]
pub struct Rect {
    pub x0: i32,
    pub z0: i32,
    pub x1: i32,
    pub z1: i32,
    pub step: i32,
}

/// 渲染结果
pub struct MapImage {
    pub palette: Vec<[u8; 3]>,
    /// 每个像素一个调色板下标，行优先
    pub data: Vec<u8>,
    /// 左上角对应的方块坐标
    pub origin_x: i32,
    pub origin_z: i32,
    /// 每个像素代表多少方块
    pub step: i32,
    pub width: i32,
    pub height: i32,
    /// 真正画进图里的区块数
    pub rendered: usize,
    /// 视野里出现最多的方块（方块名、颜色、像素数），前端拿去做图例
    pub legend: Vec<(String, [u8; 3], usize)>,
}

// --------------------------------------------------------- 全局方块 id 表 ----

/// 方块名 ↔ u16 id ↔ 颜色。缓存里存 id 而不是字符串，
/// 一个区块 256 列只占 512 字节（存字符串要好几 KB）。
struct IdTable {
    names: Vec<String>,
    index: HashMap<String, u16>,
    colors: Vec<[u8; 3]>,
}

static IDS: LazyLock<Mutex<IdTable>> = LazyLock::new(|| Mutex::new(IdTable::default()));

impl Default for IdTable {
    fn default() -> Self {
        Self {
            // 0 号留给"空气/未知"
            names: vec![String::new()],
            index: HashMap::new(),
            colors: vec![[0, 0, 0]],
        }
    }
}

impl IdTable {
    fn intern(&mut self, id: &str) -> u16 {
        if let Some(i) = self.index.get(id) {
            return *i;
        }
        if self.names.len() >= u16::MAX as usize {
            return 0;
        }
        let idx = self.names.len() as u16;
        self.names.push(id.to_string());
        self.index.insert(id.to_string(), idx);
        self.colors.push(block_color(id));
        idx
    }
}

/// 整批登记一个区块的地表方块（一个区块只锁一次）
fn intern_surface(names: &[String]) -> Vec<u16> {
    let Ok(mut t) = IDS.lock() else {
        return vec![0; names.len()];
    };
    names
        .iter()
        .map(|n| if n.is_empty() { 0 } else { t.intern(n) })
        .collect()
}

fn id_color(id: u16) -> [u8; 3] {
    IDS.lock()
        .ok()
        .and_then(|t| t.colors.get(id as usize).copied())
        .unwrap_or(FALLBACK)
}

fn id_name(id: u16) -> String {
    IDS.lock()
        .ok()
        .and_then(|t| t.names.get(id as usize).cloned())
        .unwrap_or_default()
}

// ------------------------------------------------------------ 区块地表缓存 ----

/// 一个区块 256 列的方块 id（0 = 空气/未知），行优先
type Surface = Vec<u16>;
/// 区块坐标
type ChunkKey = (i32, i32);
/// 缓存键：维度 + 区块坐标
type CacheKey = (String, i32, i32);
/// 待解压的区块，按 region 文件分组
type RegionGroups = std::collections::BTreeMap<ChunkKey, Vec<ChunkKey>>;

static CACHE: LazyLock<Mutex<HashMap<CacheKey, Surface>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// 缓存上限（一条 512 字节，6 万条约 30MB），超了整体清空
const CACHE_LIMIT: usize = 60_000;

/// 区块被改动后要失效，否则地图会显示改之前的地形
pub fn invalidate(dim: &str, cx: i32, cz: i32) {
    if let Ok(mut c) = CACHE.lock() {
        c.remove(&(dim.to_string(), cx, cz));
    }
}

fn cache_get(key: &CacheKey) -> Option<Surface> {
    CACHE.lock().ok()?.get(key).cloned()
}

fn cache_put(key: CacheKey, v: Surface) {
    if let Ok(mut c) = CACHE.lock() {
        if c.len() >= CACHE_LIMIT {
            c.clear();
        }
        c.insert(key, v);
    }
}

// ---------------------------------------------------------------- 存档范围 ----

/// 存档里已生成区块的范围（地图打开时用来定初始视野）。
/// 除了外框，还给一个"最密集区域"的中心——存档范围常常横跨几千格
/// （跑图跑得很远），直接按外框居中会看到一大片空白。
pub fn bounds(world_dir: &Path, dim: &str) -> Result<Json, String> {
    let chunks = region::list_chunks(world_dir, dim)?;
    if chunks.is_empty() {
        return Ok(json!({ "empty": true, "chunks": 0 }));
    }
    let mut min_cx = i32::MAX;
    let mut min_cz = i32::MAX;
    let mut max_cx = i32::MIN;
    let mut max_cz = i32::MIN;
    // 32×32 区块为一桶，找区块最多的那一桶
    let mut buckets: HashMap<(i32, i32), usize> = HashMap::new();
    for c in &chunks {
        let cx = c.get("cx").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        let cz = c.get("cz").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
        min_cx = min_cx.min(cx);
        max_cx = max_cx.max(cx);
        min_cz = min_cz.min(cz);
        max_cz = max_cz.max(cz);
        *buckets.entry((cx >> 5, cz >> 5)).or_default() += 1;
    }
    let dense = buckets
        .iter()
        .max_by_key(|(_, n)| **n)
        .map(|((bx, bz), _)| (bx * 32 + 16, bz * 32 + 16))
        .unwrap_or(((min_cx + max_cx) / 2, (min_cz + max_cz) / 2));

    Ok(json!({
        "empty": false,
        "minCx": min_cx,
        "minCz": min_cz,
        "maxCx": max_cx,
        "maxCz": max_cz,
        "chunks": chunks.len(),
        // 方块坐标：最密集区域中心
        "denseX": dense.0 * 16,
        "denseZ": dense.1 * 16,
    }))
}

// ------------------------------------------------------------------ 渲染 ----

/// 一格里的候选方块（票数从高到低不排序，出图时再扫一遍找最高票）
#[derive(Clone, Copy)]
struct Cell {
    ids: [u16; SLOTS],
    votes: [u32; SLOTS],
}

/// 投票：满了就整体减一（Misra-Gries），保证不会因为方块种类多而把大宗方块挤掉
fn vote(cell: &mut Cell, id: u16) {
    for k in 0..SLOTS {
        if cell.votes[k] > 0 && cell.ids[k] == id {
            cell.votes[k] += 1;
            return;
        }
    }
    for k in 0..SLOTS {
        if cell.votes[k] == 0 {
            cell.ids[k] = id;
            cell.votes[k] = 1;
            return;
        }
    }
    for k in 0..SLOTS {
        cell.votes[k] -= 1;
    }
}

fn winning_id(cell: &Cell) -> u16 {
    let mut best = 0usize;
    for k in 1..SLOTS {
        if cell.votes[k] > cell.votes[best] {
            best = k;
        }
    }
    if cell.votes[best] == 0 {
        0
    } else {
        cell.ids[best]
    }
}

/// 一个采样点：落在哪个区块的哪一列、投给哪个输出像素
#[derive(Clone, Copy)]
struct Sample {
    cx: i32,
    cz: i32,
    /// 区块内列号 `lz * 16 + lx`
    col: u16,
    /// 输出像素下标
    cell: u32,
}

/// 每个像素每条轴取几个采样点。
/// 一格的边长 ≤4 时全采（结果就是精确的多数表决），更大时取样估计。
fn samples_per_axis(step: i32) -> i32 {
    match step {
        s if s <= 4 => s.max(1),
        s if s <= 16 => 4,
        _ => 2,
    }
}

/// 渲染一块范围的方块（放大时 step=1，缩小时后端降采样，避免返回超大图）。
pub fn render(
    world_dir: &Path,
    dim: &str,
    rect: Rect,
    mut progress: impl FnMut(usize, usize),
) -> Result<MapImage, String> {
    let Rect { x0, z0, x1, z1, step } = rect;
    let step = step.clamp(1, 64);
    let (x0, x1) = (x0, x1.max(x0 + 1));
    let (z0, z1) = (z0, z1.max(z0 + 1));
    let width = ((x1 - x0) + step - 1) / step;
    let height = ((z1 - z0) + step - 1) / step;
    if width > MAX_SIDE || height > MAX_SIDE {
        return Err(format!("渲染范围过大（单次上限 {MAX_SIDE}×{MAX_SIDE} 像素）"));
    }
    let cells = (width * height) as usize;

    // 1) 采样点：每格均匀取 k×k 个，算清要解哪些区块
    let k = samples_per_axis(step);
    let mut samples: Vec<Sample> = Vec::with_capacity(cells * (k * k) as usize);
    let mut by_region: RegionGroups = Default::default();
    for pz in 0..height {
        for px in 0..width {
            let (bx, bz) = (x0 + px * step, z0 + pz * step);
            let cell = (pz * width + px) as u32;
            for i in 0..k {
                for j in 0..k {
                    let wx = bx + (step * (2 * j + 1)) / (2 * k);
                    let wz = bz + (step * (2 * i + 1)) / (2 * k);
                    let (cx, cz) = (wx >> 4, wz >> 4);
                    samples.push(Sample {
                        cx,
                        cz,
                        col: (((wz & 15) * 16 + (wx & 15)) as u16),
                        cell,
                    });
                    by_region.entry((cx >> 5, cz >> 5)).or_default().push((cx, cz));
                }
            }
        }
    }
    for list in by_region.values_mut() {
        list.sort_unstable();
        list.dedup();
    }

    // 2) 并行解压（同一个 region 文件只读一次）
    let surfaces = fetch(world_dir, dim, by_region, &mut progress);
    let rendered = surfaces.len();

    // 3) 投票：每格取出现最多的方块
    let mut tally = vec![Cell { ids: [0; SLOTS], votes: [0; SLOTS] }; cells];
    for s in &samples {
        let Some(surface) = surfaces.get(&(s.cx, s.cz)) else {
            continue;
        };
        let id = surface[s.col as usize];
        if id != 0 {
            vote(&mut tally[s.cell as usize], id);
        }
    }

    // 出图：每格取票数最高的方块 → 颜色
    let mut palette: Vec<[u8; 3]> = vec![[0, 0, 0]];
    let mut color_index: HashMap<[u8; 3], u8> = HashMap::new();
    let mut data = vec![UNKNOWN; cells];
    let mut legend: HashMap<u16, usize> = HashMap::new();
    for (i, cell) in tally.iter().enumerate() {
        let id = winning_id(cell);
        if id == 0 {
            continue;
        }
        *legend.entry(id).or_default() += 1;
        let color = id_color(id);
        let idx = match color_index.get(&color) {
            Some(i) => *i,
            None => {
                let idx = if palette.len() < PALETTE_MAX {
                    palette.push(color);
                    (palette.len() - 1) as u8
                } else {
                    if palette.len() < 256 {
                        palette.resize(256, FALLBACK);
                    }
                    255
                };
                color_index.insert(color, idx);
                idx
            }
        };
        data[i] = idx;
    }

    // 图例：视野里占像素最多的方块，前 8 种
    let mut legend: Vec<(String, [u8; 3], usize)> = legend
        .into_iter()
        .map(|(id, n)| (id_name(id), id_color(id), n))
        .collect();
    legend.sort_by_key(|l| std::cmp::Reverse(l.2));
    legend.truncate(8);

    Ok(MapImage {
        palette,
        data,
        origin_x: x0,
        origin_z: z0,
        step,
        width,
        height,
        rendered,
        legend,
    })
}

/// 按 region 文件并行解压需要的区块，返回 (cx,cz) → 地表列。
/// 结果同时写进全局缓存，平移、缩放时能直接命中。
fn fetch(
    world_dir: &Path,
    dim: &str,
    groups: RegionGroups,
    progress: &mut impl FnMut(usize, usize),
) -> HashMap<ChunkKey, Surface> {
    // 把每个 region 的区块再切成小块：小视野常常只跨 1~2 个 region 文件，
    // 按文件分线程会退化成单线程，切开才能吃满多核
    let mut items: Vec<(ChunkKey, Vec<ChunkKey>)> = Vec::new();
    for (key, list) in groups {
        for part in list.chunks(64) {
            items.push((key, part.to_vec()));
        }
    }
    // 进度按"要解多少区块"报，前端才能画出连续的进度条
    let total: usize = items.iter().map(|(_, l)| l.len()).sum();
    // 上限 16：每个线程手上最多留一个 region 文件（十几 MB）
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 16);

    let mut out: HashMap<ChunkKey, Surface> = HashMap::new();
    if threads <= 1 || items.len() <= 1 {
        let mut done = 0usize;
        let mut memo = None;
        for (key, list) in items.iter() {
            done += list.len();
            progress(done, total);
            load_region(world_dir, dim, *key, list, &mut memo, &mut out);
        }
        return out;
    }

    // 均分给线程，各自攒一份结果再合并（省掉共享锁）
    let per = items.len().div_ceil(threads);
    let parts: Vec<Vec<(ChunkKey, Vec<ChunkKey>)>> = items.chunks(per).map(|c| c.to_vec()).collect();
    let sizes: Vec<usize> = parts
        .iter()
        .map(|p| p.iter().map(|(_, l)| l.len()).sum())
        .collect();
    let results: Vec<HashMap<ChunkKey, Surface>> = std::thread::scope(|s| {
        let handles: Vec<_> = parts
            .into_iter()
            .map(|part| {
                s.spawn(move || {
                    let mut local = HashMap::new();
                    let mut memo = None;
                    for (key, list) in part {
                        load_region(world_dir, dim, key, &list, &mut memo, &mut local);
                    }
                    local
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_default())
            .collect()
    });
    let mut done = 0usize;
    for (i, part) in results.into_iter().enumerate() {
        out.extend(part);
        done += sizes.get(i).copied().unwrap_or(0);
        progress(done.min(total), total);
    }
    out
}

/// 读一个 region 文件，解出里面被点名的区块。
/// `memo` 是线程内的单文件缓存：同一个 region 文件被切成多批时只读一次盘
/// （一个文件十几 MB，重复读的代价比解压还高）。
fn load_region(
    world_dir: &Path,
    dim: &str,
    (rx, rz): ChunkKey,
    list: &[ChunkKey],
    memo: &mut Option<(ChunkKey, Vec<u8>)>,
    out: &mut HashMap<ChunkKey, Surface>,
) {
    // 先看缓存：全命中就一个字节都不用读盘（拖回已经看过的地方走的就是这条路）
    let mut todo: Vec<ChunkKey> = Vec::new();
    for (cx, cz) in list {
        if out.contains_key(&(*cx, *cz)) {
            continue;
        }
        match cache_get(&(dim.to_string(), *cx, *cz)) {
            Some(s) => {
                out.insert((*cx, *cz), s);
            }
            None => todo.push((*cx, *cz)),
        }
    }
    if todo.is_empty() {
        return;
    }

    let key = (rx, rz);
    if memo.as_ref().map(|(k, _)| *k) != Some(key) {
        let path = region::region_dir(world_dir, dim).join(format!("r.{rx}.{rz}.mca"));
        // 文件不存在（该 region 还没生成）时也记下来，避免同一批反复尝试
        *memo = Some((key, std::fs::read(&path).unwrap_or_default()));
    }
    let Some((_, bytes)) = memo.as_ref() else {
        return;
    };
    if bytes.is_empty() {
        return;
    }

    for (cx, cz) in todo {
        let Ok(chunk) = region::decode_chunk(bytes, cx, cz) else {
            continue; // 该区块尚未生成
        };
        let Some(names) = surface_ids(&chunk) else {
            continue;
        };
        let ids = intern_surface(&names);
        cache_put((dim.to_string(), cx, cz), ids.clone());
        out.insert((cx, cz), ids);
    }
}

/// 一个区块 256 列的"地表方块名"：从上往下第一个非空气方块
fn surface_ids(chunk: &Value) -> Option<Vec<String>> {
    let sections = level::as_list(level::get(chunk, "sections")?)?;
    let mut out = vec![String::new(); 256];
    let mut filled = 0usize;

    // 高处的 section 先看（地表基本在最上面那几层）
    // 注意：sections 只覆盖已生成的层，没生成的层不在列表里
    let mut order: Vec<&Value> = sections.iter().collect();
    order.sort_by_key(|s| level::get_int(s, "Y").unwrap_or(0));

    for section in order.iter().rev() {
        let Some((names, idx)) = section_blocks(section) else {
            continue;
        };
        if names.len() == 1 && is_air(&names[0]) {
            continue;
        }
        for z in 0..16usize {
            for x in 0..16usize {
                let col = z * 16 + x;
                if !out[col].is_empty() {
                    continue;
                }
                for y in (0..16usize).rev() {
                    let i = idx[(y << 8) | (z << 4) | x] as usize;
                    let Some(name) = names.get(i) else { continue };
                    if !is_air(name) {
                        out[col] = name.clone();
                        filled += 1;
                        break;
                    }
                }
            }
        }
        if filled == 256 {
            break;
        }
    }
    Some(out)
}

/// 一个 section 的调色板 + 4096 个方块的下标（顺序 `y*256 + z*16 + x`）
fn section_blocks(section: &Value) -> Option<(Vec<String>, Vec<u16>)> {
    // 1.18+ 叫 block_states/palette/data，1.13~1.17 叫 BlockStates/Palette/Data
    let states =
        level::get(section, "block_states").or_else(|| level::get(section, "BlockStates"))?;
    let palette = level::get(states, "palette").or_else(|| level::get(states, "Palette"))?;
    let names: Vec<String> = level::as_list(palette)?
        .iter()
        .filter_map(|e| level::get_string(e, "Name"))
        .collect();
    if names.is_empty() {
        return None;
    }
    let mut idx = vec![0u16; 4096];
    if names.len() > 1 {
        let data = level::get(states, "data").or_else(|| level::get(states, "Data"))?;
        let Value::LongArray(longs) = data else {
            return None;
        };
        let bits = bits_for(names.len());
        let per = 64 / bits;
        let mask = (1u64 << bits) - 1;
        for (i, slot) in idx.iter_mut().enumerate() {
            // 数据比理论长度短（存档被截断）时按 0 处理，不整块丢掉
            let Some(v) = longs.get(i / per) else { break };
            *slot = (((*v as u64) >> ((i % per) * bits)) & mask) as u16;
        }
    }
    Some((names, idx))
}

/// 每项占多少 bit（跟游戏一致：至少 4）
fn bits_for(n: usize) -> usize {
    let mut bits = 0;
    while (1usize << bits) < n {
        bits += 1;
    }
    bits.max(4)
}

fn is_air(id: &str) -> bool {
    matches!(
        id,
        "" | "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
    )
}

// ---------------------------------------------------------------- 上色 ----

/// 方块 → 颜色。原版方块尽量贴近游戏内地图的颜色，模组方块按关键字猜；
/// 实在猜不出的用低饱和度的稳定色，避免地图变成一片随机彩色块。
fn block_color(id: &str) -> [u8; 3] {
    let name = id.strip_prefix("minecraft:").unwrap_or(id);
    if let Some(c) = exact_color(name) {
        return c;
    }
    let has = |k: &str| name.contains(k);
    if has("water") {
        [63, 118, 228]
    } else if has("lava") || has("magma") {
        [212, 90, 18]
    } else if has("snow") || name == "powder_snow" {
        [250, 250, 255]
    } else if has("ice") {
        [160, 200, 240]
    } else if has("leaves") {
        [60, 130, 55]
    } else if has("log") || has("wood") || has("planks") || has("stem") || has("hyphae") {
        [120, 95, 60]
    } else if has("sand") {
        [219, 211, 160]
    } else if has("grass") || has("fern") || has("flower") || has("sapling") || has("crop")
        || has("vine") || has("moss")
    {
        [145, 189, 89]
    } else if has("dirt") || has("mud") || has("podzol") {
        [134, 96, 67]
    } else if has("netherrack") || has("nether_wart") {
        [112, 2, 0]
    } else if has("end_stone") {
        [219, 222, 158]
    } else if has("obsidian") {
        [30, 28, 45]
    } else if has("terracotta") || has("brick") {
        [150, 92, 70]
    } else if has("sculk") {
        [20, 40, 45]
    } else if has("glass") {
        [200, 220, 230]
    } else if has("ore") {
        [122, 122, 128]
    } else if has("stone")
        || has("slate")
        || has("cobble")
        || has("gravel")
        || has("tuff")
        || has("asphalt")
        || has("paving")
        || has("concrete")
        || has("cement")
        || has("tile")
        || has("brick")
        || has("road")
        || has("path")
        || has("metal")
        || has("steel")
        || has("iron")
        || has("copper")
        || has("slab")
    {
        [130, 130, 135]
    } else if has("wool") || has("carpet") || has("bed") {
        [190, 190, 195]
    } else {
        muted_color(name)
    }
}

/// 出现频率最高的地表方块，颜色对齐游戏内地图
fn exact_color(name: &str) -> Option<[u8; 3]> {
    Some(match name {
        "grass_block" => [145, 189, 89],
        "dirt" | "coarse_dirt" | "rooted_dirt" => [134, 96, 67],
        "farmland" => [110, 78, 52],
        "podzol" | "mycelium" => [140, 120, 135],
        "stone" | "andesite" | "smooth_stone" => [112, 112, 112],
        "cobblestone" | "stone_bricks" | "stone_brick_stairs" | "stone_brick_slab"
        | "mossy_cobblestone" | "cobbled_deepslate" => [122, 122, 118],
        "deepslate" | "tuff" => [78, 78, 84],
        "bedrock" => [85, 85, 85],
        "gravel" => [131, 127, 125],
        "clay" => [164, 168, 184],
        "sand" => [219, 211, 160],
        "red_sand" => [191, 106, 47],
        "sandstone" | "smooth_sandstone" | "cut_sandstone" => [216, 203, 155],
        "red_sandstone" => [186, 106, 50],
        "water" => [63, 118, 228],
        "lava" => [212, 90, 18],
        "ice" | "packed_ice" => [160, 200, 240],
        "blue_ice" => [116, 168, 231],
        "snow" | "snow_block" => [250, 250, 255],
        "oak_leaves" => [60, 130, 55],
        "spruce_leaves" => [45, 95, 45],
        "birch_leaves" => [128, 180, 92],
        "jungle_leaves" => [48, 118, 46],
        "acacia_leaves" => [110, 150, 60],
        "dark_oak_leaves" => [40, 100, 35],
        "mangrove_leaves" => [80, 140, 70],
        "cherry_leaves" => [226, 155, 190],
        "azalea_leaves" | "flowering_azalea_leaves" => [95, 150, 70],
        "short_grass" | "tall_grass" | "grass" | "fern" | "large_fern" => [145, 189, 89],
        "oak_log" | "spruce_log" | "birch_log" | "jungle_log" | "acacia_log" | "dark_oak_log"
        | "oak_planks" | "spruce_planks" | "birch_planks" | "jungle_planks" | "acacia_planks"
        | "dark_oak_planks" => [154, 125, 77],
        "moss_block" => [90, 140, 60],
        "netherrack" => [112, 2, 0],
        "soul_sand" | "soul_soil" => [85, 60, 48],
        "end_stone" => [219, 222, 158],
        "purpur_block" => [170, 118, 170],
        "obsidian" => [30, 28, 45],
        "crimson_nylium" | "warped_nylium" => [125, 45, 55],
        "basalt" | "smooth_basalt" | "blackstone" => [70, 68, 74],
        "magma_block" => [150, 70, 20],
        _ => return None,
    })
}

/// 认不出来的方块（主要是模组方块）：给灰调，按名字散列定深浅。
/// 不用高饱和度色——那会把城市里的道路、建材染成刺眼的粉紫色块，
/// 跟地图观感完全不搭。这里只保证不同的方块深浅略有区别。
fn muted_color(name: &str) -> [u8; 3] {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    let l = 0.36 + (h % 7) as f32 * 0.025;
    hsl_to_rgb((h % 360) as f32, 0.10, l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [u8; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    [
        ((r + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((g + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((b + m) * 255.0).round().clamp(0.0, 255.0) as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json as j;

    fn nbt(v: serde_json::Value) -> Value {
        level::json_to_value(&v).unwrap()
    }

    /// 造一个只有地形的区块：Y=0 整层石头（单元素调色板），Y=1 整层空气
    fn flat_chunk() -> Value {
        nbt(j!({
            "xPos": 0,
            "zPos": 0,
            "Status": "minecraft:full",
            "sections": [
                { "Y": 0, "block_states": { "palette": [{ "Name": "minecraft:stone" }] } },
                { "Y": 1, "block_states": { "palette": [{ "Name": "minecraft:air" }] } },
            ],
        }))
    }

    #[test]
    fn surface_skips_air_sections() {
        let s = surface_ids(&flat_chunk()).expect("surface");
        assert_eq!(s.len(), 256);
        assert!(
            s.iter().all(|id| id == "minecraft:stone"),
            "整层石头，地表应全是石头"
        );
    }

    /// 打包数据（多元素调色板 + LongArray）要能正确解出下标
    #[test]
    fn packed_palette_decodes() {
        // 调色板 [air, stone]，4 bit 一项：x < 8 的格子放石头
        let mut longs = vec![0i64; 256];
        for y in 0..16usize {
            for z in 0..16usize {
                for x in 0..8usize {
                    let i = (y << 8) | (z << 4) | x;
                    longs[i / 16] |= 1i64 << ((i % 16) * 4);
                }
            }
        }
        let mut chunk = nbt(j!({
            "sections": [{
                "Y": 0,
                "block_states": { "palette": [
                    { "Name": "minecraft:air" },
                    { "Name": "minecraft:stone" },
                ] },
            }],
        }));
        if let Value::Compound(c) = &mut chunk {
            if let Some(Value::List(secs)) = c.get_mut("sections") {
                if let Some(Value::Compound(s)) = secs.first_mut() {
                    if let Some(Value::Compound(bs)) = s.get_mut("block_states") {
                        bs.insert("data".into(), Value::LongArray(fastnbt::LongArray::new(longs)));
                    }
                }
            }
        }

        let s = surface_ids(&chunk).expect("surface");
        assert_eq!(s[0], "minecraft:stone");
        assert_eq!(s[7], "minecraft:stone");
        assert_eq!(s[8], "");
        assert_eq!(s[15 * 16 + 15], "");
    }

    /// 一格压倒多数时取多数，少数方块不该翻盘
    #[test]
    fn cell_picks_majority() {
        let grass = intern_surface(&["minecraft:grass_block".into()])[0];
        let water = intern_surface(&["minecraft:water".into()])[0];
        let mut cell = Cell { ids: [0; SLOTS], votes: [0; SLOTS] };
        for _ in 0..30 {
            vote(&mut cell, grass);
        }
        for _ in 0..12 {
            vote(&mut cell, water);
        }
        assert_eq!(winning_id(&cell), grass);

        // 方块种类远多于槽位时，大宗方块也不能被挤掉
        let mut cell = Cell { ids: [0; SLOTS], votes: [0; SLOTS] };
        let ids: Vec<u16> = (0..40)
            .map(|i| intern_surface(&[format!("mod:block{i}")])[0])
            .collect();
        for _ in 0..20 {
            for id in &ids {
                vote(&mut cell, *id);
            }
        }
        vote(&mut cell, grass);
        vote(&mut cell, grass);
        assert_ne!(winning_id(&cell), 0);
    }

    /// 渲染：范围裁剪、降采样、未生成区块留空
    #[test]
    fn render_crops_and_downsamples() {
        let dir = std::env::temp_dir().join(format!("qookix-map-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        region::write_chunk(&dir, "overworld", 0, 0, &flat_chunk()).unwrap();

        let rect = Rect { x0: 0, z0: 0, x1: 32, z1: 32, step: 1 };
        let img = render(&dir, "overworld", rect, |_, _| {}).unwrap();
        assert_eq!((img.width, img.height), (32, 32));
        assert_eq!(img.rendered, 1);
        assert_ne!(img.data[0], UNKNOWN, "区块内的像素要有颜色");
        assert_eq!(img.data[(16 * 32) as usize], UNKNOWN, "区块外的像素留空");
        assert!(img.legend.iter().any(|(n, _, _)| n == "minecraft:stone"));

        let img2 = render(&dir, "overworld", Rect { step: 2, ..rect }, |_, _| {}).unwrap();
        assert_eq!((img2.width, img2.height), (16, 16));
        assert_eq!(img2.step, 2);

        let huge = Rect { x0: 0, z0: 0, x1: 4096, z1: 4096, step: 1 };
        assert!(render(&dir, "overworld", huge, |_, _| {}).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 同一区域渲染两次结果必须一致（第二次全走缓存，不能画出不一样的东西）
    #[test]
    fn repeat_render_is_identical() {
        let dir = std::env::temp_dir().join(format!("qookix-map-repeat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        region::write_chunk(&dir, "overworld", 0, 0, &flat_chunk()).unwrap();

        let rect = Rect { x0: 0, z0: 0, x1: 32, z1: 32, step: 1 };
        let a = render(&dir, "overworld", rect, |_, _| {}).unwrap();
        let b = render(&dir, "overworld", rect, |_, _| {}).unwrap();
        assert_eq!(a.data, b.data, "缓存命中的结果要和首次一致");
        assert_eq!(a.palette, b.palette);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn color_lookup_is_stable() {
        assert_eq!(block_color("minecraft:grass_block"), [145, 189, 89]);
        assert_eq!(block_color("minecraft:water"), [63, 118, 228]);
        // 认不出的方块：同名同色，且是低饱和的灰调（不刺眼）
        assert_eq!(block_color("mod:weird_block"), block_color("mod:weird_block"));
        let c = block_color("mod:weird_block");
        let max = c.iter().max().unwrap();
        let min = c.iter().min().unwrap();
        assert!(max - min < 60, "模组方块颜色应该偏灰: {c:?}");
        assert_eq!(bits_for(2), 4);
        assert_eq!(bits_for(17), 5);
    }
}
