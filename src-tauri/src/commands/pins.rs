use crate::state::AppState;
use tauri::State;

// Pinned items (首页快捷卡片 / 侧边栏图标)
// ---------------------------------------------------------------------------

/// 读取全部固定项（首页与侧边栏各自独立存储）。
#[tauri::command]
pub fn get_pins(state: State<AppState>) -> Vec<crate::models::PinItem> {
    crate::pins::load_pins(&state.root)
}

/// 覆盖写入全部固定项到 `pins.json`。
#[tauri::command]
pub fn set_pins(state: State<AppState>, items: Vec<crate::models::PinItem>) -> Result<(), String> {
    crate::pins::save_pins(&state.root, &items)
}
