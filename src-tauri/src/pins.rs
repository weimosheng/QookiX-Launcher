use crate::models::PinItem;
use std::path::Path;

/// 读取固定项列表（首页 / 侧边栏）。库不可用时返回空列表。
pub fn load_pins(root: &Path) -> Vec<PinItem> {
    match crate::db::open(root) {
        Ok(conn) => crate::db::load_rows(&conn, "pins")
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// 将固定项列表写入数据库。
pub fn save_pins(root: &Path, items: &[PinItem]) -> Result<(), String> {
    let conn = crate::db::open(root)?;
    let vals: Vec<serde_json::Value> = items
        .iter()
        .map(|p| serde_json::to_value(p).unwrap_or_default())
        .collect();
    crate::db::save_rows(&conn, "pins", &vals)
}
