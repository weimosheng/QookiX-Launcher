use crate::accounts;
use crate::install;
use crate::launch;
use crate::models::*;
use crate::modrinth;
use crate::state::AppState;
use serde_json::{json, Value};
use tauri::Emitter;
use tauri::Manager;
use tauri::State;

// Instances
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_instances(state: State<AppState>) -> Result<Vec<Instance>, String> {
    Ok(crate::instances::load_instances(&state))
}

#[tauri::command]
pub fn get_instance_info(state: State<AppState>, id: String) -> Result<Instance, String> {
    crate::instances::get_instance(&state, &id)
}

#[tauri::command]
pub fn create_instance(
    app: tauri::AppHandle,
    state: State<AppState>,
    name: String,
    mc_version: String,
    loader: String,
    loader_version: Option<String>,
) -> Result<Instance, String> {
    let l: LoaderType = loader.parse()?;
    let mc = mc_version.clone();
    let instance = crate::instances::create_instance(&state, name, mc_version, l, loader_version)?;

    // Fabric（及兼容的 Quilt）实例自动补装 Fabric API：绝大多数模组的前置，
    // 后台 best-effort 安装，失败不影响实例创建。进度走通用 install://progress 事件。
    if matches!(instance.loader, LoaderType::Fabric | LoaderType::Quilt) {
        let app2 = app.clone();
        let iid = instance.id.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = modrinth::auto_install_fabric_api(&app2, &iid, &mc).await {
                eprintln!("[fabric-api] 自动安装失败（实例 {iid}，MC {mc}）: {e}");
            }
        });
    }
    Ok(instance)
}

#[tauri::command]
pub fn update_instance_settings(state: State<AppState>, patch: Value) -> Result<Instance, String> {
    crate::instances::update_instance(&state, patch)
}

#[tauri::command]
pub fn delete_instance(state: State<AppState>, id: String) -> Result<(), String> {
    crate::instances::delete_instance(&state, &id)
}

#[tauri::command]
pub fn list_instance_groups(state: State<AppState>) -> Vec<InstanceGroup> {
    crate::instances::load_groups(&state)
}

#[tauri::command]
pub fn create_instance_group(
    state: State<AppState>,
    name: String,
    color: Option<String>,
) -> Result<InstanceGroup, String> {
    crate::instances::create_group(&state, name, color)
}

#[tauri::command]
pub fn rename_instance_group(
    state: State<AppState>,
    id: String,
    name: String,
    color: Option<String>,
) -> Result<InstanceGroup, String> {
    crate::instances::rename_group(&state, &id, name, color)
}

#[tauri::command]
pub fn delete_instance_group(state: State<AppState>, id: String) -> Result<(), String> {
    crate::instances::delete_group(&state, &id)
}

#[tauri::command]
pub fn reorder_instance_groups(
    state: State<AppState>,
    ids: Vec<String>,
) -> Result<Vec<InstanceGroup>, String> {
    crate::instances::reorder_groups(&state, ids)
}

#[tauri::command]
pub async fn install_game(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<InstallPlan, String> {
    let instance = crate::instances::get_instance(&state, &instance_id)?;
    let plan = install::install_game(app.clone(), &state, &instance).await?;
    crate::instances::mark_installed(&state, &instance_id)?;
    Ok(plan)
}

#[tauri::command]
pub fn cancel_install(state: State<AppState>) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    state.install_cancel.store(true, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub async fn launch_instance(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
    world: Option<String>,
    server: Option<String>,
) -> Result<LaunchResult, String> {
    // 启动失败时悬浮进度卡没有其它收尾信号（launch://log 只在游戏真正
    // 跑起来、launch://exit 只在进程退出时发），不补发的话卡片会永远
    // 停在当前百分比。这里统一兜底：失败即广播 exit 收起卡片。
    let result = launch_instance_inner(&app, &state, &instance_id, world, server).await;
    if result.is_err() {
        let _ = app.emit("launch://exit", serde_json::json!({}));
    }
    result
}

async fn launch_instance_inner(
    app: &tauri::AppHandle,
    state: &State<'_, AppState>,
    instance_id: &str,
    world: Option<String>,
    server: Option<String>,
) -> Result<LaunchResult, String> {
    let instance = crate::instances::get_instance(state, instance_id)?;
    // resolve account: instance override -> global selected -> first
    let accounts = accounts::load_accounts(state);
    let selected = {
        let s = state.settings.read().unwrap();
        s.selected_account.clone()
    };
    let account = if let Some(aid) = &instance.account_id {
        accounts.iter().find(|a| a.uuid() == aid).cloned()
    } else if let Some(aid) = &selected {
        accounts.iter().find(|a| a.uuid() == aid).cloned()
    } else {
        accounts.first().cloned()
    };
    let account = account.ok_or("请先在左下角账号栏添加账号（正版或离线）")?;
    let _ = app.emit("launch://progress", serde_json::json!({ "step": "正在登录账号…", "progress": 10 }));
    let account = accounts::refresh_microsoft(&state, &account).await?;
    let _ = app.emit("launch://progress", serde_json::json!({ "step": "账号准备完成", "progress": 25 }));
    let resolved = launch::ResolvedAccount {
        username: account.username().to_string(),
        uuid: account.uuid().to_string(),
        access_token: match &account {
            Account::Microsoft { msa_access_token, .. } => msa_access_token.clone(),
            Account::Offline { .. } => "0".into(),
        },
        user_type: if account.is_microsoft() { "msa".into() } else { "legacy".into() },
        user_properties: "{}".into(),
    };
    let result = launch::launch_game(app.clone(), &state, &instance, resolved, world, server).await?;
    crate::instances::touch_last_played(&state, &instance_id);
    Ok(result)
}

#[tauri::command]
pub async fn stop_game(state: State<'_, AppState>) -> Result<(), String> {
    launch::kill_game(&state).await
}

#[tauri::command]
pub fn is_game_running(state: State<AppState>) -> Result<bool, String> {
    Ok(launch::is_running(&state))
}

#[tauri::command]
pub fn open_instance_folder(
    app: tauri::AppHandle,
    state: State<AppState>,
    instance_id: String,
    sub: Option<String>,
) -> Result<(), String> {
    let mut dir = state.instances_dir().join(&instance_id);
    if let Some(s) = sub {
        if !SUBFOLDERS.contains(&s.as_str()) {
            return Err("非法目录".into());
        }
        dir = dir.join(s);
    }
    if !dir.exists() {
        return Err("目录不存在".into());
    }
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Known instance subfolders usable in folder/file commands.
pub const SUBFOLDERS: [&str; 9] = [
    "mods", "shaderpacks", "resourcepacks", "saves", "screenshots", "config", "logs", "natives", "icons",
];

#[tauri::command]
pub fn list_instance_folders(state: State<AppState>, instance_id: String) -> Result<Value, String> {
    let dir = state.instances_dir().join(&instance_id);
    let folders: Vec<Value> = SUBFOLDERS
        .iter()
        .map(|f| json!({ "name": f, "exists": dir.join(f).is_dir() }))
        .collect();
    Ok(json!({ "folders": folders }))
}

#[tauri::command]
pub async fn list_instance_files(
    state: State<'_, AppState>,
    instance_id: String,
    sub: String,
) -> Result<Value, String> {
    if !SUBFOLDERS.contains(&sub.as_str()) {
        return Err("非法目录".into());
    }
    let dir = state.instances_dir().join(&instance_id).join(&sub);
    if !dir.exists() {
        return Ok(json!({ "files": [] }));
    }
    let files = tokio::task::spawn_blocking(move || {
        let mut files: Vec<Value> = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
            let e = entry.map_err(|e| e.to_string())?;
            let meta = e.metadata().map_err(|e| e.to_string())?;
            let path = e.path();
            let mut icon: Option<String> = None;
            if sub == "saves" && meta.is_dir() {
                let icon_path = path.join("icon.png");
                if icon_path.is_file() {
                    icon = Some(icon_path.to_string_lossy().to_string());
                }
            }
            files.push(json!({
                "name": e.file_name().to_string_lossy().to_string(),
                "path": path.to_string_lossy().to_string(),
                "size": meta.len(),
                "modified": meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0),
                "isDir": meta.is_dir(),
                "icon": icon,
            }));
        }
        files.sort_by(|a, b| {
            let da = a.get("isDir").and_then(|v| v.as_bool()).unwrap_or(false);
            let db = b.get("isDir").and_then(|v| v.as_bool()).unwrap_or(false);
            db.cmp(&da).then_with(|| {
                a.get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_lowercase()
                    .cmp(&b.get("name").and_then(|v| v.as_str()).unwrap_or("").to_lowercase())
            })
        });
        Ok::<Vec<Value>, String>(files)
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(json!({ "files": files }))
}

// ---------------------------------------------------------------------------
// Playtime stats / instance export & import
// ---------------------------------------------------------------------------

/// 游玩时长统计：实例排行 + 近 30 天按天曲线
#[tauri::command]
pub fn playtime_stats(state: State<AppState>) -> Result<Value, String> {
    let all = crate::instances::load_instances(&state);
    let mut by_instance: Vec<Value> = all
        .iter()
        .map(|i| {
            json!({
                "id": i.id,
                "name": i.name,
                "icon": i.icon,
                "seconds": i.total_play_time,
                "lastPlayed": i.last_played,
            })
        })
        .collect();
    by_instance.sort_by(|a, b| {
        let sa = a["seconds"].as_u64().unwrap_or(0);
        let sb = b["seconds"].as_u64().unwrap_or(0);
        sb.cmp(&sa)
    });
    let total: u64 = all.iter().map(|i| i.total_play_time).sum();

    // 按天：只返回最近 30 天（day → 秒）
    let daily = crate::instances::daily_play_time(state.inner());
    let today = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        + 8 * 3600)
        / 86400;
    let by_day: Vec<Value> = (0..30)
        .rev()
        .filter_map(|offset| {
            let day = today - offset;
            let secs = daily.get(&day.to_string()).copied().unwrap_or(0);
            Some(json!({ "day": day, "seconds": secs }))
        })
        .collect();

    Ok(json!({
        "totalSeconds": total,
        "byInstance": by_instance,
        "byDay": by_day,
    }))
}

/// 识别手动放入、未登记的模组在 Modrinth 上的来源，使其可按引用导出（减小包体积）
#[tauri::command]
pub async fn identify_manual_mods(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Vec<crate::instance_share::IdentifiedMod>, String> {
    crate::instance_share::identify_manual_mods(&app, state.inner(), &instance_id).await
}

/// 导出前扫描实例的可勾选项（模组/资源包/光影/截图/存档/附属文件夹/设置）
#[tauri::command]
pub fn export_preview(
    state: State<AppState>,
    instance_id: String,
) -> Result<crate::instance_share::ExportPreview, String> {
    crate::instance_share::preview(state.inner(), &instance_id)
}

#[tauri::command]
pub fn export_instance_pack(
    state: State<AppState>,
    instance_id: String,
    dest_path: String,
    selection: crate::instance_share::ExportSelection,
) -> Result<usize, String> {
    crate::instance_share::export_pack(
        state.inner(),
        &instance_id,
        std::path::Path::new(&dest_path),
        selection,
    )
}

/// 导入实例分享包：新建实例并还原包内内容；
/// 在线来源（Modrinth/CurseForge）且包内无文件的条目在后台逐个重新下载。
#[tauri::command]
pub async fn import_instance_pack(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    file_path: String,
) -> Result<Value, String> {
    let (id, pending) =
        crate::instance_share::import_pack(state.inner(), std::path::Path::new(&file_path))?;
    let instance = crate::instances::get_instance(&state, &id)?;
    let pending_count = pending.len();

    // 后台串起整个补装流程：
    //   1) 游戏本体（MC + 资产 + 库）
    //   2) 在线内容 4 路并发补下载
    //   3) 兜底再标一次「已安装」
    // 第 3 步是必须的：mods 安装内部 save_instance 持有的是导入时的旧实例
    // 副本（installed=false），会把第 1 步标记的已安装状态覆盖回去。
    let app2 = app.clone();
    let iid = id.clone();
    tauri::async_runtime::spawn(async move {
        // 1) 游戏本体；失败不阻塞内容下载（详情页有手动安装入口）
        let mut install_ok = false;
        let res: Result<(), String> = async {
            let state = app2.state::<crate::state::AppState>();
            let inst = crate::instances::get_instance(&state, &iid)?;
            crate::install::install_game(app2.clone(), &state, &inst).await?;
            let _ = crate::util::log_best_effort(
                "mark_installed",
                crate::instances::mark_installed(&state, &iid),
            );
            Ok(())
        }
        .await;
        if let Err(e) = res {
            // 失败原因直接弹给前端（只靠终端日志用户看不到）
            let _ = app2.emit(
                "share-import://game-install-failed",
                serde_json::json!({ "instanceId": iid.clone(), "error": e }),
            );
        } else {
            install_ok = true;
        }

        // 2) 在线内容补下载（4 路并发：串行太慢，全并发会挤爆 API 限流）
        if !pending.is_empty() {
            use futures_util::StreamExt;
            futures_util::stream::iter(pending)
                .for_each_concurrent(4, |d| {
                    let app = app2.clone();
                    let id = iid.clone();
                    async move {
                        let res: Result<Value, String> = async {
                            let state = app.state::<crate::state::AppState>();
                            // 每次重新取实例：前一次安装会更新实例记录
                            let inst = crate::instances::get_instance(&state, &id)?;
                            match d.provider.as_str() {
                                "modrinth" => {
                                    crate::modrinth::install_version(
                                        app.clone(),
                                        &state,
                                        &inst,
                                        &d.version_id,
                                        &d.kind,
                                    )
                                    .await
                                }
                                "curseforge" => {
                                    crate::curseforge::install_file(
                                        app.clone(),
                                        &state,
                                        &inst,
                                        &d.project_id,
                                        &d.version_id,
                                        &d.kind,
                                    )
                                    .await
                                }
                                _ => Err("未知内容源".into()),
                            }
                        }
                        .await;
                        if let Err(e) = res {
                            eprintln!(
                                "[share-import] 补下载失败（{} {}）: {e}",
                                d.provider,
                                d.name.unwrap_or_else(|| d.version_id.clone())
                            );
                        }
                    }
                })
                .await;
        }

        // 3) 兜底标记已安装——仅在本体安装成功时。
        //    mods 安装内部 save_instance 持有旧实例副本（installed=false），
        //    会把已安装状态覆盖回去，所以成功时最后再标一次；
        //    本体安装失败则保持未安装，让详情页提示条出现、用户可手动装。
        if install_ok {
            let state = app2.state::<crate::state::AppState>();
            let _ = crate::util::log_best_effort(
                "mark_installed",
                crate::instances::mark_installed(&state, &iid),
            );
        }
    });
    Ok(json!({ "instance": instance, "pendingDownloads": pending_count }))
}

