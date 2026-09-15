//! 云存档同步（GitHub）：授权、仓库初始化、快照上传/下载/删除。
//!
//! 设计要点：
//! - 存档存放在用户自己的私有仓库 Releases 中，启动器不托管任何数据
//! - 打包前排除 `session.lock`，恢复前自动备份现有世界（绝不静默覆盖）
//! - 每个世界保留的快照数可配（默认 5），超出时从最旧开始清理

pub mod auth;
pub mod github;
pub mod release;
pub mod repo;
pub mod saves;
pub mod store;

use crate::state::AppState;
use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::{Emitter, State};

/// 取 (token, account, repo_name)，顺带处理令牌刷新。
async fn ctx(state: &AppState) -> Result<(String, String, String), String> {
    let token = auth::ensure_token(state).await?;
    let cs = store::load(state);
    if cs.repo_name.is_empty() {
        return Err("尚未初始化云存档仓库，请先完成 GitHub 授权".into());
    }
    Ok((token, cs.account.clone(), cs.repo_name.clone()))
}

/// 连接状态（供前端初始化界面）。
/// connected 只反映"已授权"；仓库是否就绪用 repoReady 单独表示。
#[tauri::command]
pub fn cloud_sync_status(state: State<'_, AppState>) -> Value {
    let cs = store::load(&state);
    json!({
        "connected": !cs.access_token.is_empty(),
        "account": cs.account,
        "repoName": cs.repo_name,
        "keepPerWorld": release::keep_per_world(&state),
    })
}

/// 开始设备流程授权，返回用户码等信息。
#[tauri::command]
pub async fn cloud_sync_start_auth(state: State<'_, AppState>) -> Result<Value, String> {
    auth::start_device_flow(&state).await
}

/// 轮询授权结果：`{ status: "pending" | "ok" }`。
#[tauri::command]
pub async fn cloud_sync_poll_auth(
    state: State<'_, AppState>,
    device_code: String,
) -> Result<Value, String> {
    auth::poll_device_flow(&state, &device_code).await
}

/// 断开连接（清空本地令牌；云端数据不动）。
#[tauri::command]
pub fn cloud_sync_disconnect(state: State<'_, AppState>) -> Result<(), String> {
    auth::disconnect(&state)
}

/// 初始化/校验云存档仓库。
#[tauri::command]
pub async fn cloud_sync_init_repo(state: State<'_, AppState>) -> Result<Value, String> {
    let token = auth::ensure_token(&state).await?;
    let r = repo::ensure(&state, &token).await;
    match &r {
        Ok(v) => println!("[cloud_sync] init_repo ok: {}", v),
        Err(e) => println!("[cloud_sync] init_repo failed: {e}"),
    }
    r
}

/// 世界云同步信息：world_id、自动同步开关、云端快照列表。
#[tauri::command]
pub async fn cloud_sync_world_info(
    state: State<'_, AppState>,
    instance_id: String,
    world: String,
) -> Result<Value, String> {
    let world_id = saves::world_id_for(&state, &instance_id, &world);
    let cs = store::load(&state);
    let auto_sync = cs
        .worlds
        .iter()
        .find(|w| w.instance_id == instance_id && w.world_dir == world)
        .map(|w| w.auto_sync)
        .unwrap_or(false);
    // 未连接（未授权）时只返回本地信息；已授权但仓库未初始化时标记 repoReady
    if !cs.connected() {
        return Ok(json!({ "worldId": world_id, "autoSync": auto_sync, "connected": false, "repoReady": false, "snapshots": [] }));
    }
    let token = auth::ensure_token(&state).await?;
    let repo_ready = !cs.repo_name.is_empty();
    let snapshots = if repo_ready {
        release::list(&state, &token, &cs.repo_name, &cs.account).await?
    } else {
        Vec::new()
    };
    let mine: Vec<Value> = snapshots
        .into_iter()
        .filter(|s| s.get("worldId").and_then(|v| v.as_str()) == Some(world_id.as_str()))
        .collect();
    Ok(json!({ "worldId": world_id, "autoSync": auto_sync, "connected": true, "repoReady": repo_ready, "snapshots": mine }))
}

/// 上传该世界的一个快照。返回新 Release id。
/// 过程中通过 `cloud_sync://progress` 事件推送阶段与进度。
#[tauri::command]
pub async fn cloud_sync_upload(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    instance_name: String,
    world: String,
    world_name: String,
    game_version: String,
) -> Result<Value, String> {
    let emit = |payload: Value| {
        let _ = app.emit("cloud_sync://progress", payload);
    };
    let (token, account, repo_name) = ctx(&state).await?;
    let world_id = saves::world_id_for(&state, &instance_id, &world);

    // 打包（CPU 密集，放到阻塞线程池）；超过 2GB 自动分卷
    emit(json!({ "kind": "upload", "step": "pack", "sent": 0u64, "total": 0u64, "msg": "正在打包世界存档…" }));
    let root = state.root.clone();
    let (inst, wld) = (instance_id.clone(), world.clone());
    let app3 = app.clone();
    let (parts, size) = tokio::task::spawn_blocking(move || {
        saves::pack_world(&root, &inst, &wld, &move |done, total| {
            let _ = app3.emit(
                "cloud_sync://progress",
                json!({ "kind": "upload", "step": "pack", "sent": done, "total": total }),
            );
        })
    })
    .await
    .map_err(|e| format!("打包任务失败: {e}"))??;
    emit(json!({ "kind": "upload", "step": "hash", "msg": "正在计算校验值…" }));
    let sha = {
        let p = parts[0].path.clone();
        tokio::task::spawn_blocking(move || saves::sha256_file(&p))
            .await
            .map_err(|e| format!("校验任务失败: {e}"))??
    };

    let meta = json!({
        "world_id": world_id,
        "world_name": world_name,
        "instance_id": instance_id,
        "instance_name": instance_name,
        "game_version": game_version,
        "created_at": chrono_like_now(),
        "size_bytes": size,
        "sha256": sha,
    });
    emit(json!({ "kind": "upload", "step": "upload", "sent": 0u64, "total": size, "msg": "正在上传快照…" }));
    let app2 = app.clone();
    let release_id = release::create_and_upload(
        &state, &token, &account, &repo_name, &world_id, &meta, &parts,
        move |sent, total| {
            let _ = app2.emit("cloud_sync://progress", json!({ "kind": "upload", "step": "upload", "sent": sent, "total": total }));
        },
    )
    .await?;
    for p in &parts {
        let _ = std::fs::remove_file(&p.path);
    }

    // 超出配额时清理该世界最旧的快照
    emit(json!({ "kind": "upload", "step": "cleanup", "msg": "正在清理旧快照…" }));
    let keep = release::keep_per_world(&state);
    let all = release::list(&state, &token, &repo_name, &account)
        .await
        .unwrap_or_default();
    let cleaned = release::cleanup_quota(&state, &token, &account, &repo_name, &world_id, &all, keep)
        .await
        .unwrap_or(0);
    Ok(json!({ "releaseId": release_id, "sizeBytes": size, "cleaned": cleaned }))
}

/// 列出云端全部快照（供"从云端恢复"浏览模式，前端按 worldId 分组）。
#[tauri::command]
pub async fn cloud_sync_list_all(state: State<'_, AppState>) -> Result<Value, String> {
    let (token, account, repo_name) = ctx(&state).await?;
    let list = release::list(&state, &token, &repo_name, &account).await?;
    Ok(json!({ "snapshots": list }))
}

/// 从云端快照恢复世界（自动备份现有目录）。按 releaseId 下载全部分卷后合并解压。
/// `world_id`：浏览模式恢复丢失的世界时传入，恢复后重建本地目录与云端世界的映射。
#[tauri::command]
pub async fn cloud_sync_restore(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    world: String,
    release_id: u64,
    world_id: Option<String>,
) -> Result<Value, String> {
    let emit = |payload: Value| {
        let _ = app.emit("cloud_sync://progress", payload);
    };
    let (token, account, repo_name) = ctx(&state).await?;
    let tmp_dir = state.root.join("cloud_sync").join("tmp");
    std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("创建临时目录失败: {e}"))?;

    // 列出该快照的全部分卷（按名字排序即 part 顺序）
    let assets = release::list_assets(&state, &token, &account, &repo_name, release_id).await?;
    if assets.is_empty() {
        return Err("该快照没有可下载的存档文件".into());
    }
    let grand_total: u64 = assets.iter().map(|a| a.1).sum();
    emit(json!({ "kind": "restore", "step": "download", "sent": 0u64, "total": grand_total, "msg": "正在下载云端快照…" }));

    // 逐卷下载（进度跨卷连续）
    let mut zip_paths: Vec<PathBuf> = Vec::new();
    let mut done = 0u64;
    for (asset_id, size, name) in &assets {
        let zip_path = tmp_dir.join(format!("restore-{release_id}-{name}"));
        let base = done;
        let app3 = app.clone();
        release::download_asset(
            &state, &token, &account, &repo_name, *asset_id, &zip_path,
            move |received, _total| {
                let _ = app3.emit(
                    "cloud_sync://progress",
                    json!({ "kind": "restore", "step": "download", "sent": base + received, "total": grand_total }),
                );
            },
        )
        .await?;
        done += size;
        zip_paths.push(zip_path);
    }

    emit(json!({ "kind": "restore", "step": "unpack", "msg": "正在备份并恢复存档…" }));
    let root = state.root.clone();
    let (inst, wld) = (instance_id.clone(), world.clone());
    let backup = tokio::task::spawn_blocking(move || {
        let backup = saves::unpack_to_world(&root, &inst, &wld, &zip_paths);
        for p in &zip_paths {
            let _ = std::fs::remove_file(p);
        }
        backup
    })
    .await
    .map_err(|e| format!("解压任务失败: {e}"))??;
    // 浏览模式恢复：把恢复出的本地目录重新关联到云端 world_id，避免后续上传产生重复世界
    if let Some(wid) = world_id.filter(|w| !w.is_empty()) {
        let mut cs = store::load(&state);
        if !cs
            .worlds
            .iter()
            .any(|w| w.instance_id == instance_id && w.world_dir == world)
        {
            cs.worlds.push(store::WorldLink {
                instance_id,
                world_dir: world,
                world_id: wid,
                auto_sync: false,
            });
            let _ = store::save(&state, &cs);
        }
    }
    Ok(json!({ "backupName": backup }))
}

/// 删除云端快照。
#[tauri::command]
pub async fn cloud_sync_delete(state: State<'_, AppState>, release_id: u64) -> Result<(), String> {
    let (token, account, repo_name) = ctx(&state).await?;
    release::delete(&state, &token, &account, &repo_name, release_id).await
}

/// 设置自动同步开关（游戏退出后自动上传）。
#[tauri::command]
pub fn cloud_sync_set_auto(
    state: State<'_, AppState>,
    instance_id: String,
    world: String,
    enabled: bool,
) -> Result<(), String> {
    saves::set_auto_sync(&state, &instance_id, &world, enabled)
}

/// 设置每个世界保留的快照数上限。
#[tauri::command]
pub fn cloud_sync_set_keep(state: State<'_, AppState>, keep: u32) -> Result<(), String> {
    let mut cs = store::load(&state);
    cs.keep_per_world = keep.clamp(1, 50);
    store::save(&state, &cs)
}

/// ISO8601 时间戳（无 chrono 依赖，用 SystemTime 组装）
fn chrono_like_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 只保留 unix 秒，前端用 fmtDate 转本地展示
    secs.to_string()
}

/// 游戏退出后的自动上传：仅当该世界开启了 auto_sync 且游戏确实从此世界启动时调用。
/// 失败只返回错误信息（由调用方写日志），不打断退出流程。
pub async fn auto_upload_after_exit(
    state: &AppState,
    instance_id: &str,
    world: &str,
    game_version: &str,
) -> Result<(), String> {
    let cs = store::load(state);
    let enabled = cs
        .worlds
        .iter()
        .any(|w| w.instance_id == instance_id && w.world_dir == world && w.auto_sync);
    if !enabled {
        return Ok(());
    }
    if !cs.connected() || cs.repo_name.is_empty() {
        return Ok(()); // 未连接则静默跳过
    }
    let token = auth::ensure_token(state).await?;
    let world_id = saves::world_id_for(state, instance_id, world);
    let instance_name = crate::instances::get_instance(state, instance_id)
        .map(|i| i.name)
        .unwrap_or_default();
    let (parts, size) = saves::pack_world(&state.root, instance_id, world, &|_, _| {})?;
    let sha = saves::sha256_file(&parts[0].path)?;
    let meta = json!({
        "world_id": world_id,
        "world_name": world,
        "instance_id": instance_id,
        "instance_name": instance_name,
        "game_version": game_version,
        "created_at": chrono_like_now(),
        "size_bytes": size,
        "sha256": sha,
        "auto": true,
    });
    let release_id = release::create_and_upload(
        state,
        &token,
        &cs.account,
        &cs.repo_name,
        &world_id,
        &meta,
        &parts,
        |_, _| {},
    )
    .await?;
    for p in &parts {
        let _ = std::fs::remove_file(&p.path);
    }
    let keep = release::keep_per_world(state);
    let all = release::list(state, &token, &cs.repo_name, &cs.account)
        .await
        .unwrap_or_default();
    let _ = release::cleanup_quota(state, &token, &cs.account, &cs.repo_name, &world_id, &all, keep).await;
    let _ = release_id;
    Ok(())
}
