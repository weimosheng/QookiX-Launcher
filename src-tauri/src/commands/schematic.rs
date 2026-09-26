//! 投影文件预览：解析 .litematic / .schem（Sponge v2/v3），返回 manifest + 分块方块数据。
#![allow(non_snake_case)]

use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use tauri::ipc::Response;
use uuid::Uuid;

const CHUNK_SIZE: i32 = 16;
const CHUNK_VOLUME: usize = 16 * 16 * 16;
const MAX_FILE_SIZE: u64 = 256 * 1024 * 1024;
const MAX_DECOMPRESSED_SIZE: u64 = 512 * 1024 * 1024;
const MAX_VOLUME: u64 = 128_000_000;
const MAX_PALETTE_ENTRIES: usize = 1_048_576;

// ---------------------------------------------------------------------------
// 前端 ↔ 后端数据类型
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum SchematicSource {
    External { path: String },
    Instance { instance_id: String, relative_path: String },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewBlockState {
    pub name: String,
    pub properties: BTreeMap<String, String>,
}

impl PreviewBlockState {
    fn air() -> Self {
        Self {
            name: "minecraft:air".to_string(),
            properties: BTreeMap::new(),
        }
    }

    fn key(&self) -> String {
        if self.properties.is_empty() {
            return self.name.clone();
        }
        let props = self
            .properties
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(",");
        format!("{}[{props}]", self.name)
    }

    fn is_air(&self) -> bool {
        matches!(
            self.name.as_str(),
            "minecraft:air"
                | "minecraft:cave_air"
                | "minecraft:void_air"
                | "minecraft:light"
                | "minecraft:barrier"
                | "minecraft:structure_void"
        )
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewChunkDescriptor {
    pub position: [i32; 3],
    pub non_air_blocks: u32,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewRegion {
    pub id: String,
    pub name: String,
    pub origin: [i32; 3],
    pub size: [u32; 3],
    pub min: [i32; 3],
    pub max: [i32; 3],
    pub block_count: u64,
    pub chunks: Vec<PreviewChunkDescriptor>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewMaterial {
    pub name: String,
    pub count: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewManifest {
    pub session_id: String,
    pub file_name: String,
    pub source_path: String,
    pub source_instance_id: Option<String>,
    pub format: String,
    pub format_version: i32,
    pub data_version: Option<i32>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub created_at: Option<i64>,
    pub modified_at: Option<i64>,
    pub min: [i32; 3],
    pub max: [i32; 3],
    pub size: [u32; 3],
    pub block_count: u64,
    pub entity_count: u64,
    pub block_entity_count: u64,
    pub palette: Vec<PreviewBlockState>,
    pub materials: Vec<PreviewMaterial>,
    pub regions: Vec<PreviewRegion>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceSchematicFile {
    pub relative_path: String,
    pub file_name: String,
    pub format: String,
    pub size: u64,
    pub modified_at: Option<u64>,
}

// ---------------------------------------------------------------------------
// 会话管理
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct PreviewChunk {
    blocks: Box<[u32]>,
    non_air_blocks: u32,
}

#[derive(Clone)]
struct SessionRegion {
    manifest: PreviewRegion,
    chunks: HashMap<[i32; 3], PreviewChunk>,
}

#[derive(Clone)]
struct PreviewSession {
    manifest: PreviewManifest,
    regions: Vec<SessionRegion>,
}

static SESSIONS: OnceLock<Mutex<HashMap<String, Arc<PreviewSession>>>> = OnceLock::new();

fn sessions() -> &'static Mutex<HashMap<String, Arc<PreviewSession>>> {
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn find_session(session_id: &str) -> Result<Arc<PreviewSession>, String> {
    sessions()
        .lock()
        .map_err(|_| "会话存储不可用".to_string())?
        .get(session_id)
        .cloned()
        .ok_or_else(|| "投影预览会话已过期".to_string())
}

// ---------------------------------------------------------------------------
// NBT 反序列化结构
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct LitematicFile {
    #[serde(default)]
    Version: i32,
    #[serde(default)]
    MinecraftDataVersion: Option<i32>,
    #[serde(default)]
    Metadata: Option<LitematicMeta>,
    Regions: BTreeMap<String, LitematicRegion>,
}

#[derive(Deserialize, Default)]
struct LitematicMeta {
    #[serde(default)]
    Name: Option<String>,
    #[serde(default)]
    Description: Option<String>,
    #[serde(default)]
    Author: Option<String>,
    #[serde(default)]
    TimeCreated: Option<i64>,
    #[serde(default)]
    TimeModified: Option<i64>,
}

#[derive(Deserialize)]
struct LitematicRegion {
    Position: IVec3,
    Size: IVec3,
    BlockStatePalette: Vec<BlockStateTag>,
    BlockStates: fastnbt::LongArray,
}

#[derive(Deserialize)]
struct IVec3 {
    x: i32,
    y: i32,
    z: i32,
}

#[derive(Deserialize)]
struct BlockStateTag {
    Name: String,
    #[serde(default)]
    Properties: Option<BTreeMap<String, String>>,
}

/// Sponge 文件：v3 把数据嵌套在 `Schematic` 复合标签里，v2 直接放在根。
/// 用扁平结构统一处理 —— `Schematic` 为 Some 时走 v3 路径，否则走 v2。
#[derive(Deserialize)]
struct SpongeFile {
    #[serde(default)]
    Schematic: Option<SpongeBody>,
    #[serde(default)]
    Version: Option<i32>,
    #[serde(default)]
    Width: Option<i16>,
    #[serde(default)]
    Height: Option<i16>,
    #[serde(default)]
    Length: Option<i16>,
    #[serde(default)]
    Offset: Option<fastnbt::IntArray>,
    #[serde(default)]
    Blocks: Option<SpongeBlocks>,
    #[serde(default)]
    Palette: Option<BTreeMap<String, i32>>,
    #[serde(default)]
    BlockData: Option<fastnbt::ByteArray>,
    #[serde(default)]
    Data: Option<fastnbt::ByteArray>,
    #[serde(default)]
    Metadata: Option<SpongeMeta>,
    #[serde(default)]
    DataVersion: Option<i32>,
}

#[derive(Deserialize, Default)]
struct SpongeBody {
    #[serde(default)]
    Version: Option<i32>,
    #[serde(default)]
    Width: Option<i16>,
    #[serde(default)]
    Height: Option<i16>,
    #[serde(default)]
    Length: Option<i16>,
    #[serde(default)]
    Offset: Option<fastnbt::IntArray>,
    #[serde(default)]
    Blocks: Option<SpongeBlocks>,
    #[serde(default)]
    Palette: Option<BTreeMap<String, i32>>,
    #[serde(default)]
    BlockData: Option<fastnbt::ByteArray>,
    #[serde(default)]
    Data: Option<fastnbt::ByteArray>,
    #[serde(default)]
    Metadata: Option<SpongeMeta>,
    #[serde(default)]
    DataVersion: Option<i32>,
}

#[derive(Deserialize, Default)]
struct SpongeBlocks {
    #[serde(default)]
    Palette: Option<BTreeMap<String, i32>>,
    #[serde(default)]
    BlockData: Option<fastnbt::ByteArray>,
    #[serde(default)]
    Data: Option<fastnbt::ByteArray>,
}

#[derive(Deserialize, Default)]
struct SpongeMeta {
    #[serde(default)]
    Name: Option<String>,
}

// ---------------------------------------------------------------------------
// 解析入口
// ---------------------------------------------------------------------------

fn read_and_decompress(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = std::fs::metadata(path).map_err(|e| format!("读取文件信息失败: {e}"))?;
    if metadata.len() > MAX_FILE_SIZE {
        return Err("投影文件超过 256 MiB 限制".into());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("读取文件失败: {e}"))?;
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut decoder = GzDecoder::new(&bytes[..]).take(MAX_DECOMPRESSED_SIZE + 1);
        let mut out = Vec::new();
        decoder
            .read_to_end(&mut out)
            .map_err(|e| format!("解压投影文件失败: {e}"))?;
        if out.len() as u64 > MAX_DECOMPRESSED_SIZE {
            return Err("解压后的投影文件超过 512 MiB 限制".into());
        }
        Ok(out)
    } else if bytes.starts_with(&[0x28, 0xb5, 0x2d, 0xfd]) {
        Err("Zstd 压缩的投影文件暂不支持，请用 gzip 重新保存".into())
    } else {
        Ok(bytes)
    }
}

fn parse_schematic(
    path: &Path,
    source_instance_id: Option<String>,
) -> Result<PreviewSession, String> {
    let bytes = read_and_decompress(path)?;
    let file_name = path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("schematic")
        .to_string();
    let source_path = path.to_string_lossy().to_string();

    // 先尝试 litematic（有 Regions 字段），失败再尝试 sponge
    let litematic_result = fastnbt::from_bytes::<LitematicFile>(&bytes);
    if let Ok(file) = litematic_result {
        return parse_litematic(file, source_path, file_name, source_instance_id);
    }
    let litematic_err = litematic_result.err().map(|e| e.to_string()).unwrap_or_default();

    let sponge_result = fastnbt::from_bytes::<SpongeFile>(&bytes);
    if let Ok(file) = sponge_result {
        return parse_sponge(file, source_path, file_name, source_instance_id)
            .map_err(|e| format!("Litematic 解析失败: {litematic_err}；Sponge 解析失败: {e}"));
    }
    let sponge_err = sponge_result.err().map(|e| e.to_string()).unwrap_or_default();
    Err(format!(
        "无法解析投影文件。Litematic 错误: {litematic_err}; Sponge 错误: {sponge_err}"
    ))
}

// ---------------------------------------------------------------------------
// Litematic 解析
// ---------------------------------------------------------------------------

fn parse_litematic(
    file: LitematicFile,
    source_path: String,
    file_name: String,
    source_instance_id: Option<String>,
) -> Result<PreviewSession, String> {
    let format_version = file.Version;
    if !(4..=7).contains(&format_version) {
        return Err(format!("不支持的 Litematic 格式版本 {format_version}"));
    }
    let data_version = file.MinecraftDataVersion;
    let meta = file.Metadata.unwrap_or_default();
    let mut builder = SessionBuilder::new();

    for (index, (region_name, region)) in file.Regions.into_iter().enumerate() {
        let position = [region.Position.x, region.Position.y, region.Position.z];
        let signed_size = [region.Size.x, region.Size.y, region.Size.z];
        let size = signed_size.map(i32::unsigned_abs);
        validate_volume(size)?;
        let (min, _max) = signed_region_bounds(position, signed_size)?;

        let local_palette: Vec<PreviewBlockState> = region
            .BlockStatePalette
            .into_iter()
            .map(|tag| PreviewBlockState {
                name: tag.Name,
                properties: tag.Properties.unwrap_or_default(),
            })
            .collect();
        validate_palette_size(local_palette.len())?;

        let local_to_global: Vec<u32> = local_palette
            .iter()
            .map(|state| {
                if state.is_air() {
                    0
                } else {
                    builder.palette_index(state.clone())
                }
            })
            .collect();

        let packed = &region.BlockStates;
        let volume = volume(size) as usize;
        let bits = if local_palette.len() <= 1 {
            2
        } else {
            ((usize::BITS - (local_palette.len() - 1).leading_zeros()) as usize).max(2)
        };
        let required_longs = (volume * bits).div_ceil(64);
        if packed.len() < required_longs {
            return Err(format!("区域 {region_name} 的方块数据不完整"));
        }

        let mut session_region = builder.new_region(
            format!("region-{index}"),
            region_name.clone(),
            position,
            size,
            min,
        )?;

        for block_index in 0..volume {
            let palette_index = unpack_litematic_value(packed, block_index, bits) as usize;
            let global_index = *local_to_global.get(palette_index).ok_or_else(|| {
                format!("区域 {region_name} 引用了不存在的调色板条目 {palette_index}")
            })?;
            if !builder.palette[global_index as usize].is_air() {
                let x = block_index % size[0] as usize;
                let z = (block_index / size[0] as usize) % size[2] as usize;
                let y = block_index / (size[0] as usize * size[2] as usize);
                session_region.put_block(
                    [min[0] + x as i32, min[1] + y as i32, min[2] + z as i32],
                    global_index,
                );
                builder.record_material(global_index);
            }
        }
        builder.finish_region(session_region);
    }

    builder.finish(ManifestMetadata {
        source_path,
        source_instance_id,
        file_name,
        format: "litematic".to_string(),
        format_version,
        data_version,
        name: meta.Name,
        description: meta.Description,
        author: meta.Author,
        created_at: meta.TimeCreated,
        modified_at: meta.TimeModified,
        entity_count: 0,
        block_entity_count: 0,
    })
}

// ---------------------------------------------------------------------------
// Sponge 解析
// ---------------------------------------------------------------------------

fn parse_sponge(
    file: SpongeFile,
    source_path: String,
    file_name: String,
    source_instance_id: Option<String>,
) -> Result<PreviewSession, String> {
    let body = file.Schematic.unwrap_or_else(|| SpongeBody {
        Version: file.Version,
        Width: file.Width,
        Height: file.Height,
        Length: file.Length,
        Offset: file.Offset,
        Blocks: file.Blocks,
        Palette: file.Palette,
        BlockData: file.BlockData,
        Data: file.Data,
        Metadata: file.Metadata,
        DataVersion: file.DataVersion,
    });

    let format_version = body
        .Version
        .ok_or_else(|| "Sponge 投影缺少 Version 字段".to_string())?;
    if !matches!(format_version, 2 | 3) {
        return Err(format!("不支持的 Sponge 投影版本 {format_version}"));
    }

    let dimensions = [
        body.Width.ok_or("Sponge 投影缺少 Width")? as i32,
        body.Height.ok_or("Sponge 投影缺少 Height")? as i32,
        body.Length.ok_or("Sponge 投影缺少 Length")? as i32,
    ];
    if dimensions.iter().any(|v| *v <= 0) {
        return Err("Sponge 投影尺寸必须为正".into());
    }
    let size = dimensions.map(|v| v as u32);
    validate_volume(size)?;

    let offset = body
        .Offset
        .as_ref()
        .filter(|v| v.len() >= 3)
        .map(|v| [v[0], v[1], v[2]])
        .unwrap_or([0, 0, 0]);

    let blocks = body.Blocks.unwrap_or_default();
    let palette_compound = blocks
        .Palette
        .or(body.Palette)
        .ok_or_else(|| "Sponge 投影缺少 Palette".to_string())?;
    let block_data = blocks
        .BlockData
        .or(blocks.Data)
        .or(body.BlockData)
        .or(body.Data)
        .ok_or_else(|| "Sponge 投影缺少 BlockData".to_string())?;

    let max_palette_index = palette_compound.values().copied().max().unwrap_or(0);
    validate_palette_size(max_palette_index.saturating_add(1) as usize)?;

    let mut local_palette = vec![PreviewBlockState::air(); max_palette_index as usize + 1];
    for (state, index) in &palette_compound {
        if *index >= 0 {
            local_palette[*index as usize] = parse_state_string(state);
        }
    }

    let mut builder = SessionBuilder::new();
    let local_to_global: Vec<u32> = local_palette
        .iter()
        .map(|state| {
            if state.is_air() {
                0
            } else {
                builder.palette_index(state.clone())
            }
        })
        .collect();

    let mut region = builder.new_region(
        "region-0".to_string(),
        "Main".to_string(),
        offset,
        size,
        offset,
    )?;

    let total = volume(size) as usize;
    let mut bytes: &[u8] = &block_data.iter().map(|&v| v as u8).collect::<Vec<_>>();
    for index in 0..total {
        let palette_index = read_varint(&mut bytes)? as usize;
        let global_index = *local_to_global.get(palette_index).ok_or_else(|| {
            format!("Sponge 投影引用了不存在的调色板条目 {palette_index}")
        })?;
        if !builder.palette[global_index as usize].is_air() {
            let x = index % size[0] as usize;
            let z = (index / size[0] as usize) % size[2] as usize;
            let y = index / (size[0] as usize * size[2] as usize);
            region.put_block(
                [
                    offset[0] + x as i32,
                    offset[1] + y as i32,
                    offset[2] + z as i32,
                ],
                global_index,
            );
            builder.record_material(global_index);
        }
    }
    builder.finish_region(region);

    let meta = body.Metadata.unwrap_or_default();
    builder.finish(ManifestMetadata {
        source_path,
        source_instance_id,
        file_name,
        format: format!("schem_v{format_version}"),
        format_version,
        data_version: body.DataVersion,
        name: meta.Name,
        description: None,
        author: None,
        created_at: None,
        modified_at: None,
        entity_count: 0,
        block_entity_count: 0,
    })
}

// ---------------------------------------------------------------------------
// 会话构建器
// ---------------------------------------------------------------------------

struct SessionBuilder {
    palette: Vec<PreviewBlockState>,
    palette_lookup: HashMap<String, u32>,
    material_counts: HashMap<u32, u64>,
    regions: Vec<SessionRegion>,
}

struct ManifestMetadata {
    source_path: String,
    source_instance_id: Option<String>,
    file_name: String,
    format: String,
    format_version: i32,
    data_version: Option<i32>,
    name: Option<String>,
    description: Option<String>,
    author: Option<String>,
    created_at: Option<i64>,
    modified_at: Option<i64>,
    entity_count: u64,
    block_entity_count: u64,
}

impl SessionBuilder {
    fn new() -> Self {
        let air = PreviewBlockState::air();
        Self {
            palette: vec![air.clone()],
            palette_lookup: HashMap::from([(air.key(), 0)]),
            material_counts: HashMap::new(),
            regions: Vec::new(),
        }
    }

    fn palette_index(&mut self, state: PreviewBlockState) -> u32 {
        let key = state.key();
        if let Some(&index) = self.palette_lookup.get(&key) {
            return index;
        }
        let index = self.palette.len() as u32;
        self.palette.push(state);
        self.palette_lookup.insert(key, index);
        index
    }

    fn record_material(&mut self, palette_index: u32) {
        *self.material_counts.entry(palette_index).or_default() += 1;
    }

    fn new_region(
        &self,
        id: String,
        name: String,
        origin: [i32; 3],
        size: [u32; 3],
        min: [i32; 3],
    ) -> Result<SessionRegion, String> {
        let max = checked_region_max(min, size)?;
        Ok(SessionRegion {
            manifest: PreviewRegion {
                id,
                name,
                origin,
                size,
                min,
                max,
                block_count: 0,
                chunks: Vec::new(),
            },
            chunks: HashMap::new(),
        })
    }

    fn finish_region(&mut self, mut region: SessionRegion) {
        region.manifest.block_count = region
            .chunks
            .values()
            .map(|c| c.non_air_blocks as u64)
            .sum();
        let mut chunks = region
            .chunks
            .iter()
            .map(|(pos, c)| PreviewChunkDescriptor {
                position: *pos,
                non_air_blocks: c.non_air_blocks,
            })
            .collect::<Vec<_>>();
        chunks.sort_by_key(|c| (c.position[1], c.position[2], c.position[0]));
        region.manifest.chunks = chunks;
        self.regions.push(region);
    }

    fn finish(self, metadata: ManifestMetadata) -> Result<PreviewSession, String> {
        if self.regions.is_empty() {
            return Err("投影文件不包含任何区域".into());
        }
        let min = [
            self.regions.iter().map(|r| r.manifest.min[0]).min().unwrap_or(0),
            self.regions.iter().map(|r| r.manifest.min[1]).min().unwrap_or(0),
            self.regions.iter().map(|r| r.manifest.min[2]).min().unwrap_or(0),
        ];
        let max = [
            self.regions.iter().map(|r| r.manifest.max[0]).max().unwrap_or(0),
            self.regions.iter().map(|r| r.manifest.max[1]).max().unwrap_or(0),
            self.regions.iter().map(|r| r.manifest.max[2]).max().unwrap_or(0),
        ];
        let mut size = [0u32; 3];
        for axis in 0..3 {
            size[axis] = (max[axis] as i64 - min[axis] as i64 + 1)
                .try_into()
                .map_err(|_| "投影坐标跨度超出支持范围".to_string())?;
        }
        let block_count = self
            .regions
            .iter()
            .map(|r| r.manifest.block_count)
            .sum();
        let mut materials_by_name = HashMap::<String, u64>::new();
        for (index, count) in &self.material_counts {
            if let Some(state) = self.palette.get(*index as usize) {
                *materials_by_name.entry(state.name.clone()).or_default() += count;
            }
        }
        let mut materials = materials_by_name
            .into_iter()
            .map(|(name, count)| PreviewMaterial { name, count })
            .collect::<Vec<_>>();
        materials.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));

        let session_id = Uuid::new_v4().to_string();
        let manifest = PreviewManifest {
            session_id,
            file_name: metadata.file_name,
            source_path: metadata.source_path,
            source_instance_id: metadata.source_instance_id,
            format: metadata.format,
            format_version: metadata.format_version,
            data_version: metadata.data_version,
            name: metadata.name,
            description: metadata.description,
            author: metadata.author,
            created_at: metadata.created_at,
            modified_at: metadata.modified_at,
            min,
            max,
            size,
            block_count,
            entity_count: metadata.entity_count,
            block_entity_count: metadata.block_entity_count,
            palette: self.palette,
            materials,
            regions: self.regions.iter().map(|r| r.manifest.clone()).collect(),
            warnings: Vec::new(),
        };
        Ok(PreviewSession {
            manifest,
            regions: self.regions,
        })
    }
}

impl SessionRegion {
    fn put_block(&mut self, position: [i32; 3], palette_index: u32) {
        let chunk_position = position.map(|v| v.div_euclid(CHUNK_SIZE));
        let local = position.map(|v| v.rem_euclid(CHUNK_SIZE) as usize);
        let index = local[1] * 256 + local[2] * 16 + local[0];
        let chunk = self
            .chunks
            .entry(chunk_position)
            .or_insert_with(|| PreviewChunk {
                blocks: vec![0; CHUNK_VOLUME].into_boxed_slice(),
                non_air_blocks: 0,
            });
        if chunk.blocks[index] == 0 {
            chunk.non_air_blocks += 1;
        }
        chunk.blocks[index] = palette_index;
    }

    #[allow(dead_code)]
    fn block_at(&self, position: [i32; 3]) -> u32 {
        let chunk_position = position.map(|v| v.div_euclid(CHUNK_SIZE));
        let local = position.map(|v| v.rem_euclid(CHUNK_SIZE) as usize);
        let index = local[1] * 256 + local[2] * 16 + local[0];
        self.chunks
            .get(&chunk_position)
            .map(|c| c.blocks[index])
            .unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// 辅助函数
// ---------------------------------------------------------------------------

fn volume(size: [u32; 3]) -> u64 {
    size[0] as u64 * size[1] as u64 * size[2] as u64
}

fn validate_volume(size: [u32; 3]) -> Result<(), String> {
    if volume(size) > MAX_VOLUME {
        return Err(format!(
            "投影体积 {} 超过 {} 块上限",
            volume(size),
            MAX_VOLUME
        ));
    }
    Ok(())
}

fn validate_palette_size(len: usize) -> Result<(), String> {
    if len > MAX_PALETTE_ENTRIES {
        return Err(format!(
            "调色板条目数 {len} 超过 {MAX_PALETTE_ENTRIES} 上限"
        ));
    }
    Ok(())
}

fn signed_region_bounds(
    position: [i32; 3],
    signed_size: [i32; 3],
) -> Result<([i32; 3], [i32; 3]), String> {
    let min = [
        position[0].min(position[0] + signed_size[0] - 1),
        position[1].min(position[1] + signed_size[1] - 1),
        position[2].min(position[2] + signed_size[2] - 1),
    ];
    let max = [
        position[0].max(position[0] + signed_size[0] - 1),
        position[1].max(position[1] + signed_size[1] - 1),
        position[2].max(position[2] + signed_size[2] - 1),
    ];
    Ok((min, max))
}

fn checked_region_max(min: [i32; 3], size: [u32; 3]) -> Result<[i32; 3], String> {
    let max = [
        min[0]
            .checked_add(size[0] as i32 - 1)
            .ok_or("区域坐标溢出")?,
        min[1]
            .checked_add(size[1] as i32 - 1)
            .ok_or("区域坐标溢出")?,
        min[2]
            .checked_add(size[2] as i32 - 1)
            .ok_or("区域坐标溢出")?,
    ];
    Ok(max)
}

fn parse_state_string(state: &str) -> PreviewBlockState {
    let (name, properties) = if let Some(open) = state.find('[') {
        let name = state[..open].to_string();
        let props_str = &state[open + 1..state.len().saturating_sub(1)];
        let mut props = BTreeMap::new();
        for pair in props_str.split(',') {
            if let Some(eq) = pair.find('=') {
                props.insert(pair[..eq].to_string(), pair[eq + 1..].to_string());
            }
        }
        (name, props)
    } else {
        (state.to_string(), BTreeMap::new())
    };
    PreviewBlockState { name, properties }
}

fn unpack_litematic_value(packed: &[i64], index: usize, bits: usize) -> u32 {
    let bit_index = index * bits;
    let long_index = bit_index / 64;
    let bit_offset = bit_index % 64;
    let mask = (1u64 << bits) - 1;
    let long = packed[long_index] as u64;
    let mut value = (long >> bit_offset) & mask;
    if bit_offset + bits > 64 {
        let next_long = packed.get(long_index + 1).copied().unwrap_or(0) as u64;
        value |= (next_long << (64 - bit_offset)) & mask;
    }
    value as u32
}

fn read_varint(bytes: &mut &[u8]) -> Result<u32, String> {
    let mut result = 0u32;
    let mut shift = 0u32;
    loop {
        let &byte = bytes
            .first()
            .ok_or("Sponge BlockData 意外结束")?;
        *bytes = &bytes[1..];
        result |= ((byte as u32) & 0x7f) << shift;
        if (byte & 0x80) == 0 {
            return Ok(result);
        }
        shift += 7;
        if shift >= 32 {
            return Err("Sponge varint 过大".into());
        }
    }
}

fn format_from_extension(ext: &str) -> &'static str {
    match ext {
        "litematic" => "litematic",
        "schem" => "sponge",
        "schematic" => "sponge",
        _ => "unknown",
    }
}

// ---------------------------------------------------------------------------
// Tauri 命令
// ---------------------------------------------------------------------------

fn resolve_source(
    state: &crate::state::AppState,
    source: &SchematicSource,
) -> Result<(PathBuf, Option<String>), String> {
    match source {
        SchematicSource::External { path } => {
            let p = PathBuf::from(path);
            if !p.is_file() {
                return Err("投影文件不存在".into());
            }
            Ok((p, None))
        }
        SchematicSource::Instance {
            instance_id,
            relative_path,
        } => {
            if Path::new(relative_path)
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
            {
                return Err("非法实例投影路径".into());
            }
            let dir = state
                .instances_dir()
                .join(instance_id)
                .join("schematics");
            let candidate = dir.join(relative_path);
            let canonical = std::fs::canonicalize(&candidate)
                .map_err(|_| "实例投影文件不存在".to_string())?;
            let canonical_root = std::fs::canonicalize(&dir)
                .map_err(|_| "实例没有 schematics 目录".to_string())?;
            if !canonical.starts_with(&canonical_root) || !canonical.is_file() {
                return Err("非法实例投影路径".into());
            }
            Ok((canonical, Some(instance_id.clone())))
        }
    }
}

/// 打开投影文件并创建预览会话，返回 manifest（元数据/调色板/区域/材料统计）。
#[tauri::command]
pub async fn schematic_preview_open(
    state: tauri::State<'_, crate::state::AppState>,
    source: SchematicSource,
) -> Result<PreviewManifest, String> {
    let (path, source_instance_id) = resolve_source(&state, &source)?;
    let session = tokio::task::spawn_blocking(move || {
        parse_schematic(&path, source_instance_id)
    })
    .await
    .map_err(|e| e.to_string())??;
    let manifest = session.manifest.clone();
    sessions()
        .lock()
        .map_err(|_| "会话存储不可用".to_string())?
        .insert(manifest.session_id.clone(), Arc::new(session));
    Ok(manifest)
}

/// 列出实例 schematics 目录下的所有投影文件。
#[tauri::command]
pub fn schematic_preview_list_instance_files(
    state: tauri::State<'_, crate::state::AppState>,
    instance_id: String,
) -> Result<Vec<InstanceSchematicFile>, String> {
    let dir = state.instances_dir().join(&instance_id).join("schematics");
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    scan_schematics(&dir, &dir, &mut files)?;
    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(files)
}

fn scan_schematics(
    base: &Path,
    dir: &Path,
    out: &mut Vec<InstanceSchematicFile>,
) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|e| format!("读取目录失败: {e}"))? {
        let entry = entry.map_err(|e| format!("读取条目失败: {e}"))?;
        let path = entry.path();
        if path.is_dir() {
            scan_schematics(base, &path, out)?;
        } else if let Some(ext) = path.extension().and_then(|v| v.to_str()) {
            let ext = ext.to_ascii_lowercase();
            if matches!(ext.as_str(), "litematic" | "schem" | "schematic") {
                let relative = path
                    .strip_prefix(base)
                    .map_err(|_| "路径错误".to_string())?;
                let metadata = std::fs::metadata(&path).map_err(|e| format!("读取文件信息失败: {e}"))?;
                out.push(InstanceSchematicFile {
                    relative_path: relative.to_string_lossy().replace('\\', "/"),
                    file_name: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                    format: format_from_extension(&ext).to_string(),
                    size: metadata.len(),
                    modified_at: metadata
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs()),
                });
            }
        }
    }
    Ok(())
}

/// 读取一个 16³ 分块的方块数据，返回二进制 Response。
/// 布局：4 字节 "SPC1" 魔数 | 4 字节 u32 LE 分块体积 | 4096 × 4 字节 u32 LE 调色板索引
#[tauri::command]
pub async fn schematic_preview_read_chunk(
    session_id: String,
    region_id: String,
    position: [i32; 3],
) -> Result<Response, String> {
    let session = find_session(&session_id)?;
    let region = session
        .regions
        .iter()
        .find(|r| r.manifest.id == region_id)
        .ok_or_else(|| "未知投影区域".to_string())?;
    let mut output = Vec::with_capacity(8 + CHUNK_VOLUME * 4);
    output.extend_from_slice(b"SPC1");
    output.extend_from_slice(&(CHUNK_VOLUME as u32).to_le_bytes());
    if let Some(chunk) = region.chunks.get(&position) {
        for value in &chunk.blocks {
            output.extend_from_slice(&value.to_le_bytes());
        }
    } else {
        output.resize(8 + CHUNK_VOLUME * 4, 0);
    }
    Ok(Response::new(output))
}

/// 查询某个位置的方块状态。
#[tauri::command]
pub async fn schematic_preview_block_info(
    session_id: String,
    region_id: String,
    position: [i32; 3],
) -> Result<Option<PreviewBlockState>, String> {
    let session = find_session(&session_id)?;
    let region = session
        .regions
        .iter()
        .find(|r| r.manifest.id == region_id)
        .ok_or_else(|| "未知投影区域".to_string())?;
    let chunk_position = position.map(|v| v.div_euclid(CHUNK_SIZE));
    let local = position.map(|v| v.rem_euclid(CHUNK_SIZE) as usize);
    let index = local[1] * 256 + local[2] * 16 + local[0];
    let palette_index = region
        .chunks
        .get(&chunk_position)
        .map(|c| c.blocks[index] as usize)
        .unwrap_or(0);
    Ok(session.manifest.palette.get(palette_index).cloned())
}

/// 关闭预览会话，释放内存。
#[tauri::command]
pub fn schematic_preview_close(session_id: String) -> Result<(), String> {
    sessions()
        .lock()
        .map_err(|_| "会话存储不可用".to_string())?
        .remove(&session_id);
    Ok(())
}
