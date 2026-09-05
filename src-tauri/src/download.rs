use crate::state::AppState;
use crate::util::{file_sha1, file_sha512};
use futures_util::StreamExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tauri::Emitter;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Semaphore;

#[derive(Clone, Debug)]
pub struct DownloadItem {
    pub url: String,
    pub dest: PathBuf,
    pub sha1: Option<String>,
    pub sha512: Option<String>,
    pub size: Option<u64>,
    pub label: String,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct DownloadStats {
    pub done: usize,
    pub total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

/// Per-file progress slot shared between the download task and the periodic emitter.
#[derive(Clone)]
struct ActiveSlot {
    bytes_done: Arc<AtomicU64>,
    bytes_total: Arc<AtomicU64>,
    active: Arc<AtomicBool>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Parallel downloader with a concurrency limit, sha1 verification and retries.
/// Streams every file and emits `download://progress` roughly every 800ms so the
/// frontend can show live average speed and remaining-file counts.
pub async fn download_many(
    app: tauri::AppHandle,
    state: &AppState,
    task_id: u64,
    phase: &str,
    items: Vec<DownloadItem>,
) -> Result<DownloadStats, String> {
    let total = items.len();
    let bytes_total: Arc<AtomicU64> = Arc::new(AtomicU64::new(items.iter().filter_map(|i| i.size).sum()));
    let done = Arc::new(AtomicU64::new(0));
    let bytes_completed = Arc::new(AtomicU64::new(0));
    let current_bytes = Arc::new(AtomicU64::new(0));
    let current_total = Arc::new(AtomicU64::new(0));
    let cancel_flag = state.install_cancel.clone();

    let (file_threads, chunk_threads, mirror_base) = {
        let s = state.settings.read().unwrap();
        (
            s.download_threads.max(1),
            s.download_chunk_threads.max(1),
            crate::mirror::resolve_from(&s.mirror, &s.mirror_custom),
        )
    };
    let sem = Arc::new(Semaphore::new(file_threads));
    let client = state.client.clone();

    // Per-file progress slots for all items (read by the periodic emitter).
    let active_files: Vec<(String, ActiveSlot)> = items
        .iter()
        .map(|item| {
            (
                item.label.clone(),
                ActiveSlot {
                    bytes_done: Arc::new(AtomicU64::new(0)),
                    bytes_total: Arc::new(AtomicU64::new(0)),
                    active: Arc::new(AtomicBool::new(true)),
                },
            )
        })
        .collect();
    let active_files = Arc::new(active_files);

    // periodic emitter (drives smooth speed display)
    let phase_owned = phase.to_string();
    let phase_e = phase_owned.clone();
    let app2 = app.clone();
    let done_e = done.clone();
    let bc_e = bytes_completed.clone();
    let bt_e = bytes_total.clone();
    let cancel_e = cancel_flag.clone();
    let af_e = active_files.clone();
    let emitter = tauri::async_runtime::spawn(async move {
        loop {
            if cancel_e.load(Ordering::Relaxed) {
                break;
            }
            let d = done_e.load(Ordering::Relaxed);
            if d >= total as u64 {
                break;
            }
            let bd = bc_e.load(Ordering::Relaxed)
                + af_e
                    .iter()
                    .filter(|(_, s)| s.active.load(Ordering::Relaxed))
                    .map(|(_, s)| s.bytes_done.load(Ordering::Relaxed))
                    .sum::<u64>();
            let bt = bt_e.load(Ordering::Relaxed);
            let active: Vec<_> = af_e
                .iter()
                .filter(|(_, s)| s.active.load(Ordering::Relaxed))
                .map(|(label, s)| {
                    serde_json::json!({
                        "name": label,
                        "bytesDone": s.bytes_done.load(Ordering::Relaxed),
                        "bytesTotal": s.bytes_total.load(Ordering::Relaxed),
                    })
                })
                .collect();
            let _ = app2.emit(
                "download://progress",
                serde_json::json!({
                    "taskId": task_id,
                    "phase": phase_e,
                    "done": d,
                    "total": total,
                    "current": "",
                    "ok": true,
                    "bytesDone": bd,
                    "bytesTotal": bt,
                    "activeFiles": active,
                    "ts": now_ms(),
                }),
            );
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        }
    });

    let mut handles = Vec::new();
    for (i, item) in items.into_iter().enumerate() {
        let permit = sem.clone().acquire_owned().await.map_err(|e| e.to_string())?;
        let client = client.clone();
        let app = app.clone();
        let done = done.clone();
        let bytes_completed = bytes_completed.clone();
        let current_bytes = current_bytes.clone();
        let current_total = current_total.clone();
        let cancel = cancel_flag.clone();
        let phase = phase_owned.clone();
        let mirror_base = mirror_base.clone();
        let bytes_total_h = bytes_total.clone();
        let my_done = active_files[i].1.bytes_done.clone();
        let my_total = active_files[i].1.bytes_total.clone();
        let my_active = active_files[i].1.active.clone();
        handles.push(tauri::async_runtime::spawn(async move {
            let _permit = permit;
            let mut result = Err(format!("not attempted: {}", item.label));
            let bt_outer = bytes_total_h.clone();
            for attempt in 0..3 {
                if cancel.load(Ordering::Relaxed) {
                    result = Err("cancelled".into());
                    break;
                }
                let cur = current_bytes.clone();
                let ct = current_total.clone();
                let bc_p = bytes_completed.clone();
                let bt_p = bt_outer.clone();
                let md = my_done.clone();
                let mt = my_total.clone();
                let on_progress = move |written: u64, cl: u64| {
                    cur.store(written, Ordering::Relaxed);
                    if cl > 0 {
                        ct.store(cl, Ordering::Relaxed);
                        let needed = bc_p.load(Ordering::Relaxed) + cl;
                        bt_p.fetch_max(needed, Ordering::Relaxed);
                    }
                    md.store(written, Ordering::Relaxed);
                    if cl > 0 {
                        mt.store(cl, Ordering::Relaxed);
                    }
                };
                match download_one(&client, &item, &mirror_base, &on_progress, chunk_threads).await {
                    Ok(()) => {
                        result = Ok(());
                        break;
                    }
                    Err(e) => {
                        let permanent = e.contains("HTTP 4") || e.contains("HTTP 5");
                        result = Err(e);
                        if permanent {
                            break;
                        }
                        let _ = tokio::time::sleep(std::time::Duration::from_millis(400 * (attempt + 1) as u64)).await;
                    }
                }
            }
            my_active.store(false, Ordering::Relaxed);
            current_bytes.store(0, Ordering::Relaxed);
            let d = done.fetch_add(1, Ordering::Relaxed) + 1;
            let actual_size = item.size.unwrap_or_else(|| current_total.load(Ordering::Relaxed));
            let bd = bytes_completed.fetch_add(actual_size, Ordering::Relaxed) + actual_size;
            bt_outer.fetch_max(bd, Ordering::Relaxed);
            let bt = bt_outer.load(Ordering::Relaxed);
            let _ = app.emit(
                "download://progress",
                serde_json::json!({
                    "taskId": task_id,
                    "phase": phase,
                    "done": d,
                    "total": total,
                    "current": item.label,
                    "ok": result.is_ok(),
                    "bytesDone": bd,
                    "bytesTotal": bt,
                    "ts": now_ms(),
                }),
            );
            result.map_err(|e| format!("{}: {} ({})", item.label, e, item.url))
        }));
    }

    let mut errors: Vec<String> = Vec::new();
    for h in handles {
        match h.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => errors.push(e),
            Err(e) => errors.push(e.to_string()),
        }
    }
    let _ = emitter.abort();
    if state.install_cancel.load(Ordering::Relaxed) {
        state.install_cancel.store(false, Ordering::Relaxed);
        return Err("下载已取消".into());
    }
    if !errors.is_empty() {
        let sample: Vec<String> = errors.iter().take(5).cloned().collect();
        return Err(format!("{} 个文件下载失败: {}", errors.len(), sample.join("; ")));
    }
    let final_bt = bytes_total.load(Ordering::Relaxed);
    Ok(DownloadStats {
        done: done.load(Ordering::Relaxed) as usize,
        total,
        bytes_done: bytes_completed.load(Ordering::Relaxed),
        bytes_total: final_bt,
    })
}

/// Files at least this large are eligible for parallel chunked download (8 MB).
const CHUNK_THRESHOLD: u64 = 8 * 1024 * 1024;

/// Probe whether the server supports HTTP Range requests via a lightweight HEAD.
async fn probe_range_support(client: &reqwest::Client, url: &str) -> bool {
    match client
        .head(url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => resp
            .headers()
            .get("accept-ranges")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.eq_ignore_ascii_case("bytes"))
            .unwrap_or(false),
        _ => false,
    }
}

/// Path of the chunk-completion sidecar for a `.part` file.
/// Tracks which byte ranges are already written, enabling resume for chunked downloads.
fn chunk_state_path(part: &Path) -> PathBuf {
    let mut p = part.as_os_str().to_owned();
    p.push(".chunks");
    PathBuf::from(p)
}

/// Load which chunks (by index) have already been completed, if any.
fn load_chunk_state(part: &Path, chunk_count: usize) -> Vec<bool> {
    std::fs::read(chunk_state_path(part))
        .map(|bytes| {
            (0..chunk_count)
                .map(|i| bytes.get(i).copied().unwrap_or(0) != 0)
                .collect()
        })
        .unwrap_or_else(|_| vec![false; chunk_count])
}

/// Persist which chunks are complete (one byte per chunk).
fn save_chunk_state(part: &Path, done: &[bool]) {
    let bytes: Vec<u8> = done.iter().map(|&b| if b { 1 } else { 0 }).collect();
    let p = chunk_state_path(part);
    crate::util::fs_best_effort("write", &p, std::fs::write(&p, bytes));
}

/// Remove the chunk-completion sidecar (used when switching sources / restarting clean).
fn remove_chunk_state(part: &Path) {
    let p = chunk_state_path(part);
    crate::util::fs_best_effort("remove_file", &p, std::fs::remove_file(&p));
}

/// Download a single file (streamed, with `.part` staging), verify sha1 when given.
/// Uses parallel chunked download when the server supports ranges and the file is large enough.
async fn download_one(
    client: &reqwest::Client,
    item: &DownloadItem,
    mirror_base: &str,
    on_progress: &(dyn Fn(u64, u64) + Send + Sync),
    chunk_threads: usize,
) -> Result<(), String> {
    if let Some(parent) = item.dest.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("mkdir: {e}"))?;
    }
    // Skip if a complete, verified file already exists
    if item.dest.exists() {
        if let Some(sha) = &item.sha512 {
            if let Some(h) = file_sha512(&item.dest) {
                if h.eq_ignore_ascii_case(sha) {
                    return Ok(());
                }
            }
        } else if let Some(sha) = &item.sha1 {
            if let Some(h) = file_sha1(&item.dest) {
                if h.eq_ignore_ascii_case(sha) {
                    return Ok(());
                }
            }
        } else if let Some(size) = item.size {
            if std::fs::metadata(&item.dest).map(|m| m.len() == size).unwrap_or(false) {
                return Ok(());
            }
        } else {
            return Ok(());
        }
    }

    let part = item.dest.with_extension("part");
    // 支持断点续传：保留已有的 .part，由 fetch_and_verify 检测大小并从断点继续。
    // 仅当换源（镜像→官方）时才清空，避免不同来源的数据拼接错位。

    // 优先走镜像地址；镜像拉取/校验失败时回退到原始官方地址，
    // 这样即使镜像站缺失某个文件，安装也不会整体失败。
    let target = crate::mirror::map(mirror_base, &item.url);
    let mut result =
        fetch_and_verify(client, item, &target, &part, on_progress, chunk_threads).await;
    if result.is_err() && target != item.url {
        crate::util::fs_best_effort("remove_file", &part, std::fs::remove_file(&part));
        remove_chunk_state(&part);
        result =
            fetch_and_verify(client, item, &item.url, &part, on_progress, chunk_threads).await;
    }
    result?;

    tokio::fs::rename(&part, &item.dest)
        .await
        .map_err(|e| format!("移动文件失败: {e}"))?;
    Ok(())
}

/// 拉取单个文件（必要时分片并行）并校验哈希 / 大小。
async fn fetch_and_verify(
    client: &reqwest::Client,
    item: &DownloadItem,
    url: &str,
    part: &Path,
    on_progress: &(dyn Fn(u64, u64) + Send + Sync),
    chunk_threads: usize,
) -> Result<(), String> {
    // Decide whether to attempt parallel chunked download:
    //   - chunk_threads > 1
    //   - file size known and >= CHUNK_THRESHOLD
    //   - server advertises Accept-Ranges: bytes
    let can_chunk =
        chunk_threads > 1 && item.size.map(|s| s >= CHUNK_THRESHOLD).unwrap_or(false);

    if can_chunk && probe_range_support(client, url).await {
        let size = item.size.unwrap();
        match download_chunked(client, url, part, size, chunk_threads, on_progress).await {
            Ok(()) => {}
            Err(_) => {
                // Fallback: some CDNs claim range support but 404 on actual Range requests.
                crate::util::fs_best_effort("remove_file", part, std::fs::remove_file(part));
                remove_chunk_state(part);
                download_streamed(client, url, part, on_progress).await?;
            }
        }
    } else {
        download_streamed(client, url, part, on_progress).await?;
    }

    if let Some(sha) = &item.sha512 {
        let actual = file_sha512(part).ok_or("校验失败: 无法读取")?;
        if !actual.eq_ignore_ascii_case(sha) {
            let _ = tokio::fs::remove_file(part).await;
            return Err(format!("sha512 不匹配 (期望 {sha}, 实际 {actual})"));
        }
    } else if let Some(sha) = &item.sha1 {
        let actual = file_sha1(part).ok_or("校验失败: 无法读取")?;
        if !actual.eq_ignore_ascii_case(sha) {
            let _ = tokio::fs::remove_file(part).await;
            return Err(format!("sha1 不匹配 (期望 {sha}, 实际 {actual})"));
        }
    }
    if let Some(size) = item.size {
        let len = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
        if len != size {
            let _ = tokio::fs::remove_file(part).await;
            return Err(format!("大小不匹配 (期望 {size}, 实际 {len})"));
        }
    }
    Ok(())
}

/// Stream download (single connection, no chunking).
/// Resumes from an existing `.part` file when the server supports Range requests.
async fn download_streamed(
    client: &reqwest::Client,
    url: &str,
    part: &Path,
    on_progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<(), String> {
    // 断点续传：看已有 .part 写到哪了
    let existing = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);

    let mut req = client
        .get(url)
        .header("Accept-Encoding", "identity")
        .timeout(std::time::Duration::from_secs(300));
    if existing > 0 {
        req = req.header("Range", format!("bytes={existing}-"));
    }
    let resp = req.send().await.map_err(|e| format!("请求失败: {e}"))?;
    let status = resp.status();

    // 206 = 服务器支持 Range，续传；200 = 不支持，从头再来
    let resume = existing > 0 && status.as_u16() == 206;
    let write_offset = if resume { existing } else { 0 };
    if !resume && !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    // 服务器忽略 Range 返回 200：需覆盖已有部分，从头写
    let content_length = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    // 续传时 content-length 是剩余字节；完整总大小 = 已有 + 剩余
    let total = if resume {
        existing.saturating_add(content_length)
    } else {
        content_length
    };

    let mut file = if resume {
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(part)
            .await
            .map_err(|e| format!("写入失败: {e}"))?
    } else {
        tokio::fs::File::create(part)
            .await
            .map_err(|e| format!("写入失败: {e}"))?
    };
    let mut stream = resp.bytes_stream();
    let mut written = write_offset;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("读取失败: {e}"))?;
        file.write_all(&chunk).await.map_err(|e| format!("写入失败: {e}"))?;
        written += chunk.len() as u64;
        on_progress(written, total);
    }
    file.flush().await.map_err(|e| format!("写入失败: {e}"))?;
    Ok(())
}

/// Parallel chunked download using HTTP Range requests.
/// Resumes by skipping chunks already recorded as complete in the sidecar.
async fn download_chunked(
    client: &reqwest::Client,
    url: &str,
    part: &Path,
    total_size: u64,
    chunk_count: usize,
    on_progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<(), String> {
    let chunk_size = total_size / chunk_count as u64;

    // 断点续传：若 .part 已预分配且大小正确，复用 sidecar 里的完成位图，
    // 只下载尚未完成的分片；否则重新预分配。
    let existing_size = std::fs::metadata(part).map(|m| m.len()).unwrap_or(0);
    let done: Arc<std::sync::Mutex<Vec<bool>>> = if existing_size == total_size {
        Arc::new(std::sync::Mutex::new(load_chunk_state(part, chunk_count)))
    } else {
        let file = std::fs::File::create(part).map_err(|e| format!("写入失败: {e}"))?;
        file.set_len(total_size).map_err(|e| format!("预分配失败: {e}"))?;
        drop(file);
        let d = vec![false; chunk_count];
        save_chunk_state(part, &d);
        Arc::new(std::sync::Mutex::new(d))
    };

    // 已完成的字节数（用于进度显示）
    let written = Arc::new(AtomicU64::new(0));
    {
        let guard = done.lock().unwrap();
        for (i, &ok) in guard.iter().enumerate() {
            if ok {
                let start = i as u64 * chunk_size;
                let end = if i == chunk_count - 1 {
                    total_size - 1
                } else {
                    (i as u64 + 1) * chunk_size - 1
                };
                written.fetch_add(end - start + 1, Ordering::Relaxed);
            }
        }
    }

    let mut handles = Vec::new();

    for i in 0..chunk_count {
        // 已完成的片直接跳过
        {
            let guard = done.lock().unwrap();
            if guard[i] {
                continue;
            }
        }

        let start = i as u64 * chunk_size;
        let end = if i == chunk_count - 1 {
            total_size - 1
        } else {
            (i as u64 + 1) * chunk_size - 1
        };
        let len = end - start + 1;

        let client_c = client.clone();
        let url_c = url.to_string();
        let part_c = part.to_path_buf();
        let written_c = written.clone();
        let done_c = done.clone();

        handles.push(tokio::spawn(async move {
            let resp = client_c
                .get(&url_c)
                .header("Accept-Encoding", "identity")
                .header("Range", format!("bytes={start}-{end}"))
                .timeout(std::time::Duration::from_secs(300))
                .send()
                .await
                .map_err(|e| format!("分片请求失败: {e}"))?;
            let status = resp.status();
            if status.as_u16() != 206 {
                return Err(format!("分片 HTTP {status} (期望 206)"));
            }

            let mut file = tokio::fs::OpenOptions::new()
                .write(true)
                .open(&part_c)
                .await
                .map_err(|e| format!("打开文件失败: {e}"))?;
            file.seek(std::io::SeekFrom::Start(start))
                .await
                .map_err(|e| format!("seek 失败: {e}"))?;

            let mut stream = resp.bytes_stream();
            let mut chunk_written = 0u64;
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|e| format!("读取失败: {e}"))?;
                file.write_all(&chunk).await.map_err(|e| format!("写入失败: {e}"))?;
                chunk_written += chunk.len() as u64;
                written_c.fetch_add(chunk.len() as u64, Ordering::Relaxed);
            }
            file.flush().await.map_err(|e| format!("写入失败: {e}"))?;
            if chunk_written != len {
                return Err(format!(
                    "分片 {i} 大小不匹配 (期望 {len}, 实际 {chunk_written})"
                ));
            }
            // 标记本分片完成并持久化 sidecar，支持断点续传
            done_c.lock().unwrap()[i] = true;
            let state = done_c.lock().unwrap().clone();
            save_chunk_state(&part_c, &state);
            Ok(())
        }));
    }

    // Wait for all chunks with periodic progress reporting
    use futures_util::future::join_all;
    let chunks_future = join_all(handles);
    tokio::pin!(chunks_future);
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(200));
    let mut errors = Vec::new();
    loop {
        tokio::select! {
            results = &mut chunks_future => {
                for res in results {
                    match res {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => errors.push(e),
                        Err(e) => errors.push(e.to_string()),
                    }
                }
                break;
            }
            _ = interval.tick() => {
                on_progress(written.load(Ordering::Relaxed), total_size);
            }
        }
    }
    // final progress flush
    on_progress(written.load(Ordering::Relaxed), total_size);

    if !errors.is_empty() {
        // 保留 .part 与 sidecar，供断点续传重试时跳过已完成分片
        return Err(format!("分片下载失败: {}", errors.join("; ")));
    }
    // 全部完成：移除 sidecar
    remove_chunk_state(part);
    Ok(())
}

/// Simple single-file download used for metadata (no progress).
pub async fn get_text(client: &reqwest::Client, url: &str) -> Result<String, String> {
    let resp = client.get(url).send().await.map_err(|e| format!("请求 {url} 失败: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {status} for {url}"));
    }
    resp.text().await.map_err(|e| e.to_string())
}

pub async fn get_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
) -> Result<T, String> {
    let resp = client.get(url).send().await.map_err(|e| format!("请求 {url} 失败: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {status} for {url}"));
    }
    resp.json::<T>().await.map_err(|e| format!("解析 {url} 失败: {e}"))
}

/// Ensure a file exists on disk (create if missing), returns path.
#[allow(dead_code)]
pub fn ensure_file(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    if !path.exists() {
        std::fs::write(path, content)?;
    }
    Ok(())
}
