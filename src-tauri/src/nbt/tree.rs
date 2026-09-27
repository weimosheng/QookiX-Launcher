//! 树形模式的节点编辑：按路径（key 数组）修改或删除节点。
//!
//! 两点原则：
//! 1. 只改指定节点，其余分支原样保留；
//! 2. **保持原 NBT 类型**——NBT 里 `1` 是 Byte 还是 Int 由标签类型决定，
//!    前端输入的只是数字，类型必须按原节点（或前端声明的类型）还原，
//!    否则游戏按结构化反序列化读 level.dat 时会因类型不符报错。

use crate::nbt::level;
use fastnbt::Value;
use serde_json::Value as Json;
use std::path::Path;

/// 定位路径上的节点（不可变）
fn resolve<'a>(root: &'a Value, path: &[String]) -> Option<&'a Value> {
    let mut cur = root;
    for seg in path {
        cur = match cur {
            Value::Compound(m) => m.get(seg)?,
            Value::List(a) => a.get(seg.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}

/// 定位路径上的父节点（可变），路径最后一段为要操作的 key
fn resolve_mut<'a>(root: &'a mut Value, path: &[String]) -> Result<&'a mut Value, String> {
    let mut cur = root;
    for seg in path.iter().take(path.len().saturating_sub(1)) {
        cur = match cur {
            Value::Compound(m) => m
                .get_mut(seg)
                .ok_or_else(|| format!("找不到节点 {seg}"))?,
            Value::List(a) => {
                let i: usize = seg.parse().map_err(|_| format!("列表下标无效: {seg}"))?;
                a.get_mut(i).ok_or_else(|| format!("下标越界: {i}"))?
            }
            _ => return Err(format!("{seg} 不是可进入的节点")),
        };
    }
    Ok(cur)
}

/// 在给定的一棵 NBT 数据上改节点。
/// `declared_type` 是前端节点上显示的类型，仅在新节点（key 原本不存在）时使用。
pub fn set_node_in(
    data: &mut Value,
    path: &[String],
    value: &Json,
    declared_type: &str,
) -> Result<(), String> {
    if path.is_empty() {
        return Err("路径不能为空".into());
    }
    // 已存在的节点以它自己的类型为准，避免把 Byte 写成 Int
    let kind = resolve(data, path).map(level::kind_of).unwrap_or(declared_type);
    let new_value = level::typed_value(kind, value)?;
    let last = path.last().cloned().unwrap_or_default();
    let parent = resolve_mut(data, path)?;
    level::set_child(parent, &last, new_value)
}

/// 在给定的一棵 NBT 数据上删节点
pub fn delete_node_in(data: &mut Value, path: &[String]) -> Result<(), String> {
    if path.is_empty() {
        return Err("路径不能为空".into());
    }
    let last = path.last().cloned().unwrap_or_default();
    let parent = resolve_mut(data, path)?;
    level::remove_child(parent, &last)
}

/// 设置 level.dat 里某个节点的值（path 相对"世界数据层"，见 `level::world_data`）
pub fn set_node(
    level_path: &Path,
    path: Vec<String>,
    value: &Json,
    declared_type: &str,
) -> Result<(), String> {
    let mut root = level::read_level(level_path)?;
    level::on_world_data(&mut root, |data| set_node_in(data, &path, value, declared_type))?;
    level::write_level(level_path, &root)
}

/// 删除 level.dat 里某个节点
pub fn delete_node(level_path: &Path, path: Vec<String>) -> Result<(), String> {
    let mut root = level::read_level(level_path)?;
    level::on_world_data(&mut root, |data| delete_node_in(data, &path))?;
    level::write_level(level_path, &root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nbt::level::get_int;
    use serde_json::json;
    use std::collections::HashMap;

    fn sample_path(dir: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir()
            .join(format!("qookix-tree-{}-{}", std::process::id(), dir))
            .join("level.dat");
        if let Some(parent) = p.parent() {
            let _ = std::fs::remove_dir_all(parent);
            std::fs::create_dir_all(parent).unwrap();
        }
        let mut m = HashMap::new();
        m.insert("DataVersion".into(), Value::Int(3465));
        m.insert("raining".into(), Value::Byte(0));
        m.insert("RandomSeed".into(), Value::Long(42));
        m.insert("LevelName".into(), Value::String("旧名字".into()));
        let mut border = HashMap::new();
        border.insert("Size".into(), Value::Double(1000.0));
        border.insert("SafeZone".into(), Value::Double(5.0));
        m.insert("WorldBorder".into(), Value::Compound(border));
        level::write_level(&p, &Value::Compound(m)).unwrap();
        p
    }

    /// 改 Byte 字段必须还是 Byte
    #[test]
    fn byte_stays_byte() {
        let p = sample_path("byte");
        set_node(&p, vec!["raining".into()], &json!(1), "byte").unwrap();
        let d = level::read_level(&p).unwrap();
        match level::get(&d, "raining") {
            Some(Value::Byte(1)) => {}
            other => panic!("raining 必须保持 Byte(1)，实际是 {other:?}"),
        }
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// 纯数字输入改 Long 字段不能退化成 Int
    #[test]
    fn long_stays_long() {
        let p = sample_path("long");
        set_node(&p, vec!["RandomSeed".into()], &json!(12345), "long").unwrap();
        let d = level::read_level(&p).unwrap();
        assert!(matches!(level::get(&d, "RandomSeed"), Some(Value::Long(12345))));
        assert_eq!(get_int(&d, "RandomSeed"), Some(12345));
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// 嵌套字段编辑不影响兄弟字段
    #[test]
    fn nested_edit_keeps_siblings() {
        let p = sample_path("nested");
        set_node(
            &p,
            vec!["WorldBorder".into(), "Size".into()],
            &json!(2000.0),
            "double",
        )
        .unwrap();
        let d = level::read_level(&p).unwrap();
        let b = level::get(&d, "WorldBorder").unwrap();
        assert_eq!(level::get_double(b, "Size"), Some(2000.0));
        assert_eq!(level::get_double(b, "SafeZone"), Some(5.0));
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }

    /// 真实存档形态（外层 Data + fml）：树路径相对"世界数据层"，
    /// 改完 fml 必须原样还在
    #[test]
    fn edits_go_through_data_wrapper() {
        let dir = std::env::temp_dir().join(format!("qookix-tree-wrap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("level.dat");

        let mut inner = HashMap::new();
        inner.insert("raining".into(), Value::Byte(0));
        inner.insert("LevelName".into(), Value::String("包装层里的世界".into()));
        let mut fml = HashMap::new();
        fml.insert("LoadingModList".into(), Value::String("fake".into()));
        let mut wrapper = HashMap::new();
        wrapper.insert("Data".into(), Value::Compound(inner));
        wrapper.insert("fml".into(), Value::Compound(fml));
        level::write_level(&p, &Value::Compound(wrapper)).unwrap();

        // 路径不带 Data 前缀
        set_node(&p, vec!["raining".into()], &json!(1), "byte").unwrap();
        set_node(&p, vec!["LevelName".into()], &json!("改过"), "string").unwrap();

        let after = level::read_level(&p).unwrap();
        assert!(level::get(&after, "fml").is_some(), "fml 不能被树编辑挤掉");
        let d = level::world_data(&after);
        assert!(matches!(level::get(d, "raining"), Some(Value::Byte(1))));
        assert_eq!(
            level::get_string(d, "LevelName").as_deref(),
            Some("改过")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_removes_only_target() {
        let p = sample_path("delete");
        delete_node(&p, vec!["raining".into()]).unwrap();
        let d = level::read_level(&p).unwrap();
        assert!(level::get(&d, "raining").is_none());
        assert_eq!(get_int(&d, "DataVersion"), Some(3465));
        assert_eq!(level::get_string(&d, "LevelName").as_deref(), Some("旧名字"));
        let _ = std::fs::remove_dir_all(p.parent().unwrap());
    }
}
