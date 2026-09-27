//! 玩家数据（playerdata/<uuid>.dat）与背包读写。
//!
//! 玩家名优先用实例根目录的 `usercache.json` 本地解析（离线可用），
//! 解析不到时回退显示 UUID 短串（不联网，避免国内访问 Mojang 超时）。

use super::level;
use super::level::as_list;
use crate::state::AppState;
use fastnbt::Value;
use serde_json::{json, Value as Json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// playerdata 目录
pub fn playerdata_dir(state: &AppState, instance_id: &str, world: &str) -> Result<PathBuf, String> {
    Ok(super::resolve_world(state, instance_id, world)?.join("playerdata"))
}

/// 读实例根目录下的 `usercache.json`：UUID（小写）→ 玩家名
pub fn name_cache_at(instance_root: &Path) -> HashMap<String, String> {
    let Ok(text) = std::fs::read_to_string(instance_root.join("usercache.json")) else {
        return HashMap::new();
    };
    let Ok(list) = serde_json::from_str::<Vec<Json>>(&text) else {
        return HashMap::new();
    };
    let mut m = HashMap::new();
    for e in list {
        if let (Some(uuid), Some(name)) = (
            e.get("uuid").and_then(|x| x.as_str()),
            e.get("name").and_then(|x| x.as_str()),
        ) {
            m.insert(uuid.to_lowercase(), name.to_string());
        }
    }
    m
}

/// 读取 usercache.json：UUID（小写）→ 玩家名。
/// 依次尝试实例根目录、世界目录的祖父目录（手动指定的世界多半也在 `<实例>/saves/` 下）。
pub fn name_cache(state: &AppState, instance_id: &str, world_dir: &Path) -> HashMap<String, String> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if !instance_id.is_empty() {
        candidates.push(state.instances_dir().join(instance_id));
    }
    if let Some(instance_root) = world_dir.parent().and_then(|saves| saves.parent()) {
        candidates.push(instance_root.to_path_buf());
    }
    for root in candidates {
        let m = name_cache_at(&root);
        if !m.is_empty() {
            return m;
        }
    }
    HashMap::new()
}

/// 玩家显示名：缓存里没有就退化成 UUID 前 8 位
pub fn display_name(cache: &HashMap<String, String>, uuid: &str) -> String {
    cache.get(&uuid.to_lowercase()).cloned().unwrap_or_else(|| {
        if uuid.len() >= 8 {
            format!("{}…", &uuid[..8])
        } else {
            uuid.to_string()
        }
    })
}

/// 世界卡片用：玩家数据文件数量 + 第一个玩家名（名字查 `cache`）
pub fn first_player_with(
    cache: &HashMap<String, String>,
    world_dir: &Path,
) -> (usize, Option<String>) {
    let mut count = 0usize;
    let mut first: Option<String> = None;
    if let Ok(rd) = std::fs::read_dir(world_dir.join("playerdata")) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("dat") {
                continue;
            }
            count += 1;
            if first.is_none() {
                let uuid = p
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                if !uuid.is_empty() {
                    first = Some(display_name(cache, &uuid));
                }
            }
        }
    }
    (count, first)
}

/// 列出世界下所有玩家（playerdata/*.dat）
pub fn list_players(state: &AppState, instance_id: &str, world: &str) -> Result<Vec<Json>, String> {
    let dir = playerdata_dir(state, instance_id, world)?;
    let cache = name_cache(state, instance_id, &dir);
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) != Some("dat") {
                continue;
            }
            let uuid = p
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let display = display_name(&cache, &uuid);
            // 顺带读一下坐标和维度，卡片上直接显示
            let (pos, dim, health) = match level::read_level(&p) {
                Ok(d) => {
                    let pos = level::get(&d, "Pos")
                        .and_then(as_list)
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| match x {
                                    Value::Double(f) => Some(*f),
                                    Value::Int(i) => Some(*i as f64),
                                    _ => None,
                                })
                                .collect::<Vec<f64>>()
                        })
                        .unwrap_or_default();
                    let dim = level::get_string(&d, "Dimension")
                        .or_else(|| level::get_int(&d, "Dimension").map(|i| i.to_string()));
                    let health = level::get_double(&d, "Health")
                        .or_else(|| level::get_int(&d, "Health").map(|i| i as f64));
                    (pos, dim, health)
                }
                Err(_) => (Vec::new(), None, None),
            };
            out.push(json!({
                "uuid": uuid,
                "name": display,
                "pos": pos,
                "dimension": dim,
                "health": health,
            }));
        }
    }
    out.sort_by(|a, b| {
        a.get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .cmp(b.get("name").and_then(|x| x.as_str()).unwrap_or(""))
    });
    Ok(out)
}

fn pos_of(d: &Value) -> Vec<f64> {
    level::get(d, "Pos")
        .and_then(as_list)
        .map(|a| {
            a.iter()
                .filter_map(|x| match x {
                    Value::Double(f) => Some(*f),
                    Value::Int(i) => Some(*i as f64),
                    _ => None,
                })
                .collect::<Vec<f64>>()
        })
        .unwrap_or_default()
}

/// 读一个玩家的完整可编辑字段
pub fn read_player(
    state: &AppState,
    instance_id: &str,
    world: &str,
    uuid: &str,
) -> Result<Json, String> {
    let p = playerdata_dir(state, instance_id, world)?.join(format!("{uuid}.dat"));
    let d = level::read_level(&p)?;
    let abilities = level::get(&d, "abilities").cloned();
    Ok(json!({
        "pos": pos_of(&d),
        "dimension": level::get_string(&d, "Dimension")
            .or_else(|| level::get_int(&d, "Dimension").map(|i| i.to_string()))
            .unwrap_or_else(|| "minecraft:overworld".into()),
        "health": level::get_double(&d, "Health").unwrap_or(20.0),
        "foodLevel": level::get_int(&d, "foodLevel").unwrap_or(20),
        "xpLevel": level::get_int(&d, "XpLevel").unwrap_or(0),
        "xpP": level::get_double(&d, "XpP").unwrap_or(0.0),
        "gameType": level::get_int(&d, "playerGameType").unwrap_or(0),
        "abilities": abilities.map(|a| json!({
            "flying": level::get_int(&a, "flying").unwrap_or(0),
            "mayfly": level::get_int(&a, "mayfly").unwrap_or(0),
        })),
    }))
}

/// 保存玩家字段修改（自动备份）。只改 patch 中出现的字段。
pub fn save_player(
    state: &AppState,
    instance_id: &str,
    world: &str,
    uuid: &str,
    patch: &Json,
) -> Result<String, String> {
    let dir = playerdata_dir(state, instance_id, world)?;
    let p = dir.join(format!("{uuid}.dat"));
    let backup = super::backup::backup_file(&p)?;
    let mut d = level::read_level(&p)?;
    let map = match &mut d {
        Value::Compound(m) => m,
        _ => return Err("玩家数据根节点不是 compound".into()),
    };
    if let Some(pos) = patch.get("pos").and_then(|x| x.as_array()) {
        let arr: Vec<Value> = pos
            .iter()
            .map(|v| Value::Double(v.as_f64().unwrap_or(0.0)))
            .collect();
        map.insert("Pos".into(), Value::List(arr));
    }
    if let Some(dim) = patch.get("dimension").and_then(|x| x.as_str()) {
        map.insert("Dimension".into(), Value::String(dim.to_string()));
    }
    if let Some(h) = patch.get("health").and_then(|x| x.as_f64()) {
        map.insert("Health".into(), Value::Float(h as f32));
    }
    if let Some(f) = patch.get("foodLevel").and_then(|x| x.as_i64()) {
        map.insert("foodLevel".into(), Value::Int(f as i32));
    }
    if let Some(l) = patch.get("xpLevel").and_then(|x| x.as_i64()) {
        map.insert("XpLevel".into(), Value::Int(l as i32));
    }
    if let Some(x) = patch.get("xpP").and_then(|x| x.as_f64()) {
        map.insert("XpP".into(), Value::Float(x as f32));
    }
    if let Some(g) = patch.get("gameType").and_then(|x| x.as_i64()) {
        map.insert("playerGameType".into(), Value::Int(g as i32));
    }
    if let Some(ab) = patch.get("abilities") {
        // abilities 缺失时补一个空 compound（旧存档 / 从未飞过的玩家）
        let ab_map = match map.get_mut("abilities") {
            Some(Value::Compound(m)) => m,
            _ => {
                map.insert("abilities".into(), level::empty_compound());
                match map.get_mut("abilities") {
                    Some(Value::Compound(m)) => m,
                    _ => return Err("abilities 写入失败".into()),
                }
            }
        };
        if let Some(f) = ab.get("flying").and_then(|x| x.as_i64()) {
            ab_map.insert("flying".into(), Value::Byte(f as i8));
        }
        if let Some(m) = ab.get("mayfly").and_then(|x| x.as_i64()) {
            ab_map.insert("mayfly".into(), Value::Byte(m as i8));
        }
    }
    level::write_level(&p, &d)?;
    Ok(backup)
}
