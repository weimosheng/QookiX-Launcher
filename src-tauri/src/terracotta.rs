use crate::state::AppState;
use flate2::read::GzDecoder;
use futures_util::StreamExt;
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use tar::Archive;
use tauri::Emitter;
use tokio::io::AsyncWriteExt;

/// 陶瓦联机下载页地址（GitHub Releases）
pub const DOWNLOAD_URL: &str = "https://github.com/burningtnt/Terracotta/releases/latest";
/// 陶瓦联机 GitHub 仓库
const GITHUB_REPO: &str = "burningtnt/Terracotta";
/// GitHub 加速代理前缀（空串 = 直连）。中国大陆无法直连 GitHub 时按顺序回退。
const GITHUB_PROXIES: &[&str] = &[
    "",
    "https://gh-proxy.com/",
    "https://ghproxy.net/",
    "https://ghfast.top/",
    "https://mirror.ghproxy.com/",
    "https://gh.llkk.cc/",
];

/// 为 GitHub 地址套上代理前缀
fn proxied(url: &str, proxy: &str) -> String {
    if proxy.is_empty() {
        url.to_string()
    } else {
        format!("{proxy}{url}")
    }
}

/// 常见可执行文件名
const EXE_NAMES: &[&str] = &["Terracotta.exe", "terracotta.exe"];

/// 当前运行中的陶瓦联机会话（由本启动器启动）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerracottaSession {
    pub exe: PathBuf,
    pub pid: u32,
    pub port: u16,
}

/// 陶瓦联机 `--hmcl2` 写入的端口文件格式
#[derive(Debug, Deserialize)]
struct PortFile {
    port: u16,
}

/// 检测结果
#[derive(Debug, Serialize, Deserialize)]
pub struct TerracottaInfo {
    pub found: bool,
    pub path: Option<String>,
    pub running: bool,
    pub port: Option<u16>,
    pub download_url: String,
    /// 从 Terracotta.exe 提取的图标（data URL），仅 found 时存在
    pub icon: Option<String>,
}

/// 启动结果
#[derive(Debug, Serialize, Deserialize)]
pub struct TerracottaLaunch {
    pub port: u16,
    pub ui_url: String,
    pub path: String,
}

/// 在常见目录下查找陶瓦联机可执行文件
fn detect_executable(_state: &AppState) -> Option<PathBuf> {
    let mut bases: Vec<PathBuf> = Vec::new();

    // 启动器同级目录 / 启动器同级 Terracotta 目录
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            bases.push(dir.join("Terracotta"));
            bases.push(dir.to_path_buf());
        }
    }

    // 用户安装目录
    if let Ok(env) = std::env::var("LOCALAPPDATA") {
        let base = PathBuf::from(env);
        bases.push(base.join("Programs").join("Terracotta"));
        bases.push(base.join("Terracotta"));
    }

    // Program Files
    for var in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Ok(env) = std::env::var(var) {
            bases.push(PathBuf::from(env).join("Terracotta"));
        }
    }

    for base in &bases {
        for name in EXE_NAMES {
            let p = base.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }

    None
}

/// 若安装包解压到了子目录，将子目录内容提升到安装根目录；
/// 若可执行文件名带版本号等（如 terracotta-0.4.2-windows-x86_64.exe），
/// 统一重命名为 `Terracotta.exe`，返回根目录下的 exe 路径
fn flatten_package(dir: &Path) -> Option<PathBuf> {
    // 1. 根目录已存在标准命名
    if let Some(root_exe) = EXE_NAMES.iter().find_map(|n| {
        let p = dir.join(n);
        p.is_file().then_some(p)
    }) {
        return Some(root_exe);
    }

    // 2. 递归查找任意 .exe 文件
    let mut stack: Vec<PathBuf> = vec![dir.to_path_buf()];
    let mut found: Option<PathBuf> = None;
    'outer: while let Some(cur) = stack.pop() {
        if let Ok(entries) = std::fs::read_dir(&cur) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
                {
                    found = Some(path);
                    break 'outer;
                }
            }
        }
    }
    let exe = found?;

    // 3. 若 exe 在子目录中，先把该子目录里的资源（DLL 等）提升到根目录
    if let Some(src) = exe.parent().map(|p| p.to_path_buf()) {
        if src != dir {
            if let Ok(entries) = std::fs::read_dir(&src) {
                for entry in entries.flatten() {
                    let from = entry.path();
                    let to = dir.join(entry.file_name());
                    if to.exists() {
                        let _ = std::fs::remove_dir_all(&to).or_else(|_| std::fs::remove_file(&to));
                    }
                    let _ = std::fs::rename(&from, &to);
                }
            }
            let _ = std::fs::remove_dir_all(&src);
        }
    }

    // 4. 统一命名为 Terracotta.exe
    let target = dir.join("Terracotta.exe");
    if !target.is_file() && target != exe {
        // 先移动，失败则复制
        let _ = std::fs::rename(&exe, &target).or_else(|_| std::fs::copy(&exe, &target).map(|_| ()));
    }

    target.is_file().then_some(target)
}

/// 从 exe 中提取第一个图标，编码为 PNG data URL（仅 Windows）
fn extract_exe_icon(exe: &Path) -> Option<String> {
    extract_exe_icon_impl(exe)
}

#[cfg(target_os = "windows")]
fn extract_exe_icon_impl(exe: &Path) -> Option<String> {
    use base64::Engine;
    use png::{BitDepth, ColorType, Encoder};
    use std::io::Cursor;
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::UI::Shell::ExtractIconExW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DestroyIcon, GetIconInfo, HICON, ICONINFO,
    };

    // 转为 UTF-16 宽字符路径
    let wide: Vec<u16> = exe
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let mut icon_large: HICON = std::ptr::null_mut();
        let mut icon_small: HICON = std::ptr::null_mut();
        // 大图标索引 0，小图标索引 1
        let n = ExtractIconExW(wide.as_ptr(), 0, &mut icon_large, &mut icon_small, 1);
        if n == 0 {
            return None;
        }
        let hicon = if !icon_large.is_null() {
            icon_large
        } else {
            icon_small
        };
        if hicon.is_null() {
            return None;
        }

        let mut info: ICONINFO = zeroed();
        if GetIconInfo(hicon, &mut info) == 0 {
            let _ = DestroyIcon(hicon);
            return None;
        }
        let hbm: HBITMAP = info.hbmColor;
        if hbm.is_null() {
            let _ = DestroyIcon(hicon);
            return None;
        }

        let mut bm: BITMAP = zeroed();
        let bm_size = size_of::<BITMAP>() as i32;
        if GetObjectW(hbm as _, bm_size, &mut bm as *mut _ as *mut _) == 0 {
            let _ = DestroyIcon(hicon);
            return None;
        }
        let w = bm.bmWidth as u32;
        let h = bm.bmHeight as u32;
        if w == 0 || h == 0 {
            let _ = DestroyIcon(hicon);
            return None;
        }

        // 用 BITMAPINFO 指定 top-down 32bpp 读取像素
        let mut bmi: BITMAPINFO = zeroed();
        bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = w as i32;
        bmi.bmiHeader.biHeight = -(h as i32); // 自顶向下
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB;

        let mut pixels = vec![0u8; (w * h * 4) as usize];
        let dc = CreateCompatibleDC(std::ptr::null_mut());
        if dc.is_null() {
            let _ = DestroyIcon(hicon);
            return None;
        }
        let got = GetDIBits(
            dc,
            hbm,
            0,
            h,
            pixels.as_mut_ptr() as *mut _,
            &mut bmi,
            DIB_RGB_COLORS,
        );
        let _ = DeleteDC(dc);
        if got == 0 {
            let _ = DestroyIcon(hicon);
            return None;
        }

        // BGRA -> RGBA
        let mut rgba = Vec::with_capacity(pixels.len());
        for px in pixels.chunks_exact(4) {
            rgba.push(px[2]);
            rgba.push(px[1]);
            rgba.push(px[0]);
            rgba.push(px[3]);
        }

        let mut out = Cursor::new(Vec::new());
        let mut enc = Encoder::new(&mut out, w, h);
        enc.set_color(ColorType::Rgba);
        enc.set_depth(BitDepth::Eight);
        let mut writer = enc.write_header().ok()?;
        writer.write_image_data(&rgba).ok()?;
        drop(writer);
        let bytes = out.into_inner();

        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        Some(format!("data:image/png;base64,{b64}"))
    }
}

#[cfg(not(target_os = "windows"))]
fn extract_exe_icon_impl(_exe: &Path) -> Option<String> {
    None
}

fn base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

/// 陶瓦联机安装目录
fn install_dir() -> Option<PathBuf> {
    if let Ok(env) = std::env::var("LOCALAPPDATA") {
        Some(PathBuf::from(env).join("Programs").join("Terracotta"))
    } else {
        None
    }
}

/// 从 GitHub 最新 release 中解析适用于 Windows 的安装包下载地址（带代理回退）
async fn fetch_windows_asset_url(client: &reqwest::Client) -> Result<String, String> {
    #[derive(Deserialize)]
    struct ReleaseAsset {
        name: String,
        browser_download_url: String,
    }
    #[derive(Deserialize)]
    struct Release {
        assets: Vec<ReleaseAsset>,
    }

    let api_url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let mut last_err = String::new();
    for proxy in GITHUB_PROXIES {
        let url = proxied(&api_url, proxy);
        let resp = match client
            .get(&url)
            .header("User-Agent", "QookiX-Launcher")
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                last_err = e.to_string();
                continue;
            }
        };
        if !resp.status().is_success() {
            last_err = format!("HTTP {}", resp.status());
            continue;
        }
        let release: Release = match resp.json().await {
            Ok(r) => r,
            Err(e) => {
                last_err = format!("解析失败: {e}");
                continue;
            }
        };
        if let Some(asset) = release
            .assets
            .into_iter()
            .find(|a| a.name.contains("windows-x86_64-pkg.tar.gz"))
        {
            // 返回原始 GitHub 下载地址，实际下载时再套代理链以便回退
            return Ok(asset.browser_download_url);
        }
        return Err("未找到适用于 Windows 的陶瓦联机安装包".to_string());
    }
    Err(format!(
        "获取陶瓦联机最新版本失败（直连与加速镜像均不可用）: {last_err}"
    ))
}

/// 等待陶瓦联机写入端口文件，返回端口
fn wait_for_port(port_file: &Path, timeout: Duration) -> Option<u16> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Ok(content) = std::fs::read_to_string(port_file) {
            if let Ok(pf) = serde_json::from_str::<PortFile>(&content) {
                return Some(pf.port);
            }
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    None
}

/// 向陶瓦联机本地服务发送 GET 请求并解析 JSON
async fn fetch_state(state: &AppState, path: &str, query: &str) -> Result<serde_json::Value, String> {
    let session = state
        .terracotta
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "陶瓦联机未运行".to_string())?;
    let url = format!("{}{}{}", base_url(session.port), path, query);
    let resp = state
        .client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("请求陶瓦联机失败: {e}"))?;
    let text = resp
        .text()
        .await
        .map_err(|e| format!("读取陶瓦联机响应失败: {e}"))?;
    // 陶瓦联机空闲时可能返回空响应体，视为 JSON null 而不是解析失败
    if text.trim().is_empty() {
        return Ok(serde_json::Value::Null);
    }
    serde_json::from_str(&text).map_err(|e| format!("解析陶瓦联机响应失败: {e}"))
}

/// 检测陶瓦联机是否安装/运行
#[tauri::command]
pub async fn terracotta_detect(
    state: tauri::State<'_, AppState>,
) -> Result<TerracottaInfo, String> {
    let exe = detect_executable(&state);
    let session = state.terracotta.lock().unwrap().clone();
    let icon = exe.as_deref().and_then(extract_exe_icon);
    Ok(TerracottaInfo {
        found: exe.is_some(),
        path: exe.map(|p| p.to_string_lossy().to_string()),
        running: session.is_some(),
        port: session.map(|s| s.port),
        download_url: DOWNLOAD_URL.to_string(),
        icon,
    })
}

/// 下载并解压陶瓦联机到安装目录，返回可执行文件路径
#[tauri::command]
pub async fn terracotta_download(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    // 已存在则直接返回
    if let Some(exe) = detect_executable(&state) {
        return Ok(exe.to_string_lossy().to_string());
    }

    let dir = install_dir().ok_or_else(|| "无法确定安装目录（缺少 LOCALAPPDATA）".to_string())?;
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("创建安装目录失败: {e}"))?;

    let url = fetch_windows_asset_url(&state.client).await?;

    let tmp = dir.join("terracotta.pkg.tar.gz.part");
    let _ = tokio::fs::remove_file(&tmp).await;

    // 流式下载并上报进度（带代理回退）
    let mut last_err = String::new();
    let mut total = 0u64;
    let mut written = 0u64;
    let mut written_ok = false;
    for proxy in GITHUB_PROXIES {
        let full = proxied(&url, proxy);
        let resp = match state
            .client
            .get(&full)
            .header("Accept-Encoding", "identity")
            .timeout(Duration::from_secs(600))
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r,
            Ok(r) => {
                last_err = format!("HTTP {}", r.status());
                continue;
            }
            Err(e) => {
                last_err = e.to_string();
                continue;
            }
        };
        total = resp
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        let mut file = tokio::fs::File::create(&tmp)
            .await
            .map_err(|e| format!("写入失败: {e}"))?;
        let mut stream = resp.bytes_stream();
        written = 0u64;
        let mut last_pct = -1i64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("读取失败: {e}"))?;
            file.write_all(&chunk)
                .await
                .map_err(|e| format!("写入失败: {e}"))?;
            written += chunk.len() as u64;
            let pct = if total > 0 { (written * 100 / total) as i64 } else { -1 };
            if pct != last_pct {
                last_pct = pct;
                let _ = app.emit(
                    "terracotta://download-progress",
                    serde_json::json!({ "downloaded": written, "total": total, "percent": pct }),
                );
            }
        }
        file.flush().await.map_err(|e| format!("写入失败: {e}"))?;
        written_ok = true;
        break;
    }
    if !written_ok {
        return Err(format!("下载陶瓦联机失败（直连与加速镜像均不可用）: {last_err}"));
    }
    let gz_path = dir.join("terracotta.pkg.tar.gz");
    tokio::fs::rename(&tmp, &gz_path)
        .await
        .map_err(|e| format!("移动文件失败: {e}"))?;

    // 上报完成（进入解压阶段）
    let _ = app.emit(
        "terracotta://download-progress",
        serde_json::json!({
            "downloaded": written, "total": total, "percent": 100, "extracting": true
        }),
    );

    // 解压 tar.gz
    let file = std::fs::File::open(&gz_path).map_err(|e| format!("打开安装包失败: {e}"))?;
    let gz = GzDecoder::new(file);
    let mut archive = Archive::new(gz);
    archive
        .unpack(&dir)
        .map_err(|e| format!("解压陶瓦联机失败: {e}"))?;
    let _ = std::fs::remove_file(&gz_path);

    // 若解压到了子目录则提升到根目录，并重新定位 exe
    let exe = flatten_package(&dir).ok_or_else(|| "解压完成但未找到可执行文件".to_string())?;
    let _ = app.emit(
        "terracotta://download-progress",
        serde_json::json!({
            "downloaded": total, "total": total, "percent": 100, "done": true
        }),
    );
    Ok(exe.to_string_lossy().to_string())
}

/// 启动陶瓦联机（若已运行则直接返回）
#[tauri::command]
pub async fn terracotta_launch(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<TerracottaLaunch, String> {
    if let Some(s) = state.terracotta.lock().unwrap().clone() {
        return Ok(TerracottaLaunch {
            port: s.port,
            ui_url: base_url(s.port),
            path: s.exe.to_string_lossy().to_string(),
        });
    }

    let exe = detect_executable(&state)
        .ok_or_else(|| "未检测到陶瓦联机程序，请先前往下载并安装".to_string())?;

    let port_file = state.root.join("terracotta_port.json");
    let _ = std::fs::remove_file(&port_file);

    let child = Command::new(&exe)
        .arg("--hmcl2")
        .arg(&port_file)
        .spawn()
        .map_err(|e| format!("启动陶瓦联机失败: {e}"))?;

    let pid = child.id();
    let port = wait_for_port(&port_file, Duration::from_secs(20))
        .ok_or_else(|| "陶瓦联机启动超时，未获取到本地端口".to_string())?;

    let session = TerracottaSession {
        exe: exe.clone(),
        pid,
        port,
    };
    *state.terracotta.lock().unwrap() = Some(session);

    let _ = app.emit(
        "terracotta://state",
        serde_json::json!({ "running": true, "port": port }),
    );

    Ok(TerracottaLaunch {
        port,
        ui_url: base_url(port),
        path: exe.to_string_lossy().to_string(),
    })
}

/// 停止陶瓦联机进程
#[tauri::command]
pub async fn terracotta_stop(state: tauri::State<'_, AppState>) -> Result<(), String> {
    if let Some(s) = state.terracotta.lock().unwrap().take() {
        let _ = Command::new("taskkill")
            .args(["/PID", &s.pid.to_string(), "/F", "/T"])
            .spawn();
    }
    Ok(())
}

/// 获取陶瓦联机当前状态
#[tauri::command]
pub async fn terracotta_status(state: tauri::State<'_, AppState>) -> Result<serde_json::Value, String> {
    fetch_state(&state, "/state/", "").await
}

/// 创建房间（主机模式，自动生成房间码）
#[tauri::command]
pub async fn terracotta_create_room(
    state: tauri::State<'_, AppState>,
    player: Option<String>,
) -> Result<serde_json::Value, String> {
    // 玩家名可选，非空时拼到扫描接口的查询参数中
    let query = match player.map(|p| p.trim().to_string()).filter(|p| !p.is_empty()) {
        Some(p) => format!("?player={}", utf8_percent_encode(&p, NON_ALPHANUMERIC)),
        None => String::new(),
    };
    fetch_state(&state, "/state/scanning", &query).await
}

/// 加入房间（访客模式）
#[tauri::command]
pub async fn terracotta_join_room(
    state: tauri::State<'_, AppState>,
    room: String,
    player: Option<String>,
) -> Result<serde_json::Value, String> {
    let room = room.trim();
    if room.is_empty() {
        return Err("房间码不能为空".to_string());
    }
    // 房间码由字母数字与短横线组成，进行 URL 编码后放入查询参数
    let mut query = format!("?room={}", utf8_percent_encode(room, NON_ALPHANUMERIC));
    // 玩家名可选，非空时追加到查询参数
    if let Some(p) = player.map(|p| p.trim().to_string()).filter(|p| !p.is_empty()) {
        query.push_str(&format!(
            "&player={}",
            utf8_percent_encode(&p, NON_ALPHANUMERIC)
        ));
    }
    fetch_state(&state, "/state/guesting", &query).await
}

/// 退出当前房间（回到等待状态）
#[tauri::command]
pub async fn terracotta_leave(state: tauri::State<'_, AppState>) -> Result<serde_json::Value, String> {
    fetch_state(&state, "/state/ide", "").await
}
