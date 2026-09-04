use crate::models::PinItem;
use std::path::Path;

/// 读取固定项列表（首页 / 侧边栏）。文件不存在或损坏时返回空列表。
pub fn load_pins(root: &Path) -> Vec<PinItem> {
    let path = root.join("pins.json");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<PinItem>>(&s).ok())
        .unwrap_or_default()
}

/// 将固定项列表写入 `pins.json`（美化格式，便于人工查看/编辑）。
pub fn save_pins(root: &Path, items: &[PinItem]) -> Result<(), String> {
    let path = root.join("pins.json");
    let json = serde_json::to_string_pretty(items).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}
