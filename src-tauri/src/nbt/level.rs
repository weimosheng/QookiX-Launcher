//! `level.dat` 读写。
//!
//! level.dat 是 **gzip 压缩的 NBT**，根 compound 名为 `Data`。
//! 写入策略（安全关键）：**读原始 Value 树 → 只改指定字段 → 整体写回**，
//! 绝不按表单字段重建一棵新树，否则会丢掉 WorldGenSettings 等未覆盖字段，
//! 直接导致世界损坏。

use fastnbt::Value;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde_json::Value as Json;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;

/// 读取 level.dat，返回整个 Data compound（Value::Compound）。
pub fn read_level(path: &Path) -> Result<Value, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("打开 level.dat 失败: {e}"))?;
    let mut dec = GzDecoder::new(file);
    let mut raw = Vec::new();
    dec.read_to_end(&mut raw)
        .map_err(|e| format!("解压 level.dat 失败（不是有效的 gzip？）: {e}"))?;
    let v: Value = fastnbt::from_bytes(&raw).map_err(|e| format!("解析 NBT 失败: {e}"))?;
    Ok(v)
}

/// 把 NBT 写回文件（gzip 压缩）。
///
/// 根标签名用空串：游戏自己写的 level.dat / playerdata 都是空根名
/// （世界数据在 `Data` 子节点里，见 `world_data`），保持一致最稳。
pub fn write_level(path: &Path, data: &Value) -> Result<(), String> {
    let raw = fastnbt::to_bytes_with_opts(data, fastnbt::SerOpts::new())
        .map_err(|e: fastnbt::error::Error| format!("序列化 NBT 失败: {e}"))?;
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(&raw)
        .map_err(|e| format!("压缩失败: {e}"))?;
    let bytes = enc.finish().map_err(|e| format!("压缩失败: {e}"))?;
    write_bytes(path, &bytes)
}

/// 整体写回文件：先写同目录临时文件再替换，避免中途失败留下半个文件。
///
/// 路径可能是符号链接（有些存档会把 level.dat 链到别处）：rename 会把链接
/// 本身换成普通文件，这种情况改成写穿链接（copy 会跟随链接写入目标）。
pub fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or_else(|| "无法确定文件名".to_string())?;
    let mut tmp = path.to_path_buf();
    tmp.set_file_name(format!("{name}.qookix_tmp"));
    std::fs::write(&tmp, bytes).map_err(|e| format!("写入临时文件失败: {e}"))?;

    let is_link = std::fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);
    if is_link {
        std::fs::copy(&tmp, path).map_err(|e| format!("写入文件失败: {e}"))?;
        let _ = std::fs::remove_file(&tmp);
        return Ok(());
    }
    std::fs::rename(&tmp, path).map_err(|e| format!("替换文件失败: {e}"))
}

/// 取"世界数据层"。
///
/// 真实的 level.dat 是 `TAG_Compound("") { Data: {…世界数据…}, fml: {…Forge…} }`：
/// 世界数据在 `Data` 子节点里，`fml` 是 Forge 的兄弟节点。
/// 少数老工具产出的文件"根即世界数据"，这时原样返回根。
/// **所有读写都必须过这一层**，否则会写到外层（游戏不读，等于没改）。
pub fn world_data(root: &Value) -> &Value {
    match get(root, "Data") {
        Some(inner @ Value::Compound(_)) => inner,
        _ => root,
    }
}

/// 在世界数据层上做可变操作（没有包装层时直接操作根）
pub fn on_world_data<R>(root: &mut Value, f: impl FnOnce(&mut Value) -> R) -> R {
    let wrapped = matches!(
        root,
        Value::Compound(m) if matches!(m.get("Data"), Some(Value::Compound(_)))
    );
    if wrapped {
        if let Value::Compound(m) = root {
            if let Some(inner) = m.get_mut("Data") {
                return f(inner);
            }
        }
    }
    f(root)
}

/// 是否带 `Data` 包装层（真实存档都有，老文件可能没有）。
/// 生产代码走 `world_data` 自动兼容，这个主要给测试断言用。
#[allow(dead_code)]
pub fn is_wrapped(root: &Value) -> bool {
    matches!(root, Value::Compound(m) if matches!(m.get("Data"), Some(Value::Compound(_))))
}

/// 从 Data compound 中取一个字段（不存在返回 None）。
pub fn get<'a>(data: &'a Value, key: &str) -> Option<&'a Value> {
    match data {
        Value::Compound(m) => m.get(key),
        _ => None,
    }
}

/// 取出整型字段（Byte/Short/Int/Long 统一按 i64 读）。
pub fn get_int(data: &Value, key: &str) -> Option<i64> {
    match get(data, key) {
        Some(Value::Byte(b)) => Some(*b as i64),
        Some(Value::Short(s)) => Some(*s as i64),
        Some(Value::Int(i)) => Some(*i as i64),
        Some(Value::Long(l)) => Some(*l),
        _ => None,
    }
}

pub fn get_double(data: &Value, key: &str) -> Option<f64> {
    match get(data, key) {
        Some(Value::Double(d)) => Some(*d),
        Some(Value::Float(f)) => Some(*f as f64),
        _ => None,
    }
}

/// 取 list 类型（fastnbt 的 Value 没有 as_array，统一走这里）
pub fn as_list(v: &Value) -> Option<&Vec<Value>> {
    match v {
        Value::List(a) => Some(a),
        _ => None,
    }
}

pub fn get_string(data: &Value, key: &str) -> Option<String> {
    match get(data, key) {
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    }
}

/// NBT 值的类型名（与树形模式返回给前端的字符串一致）。
pub fn kind_of(v: &Value) -> &'static str {
    match v {
        Value::Byte(_) => "byte",
        Value::Short(_) => "short",
        Value::Int(_) => "int",
        Value::Long(_) => "long",
        Value::Float(_) => "float",
        Value::Double(_) => "double",
        Value::String(_) => "string",
        Value::ByteArray(_) => "byte_array",
        Value::IntArray(_) => "int_array",
        Value::LongArray(_) => "long_array",
        Value::List(_) => "list",
        Value::Compound(_) => "compound",
    }
}

fn as_i64(v: &Json) -> Option<i64> {
    match v {
        Json::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Json::Bool(b) => Some(if *b { 1 } else { 0 }),
        Json::String(s) => s.trim().parse::<i64>().ok().or_else(|| {
            s.trim().parse::<f64>().ok().map(|f| f as i64)
        }),
        _ => None,
    }
}

fn as_f64(v: &Json) -> Option<f64> {
    match v {
        Json::Number(n) => n.as_f64(),
        Json::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Json::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn as_text(v: &Json) -> Option<String> {
    match v {
        Json::String(s) => Some(s.clone()),
        Json::Number(n) => Some(n.to_string()),
        Json::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// JSON → NBT Value，按 `kind` 指定目标类型。
/// 树形模式改值必须走这里：让 `byte` 字段保持 `Byte` 而不是退化成 `Int`，
/// 否则游戏按结构化反序列化读 level.dat 时会因类型不符报错。
pub fn typed_value(kind: &str, v: &Json) -> Result<Value, String> {
    match kind {
        "byte" => as_i64(v)
            .map(|n| Value::Byte(n as i8))
            .ok_or_else(|| "需要 -128 ~ 127 的整数".to_string()),
        "short" => as_i64(v)
            .map(|n| Value::Short(n as i16))
            .ok_or_else(|| "需要整数".to_string()),
        "int" => as_i64(v)
            .map(|n| Value::Int(n as i32))
            .ok_or_else(|| "需要整数".to_string()),
        "long" => as_i64(v)
            .map(Value::Long)
            .ok_or_else(|| "需要整数".to_string()),
        "float" => as_f64(v)
            .map(|n| Value::Float(n as f32))
            .ok_or_else(|| "需要数字".to_string()),
        "double" => as_f64(v)
            .map(Value::Double)
            .ok_or_else(|| "需要数字".to_string()),
        "string" => as_text(v)
            .map(Value::String)
            .ok_or_else(|| "需要文本".to_string()),
        "byte_array" | "int_array" | "long_array" | "list" | "compound" => json_to_value(v),
        other => Err(format!("不支持的类型: {other}")),
    }
}

/// JSON → NBT Value，按 JSON 自身形态推断类型（列表 / 复合节点的整块替换用）。
pub fn json_to_value(v: &Json) -> Result<Value, String> {
    Ok(match v {
        Json::Null => Value::Byte(0),
        Json::Bool(b) => Value::Byte(if *b { 1 } else { 0 }),
        Json::Number(n) => {
            if let Some(i) = n.as_i64() {
                if (i32::MIN as i64..=i32::MAX as i64).contains(&i) {
                    Value::Int(i as i32)
                } else {
                    Value::Long(i)
                }
            } else if let Some(f) = n.as_f64() {
                Value::Double(f)
            } else {
                Value::Int(0)
            }
        }
        Json::String(s) => Value::String(s.clone()),
        Json::Array(a) => {
            let mut out = Vec::with_capacity(a.len());
            for x in a {
                out.push(json_to_value(x)?);
            }
            Value::List(out)
        }
        Json::Object(o) => {
            let mut m = HashMap::new();
            for (k, val) in o {
                m.insert(k.clone(), json_to_value(val)?);
            }
            Value::Compound(m)
        }
    })
}

/// 在容器里写入 / 覆盖一个子节点（compound 按 key，list 按数字下标）。
pub fn set_child(parent: &mut Value, key: &str, v: Value) -> Result<(), String> {
    match parent {
        Value::Compound(m) => {
            m.insert(key.to_string(), v);
            Ok(())
        }
        Value::List(a) => {
            let i: usize = key.parse().map_err(|_| format!("列表下标无效: {key}"))?;
            if i < a.len() {
                a[i] = v;
                Ok(())
            } else {
                Err(format!("下标越界: {i}"))
            }
        }
        _ => Err("父节点不是容器".into()),
    }
}

/// 在容器里移除一个子节点。
pub fn remove_child(parent: &mut Value, key: &str) -> Result<(), String> {
    match parent {
        Value::Compound(m) => {
            if m.remove(key).is_none() {
                return Err(format!("节点不存在: {key}"));
            }
            Ok(())
        }
        Value::List(a) => {
            let i: usize = key.parse().map_err(|_| format!("列表下标无效: {key}"))?;
            if i < a.len() {
                a.remove(i);
                Ok(())
            } else {
                Err(format!("下标越界: {i}"))
            }
        }
        _ => Err("父节点不是容器".into()),
    }
}

/// 空 compound（写回可选子结构时用）
pub fn empty_compound() -> Value {
    Value::Compound(HashMap::new())
}
