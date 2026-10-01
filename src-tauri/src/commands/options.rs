//! 实例 `options.txt` 的按键绑定（图形化改键的后端）。

use crate::options;
use crate::state::AppState;
use crate::util;
use serde_json::{json, Value};
use tauri::{Emitter, State};

fn instance_dir(state: &AppState, instance_id: &str) -> Result<std::path::PathBuf, String> {
    if !util::is_safe_filename(instance_id) {
        return Err("非法的实例 id".into());
    }
    let dir = state.instances_dir().join(instance_id);
    if !dir.is_dir() {
        return Err("实例不存在".into());
    }
    Ok(dir)
}

/// 列出实例 `options.txt` 里的按键绑定；`exists` 为 false 表示还没生成（先启动一次游戏）
#[tauri::command]
pub fn options_list_keybinds(state: State<AppState>, instance_id: String) -> Result<Value, String> {
    let dir = instance_dir(&state, &instance_id)?;
    let (exists, binds) = options::read_keybinds(&dir);
    Ok(json!({ "exists": exists, "binds": binds }))
}

/// 保存按键修改：`changes` 为 `动作 → 按键码`（`null` = 未绑定）；返回实际改动条数
#[tauri::command]
pub fn options_set_keybinds(
    state: State<AppState>,
    instance_id: String,
    changes: Value,
) -> Result<Value, String> {
    let dir = instance_dir(&state, &instance_id)?;
    let map = changes.as_object().ok_or("changes 必须是对象")?;
    let updated = options::apply_keybinds(&dir, map)?;
    Ok(json!({ "updated": updated }))
}

/// 采集动作中文名与模组列表（扫客户端与模组 jar 的语言文件）；
/// 扫描期间通过 `keybind://scan` 上报进度，返回 `{ labels, mods }`
#[tauri::command]
pub async fn keybind_action_labels(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Value, String> {
    // 读 jar / 资源索引是同步 IO，放到阻塞线程池，避免卡住 UI
    let inst_dir = state.instances_dir().join(&instance_id);
    let version_dir = crate::paths::resolve_version_dir(&state, &instance_id);
    let assets_dir = state.assets_dir();
    let root = state.root.clone();

    // 先按输入文件指纹查缓存：模组没动过就直接返回，省掉整个扫描
    let (inst_fp, ver_fp, assets_fp) = (inst_dir.clone(), version_dir.clone(), assets_dir.clone());
    let fingerprint = tauri::async_runtime::spawn_blocking(move || {
        crate::keybinds_lang::scan_fingerprint(&inst_fp, &ver_fp, &assets_fp)
    })
    .await
    .map_err(|e| format!("计算缓存指纹失败: {e}"))?;
    if let Some((labels, mods)) = crate::keybinds_lang::load_cached(&root, &instance_id, &fingerprint)
    {
        let mods = mods
            .into_iter()
            .map(|m| json!({ "id": m.id, "name": m.name }))
            .collect::<Vec<Value>>();
        return Ok(json!({ "labels": labels, "mods": mods, "cached": true }));
    }

    let scan_app = app.clone();
    let (scan_inst, scan_ver, scan_assets) =
        (inst_dir.clone(), version_dir.clone(), assets_dir.clone());
    let (labels, mods) = tauri::async_runtime::spawn_blocking(move || {
        let emit = |done: usize, total: usize| {
            let _ = scan_app.emit("keybind://scan", json!({ "done": done, "total": total }));
        };
        crate::keybinds_lang::collect_all(&scan_inst, &scan_ver, &scan_assets, &emit)
    })
    .await
    .map_err(|e| format!("读取语言文件失败: {e}"))?;
    crate::keybinds_lang::save_cache(&root, &instance_id, &fingerprint, &labels, &mods);
    let mods = mods
        .into_iter()
        .map(|m| json!({ "id": m.id, "name": m.name }))
        .collect::<Vec<Value>>();
    Ok(json!({ "labels": labels, "mods": mods }))
}
