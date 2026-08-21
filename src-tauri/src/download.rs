use crate::state::AppState;
use crate::util::{file_sha1, file_sha512};
use futures_util::StreamExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
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

    let threads = {
        let s = state.settings.read().unwrap();
        s.download_threads.max(1)
    };
    let sem = Arc::new(Semaphore::new(threads));
    let client = state.client.clone();

    // periodic emitter (drives smooth speed display)
    let phase_owned = phase.to_string();
    let phase_e = phase_owned.clone();
    let app2 = app.clone();
    let done_e = done.clone();
    let bc_e = bytes_completed.clone();
    let cur_e = current_bytes.clone();
    let bt_e = bytes_total.clone();
    let cancel_e = cancel_flag.clone();
    let emitter = tauri::async_runtime::spawn(async move {
        loop {
            if cancel_e.load(Ordering::Relaxed) {
                break;
            }
            let d = done_e.load(Ordering::Relaxed);
            if d >= total as u64 {
                break;
            }
            let bd = bc_e.load(Ordering::Relaxed) + cur_e.load(Ordering::Relaxed);
            let bt = bt_e.load(Ordering::Relaxed);
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
                    "ts": now_ms(),
                }),
            );
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        }
    });

    let mut handles = Vec::new();
    for item in items {
        let permit = sem.clone().acquire_owned().await.map_err(|e| e.to_string())?;
        let client = client.clone();
        let app = app.clone();
        let done = done.clone();
        let bytes_completed = bytes_completed.clone();
        let current_bytes = current_bytes.clone();
        let current_total = current_total.clone();
        let cancel = cancel_flag.clone();
        let phase = phase_owned.clone();
        let bytes_total_h = bytes_total.clone();
        handles.push(tauri::async_runtime::spawn(async move {
            let _permit = permit;
            let mut result = Err(format!("not attempted: {}", item.label));
            let last_emit = Arc::new(AtomicU64::new(0));
            let bt_outer = bytes_total_h.clone();
            for attempt in 0..3 {
                if cancel.load(Ordering::Relaxed) {
                    result = Err("cancelled".into());
                    break;
                }
                let cur = current_bytes.clone();
                let ct = current_total.clone();
                let app_p = app.clone();
                let done_p = done.clone();
                let bc_p = bytes_completed.clone();
                let bt_p = bt_outer.clone();
                let _last_p = last_emit.clone();
                let phase_p = phase.clone();
                let label_p = item.label.clone();
                let on_progress = move |written: u64, cl: u64| {
                    cur.store(written, Ordering::Relaxed);
                    if cl > 0 {
                        ct.store(cl, Ordering::Relaxed);
                        // Grow bytes_total to account for this file
                        let needed = bc_p.load(Ordering::Relaxed) + cl;
                        bt_p.fetch_max(needed, Ordering::Relaxed);
                    }
                    let now = now_ms();
                    let d = done_p.load(Ordering::Relaxed);
                    let bd = bc_p.load(Ordering::Relaxed) + written;
                    let bt = bt_p.load(Ordering::Relaxed);
                    let _ = app_p.emit(
                        "download://progress",
                        serde_json::json!({
                            "taskId": task_id,
                            "phase": phase_p,
                            "done": d,
                            "total": total,
                            "current": label_p,
                            "ok": true,
                            "bytesDone": bd,
                            "bytesTotal": bt,
                            "ts": now,
                        }),
                    );
                };
                match download_one(&client, &item, &on_progress, threads).await {
                    Ok(()) => {
                        result = Ok(());
                        break;
                    }
                    Err(e) => {
                        result = Err(e);
                        let _ = tokio::time::sleep(std::time::Duration::from_millis(400 * (attempt + 1) as u64)).await;
                    }
                }
            }
            current_bytes.store(0, Ordering::Relaxed);
            let d = done.fetch_add(1, Ordering::Relaxed) + 1;
            let actual_size = item.size.unwrap_or_else(|| current_total.load(Ordering::Relaxed));
            let bd = bytes_completed.fetch_add(actual_size, Ordering::Relaxed) + actual_size;
            // Grow bytes_total to account for this completed file
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
            result.map_err(|e| format!("{}: {}", item.label, e))
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

/// Download a single file (streamed, with `.part` staging), verify sha1 when given.
/// Uses parallel chunked download when the server supports ranges and the file is large enough.
async fn download_one(
    client: &reqwest::Client,
    item: &DownloadItem,
    on_progress: &(dyn Fn(u64, u64) + Send + Sync),
    threads: usize,
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
    let _ = std::fs::remove_file(&part);

    // Probe: check if server supports range requests and get content length
    let head = client
        .get(&item.url)
        .header("Accept-Encoding", "identity")
        .header("Range", "bytes=0-0")
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("请求失败: {e}"))?;

    let status = head.status();
    if !status.is_success() && status.as_u16() != 206 {
        return Err(format!("HTTP {status}"));
    }

    let accepts_ranges = head
        .headers()
        .get("accept-ranges")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.eq_ignore_ascii_case("bytes"))
        .unwrap_or(false);

    // Content-Length from 206 response is 1 (we asked for 1 byte),
    // so use Content-Range header to get total size
    let content_range_total = head
        .headers()
        .get("content-range")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.rsplit('/').next())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    // Fall back to Content-Length if no Content-Range
    let content_length = if content_range_total > 0 {
        content_range_total
    } else {
        head.headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0)
    };

    drop(head);

    // Use chunked download if: server supports ranges, file is large enough,
    // and we have multiple threads available
    let chunk_threshold = 2 * 1024 * 1024u64; // 2 MB
    let chunk_count = if accepts_ranges && content_length > chunk_threshold && threads > 1 {
        std::cmp::min(threads, 4) // cap at 4 chunks per file
    } else {
        1
    };

    if chunk_count > 1 {
        download_chunked(client, &item.url, &part, content_length, chunk_count, on_progress).await?
    } else {
        download_streamed(client, &item.url, &part, on_progress).await?
    }

    if let Some(sha) = &item.sha512 {
        let actual = file_sha512(&part).ok_or("校验失败: 无法读取")?;
        if !actual.eq_ignore_ascii_case(sha) {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(format!("sha512 不匹配 (期望 {sha}, 实际 {actual})"));
        }
    } else if let Some(sha) = &item.sha1 {
        let actual = file_sha1(&part).ok_or("校验失败: 无法读取")?;
        if !actual.eq_ignore_ascii_case(sha) {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(format!("sha1 不匹配 (期望 {sha}, 实际 {actual})"));
        }
    }
    if let Some(size) = item.size {
        let len = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        if len != size {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(format!("大小不匹配 (期望 {size}, 实际 {len})"));
        }
    }
    tokio::fs::rename(&part, &item.dest)
        .await
        .map_err(|e| format!("移动文件失败: {e}"))?;
    Ok(())
}

/// Stream download (single connection, no chunking).
async fn download_streamed(
    client: &reqwest::Client,
    url: &str,
    part: &Path,
    on_progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<(), String> {
    let resp = client
        .get(url)
        .header("Accept-Encoding", "identity")
        .timeout(std::time::Duration::from_secs(300))
        .send()
        .await
        .map_err(|e| format!("请求失败: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    let content_length = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let mut file = tokio::fs::File::create(part)
        .await
        .map_err(|e| format!("写入失败: {e}"))?;
    let mut stream = resp.bytes_stream();
    let mut written = 0u64;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("读取失败: {e}"))?;
        file.write_all(&chunk).await.map_err(|e| format!("写入失败: {e}"))?;
        written += chunk.len() as u64;
        on_progress(written, content_length);
    }
    file.flush().await.map_err(|e| format!("写入失败: {e}"))?;
    Ok(())
}

/// Parallel chunked download using HTTP Range requests.
async fn download_chunked(
    client: &reqwest::Client,
    url: &str,
    part: &Path,
    total_size: u64,
    chunk_count: usize,
    on_progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<(), String> {
    // Pre-allocate the file
    let file = std::fs::File::create(part).map_err(|e| format!("写入失败: {e}"))?;
    file.set_len(total_size).map_err(|e| format!("预分配失败: {e}"))?;
    drop(file);

    let chunk_size = total_size / chunk_count as u64;
    let written = Arc::new(AtomicU64::new(0));
    let mut handles = Vec::new();

    for i in 0..chunk_count {
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
            if status.as_u16() != 206 && !status.is_success() {
                return Err(format!("分片 HTTP {status}"));
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
        let _ = tokio::fs::remove_file(part).await;
        return Err(format!("分片下载失败: {}", errors.join("; ")));
    }
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
