//! NBT 存档编辑相关命令。
//!
//! 世界既可以是实例内的 `saves/<目录名>`，也可以是手动指定的绝对路径
//! （`world` 参数传绝对路径即可，见 `nbt::resolve_world`）。

use crate::nbt;
use crate::state::AppState;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde_json::{json, Value as Json};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, State};

/// 扫描进度事件名（前端 listen 同一个字符串）
const PROGRESS_EVENT: &str = "nbt-progress";

/// 该实例是否有游戏正在运行（运行中改存档会被覆盖/损坏）
fn is_running(state: &AppState, instance_id: &str) -> bool {
    state
        .game_pids
        .lock()
        .map(|g| g.contains_key(instance_id))
        .unwrap_or(false)
}

fn ensure_not_running(state: &AppState, instance_id: &str) -> Result<(), String> {
    if instance_id.is_empty() {
        return Ok(());
    }
    if is_running(state, instance_id) {
        return Err("该实例的游戏正在运行，请先退出游戏再修改存档".into());
    }
    Ok(())
}

/// 扫描实例 saves 目录，返回世界列表摘要。
/// 每扫描完一个世界往 `nbt-progress` 发一次进度（前端据此显示进度条）。
#[tauri::command]
pub async fn nbt_list_worlds(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Json, String> {
    let worlds = nbt::list_worlds(&state, &instance_id, |current, total, label| {
        let _ = app.emit(
            PROGRESS_EVENT,
            json!({ "current": current, "total": total, "label": label }),
        );
    })?;
    Ok(json!({ "worlds": worlds }))
}

/// 打开世界：返回表单字段 + 是否在游玩中 + 是否可写。
#[tauri::command]
pub async fn nbt_open_world(
    state: State<'_, AppState>,
    instance_id: String,
    world: String,
) -> Result<Json, String> {
    let dir = nbt::resolve_world(&state, &instance_id, &world)?;
    let form = nbt::world_form(&dir.join("level.dat"))?;
    let reason = nbt::check_editable(&dir).err();
    Ok(json!({
        "form": form,
        "running": is_running(&state, &instance_id),
        "editable": reason.is_none(),
        "reason": reason,
    }))
}

/// 保存世界表单修改（自动备份；游戏运行中或只读时拒绝）。
#[tauri::command]
pub async fn nbt_save_world(
    state: State<'_, AppState>,
    instance_id: String,
    world: String,
    patch: Json,
) -> Result<Json, String> {
    ensure_not_running(&state, &instance_id)?;
    let dir = nbt::resolve_world(&state, &instance_id, &world)?;
    nbt::check_editable(&dir)?;
    let backup = nbt::save_world_form(&state, &instance_id, &world, &patch)?;
    Ok(json!({ "backup": backup }))
}

/// 返回完整 NBT 树（树形模式用），带类型标记便于前端选控件。
#[tauri::command]
pub async fn nbt_tree_view(
    state: State<'_, AppState>,
    instance_id: String,
    world: String,
) -> Result<Json, String> {
    let dir = nbt::resolve_world(&state, &instance_id, &world)?;
    let root = nbt::level::read_level(&dir.join("level.dat"))?;
    // 树展示"世界数据层"：真实存档是 { Data: {…}, fml: {…} }，路径也相对这一层
    Ok(json!({ "root": to_tree("Data", nbt::level::world_data(&root)) }))
}

fn to_tree(name: &str, v: &fastnbt::Value) -> Json {
    match v {
        // NBT 的 compound 是无序表，HashMap 的遍历顺序会让每次看到的树都不一样。
        // 按 key 排序后展示，跟 NBTExplorer 一致，也方便找字段。
        fastnbt::Value::Compound(m) => {
            let mut children: Vec<(&String, &fastnbt::Value)> = m.iter().collect();
            children.sort_by(|a, b| a.0.cmp(b.0));
            json!({
                "name": name,
                "type": "compound",
                "children": children
                    .into_iter()
                    .map(|(k, val)| to_tree(k, val))
                    .collect::<Vec<_>>(),
            })
        }
        fastnbt::Value::List(items) => json!({
            "name": name,
            "type": "list",
            "children": items.iter().enumerate().map(|(i, val)| to_tree(&i.to_string(), val)).collect::<Vec<_>>(),
        }),
        other => json!({
            "name": name,
            "type": crate::nbt::level::kind_of(other),
            "value": scalar_to_json(other),
        }),
    }
}

fn scalar_to_json(v: &fastnbt::Value) -> Json {
    match v {
        fastnbt::Value::Byte(b) => json!(b),
        fastnbt::Value::Short(s) => json!(s),
        fastnbt::Value::Int(i) => json!(i),
        fastnbt::Value::Long(l) => json!(l),
        fastnbt::Value::Float(f) => json!(f),
        fastnbt::Value::Double(d) => json!(d),
        fastnbt::Value::String(s) => json!(s),
        fastnbt::Value::ByteArray(a) => json!(format!("<byte[{}]>", a.len())),
        fastnbt::Value::IntArray(a) => json!(format!("<int[{}]>", a.len())),
        fastnbt::Value::LongArray(a) => json!(format!("<long[{}]>", a.len())),
        _ => Json::Null,
    }
}

/// 世界当前是否可写（只读 / 缺 level.dat 时给出原因）
#[tauri::command]
pub async fn nbt_can_edit(
    state: State<'_, AppState>,
    instance_id: String,
    world: String,
) -> Result<Json, String> {
    let reason = match nbt::resolve_world(&state, &instance_id, &world) {
        Ok(dir) => nbt::check_editable(&dir).err(),
        Err(e) => Some(e),
    };
    Ok(json!({ "editable": reason.is_none(), "reason": reason }))
}

// ---------------------------------------------------------------------------
// 备份
// ---------------------------------------------------------------------------

/// 列出当前世界的单文件备份（level.dat + playerdata）
#[tauri::command]
pub async fn nbt_list_backups(
    state: State<'_, AppState>,
    instance_id: String,
    world: String,
) -> Result<Json, String> {
    let dir = nbt::resolve_world(&state, &instance_id, &world)?;
    Ok(json!({ "backups": nbt::backup::list_backups(&dir) }))
}

/// 用备份覆盖回原文件（恢复前自动为当前内容再留一份安全快照）
#[tauri::command]
pub async fn nbt_restore_backup(
    state: State<'_, AppState>,
    instance_id: String,
    world: String,
    file: String,
    name: String,
) -> Result<Json, String> {
    ensure_not_running(&state, &instance_id)?;
    let dir = nbt::resolve_world(&state, &instance_id, &world)?;
    nbt::check_editable(&dir)?;
    let safety = nbt::backup::restore_backup(&dir, &file, &name)?;
    Ok(json!({ "backup": safety }))
}

/// 删除一个备份文件
#[tauri::command]
pub async fn nbt_delete_backup(
    state: State<'_, AppState>,
    instance_id: String,
    world: String,
    file: String,
    name: String,
) -> Result<Json, String> {
    let dir = nbt::resolve_world(&state, &instance_id, &world)?;
    nbt::backup::delete_backup(&dir, &file, &name)?;
    Ok(json!({}))
}

// ---------------------------------------------------------------------------
// 区块（.mca）
// ---------------------------------------------------------------------------

/// 区块编辑的备份保留份数：.mca 动辄好几 MB，比 level.dat 占地方
const CHUNK_BACKUP_KEEP: usize = 3;

/// 区块写盘前的公共检查与备份，返回 (世界目录, 备份文件名)
fn prepare_chunk_write(
    state: &AppState,
    instance_id: &str,
    world: &str,
    dim: &str,
    cx: i32,
    cz: i32,
) -> Result<(PathBuf, String), String> {
    ensure_not_running(state, instance_id)?;
    let dir = nbt::resolve_world(state, instance_id, world)?;
    nbt::check_editable(&dir)?;
    let file = nbt::region::region_file(&dir, dim, cx, cz);
    let backup = if file.is_file() {
        nbt::backup::backup_file_with(&file, CHUNK_BACKUP_KEEP)?
    } else {
        String::new()
    };
    Ok((dir, backup))
}

/// 区块地图：存档里已生成区块的范围（用来定初始视野）
#[tauri::command]
pub async fn nbt_map_bounds(
    state: State<'_, AppState>,
    instance_id: String,
    world: String,
    dim: String,
) -> Result<Json, String> {
    let dir = nbt::resolve_world(&state, &instance_id, &world)?;
    nbt::map::bounds(&dir, &dim)
}

/// 区块地图：渲染一块区域（方块坐标，左闭右开）。
/// `step` 是每个像素代表的方块数，缩小时由后端降采样，避免返回超大图。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn nbt_render_map(
    app: AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    world: String,
    dim: String,
    x0: i32,
    z0: i32,
    x1: i32,
    z1: i32,
    step: i32,
) -> Result<Json, String> {
    let dir = nbt::resolve_world(&state, &instance_id, &world)?;
    let rect = nbt::map::Rect {
        x0,
        z0,
        x1,
        z1,
        step,
    };
    let img = nbt::map::render(&dir, &dim, rect, |current, total| {
        let _ = app.emit(
            PROGRESS_EVENT,
            json!({ "current": current, "total": total, "label": "" }),
        );
    })?;
    // 调色板拍平成一维（每 3 个一组 RGB），比二维数组省一大半体积
    let flat: Vec<u8> = img.palette.iter().flat_map(|c| c.iter().copied()).collect();
    // 图例：视野里占比最高的几种方块（前端显示"这些颜色是什么"）
    let legend: Vec<Json> = img
        .legend
        .iter()
        .map(|(name, color, n)| {
            json!({
                "name": name,
                "color": format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2]),
                "pixels": n,
            })
        })
        .collect();
    Ok(json!({
        "originX": img.origin_x,
        "originZ": img.origin_z,
        "step": img.step,
        "width": img.width,
        "height": img.height,
        "rendered": img.rendered,
        "palette": flat,
        "data": BASE64.encode(&img.data),
        "legend": legend,
    }))
}

/// 列出某维度已生成的区块
#[tauri::command]
pub async fn nbt_list_chunks(
    state: State<'_, AppState>,
    instance_id: String,
    world: String,
    dim: String,
) -> Result<Json, String> {
    let dir = nbt::resolve_world(&state, &instance_id, &world)?;
    Ok(json!({ "chunks": nbt::region::list_chunks(&dir, &dim)? }))
}

/// 读一个区块的 NBT 树（区块 NBT 没有 Data 包装层，根就是区块数据）
#[tauri::command]
pub async fn nbt_read_chunk(
    state: State<'_, AppState>,
    instance_id: String,
    world: String,
    dim: String,
    cx: i32,
    cz: i32,
) -> Result<Json, String> {
    let dir = nbt::resolve_world(&state, &instance_id, &world)?;
    let data = nbt::region::read_chunk(&dir, &dim, cx, cz)?;
    Ok(json!({ "root": to_tree("Chunk", &data) }))
}

/// 改区块里的某个节点
// 参数与前端、NBT 语义一一对应，硬拆成结构体反而更难对照
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn nbt_set_chunk_node(
    state: State<'_, AppState>,
    instance_id: String,
    world: String,
    dim: String,
    cx: i32,
    cz: i32,
    path: Vec<String>,
    value: Json,
    node_type: String,
) -> Result<Json, String> {
    let (dir, backup) = prepare_chunk_write(&state, &instance_id, &world, &dim, cx, cz)?;
    let mut data = nbt::region::read_chunk(&dir, &dim, cx, cz)?;
    nbt::tree::set_node_in(&mut data, &path, &value, &node_type)?;
    nbt::region::write_chunk(&dir, &dim, cx, cz, &data)?;
    // 地图缓存里那份地表数据已经过期了
    nbt::map::invalidate(&dim, cx, cz);
    Ok(json!({ "backup": backup }))
}

/// 删区块里的某个节点
#[tauri::command]
pub async fn nbt_delete_chunk_node(
    state: State<'_, AppState>,
    instance_id: String,
    world: String,
    dim: String,
    cx: i32,
    cz: i32,
    path: Vec<String>,
) -> Result<Json, String> {
    let (dir, backup) = prepare_chunk_write(&state, &instance_id, &world, &dim, cx, cz)?;
    let mut data = nbt::region::read_chunk(&dir, &dim, cx, cz)?;
    nbt::tree::delete_node_in(&mut data, &path)?;
    nbt::region::write_chunk(&dir, &dim, cx, cz, &data)?;
    nbt::map::invalidate(&dim, cx, cz);
    Ok(json!({ "backup": backup }))
}

// ---------------------------------------------------------------------------
// 玩家 / 背包 / 物品 / 树节点
// ---------------------------------------------------------------------------

fn safe_uuid(uuid: &str) -> Result<(), String> {
    // playerdata 文件名形如 <uuid>.dat，只允许十六进制与连字符
    if uuid
        .chars()
        .all(|c| c.is_ascii_hexdigit() || c == '-')
        && !uuid.is_empty()
    {
        Ok(())
    } else {
        Err("非法的玩家 UUID".into())
    }
}

/// 列出世界下所有玩家（名字用 usercache.json 本地解析）
#[tauri::command]
pub async fn nbt_list_players(
    state: State<'_, AppState>,
    instance_id: String,
    world: String,
) -> Result<Json, String> {
    Ok(json!({ "players": nbt::player::list_players(&state, &instance_id, &world)? }))
}

/// 读取玩家可编辑字段
#[tauri::command]
pub async fn nbt_read_player(
    state: State<'_, AppState>,
    instance_id: String,
    world: String,
    uuid: String,
) -> Result<Json, String> {
    safe_uuid(&uuid)?;
    nbt::player::read_player(&state, &instance_id, &world, &uuid)
}

/// 保存玩家修改（自动备份；游戏运行中拒绝）
#[tauri::command]
pub async fn nbt_save_player(
    state: State<'_, AppState>,
    instance_id: String,
    world: String,
    uuid: String,
    patch: Json,
) -> Result<Json, String> {
    safe_uuid(&uuid)?;
    ensure_not_running(&state, &instance_id)?;
    let dir = nbt::resolve_world(&state, &instance_id, &world)?;
    nbt::check_editable(&dir)?;
    let backup = nbt::player::save_player(&state, &instance_id, &world, &uuid, &patch)?;
    Ok(json!({ "backup": backup }))
}

/// 读取玩家背包与末影箱
#[tauri::command]
pub async fn nbt_read_inventory(
    state: State<'_, AppState>,
    instance_id: String,
    world: String,
    uuid: String,
) -> Result<Json, String> {
    safe_uuid(&uuid)?;
    nbt::item::read_inventory(&state, &instance_id, &world, &uuid)
}

/// 写回一个物品槽位
#[tauri::command]
pub async fn nbt_save_item(
    state: State<'_, AppState>,
    instance_id: String,
    world: String,
    uuid: String,
    container: String,
    index: usize,
    item: Json,
) -> Result<Json, String> {
    safe_uuid(&uuid)?;
    ensure_not_running(&state, &instance_id)?;
    let dir = nbt::resolve_world(&state, &instance_id, &world)?;
    nbt::check_editable(&dir)?;
    let backup =
        nbt::item::save_item(&state, &instance_id, &world, &uuid, &container, index, &item)?;
    Ok(json!({ "backup": backup }))
}

/// 树形模式：修改某个节点（`node_type` 用于新节点，已有节点按原类型写回）
#[tauri::command]
pub async fn nbt_set_node(
    state: State<'_, AppState>,
    instance_id: String,
    world: String,
    path: Vec<String>,
    value: Json,
    node_type: String,
) -> Result<Json, String> {
    ensure_not_running(&state, &instance_id)?;
    let dir = nbt::resolve_world(&state, &instance_id, &world)?;
    nbt::check_editable(&dir)?;
    let level_path = dir.join("level.dat");
    let backup = nbt::backup::backup_file(&level_path)?;
    nbt::tree::set_node(&level_path, path, &value, &node_type)?;
    Ok(json!({ "backup": backup }))
}

/// 树形模式：删除某个节点
#[tauri::command]
pub async fn nbt_delete_node(
    state: State<'_, AppState>,
    instance_id: String,
    world: String,
    path: Vec<String>,
) -> Result<Json, String> {
    ensure_not_running(&state, &instance_id)?;
    let dir = nbt::resolve_world(&state, &instance_id, &world)?;
    nbt::check_editable(&dir)?;
    let level_path = dir.join("level.dat");
    let backup = nbt::backup::backup_file(&level_path)?;
    nbt::tree::delete_node(&level_path, path)?;
    Ok(json!({ "backup": backup }))
}
