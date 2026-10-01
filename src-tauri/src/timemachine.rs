//! 时光机：按"定时 / 游戏内事件 / 日志正则"自动截图，上传到用户自己的独立
//! GitHub 仓库（与云存档互不干扰）供时间轴回顾。共用云存档的授权与 Release
//! 读写，只是仓库名、附件类型（image/png）与元数据不同。

use crate::cloud_sync::{auth, release, repo, saves};
use crate::state::AppState;
use tauri::{Emitter, Manager};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub const DEFAULT_REPO: &str = "Qookix-TimeMachine";

// ---- 配置 ----

#[derive(Clone, Serialize, Deserialize)]
pub struct CustomTrigger {
    pub id: String,
    pub label: String,
    pub pattern: String,
    pub enabled: bool,
    /// 冷却秒数，避免一条规则连续刷屏
    pub cooldown_secs: u64,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub repo_name: String,
    pub enabled: bool,
    /// 启用 GitHub 云保存。关掉＝纯本地：截图只留在这台电脑上，不碰网络。
    /// （老配置文件里没有这个字段，`serde(default)` 让它平滑升级成"关闭"）
    #[serde(default)]
    pub cloud_enabled: bool,
    /// 攒够几张截图打包成一个 Release 上传（一张图一个 Release 太浪费）
    #[serde(default = "default_batch_size")]
    pub batch_size: u32,
    /// 定时截图间隔（秒）
    pub interval_secs: u64,
    /// 每个实例云端保留的记忆点数
    pub keep_per_instance: u32,
    /// 内置触发开关
    pub on_interval: bool,
    pub on_world_enter: bool,
    pub on_world_exit: bool,
    pub on_death: bool,
    pub on_advancement: bool,
    /// 只在游戏窗口处于前台时截图（避免截到别的东西）
    pub foreground_only: bool,
    /// 本地也保留一份截图副本
    pub keep_local_copy: bool,
    pub custom: Vec<CustomTrigger>,
}

/// 每批上传的张数范围（前端输入框同步）。上限 500 是 GitHub 对单个 Release
/// 附件数的限制，下限 10——再小就退化成"一张图一个 Release"了。
const BATCH_MIN: u32 = 10;
const BATCH_MAX: u32 = 500;

fn default_batch_size() -> u32 {
    BATCH_MIN
}

impl Default for Config {
    fn default() -> Self {
        Self {
            repo_name: DEFAULT_REPO.to_string(),
            enabled: false,
            cloud_enabled: false,
            batch_size: default_batch_size(),
            interval_secs: 300,
            keep_per_instance: 500,
            on_interval: true,
            on_world_enter: true,
            on_world_exit: true,
            on_death: true,
            on_advancement: true,
            foreground_only: true,
            keep_local_copy: true,
            custom: Vec::new(),
        }
    }
}

fn state_path(root: &Path) -> PathBuf {
    root.join("timemachine").join("state.json")
}

pub fn load(state: &AppState) -> Config {
    std::fs::read_to_string(state_path(&state.root))
        .ok()
        .and_then(|s| serde_json::from_str::<Config>(&s).ok())
        .unwrap_or_default()
}

pub fn save(state: &AppState, cfg: &Config) -> Result<(), String> {
    let path = state_path(&state.root);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let text = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| format!("写入失败: {e}"))
}

/// 本地截图存放目录（保留副本时用）
fn shots_dir(state: &AppState, instance_id: &str) -> PathBuf {
    state.root.join("timemachine").join("shots").join(instance_id)
}

// ---- 本地索引（本地优先：截图先落盘记账，攒够一批才上传）----

/// 一张本地截图。上传成功后补上 release / asset id，时间轴据此按需回源下载。
#[derive(Clone, Serialize, Deserialize)]
pub struct LocalShot {
    pub file: String,
    pub trigger: String,
    pub created_at: u64,
    /// 截图那一刻游戏日志的末尾几行（"当时发生了什么"）
    #[serde(default)]
    pub log_snippet: String,
    #[serde(default)]
    pub uploaded: bool,
    #[serde(default)]
    pub release_id: u64,
    #[serde(default)]
    pub asset_id: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct ShotIndex {
    #[serde(default)]
    shots: Vec<LocalShot>,
}

fn index_path(state: &AppState, instance_id: &str) -> PathBuf {
    shots_dir(state, instance_id).join("index.json")
}

/// 某个实例的本地截图账本（含已上传、本地文件可能已删的条目）
pub fn load_index(state: &AppState, instance_id: &str) -> Vec<LocalShot> {
    let mut v: Vec<LocalShot> = std::fs::read_to_string(index_path(state, instance_id))
        .ok()
        .and_then(|s| serde_json::from_str::<ShotIndex>(&s).ok())
        .map(|i| i.shots)
        .unwrap_or_default();
    v.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    v
}

fn save_index(state: &AppState, instance_id: &str, shots: &[LocalShot]) -> Result<(), String> {
    let path = index_path(state, instance_id);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let text = serde_json::to_string_pretty(&ShotIndex {
        shots: shots.to_vec(),
    })
    .map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| format!("写入失败: {e}"))
}

/// 记一张刚截好的本地截图（上传是后面的事）
fn record_local(
    state: &AppState,
    instance_id: &str,
    file: &str,
    trigger: &str,
    created_at: u64,
    log_snippet: &str,
) -> Result<(), String> {
    let mut shots = load_index(state, instance_id);
    shots.retain(|s| s.file != file);
    shots.push(LocalShot {
        file: file.to_string(),
        trigger: trigger.to_string(),
        created_at,
        log_snippet: log_snippet.to_string(),
        uploaded: false,
        release_id: 0,
        asset_id: 0,
    });
    shots.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    save_index(state, instance_id, &shots)
}

/// 读文件末尾 n 行非空内容。只读末尾 16 KiB：latest.log 动辄几 MB，
/// 而截图可能每 10 秒一次。
fn tail_lines(path: &Path, n: usize) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return String::new();
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let from = len.saturating_sub(16 * 1024);
    if f.seek(SeekFrom::Start(from)).is_err() {
        return String::new();
    }
    let mut buf = Vec::new();
    if f.read_to_end(&mut buf).is_err() {
        return String::new();
    }
    // 老版本 MC 的日志是 GBK：不能用 from_utf8_lossy，否则中文全成乱码
    let text = crate::util::decode_text(&buf);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

/// 待上传的本地截图（按时间正序）
fn pending_shots(state: &AppState, instance_id: &str) -> Vec<LocalShot> {
    load_index(state, instance_id)
        .into_iter()
        .filter(|s| !s.uploaded)
        .collect()
}

/// 有截图待上传的实例 id
fn instances_with_pending(state: &AppState) -> Vec<String> {
    let mut out = Vec::new();
    let root = state.root.join("timemachine").join("shots");
    let Ok(rd) = std::fs::read_dir(&root) else {
        return out;
    };
    for e in rd.flatten() {
        if !e.path().is_dir() {
            continue;
        }
        let Some(id) = e.file_name().to_str().map(|s| s.to_string()) else {
            continue;
        };
        if !pending_shots(state, &id).is_empty() {
            out.push(id);
        }
    }
    out.sort();
    out
}

// ---- 仓库 ----

async fn ctx(state: &AppState) -> Result<(String, String, String), String> {
    let token = auth::ensure_token(state).await?;
    let cfg = load(state);
    let cs = crate::cloud_sync::store::load(state);
    if cs.account.is_empty() {
        return Err("尚未完成 GitHub 授权".into());
    }
    Ok((token, cs.account.clone(), cfg.repo_name))
}

/// 本会话里已经确认存在的时光机仓库（免得每次截图都多打一次 API）
fn repo_known_ok() -> &'static std::sync::Mutex<Option<String>> {
    static OK: std::sync::OnceLock<std::sync::Mutex<Option<String>>> =
        std::sync::OnceLock::new();
    OK.get_or_init(|| std::sync::Mutex::new(None))
}

/// 确保仓库存在（不存在就建私有仓库 + 写标识文件），结果缓存本会话。
/// 必须在上传前确认：`repo_name` 有默认值、界面看不出"还没建过"，
/// 直接建 Release 会 404。
pub async fn ensure_repo_ready(
    state: &AppState,
    token: &str,
    account: &str,
    repo: &str,
) -> Result<String, String> {
    if repo_known_ok()
        .lock()
        .map(|g| g.as_deref() == Some(repo))
        .unwrap_or(false)
    {
        return Ok(repo.to_string());
    }
    let (name, _rid, created) = repo::ensure_repo(
        state,
        token,
        account,
        repo,
        "Qookix Launcher 时光机：自动截图回顾（自动创建；请勿手动修改 qookix.json）",
        repo::MARKER_TYPE_TIMEMACHINE,
        "chore: 初始化 Qookix 时光机仓库",
    )
    .await?;
    if created {
        println!("[timemachine] repo created: {name}");
    }
    if let Ok(mut g) = repo_known_ok().lock() {
        *g = Some(name.clone());
    }
    Ok(name)
}

/// 初始化时光机仓库（独立仓库，不影响云存档仓库）
pub async fn init_repo(state: &AppState) -> Result<Value, String> {
    let token = auth::ensure_token(state).await?;
    let mut cfg = load(state);
    let cs = crate::cloud_sync::store::load(state);
    if cs.account.is_empty() {
        return Err("账号信息缺失，请重新授权".into());
    }
    let (name, rid, created) = repo::ensure_repo(
        state,
        &token,
        &cs.account,
        &cfg.repo_name,
        "Qookix Launcher 时光机：自动截图回顾（自动创建；请勿手动修改 qookix.json）",
        repo::MARKER_TYPE_TIMEMACHINE,
        "chore: 初始化 Qookix 时光机仓库",
    )
    .await?;
    if let Ok(mut g) = repo_known_ok().lock() {
        *g = Some(name.clone());
    }
    if cfg.repo_name != name {
        cfg.repo_name = name.clone();
        save(state, &cfg)?;
    }
    println!("[timemachine] init_repo ok: {name} created={created}");
    Ok(json!({ "repo": name, "repositoryId": rid, "created": created }))
}

// ---- 截图（Windows：向游戏窗口发送 F2 按键）----

/// 截一张图：先落本地并记账，再按需攒批上传。
/// `trigger`：interval / world_enter / world_exit / death / advancement /
/// custom:<id> / manual。上传失败不丢本地截图（本地是主存储），错误随
/// `uploadError` 返回，由 `fire` 转成前端提示。
pub async fn capture_and_upload(
    state: &AppState,
    instance_id: &str,
    trigger: &str,
    _world: &str,
) -> Result<Value, String> {
    let instance_dir = state.instances_dir().join(instance_id);
    let png = capture_game_screenshot(state, instance_id, &instance_dir)?;
    let size = std::fs::metadata(&png).map(|m| m.len()).unwrap_or(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let file = png
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    // 顺手抓一份"当时发生了什么"：日志末尾十几行（冒险日志里点开大图能看到）
    let snippet = tail_lines(&instance_dir.join("logs").join("latest.log"), 12);
    record_local(state, instance_id, &file, trigger, stamp, &snippet)?;

    let cfg = load(state);
    let mut upload_error = Value::Null;
    let mut uploaded = 0u64;
    let mut release_id = 0u64;
    let mut cleaned = 0u64;
    if cfg.cloud_enabled {
        match flush(state, instance_id, false).await {
            Ok(v) => {
                uploaded = v["uploaded"].as_u64().unwrap_or(0);
                release_id = v["releaseId"].as_u64().unwrap_or(0);
                cleaned = v["cleaned"].as_u64().unwrap_or(0);
                // 上传过的本地副本：用户不要就删掉（索引条目留着，时间轴回源下载）
                if uploaded > 0 && !cfg.keep_local_copy {
                    for s in load_index(state, instance_id).iter().filter(|s| s.uploaded) {
                        let _ = std::fs::remove_file(shots_dir(state, instance_id).join(&s.file));
                    }
                }
            }
            Err(e) => upload_error = json!(e),
        }
    }

    Ok(json!({
        "releaseId": release_id,
        "sizeBytes": size,
        "file": file,
        "uploaded": uploaded,
        "pending": pending_shots(state, instance_id).len(),
        "cleaned": cleaned,
        "uploadError": upload_error,
    }))
}

/// 把某个实例的待传截图打包成一个 Release 上传（一次 API 传一批）。
/// `force=false` 时攒够 `batch_size` 才传，`force=true`（手动上传 / 游戏退出
/// 收尾）有多少传多少；返回 `{ uploaded, releaseId, waiting, cleaned }`。
pub async fn flush(state: &AppState, instance_id: &str, force: bool) -> Result<Value, String> {
    let cfg = load(state);
    if !cfg.cloud_enabled {
        return Err("未启用 GitHub 云保存".into());
    }
    let pending = pending_shots(state, instance_id);
    if pending.is_empty() {
        return Ok(json!({ "uploaded": 0, "releaseId": 0, "waiting": 0, "cleaned": 0 }));
    }
    let batch = cfg.batch_size.clamp(BATCH_MIN, BATCH_MAX) as usize;
    if !force && pending.len() < batch {
        return Ok(json!({ "uploaded": 0, "releaseId": 0, "waiting": pending.len(), "cleaned": 0 }));
    }
    // force 时有多少传多少（游戏退出收尾 / 手动点上传），否则只传一批
    let take = if force { pending.len() } else { batch };
    let chosen: Vec<LocalShot> = pending.into_iter().take(take).collect();

    let (token, account, repo_name) = ctx(state).await?;
    // 上传前确认仓库存在：`repo_name` 有默认值，界面上看不出"还没建过"，
    // 直接建 Release 会 404（第一次用时光机必然遇到）
    let repo_name = ensure_repo_ready(state, &token, &account, &repo_name).await?;
    {
        let mut c = load(state);
        if c.repo_name != repo_name {
            c.repo_name = repo_name.clone();
            save(state, &c)?;
        }
    }

    let dir = shots_dir(state, instance_id);
    let mut parts = Vec::new();
    // 描述里只放列表要用的精简字段（名字/触发/时间），时间轴据此拆条目；
    // "那一刻"的日志片段另走 shots.json 附件，见下面
    let mut meta_shots = Vec::new();
    let mut full_shots = Vec::new();
    let mut total = 0u64;
    for s in &chosen {
        let path = dir.join(&s.file);
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if size == 0 {
            continue; // 文件被手动删了：跳过，下面顺手把索引条目清掉
        }
        total += size;
        parts.push(saves::PackedPart {
            path,
            asset_name: s.file.clone(),
            size,
        });
        meta_shots.push(json!({
            "name": s.file,
            "trigger": s.trigger,
            "created_at": s.created_at,
        }));
        full_shots.push(json!({
            "name": s.file,
            "trigger": s.trigger,
            "created_at": s.created_at,
            // 日志尾部限长，避免附件无上限膨胀
            "log": s.log_snippet.chars().take(1200).collect::<String>(),
        }));
    }
    if parts.is_empty() {
        let left: Vec<LocalShot> = load_index(state, instance_id)
            .into_iter()
            .filter(|x| !chosen.iter().any(|c| c.file == x.file))
            .collect();
        save_index(state, instance_id, &left)?;
        return Ok(json!({ "uploaded": 0, "releaseId": 0, "waiting": 0, "cleaned": 0 }));
    }

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // 完整元数据（含日志）作为 `shots.json` 附件上传，不塞进 Release 描述：
    // 一批 10 张轻松上万字，而列表用不到日志（点开大图才要）。
    let shots_file = "shots.json";
    let tmp_json = state
        .root
        .join("timemachine")
        .join("cache")
        .join(format!("shots-{stamp}-{}.json", std::process::id()));
    if let Some(parent) = tmp_json.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let json_text = serde_json::to_string(&full_shots).unwrap_or_else(|_| "[]".into());
    if std::fs::write(&tmp_json, &json_text).is_ok() {
        parts.push(saves::PackedPart {
            path: tmp_json.clone(),
            asset_name: shots_file.to_string(),
            size: json_text.len() as u64,
        });
    }

    let meta = json!({
        "type": "timemachine",
        "instance_id": instance_id,
        // 复用云存档那套 Release 读写：归属键是 world_id（列表过滤、配额清理都看它），
        // 时光机按实例归属，所以填实例 id
        "world_id": instance_id,
        "trigger": "batch",
        "created_at": stamp,
        "size_bytes": total,
        // 这一批里每张图的名字 / 触发来源 / 时间，时间轴据此拆成一条条记忆点
        "shots": meta_shots,
        // 上面这些字段的完整版（多一个 log）在哪个附件里
        "shots_file": shots_file,
        "shot_count": meta_shots.len(),
    });
    let release_id = release::create_and_upload(
        state,
        &token,
        &account,
        &repo_name,
        instance_id,
        &meta,
        &parts,
        "image/png",
        |_, _| {},
    )
    .await?;
    // 临时元数据文件用完即删（内容已经在附件里了）
    let _ = std::fs::remove_file(&tmp_json);

    // 回查这次 Release 的附件，把每张图对上一个 asset id 写进索引
    // （时间轴点开单张时要按 asset 下载）
    let all = release::list(state, &token, &repo_name, &account)
        .await
        .unwrap_or_default();
    let mut name_to_asset: HashMap<String, u64> = HashMap::new();
    if let Some(rel) = all
        .iter()
        .find(|r| r.get("releaseId").and_then(|v| v.as_u64()) == Some(release_id))
    {
        let names = rel
            .get("assetNames")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let ids = rel
            .get("assetIds")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        for (n, i) in names.iter().zip(ids.iter()) {
            if let (Some(n), Some(i)) = (n.as_str(), i.as_u64()) {
                name_to_asset.insert(n.to_string(), i);
            }
        }
    }
    let mut idx = load_index(state, instance_id);
    for s in idx.iter_mut() {
        if chosen.iter().any(|c| c.file == s.file) {
            s.uploaded = true;
            s.release_id = release_id;
            s.asset_id = name_to_asset.get(&s.file).copied().unwrap_or(0);
        }
    }
    save_index(state, instance_id, &idx)?;

    // 超出保留数时清理最早的那几批
    let cleaned = release::cleanup_quota(
        state,
        &token,
        &account,
        &repo_name,
        instance_id,
        &all,
        cfg.keep_per_instance,
    )
    .await
    .unwrap_or(0);

    Ok(json!({
        "uploaded": parts.len(),
        "releaseId": release_id,
        "waiting": pending_shots(state, instance_id).len(),
        "cleaned": cleaned,
    }))
}

/// 让游戏自己截一张图（相当于按 F2），返回截图文件路径。
fn capture_game_screenshot(
    state: &AppState,
    instance_id: &str,
    instance_dir: &Path,
) -> Result<PathBuf, String> {
    let pid = state
        .game_pids
        .lock()
        .ok()
        .and_then(|g| g.get(instance_id).copied())
        .unwrap_or(0);
    if pid == 0 {
        return Err("游戏进程未运行".into());
    }
    let hwnd = find_game_window(pid).ok_or("找不到游戏窗口（请在窗口化 / 无边框全屏下使用）")?;
    let shots = instance_dir.join("screenshots");
    let t0 = std::time::SystemTime::now();
    press_f2(hwnd)?;

    // 等游戏把 png 写出来（读帧缓冲会有短暂卡顿，最多等 2.5s）
    for _ in 0..25 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if let Some(p) = newest_png_since(&shots, t0) {
            // 等文件写完（大小不再变化）
            let mut last = 0u64;
            for _ in 0..10 {
                let sz = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                if sz > 0 && sz == last {
                    break;
                }
                last = sz;
                std::thread::sleep(std::time::Duration::from_millis(60));
            }
            return copy_to_local(state, instance_id, &p);
        }
    }
    Err("游戏没有产生新的截图文件（可能是独占全屏，或该版本禁用了 F2）".into())
}

fn newest_png_since(dir: &Path, since: std::time::SystemTime) -> Option<PathBuf> {
    let rd = std::fs::read_dir(dir).ok()?;
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("png") {
            continue;
        }
        let Ok(m) = p.metadata() else { continue };
        let Ok(mt) = m.modified() else { continue };
        if mt <= since {
            continue;
        }
        if best.as_ref().map(|(t, _)| mt > *t).unwrap_or(true) {
            best = Some((mt, p));
        }
    }
    best.map(|(_, p)| p)
}

fn copy_to_local(state: &AppState, instance_id: &str, src: &Path) -> Result<PathBuf, String> {
    let dir = shots_dir(state, instance_id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败: {e}"))?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let dest = dir.join(format!("shot-{stamp}.png"));
    std::fs::copy(src, &dest).map_err(|e| format!("复制截图失败: {e}"))?;
    Ok(dest)
}

// ---- Win32：找窗口 + 发按键 ----

#[cfg(windows)]
mod win32 {
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowThreadProcessId,
        IsWindowVisible, PostMessageW, WM_KEYDOWN, WM_KEYUP,
    };

    pub const VK_F2: usize = 0x71;

    struct FindCtx {
        pid: u32,
        hwnd: HWND,
    }

    unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = &mut *(lparam as *mut FindCtx);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid != ctx.pid || IsWindowVisible(hwnd) == 0 {
            return 1; // 继续枚举
        }
        let mut cls = [0u16; 128];
        let n = GetClassNameW(hwnd, cls.as_mut_ptr(), cls.len() as i32);
        let name = String::from_utf16_lossy(&cls[..n as usize]);
        // GLFW（原版/Forge/Fabric）优先；否则取第一个可见窗口
        if ctx.hwnd.is_null() || name.starts_with("GLFW") {
            ctx.hwnd = hwnd;
            // 找到 GLFW 主窗口就不再遍历（返回 0 停止）
            if name.starts_with("GLFW") {
                return 0;
            }
        }
        1
    }

    /// 找属于该进程的游戏主窗口
    pub fn find_window(pid: u32) -> Option<HWND> {
        let mut ctx = FindCtx {
            pid,
            hwnd: std::ptr::null_mut(),
        };
        unsafe {
            EnumWindows(
                Some(enum_cb),
                &mut ctx as *mut FindCtx as LPARAM,
            );
        }
        if ctx.hwnd.is_null() {
            None
        } else {
            Some(ctx.hwnd)
        }
    }

    pub fn is_foreground(hwnd: HWND) -> bool {
        unsafe { GetForegroundWindow() == hwnd }
    }

    /// 向窗口投递 F2 按下/抬起（后台也能收到，不需要把窗口切到前台）
    pub fn press_f2(hwnd: HWND) -> Result<(), String> {
        // lparam：F2 扫描码 0x3C；按下 repeat=1，抬起带 transition(0x8000)+prev(0x4000)
        let down: LPARAM = 0x003C_0001;
        let up: LPARAM = 0xC03C_0001;
        unsafe {
            if PostMessageW(hwnd, WM_KEYDOWN, VK_F2, down) == 0 {
                return Err("发送按键消息失败（游戏窗口未响应）".into());
            }
            PostMessageW(hwnd, WM_KEYUP, VK_F2, up);
        }
        Ok(())
    }
}

#[cfg(windows)]
fn find_game_window(pid: u32) -> Option<isize> {
    win32::find_window(pid).map(|h| h as isize)
}
#[cfg(not(windows))]
fn find_game_window(_pid: u32) -> Option<isize> {
    None
}

#[cfg(windows)]
fn press_f2(hwnd: isize) -> Result<(), String> {
    win32::press_f2(hwnd as windows_sys::Win32::Foundation::HWND)
}
#[cfg(not(windows))]
fn press_f2(_hwnd: isize) -> Result<(), String> {
    Err("当前平台不支持自动截图".into())
}

#[cfg(windows)]
fn is_foreground(hwnd: isize) -> bool {
    win32::is_foreground(hwnd as windows_sys::Win32::Foundation::HWND)
}
#[cfg(not(windows))]
fn is_foreground(_hwnd: isize) -> bool {
    true
}

// ---- 触发引擎 ----

/// 内置事件规则：(id, 正则)，按 MC 日志常见文案匹配
fn builtin_rules(cfg: &Config) -> Vec<(String, String, u64)> {
    let mut v = Vec::new();
    if cfg.on_world_enter {
        v.push((
            "world_enter".into(),
            r"Starting integrated (Minecraft )?server|Preparing start region".into(),
            30,
        ));
    }
    if cfg.on_world_exit {
        v.push((
            "world_exit".into(),
            r"Stopping (singleplayer )?server|Stopping!".into(),
            30,
        ));
    }
    if cfg.on_death {
        v.push((
            "death".into(),
            r"\[CHAT\].*(was slain|was shot|drowned|burned to death|blew up|hit the ground|fell from|was killed|starved|suffocated|was pricked|was pummeled|was squished|was impaled|was struck by lightning|withered away|experienced kinetic energy)".into(),
            10,
        ));
    }
    if cfg.on_advancement {
        v.push((
            "advancement".into(),
            r"Advancement made|has made advancement|已达成进度|进度已达成".into(),
            30,
        ));
    }
    v
}

struct TriggerState {
    offset: u64,
    last_fire: HashMap<String, u64>,
}

/// 已经跑着引擎的实例：同一实例只留一个（开关反复切换 / 中途开启都会调 start）
fn engines() -> &'static std::sync::Mutex<HashSet<String>> {
    static E: std::sync::OnceLock<std::sync::Mutex<HashSet<String>>> = std::sync::OnceLock::new();
    E.get_or_init(|| std::sync::Mutex::new(HashSet::new()))
}

/// 游戏启动后（或中途打开开关时）跑起来：定时 + 日志事件 双通道触发截图。
/// 游戏进程从 `state.game_pids` 移除、或用户在设置里关掉总开关后自动结束。
pub fn start_engine(app: tauri::AppHandle, instance_id: String, pid: u32) {
    match engines().lock() {
        Ok(mut g) => {
            if !g.insert(instance_id.clone()) {
                return; // 这个实例已经有引擎在跑
            }
        }
        Err(_) => return,
    }
    tauri::async_runtime::spawn(async move {
        {
            let state = app.state::<AppState>();
            if load(state.inner()).enabled {
                engine_loop(app.clone(), state.inner(), &instance_id, pid).await;
            }
        }
        if let Ok(mut g) = engines().lock() {
            g.remove(&instance_id);
        }
    });
}

async fn engine_loop(app: tauri::AppHandle, state: &AppState, instance_id: &str, pid: u32) {
    let instance_dir = state.instances_dir().join(instance_id);
    let log_path = instance_dir.join("logs").join("latest.log");
    let mut cfg = load(state);
    let mut ts = TriggerState {
        offset: std::fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0),
        last_fire: HashMap::new(),
    };
    let mut last_tick = std::time::Instant::now();
    let mut ticks: u64 = 0;

    loop {
        // 游戏退出了就停
        let alive = state
            .game_pids
            .lock()
            .map(|g| g.get(instance_id).copied() == Some(pid))
            .unwrap_or(false);
        if !alive {
            // 游戏退了：把这局攒着的零头也传上去，不然要等下局凑够一批
            if load(state).cloud_enabled {
                if let Err(e) = flush(state, instance_id, true).await {
                    println!("[timemachine] 收尾上传失败: {e}");
                    let _ = app.emit(
                        "timemachine://error",
                        json!({
                            "instanceId": instance_id,
                            "trigger": "exit",
                            "message": format!("退出游戏后上传剩余截图失败：{e}"),
                        }),
                    );
                }
            }
            println!("[timemachine] engine stopped for {instance_id}");
            break;
        }

        // 每 5 秒重读一次配置：改间隔 / 关总开关能立刻生效，不用重开游戏
        ticks += 1;
        if ticks % 10 == 0 {
            cfg = load(state);
            if !cfg.enabled {
                println!("[timemachine] disabled, engine stopped for {instance_id}");
                break;
            }
        }

        // 1) 日志事件
        if let Ok(new_text) = read_new_bytes(&log_path, &mut ts.offset) {
            for line in new_text.lines() {
                if let Some(trigger) = match_line(&cfg, line, &mut ts.last_fire) {
                    let _ = fire(app.clone(), state, instance_id, &trigger).await;
                }
            }
        }

        // 2) 定时：按"上次到点 + 间隔"推进（截图+上传要一秒左右，
        //    直接取 now() 会把耗时算进周期，设 10 秒会变成 11 秒）
        let interval = std::time::Duration::from_secs(cfg.interval_secs.max(10));
        if cfg.on_interval && last_tick.elapsed() >= interval {
            if last_tick.elapsed() > interval + interval {
                // 落后太多（上传卡住 / 电脑休眠等）就对一次表，别补发一串
                last_tick = std::time::Instant::now();
            } else {
                last_tick += interval;
            }
            if let Some(hwnd) = find_game_window(pid) {
                if !cfg.foreground_only || is_foreground(hwnd) {
                    let _ = fire(app.clone(), state, instance_id, "interval").await;
                }
            }
        }

        // 200ms 一轮：到点更准（500ms 一轮会被凑成整 500ms 的倍数）
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

async fn fire(
    app: tauri::AppHandle,
    state: &AppState,
    instance_id: &str,
    trigger: &str,
) -> Result<(), String> {
    match capture_and_upload(state, instance_id, trigger, "").await {
        Ok(v) => {
            // 截到了但云端没传上去（云保存开着的情况）：本地是安全的，但也得让用户知道
            if let Some(err) = v.get("uploadError").and_then(|x| x.as_str()) {
                let _ = app.emit(
                    "timemachine://error",
                    json!({
                        "instanceId": instance_id,
                        "trigger": trigger,
                        "message": format!("云端上传失败（截图已存本地）：{err}"),
                    }),
                );
            }
            let _ = app.emit(
                "timemachine://shot",
                json!({
                    "instanceId": instance_id,
                    "trigger": trigger,
                    "releaseId": v["releaseId"],
                    "uploaded": v["uploaded"],
                    "pending": v["pending"],
                }),
            );
            Ok(())
        }
        Err(e) => {
            println!("[timemachine] 截图失败（{trigger}）: {e}");
            // 引擎在后台跑，光打日志用户完全看不见（"定时截图没生效"就是这么来的）：
            // 抛给前端提示（前端按 实例+消息 去重节流）
            let _ = app.emit(
                "timemachine://error",
                json!({ "instanceId": instance_id, "trigger": trigger, "message": e.clone() }),
            );
            Err(e)
        }
    }
}

fn match_line(cfg: &Config, line: &str, last_fire: &mut HashMap<String, u64>) -> Option<String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let rules: Vec<(String, String, u64)> = builtin_rules(cfg)
        .into_iter()
        .chain(cfg.custom.iter().filter(|c| c.enabled).map(|c| {
            (
                format!("custom:{}", c.id),
                c.pattern.clone(),
                c.cooldown_secs,
            )
        }))
        .collect();
    for (id, pattern, cooldown) in rules {
        if let Ok(re) = regex::Regex::new(&pattern) {
            if re.is_match(line) {
                let last = last_fire.get(&id).copied().unwrap_or(0);
                if now.saturating_sub(last) < cooldown {
                    continue;
                }
                last_fire.insert(id.clone(), now);
                return Some(id);
            }
        }
    }
    None
}

/// 读取日志新增的字节（游戏会重写 latest.log，长度变小时从头读）
fn read_new_bytes(path: &Path, offset: &mut u64) -> Result<String, String> {
    use std::io::Read;
    let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if len == 0 {
        return Ok(String::new());
    }
    let start = if len < *offset { 0 } else { *offset };
    if start == len {
        return Ok(String::new());
    }
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    std::io::Seek::seek(&mut f, std::io::SeekFrom::Start(start)).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; (len - start) as usize];
    let n = f.read(&mut buf).map_err(|e| e.to_string())?;
    *offset = start + n as u64;
    Ok(crate::util::decode_text(&buf[..n]))
}

// ---- 列表 / 删除 ----

/// 时间轴总览：本地 + 云端合并。本地条目带 `localPath`（前端直接显示），
/// 云端条目按 asset 回源下载；同一记忆点优先保留本地那条。
pub async fn list_all(state: &AppState) -> Result<Value, String> {
    let mut items: Vec<Value> = Vec::new();

    // ---- 本地（所有实例）----
    let root = state.root.join("timemachine").join("shots");
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten() {
            if !e.path().is_dir() {
                continue;
            }
            let Some(instance_id) = e.file_name().to_str().map(|s| s.to_string()) else {
                continue;
            };
            let dir = shots_dir(state, &instance_id);
            for s in load_index(state, &instance_id) {
                let path = dir.join(&s.file);
                let exists = path.exists();
                items.push(json!({
                    "id": format!("local:{instance_id}:{}", s.file),
                    "instanceId": instance_id,
                    "file": s.file,
                    "trigger": s.trigger,
                    "createdAt": s.created_at,
                    "logSnippet": s.log_snippet,
                    "uploaded": s.uploaded,
                    "localPath": if exists { json!(path.to_string_lossy()) } else { Value::Null },
                    "releaseId": s.release_id,
                    "assetId": s.asset_id,
                }));
            }
        }
    }

    // ---- 云端（授权过就顺带拉一次；失败不影响本地浏览）----
    if let Ok((token, account, repo_name)) = ctx(state).await {
        if let Ok(all) = release::list(state, &token, &repo_name, &account).await {
            for rel in all {
                let release_id = rel.get("releaseId").and_then(|v| v.as_u64()).unwrap_or(0);
                let instance_id = rel
                    .get("worldId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let names = rel
                    .get("assetNames")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let ids = rel
                    .get("assetIds")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                // 攒批上传时每张图自带触发来源/时间；老的单张 Release 用 release 级的
                let shots = rel
                    .get("shots")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                for (i, name) in names.iter().enumerate() {
                    let Some(name) = name.as_str() else { continue };
                    // 附件里还混着元数据文件（shots.json），它不是截图
                    if !name.ends_with(".png") {
                        continue;
                    }
                    let asset_id = ids.get(i).and_then(|v| v.as_u64()).unwrap_or(0);
                    // 本地已记过这条（已上传）就不再重复列：按 release+文件名 或 release+asset 认亲
                    let dup = items.iter().any(|it| {
                        it.get("releaseId").and_then(|v| v.as_u64()) == Some(release_id)
                            && it.get("uploaded").and_then(|v| v.as_bool()) == Some(true)
                            && (it.get("file").and_then(|v| v.as_str()) == Some(name)
                                || (asset_id != 0
                                    && it.get("assetId").and_then(|v| v.as_u64()) == Some(asset_id)))
                    });
                    if dup {
                        continue;
                    }
                    let sm = shots
                        .iter()
                        .find(|s| s.get("name").and_then(|v| v.as_str()) == Some(name));
                    let trigger = sm
                        .and_then(|s| s.get("trigger"))
                        .cloned()
                        .or_else(|| rel.get("trigger").cloned())
                        .unwrap_or(Value::Null);
                    // 攒批上传的每张图自带时间；老的单张 Release 用元数据里的时间
                    let created_at = sm
                        .and_then(|s| s.get("created_at"))
                        .and_then(|v| v.as_u64())
                        .or_else(|| rel.get("createdAtUnix").and_then(|v| v.as_u64()))
                        .unwrap_or(0);
                    items.push(json!({
                        "id": format!("cloud:{release_id}:{asset_id}"),
                        "instanceId": instance_id,
                        "file": name,
                        "trigger": trigger,
                        "createdAt": created_at,
                        "logSnippet": sm.and_then(|s| s.get("log")).cloned().unwrap_or(Value::Null),
                        "uploaded": true,
                        "localPath": Value::Null,
                        "releaseId": release_id,
                        "assetId": asset_id,
                    }));
                }
            }
        }
    }

    items.sort_by(|a, b| {
        let ta = a.get("createdAt").and_then(|v| v.as_u64()).unwrap_or(0);
        let tb = b.get("createdAt").and_then(|v| v.as_u64()).unwrap_or(0);
        tb.cmp(&ta)
    });
    Ok(json!({ "shots": items }))
}

/// 列出某实例的云端记忆点（按时间倒序）
pub async fn list(state: &AppState, instance_id: &str) -> Result<Value, String> {
    let (token, account, repo_name) = ctx(state).await?;
    let all = release::list(state, &token, &repo_name, &account).await?;
    let mine: Vec<Value> = all
        .into_iter()
        .filter(|s| s.get("worldId").and_then(|v| v.as_str()) == Some(instance_id))
        .collect();
    Ok(json!({ "shots": mine }))
}

pub async fn delete(state: &AppState, release_id: u64) -> Result<(), String> {
    let (token, account, repo_name) = ctx(state).await?;
    release::delete(state, &token, &account, &repo_name, release_id).await
}

/// 删除一张记忆点：本地文件 + 索引条目，已上传的连云端附件一起删。
pub async fn delete_shot(
    state: &AppState,
    instance_id: &str,
    file: &str,
    release_id: u64,
    asset_id: u64,
) -> Result<(), String> {
    if !instance_id.is_empty() && !file.is_empty() {
        let _ = std::fs::remove_file(shots_dir(state, instance_id).join(file));
        let left: Vec<LocalShot> = load_index(state, instance_id)
            .into_iter()
            .filter(|s| s.file != file)
            .collect();
        save_index(state, instance_id, &left)?;
    }
    // 云端：只删这一张附件；这一批里没别的图了就顺手删掉 Release，别在仓库里留空壳
    if release_id > 0 && asset_id > 0 {
        let (token, account, repo_name) = ctx(state).await?;
        release::delete_asset(state, &token, &account, &repo_name, asset_id).await?;
        if let Ok(ids) = release::asset_ids(state, &token, &account, &repo_name, release_id).await {
            if ids.is_empty() {
                let _ = release::delete(state, &token, &account, &repo_name, release_id).await;
            }
        }
    }
    Ok(())
}

/// 下载某个记忆点的截图到本地（供时间轴显示大图）
pub async fn download(
    state: &AppState,
    release_id: u64,
    asset_id: u64,
) -> Result<PathBuf, String> {
    let (token, account, repo_name) = ctx(state).await?;
    let dir = state.root.join("timemachine").join("cache");
    std::fs::create_dir_all(&dir).ok();
    // 缓存名要带上 asset：攒批上传时一个 Release 里有好几张图，只用 releaseId 会互相覆盖
    let dest = dir.join(format!("shot-{release_id}-{asset_id}.png"));
    if dest.exists() {
        return Ok(dest);
    }
    release::download_asset(
        state,
        &token,
        &account,
        &repo_name,
        asset_id,
        &dest,
        |_, _| {},
    )
    .await?;
    Ok(dest)
}

/// 取某张截图"那一刻"的日志。日志在 `shots.json` 附件里（不在描述里），
/// 这里按需下载解析、按 release 缓存（同一批只下一次）。老快照的日志由列表带回。
pub async fn shot_log(state: &AppState, release_id: u64, file: &str) -> Result<String, String> {
    let cache_dir = state.root.join("timemachine").join("cache");
    std::fs::create_dir_all(&cache_dir).ok();
    let cache = cache_dir.join(format!("meta-{release_id}.json"));
    if !cache.exists() {
        let (token, account, repo_name) = ctx(state).await?;
        let assets = release::list_assets(state, &token, &account, &repo_name, release_id).await?;
        let Some((asset_id, _, _)) = assets.into_iter().find(|(_, _, name)| name == "shots.json")
        else {
            return Ok(String::new());
        };
        release::download_asset(
            state,
            &token,
            &account,
            &repo_name,
            asset_id,
            &cache,
            |_, _| {},
        )
        .await?;
    }
    let text = std::fs::read(&cache)
        .map(|b| crate::util::decode_text(&b))
        .unwrap_or_default();
    let list: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let log = list
        .as_array()
        .and_then(|arr| {
            arr.iter()
                .find(|s| s.get("name").and_then(|v| v.as_str()) == Some(file))
                .and_then(|s| s.get("log"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or("")
        .to_string();
    Ok(log)
}

// ---- 命令 ----

use tauri::State;

#[tauri::command]
pub fn timemachine_status(state: State<'_, AppState>) -> Value {
    let cfg = load(&state);
    let cs = crate::cloud_sync::store::load(&state);
    let pending: usize = instances_with_pending(&state)
        .iter()
        .map(|id| pending_shots(&state, id).len())
        .sum();
    json!({
        "connected": !cs.access_token.is_empty(),
        "account": cs.account,
        "repoName": cfg.repo_name,
        "cloudEnabled": cfg.cloud_enabled,
        "batchSize": cfg.batch_size,
        "pending": pending,
        "enabled": cfg.enabled,
        "intervalSecs": cfg.interval_secs,
        "keepPerInstance": cfg.keep_per_instance,
        "onInterval": cfg.on_interval,
        "onWorldEnter": cfg.on_world_enter,
        "onWorldExit": cfg.on_world_exit,
        "onDeath": cfg.on_death,
        "onAdvancement": cfg.on_advancement,
        "foregroundOnly": cfg.foreground_only,
        "keepLocalCopy": cfg.keep_local_copy,
        "custom": cfg.custom,
    })
}

#[tauri::command]
pub async fn timemachine_init_repo(state: State<'_, AppState>) -> Result<Value, String> {
    init_repo(&state).await
}

/// 正在运行的实例 id（`game_pids` 只记本启动器启动、还没退出的游戏）。
/// 前端那份来自 `launch://state` 事件，重启就丢——这里是权威来源。
pub fn running_instance_ids(state: &AppState) -> Vec<String> {
    let mut ids: Vec<String> = state
        .game_pids
        .lock()
        .map(|g| g.keys().cloned().collect())
        .unwrap_or_default();
    ids.sort(); // 结果稳定，前端显示与挑选不会来回跳
    ids
}

#[tauri::command]
pub fn timemachine_running_instances(state: State<'_, AppState>) -> Vec<String> {
    running_instance_ids(&state)
}

/// 正在运行的实例 + 进程号（"中途打开总开关"时补起引擎要用）
fn running_pids(state: &AppState) -> Vec<(String, u32)> {
    state
        .game_pids
        .lock()
        .map(|g| g.iter().map(|(k, v)| (k.clone(), *v)).collect())
        .unwrap_or_default()
}

#[tauri::command]
pub async fn timemachine_capture(
    state: State<'_, AppState>,
    instance_id: String,
    trigger: Option<String>,
) -> Result<Value, String> {
    let id = if instance_id.trim().is_empty() {
        match running_instance_ids(&state).first() {
            Some(id) => id.clone(),
            None => {
                return Err("没有正在运行的游戏：启动游戏后再试".into())
            }
        }
    } else {
        instance_id
    };
    capture_and_upload(
        &state,
        &id,
        &trigger.unwrap_or_else(|| "manual".into()),
        "",
    )
    .await
}

#[tauri::command]
pub async fn timemachine_list(
    state: State<'_, AppState>,
    instance_id: String,
) -> Result<Value, String> {
    list(&state, &instance_id).await
}

/// 时间轴总览页：跨实例列出全部截图
#[tauri::command]
pub async fn timemachine_list_all(state: State<'_, AppState>) -> Result<Value, String> {
    list_all(&state).await
}

#[tauri::command]
pub async fn timemachine_delete(state: State<'_, AppState>, release_id: u64) -> Result<(), String> {
    delete(&state, release_id).await
}

/// 删除一张记忆点（本地文件 + 索引；已上传的连云端附件一起删）
#[tauri::command]
pub async fn timemachine_delete_shot(
    state: State<'_, AppState>,
    instance_id: String,
    file: String,
    release_id: u64,
    asset_id: u64,
) -> Result<(), String> {
    delete_shot(&state, &instance_id, &file, release_id, asset_id).await
}

/// 取某张截图"那一刻"的日志（日志在 `shots.json` 附件里，按需下载）
#[tauri::command]
pub async fn timemachine_shot_log(
    state: State<'_, AppState>,
    release_id: u64,
    file: String,
) -> Result<String, String> {
    shot_log(&state, release_id, &file).await
}

/// 手动把待传的截图传上去（攒批模式下"我就想现在传"）。
/// `instanceId` 留空＝所有有积压的实例。
#[tauri::command]
pub async fn timemachine_flush(
    state: State<'_, AppState>,
    instance_id: Option<String>,
) -> Result<Value, String> {
    let ids: Vec<String> = match instance_id.filter(|s| !s.trim().is_empty()) {
        Some(id) => vec![id],
        None => instances_with_pending(&state),
    };
    let mut uploaded = 0u64;
    let mut failures: Vec<String> = Vec::new();
    for id in ids {
        match flush(&state, &id, true).await {
            Ok(v) => uploaded += v["uploaded"].as_u64().unwrap_or(0),
            Err(e) => failures.push(e),
        }
    }
    if uploaded == 0 && !failures.is_empty() {
        return Err(failures.join("；"));
    }
    Ok(json!({ "uploaded": uploaded, "failures": failures }))
}

#[tauri::command]
pub async fn timemachine_download(
    state: State<'_, AppState>,
    release_id: u64,
    asset_id: u64,
) -> Result<String, String> {
    let p = download(&state, release_id, asset_id).await?;
    Ok(p.to_string_lossy().to_string())
}

#[tauri::command]
pub fn timemachine_set_config(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    patch: Value,
) -> Result<(), String> {
    let mut cfg = load(&state);
    let was_enabled = cfg.enabled;
    let was_cloud = cfg.cloud_enabled;
    if let Some(v) = patch.get("enabled").and_then(|x| x.as_bool()) {
        cfg.enabled = v;
    }
    if let Some(v) = patch.get("cloudEnabled").and_then(|x| x.as_bool()) {
        cfg.cloud_enabled = v;
    }
    if let Some(v) = patch.get("batchSize").and_then(|x| x.as_u64()) {
        cfg.batch_size = v.clamp(BATCH_MIN as u64, BATCH_MAX as u64) as u32;
    }
    if let Some(v) = patch.get("intervalSecs").and_then(|x| x.as_u64()) {
        cfg.interval_secs = v.clamp(10, 24 * 3600);
    }
    if let Some(v) = patch.get("keepPerInstance").and_then(|x| x.as_u64()) {
        cfg.keep_per_instance = v.clamp(10, 5000) as u32;
    }
    let bools = [
        ("onInterval", "on_interval"),
        ("onWorldEnter", "on_world_enter"),
        ("onWorldExit", "on_world_exit"),
        ("onDeath", "on_death"),
        ("onAdvancement", "on_advancement"),
        ("foregroundOnly", "foreground_only"),
        ("keepLocalCopy", "keep_local_copy"),
    ];
    for (key, field) in bools {
        if let Some(v) = patch.get(key).and_then(|x| x.as_bool()) {
            match field {
                "on_interval" => cfg.on_interval = v,
                "on_world_enter" => cfg.on_world_enter = v,
                "on_world_exit" => cfg.on_world_exit = v,
                "on_death" => cfg.on_death = v,
                "on_advancement" => cfg.on_advancement = v,
                "foreground_only" => cfg.foreground_only = v,
                "keep_local_copy" => cfg.keep_local_copy = v,
                _ => {}
            }
        }
    }
    if let Some(v) = patch.get("custom") {
        if let Ok(list) = serde_json::from_value::<Vec<CustomTrigger>>(v.clone()) {
            cfg.custom = list;
        }
    }
    save(&state, &cfg)?;
    // 刚打开总开关、而游戏已经在跑：立刻补起引擎。
    // 否则要等"下次启动游戏"才生效——用户会以为开关没起作用。
    if !was_enabled && cfg.enabled {
        for (id, pid) in running_pids(&state) {
            println!("[timemachine] enabled mid-game, starting engine for {id}");
            start_engine(app.clone(), id, pid);
        }
    }
    // 刚打开云保存：把已经攒着的待传截图试着送上去（够一批才传）
    if !was_cloud && cfg.cloud_enabled {
        let app2 = app.clone();
        let ids = instances_with_pending(&state);
        tauri::async_runtime::spawn(async move {
            let st = app2.state::<AppState>();
            for id in ids {
                if let Err(e) = flush(st.inner(), &id, false).await {
                    let _ = app2.emit(
                        "timemachine://error",
                        json!({ "instanceId": id, "trigger": "manual", "message": e }),
                    );
                }
            }
        });
    }
    Ok(())
}

/// 本地保留的截图副本（未开启上传也能看）
#[tauri::command]
pub fn timemachine_local_shots(state: State<'_, AppState>, instance_id: String) -> Vec<String> {
    local_shots(&state, &instance_id)
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect()
}

/// 本地副本列表（未上传成功或保留本地时的截图）
pub fn local_shots(state: &AppState, instance_id: &str) -> Vec<PathBuf> {
    let dir = shots_dir(state, instance_id);
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("png"))
        .collect();
    v.sort();
    v
}
