//! 物品 NBT 操作：背包读取、槽位写回。
//!
//! 安全原则（与 level.dat 一致）：**读原始物品 → 只改前端提交的字段 → 写回**。
//! 前端只认识 id / 数量 / 名称 / Lore / 附魔 / 属性修饰符 / 无限耐久这几项，
//! 物品上还有 Damage、CustomModelData、SkullOwner、BlockEntityTag、HideFlags…
//! 这些未展示的标签必须原样保留，否则一保存等于把物品洗成白板。
//!
//! 同时兼容两种物品格式：
//! - 旧格式（≤ 1.20.4，DataVersion < 3837）：`tag{ display{Name,Lore}, Enchantments, Unbreakable, AttributeModifiers }`，`Count` / `Slot`
//! - 新格式（≥ 1.20.5）：`components{ minecraft:custom_name, minecraft:lore, minecraft:enchantments, … }`，`count` / `slot`

use crate::nbt::level;
use crate::state::AppState;
use fastnbt::Value;
use serde_json::{json, Value as Json};
use std::collections::HashMap;
use std::path::PathBuf;

/// 1.20.5（24w09a）起物品由 `tag` 改为 `components`
const DATA_VERSION_COMPONENTS: i64 = 3837;

const CUSTOM_NAME: &str = "minecraft:custom_name";
const LORE: &str = "minecraft:lore";
const UNBREAKABLE: &str = "minecraft:unbreakable";
const ENCHANTMENTS: &str = "minecraft:enchantments";
const ATTR_MODIFIERS: &str = "minecraft:attribute_modifiers";

fn playerdata_dir(state: &AppState, instance_id: &str, world: &str) -> Result<PathBuf, String> {
    super::player::playerdata_dir(state, instance_id, world)
}

// ------------------------------------------------------------- 文本组件 ----

/// 旧版 `display.Name` / `Lore` 存的是 JSON 文本组件字符串，这里还原成纯文本
fn parse_text_component(s: &str) -> String {
    let t = s.trim();
    if t.starts_with('{') || t.starts_with('[') || t.starts_with('"') {
        if let Ok(v) = serde_json::from_str::<Json>(t) {
            if let Some(text) = json_text(&v) {
                return text;
            }
        }
    }
    s.to_string()
}

fn json_text(v: &Json) -> Option<String> {
    match v {
        Json::String(s) => Some(s.clone()),
        Json::Array(a) => Some(a.iter().filter_map(json_text).collect::<Vec<_>>().join("")),
        Json::Object(o) => {
            let mut out = o
                .get("text")
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .to_string();
            if let Some(extra) = o.get("extra") {
                out.push_str(&json_text(extra).unwrap_or_default());
            }
            Some(out)
        }
        _ => None,
    }
}

/// NBT 文本组件 → 纯文本；`legacy` 时字符串还要按 JSON 解析一次
fn component_text(v: &Value, legacy: bool) -> Option<String> {
    match v {
        Value::String(s) => Some(if legacy {
            parse_text_component(s)
        } else {
            s.clone()
        }),
        Value::List(a) => Some(
            a.iter()
                .filter_map(|x| component_text(x, legacy))
                .collect::<Vec<_>>()
                .join(""),
        ),
        Value::Compound(m) => {
            let mut out = m
                .get("text")
                .and_then(|x| match x {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                })
                .unwrap_or_default();
            if let Some(extra) = m.get("extra") {
                out.push_str(&component_text(extra, legacy).unwrap_or_default());
            }
            Some(out)
        }
        _ => None,
    }
}

/// 纯文本 → 旧格式需要的 JSON 文本组件字符串（`"名字"`）
fn to_json_component(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_string())
}

// ------------------------------------------------------------- 读取 ----

fn list_of<'a>(v: &'a Value, key: &str) -> Option<&'a Vec<Value>> {
    level::get(v, key).and_then(level::as_list)
}

/// 新格式物品（有 components）还是旧格式（有 tag）
fn item_is_modern(item: &Value, data_version: i64) -> bool {
    if level::get(item, "components").is_some() {
        true
    } else if level::get(item, "tag").is_some() {
        false
    } else {
        data_version >= DATA_VERSION_COMPONENTS
    }
}

fn op_from_str(s: &str) -> i64 {
    match s {
        "add_multiplied_base" => 1,
        "add_multiplied_total" => 2,
        _ => 0,
    }
}

fn op_to_str(n: i64) -> &'static str {
    match n {
        1 => "add_multiplied_base",
        2 => "add_multiplied_total",
        _ => "add_value",
    }
}

/// 读背包（Inventory + 末影箱）。只返回实际存在的格子，空位由前端补。
pub fn read_inventory(
    state: &AppState,
    instance_id: &str,
    world: &str,
    uuid: &str,
) -> Result<Json, String> {
    let p = playerdata_dir(state, instance_id, world)?.join(format!("{uuid}.dat"));
    let d = level::read_level(&p)?;
    let data_version = level::get_int(&d, "DataVersion").unwrap_or(0);
    let to_slots = |key: &str| -> Vec<Json> {
        level::get(&d, key)
            .and_then(level::as_list)
            .map(|a| {
                a.iter()
                    .enumerate()
                    .map(|(i, item)| slot_json(i, item, data_version))
                    .collect()
            })
            .unwrap_or_default()
    };
    Ok(json!({
        "inventory": to_slots("Inventory"),
        "enderChest": to_slots("EnderItems"),
    }))
}

fn slot_json(index: usize, item: &Value, data_version: i64) -> Json {
    let id = level::get_string(item, "id").unwrap_or_default();
    let count = level::get_int(item, "Count")
        .or_else(|| level::get_int(item, "count"))
        .unwrap_or(1);
    let slot = level::get_int(item, "Slot")
        .or_else(|| level::get_int(item, "slot"))
        .unwrap_or(index as i64);
    let modern = item_is_modern(item, data_version);
    let container = level::get(item, if modern { "components" } else { "tag" });

    let (name, lore, unbreakable, enchantments, modifiers) = match container {
        Some(c) if modern => modern_fields(c),
        Some(c) => legacy_fields(c),
        None => (None, Vec::new(), 0, Vec::new(), Vec::new()),
    };

    json!({
        "index": index,
        "slot": slot,
        "id": id,
        "count": count,
        "name": name,
        "lore": lore,
        "unbreakable": unbreakable,
        "enchantments": enchantments,
        "modifiers": modifiers,
        "modern": modern,
    })
}

/// 旧格式（tag 容器）里的展示字段
fn legacy_fields(
    tag: &Value,
) -> (Option<String>, Vec<String>, i64, Vec<Json>, Vec<Json>) {
    let display = level::get(tag, "display");
    let name = display
        .and_then(|d| level::get(d, "Name"))
        .and_then(|v| component_text(v, true));
    let lore = display
        .and_then(|d| level::get(d, "Lore"))
        .and_then(level::as_list)
        .map(|a| {
            a.iter()
                .filter_map(|x| component_text(x, true))
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();
    let unbreakable = if level::get_int(tag, "Unbreakable").unwrap_or(0) != 0 {
        1
    } else {
        0
    };
    // 附魔：1.13+ 用 Enchantments（id 为字符串），旧版用 ench（数字 id）
    let ench_key = if level::get(tag, "Enchantments").is_some() {
        "Enchantments"
    } else {
        "ench"
    };
    let enchantments = list_of(tag, ench_key)
        .map(|a| {
            a.iter()
                .map(|e| {
                    json!({
                        "id": level::get_string(e, "id").unwrap_or_default(),
                        "level": level::get_int(e, "lvl").unwrap_or(1),
                    })
                })
                .collect::<Vec<Json>>()
        })
        .unwrap_or_default();
    let modifiers = list_of(tag, "AttributeModifiers")
        .map(|a| {
            a.iter()
                .map(|m| {
                    json!({
                        "name": level::get_string(m, "Name").unwrap_or_default(),
                        "attribute": level::get_string(m, "AttributeName").unwrap_or_default(),
                        "amount": level::get_double(m, "Amount").unwrap_or(0.0),
                        "operation": level::get_int(m, "Operation").unwrap_or(0),
                        "slot": level::get_string(m, "Slot"),
                    })
                })
                .collect::<Vec<Json>>()
        })
        .unwrap_or_default();
    (name, lore, unbreakable, enchantments, modifiers)
}

/// 新格式（components 容器）里的展示字段
fn modern_fields(
    c: &Value,
) -> (Option<String>, Vec<String>, i64, Vec<Json>, Vec<Json>) {
    let name = level::get(c, CUSTOM_NAME).and_then(|v| component_text(v, false));
    let lore = level::get(c, LORE)
        .and_then(level::as_list)
        .map(|a| {
            a.iter()
                .filter_map(|x| component_text(x, false))
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();
    let unbreakable = if level::get(c, UNBREAKABLE).is_some() { 1 } else { 0 };
    let enchantments = level::get(c, ENCHANTMENTS)
        .and_then(|e| level::get(e, "levels"))
        .and_then(|l| match l {
            Value::Compound(m) => Some(
                m.iter()
                    .map(|(k, v)| {
                        let level = match v {
                            Value::Int(i) => *i as i64,
                            Value::Short(s) => *s as i64,
                            Value::Byte(b) => *b as i64,
                            _ => 1,
                        };
                        json!({ "id": k, "level": level })
                    })
                    .collect::<Vec<Json>>(),
            ),
            _ => None,
        })
        .unwrap_or_default();
    let modifiers = level::get(c, ATTR_MODIFIERS)
        .and_then(|m| level::get(m, "modifiers"))
        .and_then(level::as_list)
        .map(|a| {
            a.iter()
                .map(|m| {
                    let op = level::get_string(m, "operation")
                        .map(|s| op_from_str(&s))
                        .or_else(|| level::get_int(m, "operation"))
                        .unwrap_or(0);
                    json!({
                        "name": level::get_string(m, "id").unwrap_or_default(),
                        "attribute": level::get_string(m, "type").unwrap_or_default(),
                        "amount": level::get_double(m, "amount").unwrap_or(0.0),
                        "operation": op,
                        "slot": level::get_string(m, "slot"),
                    })
                })
                .collect::<Vec<Json>>()
        })
        .unwrap_or_default();
    (name, lore, unbreakable, enchantments, modifiers)
}

// ------------------------------------------------------------- 写入 ----

/// 写回一个物品槽位。传入的是前端完整的槽位状态（含未改动字段），
/// 后端据此只覆盖已知字段，其余标签保持原样。
pub fn save_item(
    state: &AppState,
    instance_id: &str,
    world: &str,
    uuid: &str,
    container: &str,
    index: usize,
    item: &Json,
) -> Result<String, String> {
    let key = if container == "enderChest" {
        "EnderItems"
    } else {
        "Inventory"
    };
    let p = playerdata_dir(state, instance_id, world)?.join(format!("{uuid}.dat"));
    let backup = crate::nbt::backup::backup_file(&p)?;
    let mut d = level::read_level(&p)?;
    let data_version = level::get_int(&d, "DataVersion").unwrap_or(0);
    let map = match &mut d {
        Value::Compound(m) => m,
        _ => return Err("玩家数据根节点不是 compound".into()),
    };

    let slot_no = item
        .get("slot")
        .and_then(|x| x.as_i64())
        .unwrap_or(index as i64);

    let arr = match map.entry(key.to_string()).or_insert_with(|| Value::List(Vec::new())) {
        Value::List(a) => a,
        _ => return Err(format!("{key} 不是列表")),
    };

    // 先按 Slot 字段定位（列表顺序不一定等于槽位顺序）；
    // 只有列表里连 Slot 字段都没有的条目，才按下标兜底，
    // 避免"往空槽粘贴"误覆盖列表第 index 个已有物品。
    let position = arr
        .iter()
        .position(|it| slot_of(it) == Some(slot_no))
        .or_else(|| {
            arr.get(index)
                .filter(|it| slot_of(it).is_none())
                .map(|_| index)
        });

    let existing = position.map(|i| arr[i].clone());
    let modern = existing
        .as_ref()
        .map(|e| item_is_modern(e, data_version))
        .unwrap_or(data_version >= DATA_VERSION_COMPONENTS);
    let new_item = build_item(item, existing.as_ref(), modern, slot_no)?;

    match position {
        Some(i) => arr[i] = new_item,
        None => arr.push(new_item),
    }
    level::write_level(&p, &d)?;
    Ok(backup)
}

fn get_arr<'a>(v: &'a Json, key: &str) -> &'a [Json] {
    v.get(key).and_then(|x| x.as_array()).map(|a| a.as_slice()).unwrap_or(&[])
}

/// 物品所在的槽位号（新旧格式的 key 不同）
fn slot_of(item: &Value) -> Option<i64> {
    level::get_int(item, "Slot").or_else(|| level::get_int(item, "slot"))
}

/// 在前端槽位状态的基础上重建物品，保留该物品原有的其它标签
fn build_item(
    patch: &Json,
    existing: Option<&Value>,
    modern: bool,
    slot_no: i64,
) -> Result<Value, String> {
    let mut out = existing.cloned().unwrap_or_else(level::empty_compound);
    let map = match &mut out {
        Value::Compound(m) => m,
        _ => return Err("该槽位不是 compound".into()),
    };

    // id（粘贴到空槽时必须提供）
    let id = patch.get("id").and_then(|x| x.as_str()).unwrap_or("");
    if existing.is_none() && id.is_empty() {
        return Err("物品 ID 不能为空".into());
    }
    if !id.is_empty() {
        map.insert("id".into(), Value::String(id.to_string()));
    }

    // 槽位号：保持原 key 与类型
    let slot_key = if modern { "slot" } else { "Slot" };
    map.insert(slot_key.into(), Value::Int(slot_no as i32));

    // 数量：保留原有 key / 类型（≤1.20.4 的 Count 是 Byte）
    if let Some(c) = patch.get("count").and_then(|x| x.as_i64()) {
        let count_key = if map.contains_key("Count") {
            "Count"
        } else if map.contains_key("count") || modern {
            "count"
        } else {
            "Count"
        };
        let value = match map.get(count_key) {
            Some(Value::Int(_)) => Value::Int(c as i32),
            Some(Value::Long(_)) => Value::Long(c),
            _ => {
                if modern {
                    Value::Int(c as i32)
                } else {
                    Value::Byte(c as i8)
                }
            }
        };
        map.insert(count_key.into(), value);
    }

    let container_key = if modern { "components" } else { "tag" };
    let wants_container = !get_arr(patch, "lore").is_empty()
        || !get_arr(patch, "enchantments").is_empty()
        || !get_arr(patch, "modifiers").is_empty()
        || patch
            .get("name")
            .and_then(|x| x.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false)
        || patch.get("unbreakable").and_then(|x| x.as_i64()).unwrap_or(0) != 0;
    if wants_container && !matches!(map.get(container_key), Some(Value::Compound(_))) {
        map.insert(container_key.into(), level::empty_compound());
    }
    if let Some(Value::Compound(c)) = map.get_mut(container_key) {
        if modern {
            write_modern(c, patch)?;
        } else {
            write_legacy(c, patch)?;
        }
    }
    Ok(out)
}

/// 旧格式：写回 tag 容器
fn write_legacy(tag: &mut HashMap<String, Value>, patch: &Json) -> Result<(), String> {
    // display.Name / display.Lore
    let name = patch.get("name").and_then(|x| x.as_str()).unwrap_or("");
    let lore = get_arr(patch, "lore");
    if !tag.contains_key("display") && (!name.is_empty() || !lore.is_empty()) {
        tag.insert("display".into(), level::empty_compound());
    }
    let mut display_left_empty = false;
    if let Some(Value::Compound(display)) = tag.get_mut("display") {
        if name.is_empty() {
            display.remove("Name");
        } else {
            display.insert("Name".into(), Value::String(to_json_component(name)));
        }
        if lore.is_empty() {
            display.remove("Lore");
        } else {
            let list = lore
                .iter()
                .map(|l| Value::String(to_json_component(l.as_str().unwrap_or_default())))
                .collect();
            display.insert("Lore".into(), Value::List(list));
        }
        display_left_empty = display.is_empty();
    }
    if display_left_empty {
        tag.remove("display");
    }

    // 无限耐久
    if patch.get("unbreakable").and_then(|x| x.as_i64()).unwrap_or(0) != 0 {
        tag.insert("Unbreakable".into(), Value::Byte(1));
    } else {
        tag.remove("Unbreakable");
    }

    // 附魔：按位置合并，保留原有 id/lvl 类型（老版本是数字 id / Short）
    let incoming = get_arr(patch, "enchantments");
    let ench_key = if tag.contains_key("ench") { "ench" } else { "Enchantments" };
    let existing: Vec<Value> = tag
        .get(ench_key)
        .and_then(level::as_list)
        .cloned()
        .unwrap_or_default();
    if incoming.is_empty() {
        tag.remove(ench_key);
    } else {
        let mut out = Vec::with_capacity(incoming.len());
        for (i, e) in incoming.iter().enumerate() {
            let id = e.get("id").and_then(|x| x.as_str()).unwrap_or("");
            let lvl = e.get("level").and_then(|x| x.as_i64()).unwrap_or(1);
            let mut entry = existing.get(i).cloned().unwrap_or_else(level::empty_compound);
            if let Value::Compound(m) = &mut entry {
                let numeric_id = matches!(m.get("id"), Some(Value::Short(_)) | Some(Value::Int(_)));
                if numeric_id {
                    if let Ok(n) = id.parse::<i64>() {
                        m.insert("id".into(), Value::Short(n as i16));
                    }
                } else if !id.is_empty() {
                    m.insert("id".into(), Value::String(id.to_string()));
                }
                let value = match m.get("lvl") {
                    Some(Value::Int(_)) => Value::Int(lvl as i32),
                    Some(Value::Long(_)) => Value::Long(lvl),
                    _ => Value::Short(lvl as i16),
                };
                m.insert("lvl".into(), value);
            }
            out.push(entry);
        }
        tag.insert(ench_key.into(), Value::List(out));
    }

    // 属性修饰符：按位置合并，保留 Name / UUID 等未展示字段
    write_legacy_modifiers(tag, patch)
}

fn write_legacy_modifiers(tag: &mut HashMap<String, Value>, patch: &Json) -> Result<(), String> {
    let incoming = get_arr(patch, "modifiers");
    let existing: Vec<Value> = tag
        .get("AttributeModifiers")
        .and_then(level::as_list)
        .cloned()
        .unwrap_or_default();
    if incoming.is_empty() {
        tag.remove("AttributeModifiers");
        return Ok(());
    }
    let mut out = Vec::with_capacity(incoming.len());
    for (i, src) in incoming.iter().enumerate() {
        let attr = src.get("attribute").and_then(|x| x.as_str()).unwrap_or("");
        let amount = src.get("amount").and_then(|x| x.as_f64()).unwrap_or(0.0);
        let op = src.get("operation").and_then(|x| x.as_i64()).unwrap_or(0);
        let slot = src.get("slot").and_then(|x| x.as_str());
        let mut entry = existing.get(i).cloned().unwrap_or_else(level::empty_compound);
        if let Value::Compound(m) = &mut entry {
            if !attr.is_empty() {
                m.insert("AttributeName".into(), Value::String(attr.to_string()));
            }
            m.insert("Amount".into(), Value::Double(amount));
            m.insert("Operation".into(), Value::Int(op as i32));
            match slot {
                Some(s) => {
                    m.insert("Slot".into(), Value::String(s.to_string()));
                }
                None => {
                    m.remove("Slot");
                }
            }
            if !m.contains_key("Name") {
                m.insert("Name".into(), Value::String(format!("qookix_modifier_{i}")));
            }
            // 旧格式条目的 UUID 是必填项，新建的补一个
            if !m.contains_key("UUID") {
                m.insert(
                    "UUID".into(),
                    Value::IntArray(fastnbt::IntArray::new(fresh_uuid_ints())),
                );
            }
        }
        out.push(entry);
    }
    tag.insert("AttributeModifiers".into(), Value::List(out));
    Ok(())
}

/// 新格式：写回 components 容器
fn write_modern(c: &mut HashMap<String, Value>, patch: &Json) -> Result<(), String> {
    let name = patch.get("name").and_then(|x| x.as_str()).unwrap_or("");
    if name.is_empty() {
        c.remove(CUSTOM_NAME);
    } else {
        // 新格式的文本组件：纯字符串本身合法
        c.insert(CUSTOM_NAME.into(), Value::String(name.to_string()));
    }

    let lore = get_arr(patch, "lore");
    if lore.is_empty() {
        c.remove(LORE);
    } else {
        let list = lore
            .iter()
            .map(|l| Value::String(l.as_str().unwrap_or_default().to_string()))
            .collect();
        c.insert(LORE.into(), Value::List(list));
    }

    if patch.get("unbreakable").and_then(|x| x.as_i64()).unwrap_or(0) != 0 {
        c.insert(UNBREAKABLE.into(), level::empty_compound());
    } else {
        c.remove(UNBREAKABLE);
    }

    let incoming = get_arr(patch, "enchantments");
    if incoming.is_empty() {
        c.remove(ENCHANTMENTS);
    } else {
        let mut levels = HashMap::new();
        for e in incoming {
            let id = e.get("id").and_then(|x| x.as_str()).unwrap_or("");
            if id.is_empty() {
                continue;
            }
            let lvl = e.get("level").and_then(|x| x.as_i64()).unwrap_or(1);
            levels.insert(id.to_string(), Value::Int(lvl as i32));
        }
        let mut ench = HashMap::new();
        // 保留原有 show_in_tooltip 之类未展示的键
        if let Some(Value::Compound(old)) = c.get(ENCHANTMENTS) {
            for (k, v) in old {
                if k != "levels" {
                    ench.insert(k.clone(), v.clone());
                }
            }
        }
        ench.insert("levels".into(), Value::Compound(levels));
        c.insert(ENCHANTMENTS.into(), Value::Compound(ench));
    }

    let incoming = get_arr(patch, "modifiers");
    if incoming.is_empty() {
        c.remove(ATTR_MODIFIERS);
    } else {
        let mut old_modifiers: Vec<Value> = Vec::new();
        let mut extra = HashMap::new();
        if let Some(Value::Compound(old)) = c.get(ATTR_MODIFIERS) {
            for (k, v) in old {
                if k == "modifiers" {
                    old_modifiers = level::as_list(v).cloned().unwrap_or_default();
                } else {
                    extra.insert(k.clone(), v.clone());
                }
            }
        }
        let mut out = Vec::with_capacity(incoming.len());
        for (i, src) in incoming.iter().enumerate() {
            let attr = src.get("attribute").and_then(|x| x.as_str()).unwrap_or("");
            let amount = src.get("amount").and_then(|x| x.as_f64()).unwrap_or(0.0);
            let op = src.get("operation").and_then(|x| x.as_i64()).unwrap_or(0);
            let slot = src.get("slot").and_then(|x| x.as_str()).unwrap_or("any");
            let mut entry = old_modifiers.get(i).cloned().unwrap_or_else(level::empty_compound);
            if let Value::Compound(m) = &mut entry {
                if !attr.is_empty() {
                    m.insert("type".into(), Value::String(attr.to_string()));
                }
                let raw_id = src.get("name").and_then(|x| x.as_str()).unwrap_or("");
                let id = if raw_id.contains(':') {
                    raw_id.to_string()
                } else if let Some(Value::String(old)) = m.get("id") {
                    old.clone()
                } else {
                    format!("qookix:modifier_{i}")
                };
                m.insert("id".into(), Value::String(id));
                m.insert("amount".into(), Value::Double(amount));
                m.insert("operation".into(), Value::String(op_to_str(op).into()));
                m.insert("slot".into(), Value::String(slot.to_string()));
            }
            out.push(entry);
        }
        extra.insert("modifiers".into(), Value::List(out));
        c.insert(ATTR_MODIFIERS.into(), Value::Compound(extra));
    }
    Ok(())
}

/// 旧格式 AttributeModifiers 需要的 UUID（IntArray[4]），只需唯一无需密码学强度
fn fresh_uuid_ints() -> Vec<i32> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut x = nanos ^ 0x9E37_79B9_7F4A_7C15;
    let mut out = Vec::with_capacity(4);
    for _ in 0..4 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        out.push(x as i32);
    }
    out
}

/// 区块文件（.mca）第一期只读不写
#[allow(dead_code)]
pub fn is_chunk_file(path: &std::path::Path) -> bool {
    path.extension().and_then(|x| x.to_str()) == Some("mca")
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastnbt::SerOpts;

    /// 造一把"带了很多未展示标签"的旧格式剑，模拟真实存档
    fn legacy_sword() -> Value {
        let mut ench = HashMap::new();
        ench.insert("id".into(), Value::String("minecraft:sharpness".into()));
        ench.insert("lvl".into(), Value::Short(3));

        let mut display = HashMap::new();
        display.insert("Name".into(), Value::String("\"旧名字\"".into()));
        display.insert(
            "Lore".into(),
            Value::List(vec![Value::String("\"第一行\"".into())]),
        );
        display.insert("color".into(), Value::Int(123456));

        let mut tag = HashMap::new();
        tag.insert("Damage".into(), Value::Int(7));
        tag.insert("CustomModelData".into(), Value::Int(9));
        tag.insert("HideFlags".into(), Value::Int(1));
        tag.insert("display".into(), Value::Compound(display));
        tag.insert("Enchantments".into(), Value::List(vec![Value::Compound(ench)]));

        let mut item = HashMap::new();
        item.insert("id".into(), Value::String("minecraft:diamond_sword".into()));
        item.insert("Count".into(), Value::Byte(1));
        item.insert("Slot".into(), Value::Int(0));
        item.insert("tag".into(), Value::Compound(tag));
        Value::Compound(item)
    }

    #[test]
    fn legacy_item_edit_keeps_unknown_tags() {
        let existing = legacy_sword();
        let patch = json!({
            "id": "minecraft:diamond_sword",
            "count": 1,
            "slot": 0,
            "name": "新名字",
            "lore": ["第一行", "第二行"],
            "unbreakable": 1,
            "enchantments": [{ "id": "minecraft:sharpness", "level": 5 }],
            "modifiers": [],
        });
        let out = build_item(&patch, Some(&existing), false, 0).unwrap();
        let get = |k: &str| level::get(&out, k);

        assert_eq!(level::get_string(&out, "id").as_deref(), Some("minecraft:diamond_sword"));
        // Count 保持 Byte（1.20.4 及以下要求）
        assert!(matches!(get("Count"), Some(Value::Byte(1))), "Count 必须是 Byte");
        // 未展示的标签必须原样保留
        let tag = get("tag").expect("tag 不能丢");
        assert_eq!(level::get_int(tag, "Damage"), Some(7));
        assert_eq!(level::get_int(tag, "CustomModelData"), Some(9));
        assert_eq!(level::get_int(tag, "HideFlags"), Some(1));
        let display = level::get(tag, "display").unwrap();
        assert_eq!(level::get_int(display, "color"), Some(123456), "display 的其它键要保留");
        assert_eq!(
            level::get_string(display, "Name").as_deref(),
            Some("\"新名字\"")
        );
        let lore = level::get(display, "Lore").and_then(level::as_list).unwrap();
        assert_eq!(lore.len(), 2);
        assert_eq!(level::get_int(tag, "Unbreakable"), Some(1));
        let ench = level::get(tag, "Enchantments").and_then(level::as_list).unwrap();
        assert_eq!(level::get_int(&ench[0], "lvl"), Some(5));
        assert!(matches!(ench[0], Value::Compound(_)));
        // lvl 仍是 Short（老版本 codec 认 Short）
        if let Value::Compound(m) = &ench[0] {
            assert!(matches!(m.get("lvl"), Some(Value::Short(5))), "lvl 保持 Short");
        }
    }

    #[test]
    fn legacy_item_clearing_fields_removes_them() {
        let mut existing = legacy_sword();
        // 预置 Unbreakable，便于验证"关掉后被移除"
        if let Value::Compound(m) = &mut existing {
            if let Some(Value::Compound(tag)) = m.get_mut("tag") {
                tag.insert("Unbreakable".into(), Value::Byte(1));
            }
        }
        let patch = json!({
            "id": "minecraft:diamond_sword",
            "count": 1,
            "slot": 0,
            "name": "",
            "lore": [],
            "unbreakable": 0,
            "enchantments": [],
            "modifiers": [],
        });
        let out = build_item(&patch, Some(&existing), false, 0).unwrap();
        let tag = level::get(&out, "tag").unwrap();
        assert!(level::get(tag, "Unbreakable").is_none());
        assert!(level::get(tag, "Enchantments").is_none());
        let display = level::get(tag, "display").unwrap();
        assert!(level::get(display, "Name").is_none());
        assert!(level::get(display, "Lore").is_none());
        // 只剩 color 时 display 保留、其余不动
        assert_eq!(level::get_int(display, "color"), Some(123456));
    }

    #[test]
    fn modern_item_uses_components() {
        let patch = json!({
            "id": "minecraft:diamond_sword",
            "count": 2,
            "slot": 5,
            "name": "新名字",
            "lore": ["描述"],
            "unbreakable": 1,
            "enchantments": [{ "id": "minecraft:sharpness", "level": 4 }],
            "modifiers": [{ "attribute": "minecraft:attack_damage", "amount": 3.0, "operation": 0, "slot": null }],
        });
        let out = build_item(&patch, None, true, 5).unwrap();
        assert_eq!(level::get_int(&out, "count"), Some(2));
        assert_eq!(level::get_int(&out, "slot"), Some(5));
        assert!(level::get(&out, "tag").is_none(), "新格式不该写 tag");
        let c = level::get(&out, "components").expect("components 必须有");
        assert_eq!(level::get_string(c, CUSTOM_NAME).as_deref(), Some("新名字"));
        assert!(level::get(c, UNBREAKABLE).is_some());
        let lv = level::get(c, ENCHANTMENTS)
            .and_then(|e| level::get(e, "levels"))
            .unwrap();
        assert_eq!(level::get_int(lv, "minecraft:sharpness"), Some(4));
        let m = level::get(c, ATTR_MODIFIERS)
            .and_then(|a| level::get(a, "modifiers"))
            .and_then(level::as_list)
            .unwrap();
        assert_eq!(level::get_string(&m[0], "operation").as_deref(), Some("add_value"));
        assert_eq!(level::get_string(&m[0], "slot").as_deref(), Some("any"));

        // 再读回来（模拟背包列表），字段应能对上
        let back = slot_json(5, &out, 4000);
        assert_eq!(back.get("id").and_then(|x| x.as_str()), Some("minecraft:diamond_sword"));
        assert_eq!(back.get("count").and_then(|x| x.as_i64()), Some(2));
        assert_eq!(back.get("name").and_then(|x| x.as_str()), Some("新名字"));
        assert_eq!(back.get("unbreakable").and_then(|x| x.as_i64()), Some(1));
        assert_eq!(
            back.get("enchantments").and_then(|x| x.as_array()).unwrap()[0]
                .get("level")
                .and_then(|x| x.as_i64()),
            Some(4)
        );
    }

    #[test]
    fn legacy_read_roundtrips_through_patch() {
        let existing = legacy_sword();
        let as_json = slot_json(0, &existing, 3000);
        let patch = json!({
            "id": as_json.get("id").unwrap().as_str().unwrap(),
            "count": as_json.get("count").unwrap().as_i64().unwrap(),
            "slot": as_json.get("slot").unwrap().as_i64().unwrap(),
            "name": as_json.get("name").unwrap().as_str().unwrap(),
            "lore": as_json.get("lore").unwrap(),
            "unbreakable": as_json.get("unbreakable").unwrap().as_i64().unwrap(),
            "enchantments": as_json.get("enchantments").unwrap(),
            "modifiers": as_json.get("modifiers").unwrap(),
        });
        let out = build_item(&patch, Some(&existing), false, 0).unwrap();
        let tag = level::get(&out, "tag").unwrap();
        assert_eq!(
            level::get_string(level::get(tag, "display").unwrap(), "Name").as_deref(),
            Some("\"旧名字\"")
        );
        assert_eq!(level::get_int(tag, "Damage"), Some(7));
        // 纯往返后可序列化（确认 NBT 合法）
        assert!(fastnbt::to_bytes_with_opts(&out, SerOpts::new().root_name("")).is_ok());
    }
}
