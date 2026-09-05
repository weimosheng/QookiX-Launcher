use crate::crash::CrashDiagnosis;
use crate::state::AppState;
use serde_json::json;
use tauri::State;

// Crash analysis
// ---------------------------------------------------------------------------

#[derive(serde::Serialize)]
pub struct CrashLogEntry {
    pub filename: String,
    pub modified: u64,
    pub size: u64,
    pub kind: String,
}

/// List crash reports and JVM hs_err logs for an instance.
#[tauri::command]
pub fn list_crash_logs(state: State<AppState>, id: String) -> Result<Vec<CrashLogEntry>, String> {
    let inst_dir = state.root.join("instances").join(&id);
    let crash_dir = inst_dir.join("crash-reports");
    let mut out: Vec<CrashLogEntry> = Vec::new();

    if crash_dir.is_dir() {
        for entry in std::fs::read_dir(&crash_dir).map_err(|e| e.to_string())? {
            let e = entry.map_err(|e| e.to_string())?;
            let meta = e.metadata().map_err(|e| e.to_string())?;
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("crash-") && name.ends_with(".txt") {
                out.push(CrashLogEntry {
                    filename: name,
                    modified: meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0),
                    size: meta.len(),
                    kind: "crash".into(),
                });
            }
        }
    }

    for entry in std::fs::read_dir(&inst_dir).map_err(|e| e.to_string())? {
        let e = entry.map_err(|e| e.to_string())?;
        let meta = e.metadata().map_err(|e| e.to_string())?;
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with("hs_err_pid") && name.ends_with(".log") {
            out.push(CrashLogEntry {
                filename: name,
                modified: meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0),
                size: meta.len(),
                kind: "jvm".into(),
            });
        }
    }

    out.sort_by_key(|e| std::cmp::Reverse(e.modified));
    Ok(out)
}

/// Run the built-in crash diagnosis engine against a specific crash report.
///
/// 诊断逻辑（正则规则集 + 堆栈关键词反查模组）全部在 `crash` 模块中，
/// 这里只负责读取报告文本并回填报告文件名。
#[tauri::command]
pub fn analyze_crash_log(state: State<AppState>, id: String, filename: String) -> Result<CrashDiagnosis, String> {
    let inst_dir = state.root.join("instances").join(&id);
    let crash_path = if filename.starts_with("crash-") {
        inst_dir.join("crash-reports").join(&filename)
    } else {
        inst_dir.join(&filename)
    };
    if !crash_path.is_file() {
        return Err("崩溃报告文件不存在".into());
    }
    let content =
        String::from_utf8_lossy(&std::fs::read(&crash_path).map_err(|e| e.to_string())?).to_string();

    let mut diagnosis = crate::crash::analyze_text(&content, None);
    diagnosis.crash_report = Some(filename);
    Ok(diagnosis)
}


/// Read the raw content of a crash report or hs_err log.
#[tauri::command]
pub fn get_crash_report_content(state: State<AppState>, id: String, filename: String) -> Result<String, String> {
    let inst_dir = state.root.join("instances").join(&id);
    let path = if filename.starts_with("crash-") {
        inst_dir.join("crash-reports").join(&filename)
    } else {
        inst_dir.join(&filename)
    };
    if !path.is_file() {
        return Err("文件不存在".into());
    }
    Ok(String::from_utf8_lossy(&std::fs::read(&path).map_err(|e| e.to_string())?).to_string())
}
/// Minecraft 官方新闻搜索接口（minecraft.net 官网自用），按时间倒序取最新中文条目
const NEWS_API: &str = "https://net-secondary.web.minecraft-services.net/api/v1.0/zh-cn/search?pageSize=24&sortType=Recent&category=News&newsOnly=true&geography=CN";

#[derive(serde::Deserialize)]
struct NewsEntry {
    title: Option<String>,
    description: Option<String>,
    author: Option<String>,
    image: Option<String>,
    #[serde(rename = "imageAltText")]
    image_alt: Option<String>,
    url: Option<String>,
    /// Unix 时间戳（秒）
    time: Option<i64>,
}

#[derive(serde::Deserialize)]
struct NewsResult {
    results: Option<Vec<NewsEntry>>,
}

#[derive(serde::Deserialize)]
struct NewsResponse {
    result: Option<NewsResult>,
}

/// 官网原始数据里含有 `&amp;` 等 HTML 实体，直接展示会露出生标记
fn decode_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

/// 拉取 Minecraft 官方新闻。网络不可用时返回空数组，由前端展示「暂无新闻」。
#[tauri::command]
pub async fn fetch_news(state: State<'_, AppState>) -> Result<Vec<serde_json::Value>, String> {
    let resp = state
        .client
        .get(NEWS_API)
        .timeout(std::time::Duration::from_secs(15))
        .header("User-Agent", "QookiX-Launcher")
        .send()
        .await
        .map_err(|e| format!("获取新闻失败: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("获取新闻失败: HTTP {status}"));
    }
    let body: NewsResponse = resp
        .json()
        .await
        .map_err(|e| format!("解析新闻失败: {e}"))?;

    let items = body
        .result
        .and_then(|r| r.results)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|e| {
            let title = e.title.unwrap_or_default();
            if title.trim().is_empty() {
                return None;
            }
            Some(json!({
                "title": decode_entities(&title),
                "description": e.description.map(|d| decode_entities(&d)).unwrap_or_default(),
                "author": e.author.unwrap_or_default(),
                "time": e.time.unwrap_or(0),
                "image": e.image.unwrap_or_default(),
                "image_alt": e.image_alt.unwrap_or_default(),
                "url": e.url.unwrap_or_default(),
            }))
        })
        .collect();
    Ok(items)
}
