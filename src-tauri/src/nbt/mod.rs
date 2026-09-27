//! NBT 存档编辑器后端。
//!
//! 安全原则：
//! 1. 写回只改指定字段，绝不重建 NBT 树（保住 WorldGenSettings 等未知字段）
//! 2. 每次写盘前自动单文件备份，保留最近 10 份
//! 3. 世界正在游玩（该实例有运行中的游戏进程）时拒绝写盘

pub mod backup;
pub mod item;
pub mod level;
pub mod map;
pub mod player;
pub mod region;
pub mod tree;

use crate::state::AppState;
use fastnbt::Value;
use level::{get_double, get_int, get_string};
use serde_json::{json, Map, Value as Json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 实例的 saves 根目录（instances/<id>/saves）
pub fn saves_dir(state: &AppState, instance_id: &str) -> PathBuf {
    state.instances_dir().join(instance_id).join("saves")
}

/// `world` 是否为绝对路径（手动指定的世界目录走这条路）
fn is_absolute(world: &str) -> bool {
    Path::new(world).is_absolute()
}

/// 解析世界目录：
/// - `world` 为绝对路径 → 手动指定的目录，直接使用
/// - 否则 → `instances/<instance_id>/saves/<world>`（需要合法目录名）
pub fn resolve_world(state: &AppState, instance_id: &str, world: &str) -> Result<PathBuf, String> {
    let path = if is_absolute(world) {
        PathBuf::from(world)
    } else {
        if !crate::util::is_safe_filename(world) {
            return Err("非法的世界目录名".into());
        }
        if instance_id.is_empty() {
            return Err("请先选择实例，或手动指定世界文件夹".into());
        }
        saves_dir(state, instance_id).join(world)
    };
    if !path.is_dir() {
        return Err(format!("世界目录不存在: {}", path.display()));
    }
    Ok(path)
}

/// 打开世界前的前置校验：必须存在 level.dat，且不是只读文件。
pub fn check_editable(world_dir: &Path) -> Result<(), String> {
    let level_path = world_dir.join("level.dat");
    if !level_path.is_file() {
        return Err("该目录没有 level.dat，无法编辑".into());
    }
    if std::fs::metadata(&level_path)
        .map(|m| m.permissions().readonly())
        .unwrap_or(false)
    {
        return Err("level.dat 为只读文件，无法保存".into());
    }
    Ok(())
}

/// 扫描实例的 saves 目录，返回每个世界的摘要（读 level.dat 取信息）。
///
/// `progress` 每处理完一个世界回调一次 `(已完成, 总数, 当前世界名)`，
/// 供前端显示进度（世界多/体积大时扫描要几秒，没有反馈用户会以为卡死）。
pub fn list_worlds(
    state: &AppState,
    instance_id: &str,
    mut progress: impl FnMut(usize, usize, &str),
) -> Result<Vec<Json>, String> {
    let saves = saves_dir(state, instance_id);
    if !saves.is_dir() {
        return Ok(Vec::new());
    }
    let mut dirs: Vec<PathBuf> = Vec::new();
    for e in std::fs::read_dir(&saves).map_err(|e| format!("读取 saves 失败: {e}"))?.flatten() {
        let p = e.path();
        if p.is_dir() {
            dirs.push(p);
        }
    }
    let total = dirs.len();
    // usercache.json 只读一次（原来每个世界都重读一遍）
    let cache = if instance_id.is_empty() {
        std::collections::HashMap::new()
    } else {
        player::name_cache_at(&state.instances_dir().join(instance_id))
    };

    let mut out = Vec::new();
    for (i, p) in dirs.iter().enumerate() {
        let dir_name = p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        progress(i + 1, total, &dir_name);

        let level_path = p.join("level.dat");
        let (level_name, game_type, difficulty, seed) = match level::read_level(&level_path) {
            Ok(root) => {
                let d = level::world_data(&root);
                (
                    get_string(d, "LevelName"),
                    get_int(d, "GameType"),
                    get_int(d, "Difficulty"),
                    world_seed(d),
                )
            }
            Err(_) => (None, None, None, None),
        };
        let size = world_size(p);
        let modified = p
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // 玩家数量与第一个玩家名（卡片上直接展示，省得进世界才看得到）
        let (player_count, player_name) = player::first_player_with(&cache, p);
        out.push(json!({
            "dir": dir_name,
            "levelName": level_name,
            "gameType": game_type,
            "difficulty": difficulty,
            "seed": seed,
            "size": size,
            "modified": modified,
            "playerCount": player_count,
            "playerName": player_name,
        }));
    }
    out.sort_by(|a, b| {
        b.get("modified")
            .and_then(|x| x.as_u64())
            .unwrap_or(0)
            .cmp(&a.get("modified").and_then(|x| x.as_u64()).unwrap_or(0))
    });
    Ok(out)
}

/// 世界体积：顶层文件 + 各 region/entities/poi 目录下的文件。
/// 不做全目录递归——playerdata / advancements / stats 动辄上万个文件，
/// 逐个 stat 会让"扫描世界"慢到像卡死。
fn world_size(dir: &Path) -> u64 {
    let mut total = 0u64;
    let subdirs = [
        ["region", ""],
        ["entities", ""],
        ["poi", ""],
        ["DIM-1", "region"],
        ["DIM-1", "entities"],
        ["DIM-1", "poi"],
        ["DIM1", "region"],
        ["DIM1", "entities"],
        ["DIM1", "poi"],
    ];
    for [a, b] in subdirs {
        let path = if b.is_empty() {
            dir.join(a)
        } else {
            dir.join(a).join(b)
        };
        if let Ok(rd) = std::fs::read_dir(path) {
            for e in rd.flatten() {
                if let Ok(m) = e.metadata() {
                    total += m.len();
                }
            }
        }
    }
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if let Ok(m) = e.metadata() {
                if m.is_file() {
                    total += m.len();
                }
            }
        }
    }
    total
}

/// 读出一个世界的表单字段（只挑 UI 关心的，完整树另由 nbt_tree_view 提供）。
pub fn world_form(path: &Path) -> Result<Json, String> {
    let root = level::read_level(path)?;
    let data = level::world_data(&root);
    let rules = level::get(data, "GameRules")
        .map(game_rules_json)
        .unwrap_or(Json::Null);
    Ok(json!({
        "levelName": get_string(data, "LevelName"),
        "gameType": get_int(data, "GameType"),
        "difficulty": get_int(data, "Difficulty"),
        "difficultyLocked": get_int(data, "DifficultyLocked"),
        "hardcore": get_int(data, "hardcore"),
        // 种子是 Long，可能超过 JS 的安全整数范围，这里按十进制字符串给前端
        "seed": world_seed(data).map(|n| n.to_string()),
        "time": get_int(data, "Time"),
        "dayTime": get_int(data, "DayTime"),
        "raining": get_int(data, "raining"),
        "thundering": get_int(data, "thundering"),
        "spawnX": get_int(data, "SpawnX"),
        "spawnY": get_int(data, "SpawnY"),
        "spawnZ": get_int(data, "SpawnZ"),
        "allowCommands": get_int(data, "allowCommands"),
        "gameRules": rules,
        "border": border_values(data),
        "borderFlat": border_is_flat(data),
    }))
}

/// 种子：1.16+ 存在 `WorldGenSettings.seed`，更老的存档在顶层 `RandomSeed`
fn world_seed(data: &Value) -> Option<i64> {
    level::get(data, "WorldGenSettings")
        .and_then(|w| get_int(w, "seed"))
        .or_else(|| get_int(data, "RandomSeed"))
}

/// 世界边界的两种存法：老的扁平 `BorderSize/BorderCenterX…`，或 `WorldBorder` 子复合节点
fn border_is_flat(data: &Value) -> bool {
    level::get(data, "WorldBorder").is_none() && level::get(data, "BorderSize").is_some()
}

fn border_values(data: &Value) -> Option<Json> {
    if let Some(wb) = level::get(data, "WorldBorder") {
        return Some(json!({
            "centerX": get_double(wb, "CenterX"),
            "centerZ": get_double(wb, "CenterZ"),
            "size": get_double(wb, "Size"),
        }));
    }
    if level::get(data, "BorderSize").is_some()
        || level::get(data, "BorderCenterX").is_some()
        || level::get(data, "BorderCenterZ").is_some()
    {
        return Some(json!({
            "centerX": get_double(data, "BorderCenterX"),
            "centerZ": get_double(data, "BorderCenterZ"),
            "size": get_double(data, "BorderSize"),
        }));
    }
    None
}

/// 写回种子：只写到存档里实际存在的那个位置，不新增另一种布局
fn set_seed(map: &mut HashMap<String, Value>, seed: i64) {
    let mut wrote = false;
    if let Some(Value::Compound(w)) = map.get_mut("WorldGenSettings") {
        if w.contains_key("seed") {
            w.insert("seed".into(), Value::Long(seed));
        }
        wrote = true; // WorldGenSettings 在，就在里面写（1.16+ 权威位置）
    }
    if map.contains_key("RandomSeed") {
        map.insert("RandomSeed".into(), Value::Long(seed));
        wrote = true;
    }
    if !wrote {
        map.insert("RandomSeed".into(), Value::Long(seed));
    }
}

/// GameRules 是一组字符串值（"true"/"false"/数字字符串），原样交给前端渲染。
fn game_rules_json(v: &Value) -> Json {
    match v {
        Value::Compound(m) => {
            let mut o = Map::new();
            for (k, val) in m {
                if let Value::String(s) = val {
                    o.insert(k.clone(), json!(s));
                }
            }
            Json::Object(o)
        }
        _ => Json::Null,
    }
}

/// 把表单字段写回 level.dat。只改 patch 中出现的字段，其余原样保留。
/// 返回备份文件名。
pub fn save_world_form(
    state: &AppState,
    instance_id: &str,
    world: &str,
    patch: &Json,
) -> Result<String, String> {
    let dir = resolve_world(state, instance_id, world)?;
    let level_path = dir.join("level.dat");
    let backup_name = backup::backup_file(&level_path)?;

    let mut data = level::read_level(&level_path)?;
    apply_patch(&mut data, patch)?;
    level::write_level(&level_path, &data)?;
    Ok(backup_name)
}

/// 只修改 patch 中列出的字段；未知/未列出的字段原封不动。
/// 自动穿过 level.dat 的 `Data` 包装层（`fml` 等同级字段原样保留）。
fn apply_patch(root: &mut Value, patch: &Json) -> Result<(), String> {
    level::on_world_data(root, |data| apply_data_patch(data, patch))
}

fn apply_data_patch(data: &mut Value, patch: &Json) -> Result<(), String> {
    let map = match data {
        Value::Compound(m) => m,
        _ => return Err("level.dat 的世界数据不是 compound".into()),
    };
    let obj = patch
        .as_object()
        .ok_or_else(|| "patch 必须是对象".to_string())?;

    for (key, v) in obj {
        match key.as_str() {
            // 种子：按存档里的实际位置写（1.16+ 在 WorldGenSettings.seed，老存档在 RandomSeed）。
            // 前端传的是十进制字符串（Long 超过 JS 安全整数范围，走数字会被截断）
            "RandomSeed" => {
                let seed = match v {
                    Json::String(s) => s.trim().parse::<i64>().ok(),
                    other => other.as_i64(),
                };
                if let Some(n) = seed {
                    set_seed(map, n);
                }
            }
            // 世界边界：compound 布局改 CenterX/CenterZ/Size，扁平布局改 Border*，
            // 都只动已存在的字段，不新建另一种布局
            "border" => {
                let Some(p) = v.as_object() else { continue };
                if let Some(Value::Compound(b)) = map.get_mut("WorldBorder") {
                    if let Some(x) = p.get("centerX").and_then(|x| x.as_f64()) {
                        b.insert("CenterX".into(), Value::Double(x));
                    }
                    if let Some(z) = p.get("centerZ").and_then(|x| x.as_f64()) {
                        b.insert("CenterZ".into(), Value::Double(z));
                    }
                    if let Some(s) = p.get("size").and_then(|x| x.as_f64()) {
                        b.insert("Size".into(), Value::Double(s));
                    }
                } else {
                    for (src, dst) in [
                        ("centerX", "BorderCenterX"),
                        ("centerZ", "BorderCenterZ"),
                        ("size", "BorderSize"),
                    ] {
                        if let Some(num) = p.get(src).and_then(|x| x.as_f64()) {
                            if matches!(map.get(dst), Some(Value::Double(_))) {
                                map.insert(dst.into(), Value::Double(num));
                            }
                        }
                    }
                }
            }
            // 游戏规则：只改 patch 里出现的规则项，其余规则原样保留
            "gameRules" => {
                if let Some(Value::Compound(g)) = map.get_mut("GameRules") {
                    if let Some(p) = v.as_object() {
                        for (k, val) in p {
                            if let Some(s) = val.as_str() {
                                g.insert(k.clone(), Value::String(s.to_string()));
                            }
                        }
                    }
                }
            }
            _ => {
                if v.is_null() {
                    continue;
                }
                let tag = type_of(key);
                match tag {
                    "string" => {
                        if let Some(s) = v.as_str() {
                            map.insert(key.clone(), Value::String(s.to_string()));
                        }
                    }
                    "long" => {
                        if let Some(n) = v.as_i64() {
                            map.insert(key.clone(), Value::Long(n));
                        }
                    }
                    "int" => {
                        if let Some(n) = v.as_i64() {
                            map.insert(key.clone(), Value::Int(n as i32));
                        }
                    }
                    "byte" => {
                        if let Some(n) = v.as_i64() {
                            map.insert(key.clone(), Value::Byte(n as i8));
                        }
                    }
                    _ => {
                        // 未知字段：保持原类型，只接受数字/字符串
                        match v {
                            Json::Number(n) => {
                                if let Some(i) = n.as_i64() {
                                    map.insert(key.clone(), Value::Int(i as i32));
                                }
                            }
                            Json::String(s) => {
                                map.insert(key.clone(), Value::String(s.clone()));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// level.dat 中各字段的 NBT 类型（类型写错会导致游戏读不出值）
fn type_of(key: &str) -> &'static str {
    match key {
        "LevelName" => "string",
        "RandomSeed" | "Time" | "DayTime" => "long",
        "SpawnX" | "SpawnY" | "SpawnZ" => "int",
        "GameType" => "int",
        "Difficulty" | "DifficultyLocked" | "raining" | "thundering" | "allowCommands" | "hardcore" => {
            "byte"
        }
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastnbt::Value;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;

    /// 造一个"像真的" level.dat。
    /// 结构照抄真实存档：外层 `{ Data: {…世界数据…}, fml: {…Forge…} }`，
    /// 种子在 `WorldGenSettings.seed`，世界边界是扁平的 `Border*` 字段。
    fn make_sample_level() -> Value {
        let mut m = std::collections::HashMap::new();
        m.insert("DataVersion".into(), Value::Int(3465));
        m.insert("LevelName".into(), Value::String("旧世界名".into()));
        m.insert("GameType".into(), Value::Int(0));
        m.insert("Difficulty".into(), Value::Byte(2));
        m.insert("DifficultyLocked".into(), Value::Byte(0));
        m.insert("Time".into(), Value::Long(1000));
        m.insert("DayTime".into(), Value::Long(1000));
        m.insert("SpawnX".into(), Value::Int(12));
        m.insert("SpawnY".into(), Value::Int(64));
        m.insert("SpawnZ".into(), Value::Int(-34));
        m.insert("allowCommands".into(), Value::Byte(0));
        m.insert("raining".into(), Value::Byte(0));
        m.insert("thundering".into(), Value::Byte(0));
        m.insert("hardcore".into(), Value::Byte(0));
        // 扁平的世界边界（真实存档就是这几个字段）
        m.insert("BorderCenterX".into(), Value::Double(0.0));
        m.insert("BorderCenterZ".into(), Value::Double(0.0));
        m.insert("BorderSize".into(), Value::Double(59999968.0));
        m.insert("BorderSizeLerpTime".into(), Value::Long(0));
        m.insert("BorderSafeZone".into(), Value::Double(5.0));
        // 游戏规则
        let mut rules = std::collections::HashMap::new();
        rules.insert("keepInventory".into(), Value::String("false".into()));
        rules.insert("randomTickSpeed".into(), Value::String("3".into()));
        rules.insert("doDaylightCycle".into(), Value::String("true".into()));
        m.insert("GameRules".into(), Value::Compound(rules));
        // 表单不覆盖的大块字段——写回后必须还在
        let mut wgs = std::collections::HashMap::new();
        wgs.insert("seed".into(), Value::Long(-1234567890));
        wgs.insert("generate_features".into(), Value::Byte(1));
        let mut dim = std::collections::HashMap::new();
        dim.insert("type".into(), Value::String("minecraft:overworld".into()));
        wgs.insert("dimensions".into(), Value::Compound(dim));
        m.insert("WorldGenSettings".into(), Value::Compound(wgs));

        let mut fml = std::collections::HashMap::new();
        fml.insert("LoadingModList".into(), Value::String("fake".into()));
        let mut wrapper = std::collections::HashMap::new();
        wrapper.insert("Data".into(), Value::Compound(m));
        wrapper.insert("fml".into(), Value::Compound(fml));
        Value::Compound(wrapper)
    }

    fn write_sample(dir: &Path, data: &Value) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join("level.dat");
        let raw = fastnbt::to_bytes_with_opts(data, fastnbt::SerOpts::new())
            .expect("serialize");
        let mut enc = GzEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&raw).unwrap();
        let bytes = enc.finish().unwrap();
        std::fs::write(&p, bytes).unwrap();
        p
    }

    /// 核心安全用例：改几个表单字段后写回，未涉及的字段必须原封不动
    #[test]
    fn roundtrip_keeps_untouched_fields() {
        let tmp = std::env::temp_dir().join(format!("qookix-nbt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let path = write_sample(&tmp, &make_sample_level());

        // 读 → 只改表单里几个字段
        let mut root = level::read_level(&path).expect("读取 level.dat");
        let patch = json!({
            "LevelName": "新世界名",
            "GameType": 1,
            "Difficulty": 3,
            "RandomSeed": 4242,
        });
        apply_patch(&mut root, &patch).expect("应用修改");
        level::write_level(&path, &root).expect("写回");

        // 再读，逐项校验
        let after = level::read_level(&path).expect("二次读取");
        assert!(level::is_wrapped(&after), "Data 包装层不能丢");
        assert!(level::get(&after, "fml").is_some(), "Forge 的 fml 不能丢");
        let d = level::world_data(&after);
        assert_eq!(get_string(d, "LevelName").as_deref(), Some("新世界名"));
        assert_eq!(get_int(d, "GameType"), Some(1));
        assert_eq!(get_int(d, "Difficulty"), Some(3));
        // 种子写到 WorldGenSettings.seed（1.16+ 的权威位置）
        assert_eq!(world_seed(d), Some(4242));
        // 未改字段必须原样保留
        assert_eq!(get_int(d, "DataVersion"), Some(3465), "DataVersion 不该变");
        assert_eq!(get_int(d, "SpawnX"), Some(12));
        assert_eq!(get_int(d, "SpawnY"), Some(64));
        assert_eq!(get_int(d, "SpawnZ"), Some(-34));
        assert_eq!(get_int(d, "allowCommands"), Some(0));
        // 大块嵌套字段必须还在
        let wgs = level::get(d, "WorldGenSettings").expect("WorldGenSettings 丢失会导致世界损坏");
        assert!(level::get(wgs, "dimensions").is_some());
        // 没被 patch 的边界字段保持原样
        assert_eq!(get_double(d, "BorderSize"), Some(59999968.0));
        assert_eq!(get_double(d, "BorderSafeZone"), Some(5.0));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 表单读取：要穿过 Data 包装层，种子从 WorldGenSettings.seed 取
    #[test]
    fn form_reads_through_wrapper() {
        let tmp = std::env::temp_dir().join(format!("qookix-nbt-f-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let path = write_sample(&tmp, &make_sample_level());

        let f = world_form(&path).unwrap();
        assert_eq!(f.get("levelName").and_then(|x| x.as_str()), Some("旧世界名"));
        assert_eq!(f.get("gameType").and_then(|x| x.as_i64()), Some(0));
        // 种子以十进制字符串下发（Long 超过 JS 安全整数范围）
        assert_eq!(f.get("seed").and_then(|x| x.as_str()), Some("-1234567890"));
        assert_eq!(f.get("borderFlat").and_then(|x| x.as_bool()), Some(true));
        assert_eq!(
            f.get("border")
                .and_then(|b| b.get("size"))
                .and_then(|x| x.as_f64()),
            Some(59999968.0)
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 没有 Data 包装层的老格式文件也要能读写
    #[test]
    fn unwrapped_level_still_supported() {
        let tmp = std::env::temp_dir().join(format!("qookix-nbt-u-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let mut m = std::collections::HashMap::new();
        m.insert("LevelName".into(), Value::String("裸根".into()));
        m.insert("RandomSeed".into(), Value::Long(7));
        let path = write_sample(&tmp, &Value::Compound(m));

        let mut root = level::read_level(&path).unwrap();
        assert!(!level::is_wrapped(&root));
        apply_patch(&mut root, &json!({ "LevelName": "改过" })).unwrap();
        level::write_level(&path, &root).unwrap();

        let after = level::read_level(&path).unwrap();
        assert_eq!(get_string(&after, "LevelName").as_deref(), Some("改过"));
        // 老布局只有 RandomSeed，就写进 RandomSeed
        assert_eq!(get_int(&after, "RandomSeed"), Some(7));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 边界：扁平 Border* 布局（真实存档用的就是这套），只改给定字段
    #[test]
    fn border_patch_flat_layout() {
        let tmp = std::env::temp_dir().join(format!("qookix-nbt-b-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let path = write_sample(&tmp, &make_sample_level());

        let mut root = level::read_level(&path).unwrap();
        let patch = json!({ "border": { "centerX": 100.0, "centerZ": -200.0, "size": 1000.0 } });
        apply_patch(&mut root, &patch).unwrap();
        level::write_level(&path, &root).unwrap();

        let after = level::read_level(&path).unwrap();
        let d = level::world_data(&after);
        assert_eq!(get_double(d, "BorderCenterX"), Some(100.0));
        assert_eq!(get_double(d, "BorderCenterZ"), Some(-200.0));
        assert_eq!(get_double(d, "BorderSize"), Some(1000.0));
        assert_eq!(get_double(d, "BorderSafeZone"), Some(5.0), "未给的字段要保留");
        assert_eq!(get_int(d, "BorderSizeLerpTime"), Some(0));
        // 不该凭空造出 WorldBorder 子节点
        assert!(level::get(d, "WorldBorder").is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 边界：WorldBorder 子 compound 布局（另一部分版本用这种）
    #[test]
    fn border_patch_compound_layout() {
        let tmp = std::env::temp_dir().join(format!("qookix-nbt-bc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let mut border = std::collections::HashMap::new();
        border.insert("CenterX".into(), Value::Double(0.0));
        border.insert("CenterZ".into(), Value::Double(0.0));
        border.insert("Size".into(), Value::Double(500.0));
        border.insert("SafeZone".into(), Value::Double(5.0));
        let mut inner = std::collections::HashMap::new();
        inner.insert("WorldBorder".into(), Value::Compound(border));
        let mut wrapper = std::collections::HashMap::new();
        wrapper.insert("Data".into(), Value::Compound(inner));
        let path = write_sample(&tmp, &Value::Compound(wrapper));

        let mut root = level::read_level(&path).unwrap();
        apply_patch(&mut root, &json!({ "border": { "size": 800.0 } })).unwrap();
        level::write_level(&path, &root).unwrap();

        let after = level::read_level(&path).unwrap();
        let d = level::world_data(&after);
        let b = level::get(d, "WorldBorder").unwrap();
        assert_eq!(get_double(b, "Size"), Some(800.0));
        assert_eq!(get_double(b, "SafeZone"), Some(5.0), "未给的子字段要保留");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 游戏规则：只改提交的规则项，其余规则原样保留；hardcore 必须是 Byte
    #[test]
    fn gamerules_patch_only_touches_given_keys() {
        let tmp = std::env::temp_dir().join(format!("qookix-nbt-r-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let path = write_sample(&tmp, &make_sample_level());

        let mut data = level::read_level(&path).unwrap();
        let patch = json!({
            "hardcore": 1,
            "gameRules": { "keepInventory": "true", "randomTickSpeed": "30" },
        });
        apply_patch(&mut data, &patch).unwrap();
        level::write_level(&path, &data).unwrap();

        let after = level::read_level(&path).unwrap();
        let d = level::world_data(&after);
        match level::get(d, "hardcore") {
            Some(Value::Byte(1)) => {}
            other => panic!("hardcore 必须是 Byte(1)，实际是 {other:?}"),
        }
        let rules = level::get(d, "GameRules").expect("GameRules 不能丢");
        assert_eq!(
            level::get_string(rules, "keepInventory").as_deref(),
            Some("true")
        );
        assert_eq!(level::get_string(rules, "randomTickSpeed").as_deref(), Some("30"));
        // 未提交的规则保持原值
        assert_eq!(
            level::get_string(rules, "doDaylightCycle").as_deref(),
            Some("true")
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// 符号链接的存档要"写穿"链接：内容进真实文件，链接本身保留
    /// （玩家的存档目录里确实有链到别处的 level.dat）
    #[cfg(windows)]
    #[test]
    fn write_through_symlink_is_preserved() {
        let dir = std::env::temp_dir().join(format!("qookix-nbt-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let real = dir.join("real.dat");
        let link = dir.join("level.dat");
        level::write_level(&real, &Value::Compound(std::collections::HashMap::new())).unwrap();
        if std::os::windows::fs::symlink_file(&real, &link).is_err() {
            // 没开开发者模式 / 无权限建符号链接 → 跳过
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }
        let mut m = std::collections::HashMap::new();
        m.insert("LevelName".into(), Value::String("写穿链接".into()));
        level::write_level(&link, &Value::Compound(m)).unwrap();

        assert!(
            std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(),
            "符号链接不能被替换成普通文件"
        );
        let after = level::read_level(&link).unwrap();
        assert_eq!(get_string(&after, "LevelName").as_deref(), Some("写穿链接"));
        // 通过真实路径读也是新内容
        let via_real = level::read_level(&real).unwrap();
        assert_eq!(get_string(&via_real, "LevelName").as_deref(), Some("写穿链接"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 字节级：纯往返（不改任何字段）应保持一致
    #[test]
    fn roundtrip_is_byte_stable() {
        let tmp = std::env::temp_dir().join(format!("qookix-nbt-c-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let sample = make_sample_level();
        let p1 = write_sample(&tmp, &sample);
        let data = level::read_level(&p1).unwrap();
        let p2 = tmp.join("level2.dat");
        level::write_level(&p2, &data).unwrap();
        let a = std::fs::read(&p1).unwrap();
        let b = std::fs::read(&p2).unwrap();
        // gzip 头含时间戳，字节不一定相等；退而校验解析后结构完全一致
        let d1 = level::read_level(&p1).unwrap();
        let d2 = level::read_level(&p2).unwrap();
        assert_eq!(
            serde_json::to_string(&to_tree_json(&d1)).unwrap(),
            serde_json::to_string(&to_tree_json(&d2)).unwrap(),
            "纯往返后结构必须一致"
        );
        assert!(!a.is_empty() && !b.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    fn to_tree_json(v: &Value) -> Json {
        match v {
            Value::Compound(m) => {
                let o: serde_json::Map<String, Json> =
                    m.iter().map(|(k, val)| (k.clone(), to_tree_json(val))).collect();
                Json::Object(o)
            }
            Value::List(items) => Json::Array(items.iter().map(to_tree_json).collect()),
            other => match other {
                Value::Byte(b) => json!(b),
                Value::Short(s) => json!(s),
                Value::Int(i) => json!(i),
                Value::Long(l) => json!(l),
                Value::Float(f) => json!(f),
                Value::Double(d) => json!(d),
                Value::String(s) => json!(s),
                Value::ByteArray(a) => json!(a.len()),
                Value::IntArray(a) => json!(a.len()),
                Value::LongArray(a) => json!(a.len()),
                Value::List(items) => Json::Array(items.iter().map(to_tree_json).collect()),
                Value::Compound(_) => unreachable!("上面已匹配"),
            },
        }
    }
}
