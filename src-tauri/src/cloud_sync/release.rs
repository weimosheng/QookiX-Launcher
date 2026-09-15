//! 云存档快照（GitHub Release）：创建并上传、列表、下载、删除与配额清理。
//!
//! - tag 形如 `world-<world_id>-<unix_secs>`
//! - Release body 存 JSON 元数据（世界名/版本/大小/sha256），列表无需下载内容
//! - 上传走 uploads.github.com；私有仓库下载走 assets API（带令牌）

use super::github;
use super::store;
use crate::state::AppState;
use serde_json::{json, Value};
use std::path::Path;

/// 解析 Release body 中的元数据 JSON（非本功能的 Release 返回 None）。
fn parse_meta(release: &Value) -> Option<Value> {
    let body = release.get("body").and_then(|b| b.as_str())?;
    let meta: Value = serde_json::from_str(body.trim()).ok()?;
    if meta.get("world_id").is_some() {
        Some(meta)
    } else {
        None
    }
}

/// 列出全部云存档快照（最新在前）。
pub async fn list(
    state: &AppState,
    token: &str,
    repo_name: &str,
    account: &str,
) -> Result<Vec<Value>, String> {
    let url = format!(
        "{}/repos/{}/{}/releases?per_page=100",
        github::API,
        account,
        repo_name
    );
    let Some(v) = github::get(&state.client, &url, token).await? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for rel in v.as_array().cloned().unwrap_or_default() {
        let Some(meta) = parse_meta(&rel) else { continue };
        let release_id = rel.get("id").and_then(|x| x.as_u64()).unwrap_or(0);
        let tag = rel.get("tag_name").and_then(|x| x.as_str()).unwrap_or("");
        let created = rel
            .get("created_at")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        // 附件信息（zip 本体，可能多个分卷；按名字排序保证 part 顺序）
        let mut assets: Vec<(u64, u64, String)> = rel
            .get("assets")
            .and_then(|a| a.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| {
                        Some((
                            x.get("id").and_then(|v| v.as_u64())?,
                            x.get("size").and_then(|v| v.as_u64())?,
                            x.get("name").and_then(|v| v.as_str())?.to_string(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        assets.sort_by(|a, b| a.2.cmp(&b.2));
        let asset_ids: Vec<u64> = assets.iter().map(|a| a.0).collect();
        let asset_size: u64 = assets.iter().map(|a| a.1).sum();
        out.push(json!({
            "releaseId": release_id,
            "tag": tag,
            "createdAt": created,
            "assetId": asset_ids.first().copied().unwrap_or(0),
            "assetIds": asset_ids,
            "assetNames": assets.iter().map(|a| a.2.clone()).collect::<Vec<_>>(),
            "assetSize": asset_size,
            "partCount": meta.get("part_count").cloned().unwrap_or(json!(1)),
            "worldId": meta.get("world_id").cloned().unwrap_or(Value::Null),
            "worldName": meta.get("world_name").cloned().unwrap_or(Value::Null),
            "instanceId": meta.get("instance_id").cloned().unwrap_or(Value::Null),
            "instanceName": meta.get("instance_name").cloned().unwrap_or(Value::Null),
            "gameVersion": meta.get("game_version").cloned().unwrap_or(Value::Null),
            "sha256": meta.get("sha256").cloned().unwrap_or(Value::Null),
        }));
    }
    Ok(out)
}

/// 创建 Release 并上传压缩包（支持多分卷），返回 release_id。
/// `parts`: (路径, 附件名)；`progress(sent, total)` 跨所有分卷连续回调。
pub async fn create_and_upload(
    state: &AppState,
    token: &str,
    account: &str,
    repo_name: &str,
    world_id: &str,
    meta: &Value,
    parts: &[super::saves::PackedPart],
    progress: impl Fn(u64, u64) + Send + Sync + 'static,
) -> Result<u64, String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let tag = format!("world-{world_id}-{stamp}");
    let mut meta_obj = meta.clone();
    if parts.len() > 1 {
        meta_obj["part_count"] = json!(parts.len());
    }
    let body = serde_json::to_string(&meta_obj).unwrap_or_default();
    let create_url = format!(
        "{}/repos/{}/{}/releases",
        github::API,
        account,
        repo_name
    );
    let rel = github::send_json(
        &state.client,
        reqwest::Method::POST,
        &create_url,
        token,
        Some(&json!({ "tag_name": tag, "name": tag, "body": body })),
    )
    .await
    .map_err(|e| format!("创建 Release 失败: {e}"))?;
    let release_id = rel.get("id").and_then(|x| x.as_u64()).unwrap_or(0);
    if release_id == 0 {
        return Err("创建 Release 失败：响应缺少 id".into());
    }

    let grand_total: u64 = parts.iter().map(|p| p.size).sum();
    let mut grand_sent = 0u64;
    let progress = std::sync::Arc::new(progress);
    for (pi, part) in parts.iter().enumerate() {
        let base = grand_sent;
        let label = if parts.len() > 1 {
            format!("正在上传快照（第 {}/{} 卷）…", pi + 1, parts.len())
        } else {
            "正在上传快照…".into()
        };
        progress(base, grand_total);
        let _ = label; // 阶段文案由前端按 part_count 拼接

        let upload_url = format!(
            "{}/repos/{}/{}/releases/{}/assets?name={}",
            github::UPLOADS,
            account,
            repo_name,
            release_id,
            part.asset_name
        );

        // 流式上传：分块读取压缩包，边读边回调进度
        let total = part.size;
        let file = tokio::fs::File::open(&part.path)
            .await
            .map_err(|e| format!("打开压缩包失败: {e}"))?;
        let reader = tokio::io::BufReader::with_capacity(256 * 1024, file);
        use tokio::io::AsyncReadExt;
        let progress2 = progress.clone();
        let stream = futures_util::stream::unfold(
            (reader, 0u64, progress2),
            move |(mut r, sent, progress)| async move {
                let mut buf = vec![0u8; 256 * 1024];
                match r.read(&mut buf).await {
                    Ok(0) => None,
                    Ok(n) => {
                        let sent = sent + n as u64;
                        progress(base + sent, grand_total);
                        Some((
                            Ok::<bytes::Bytes, std::io::Error>(bytes::Bytes::copy_from_slice(&buf[..n])),
                            (r, sent, progress),
                        ))
                    }
                    Err(e) => Some((Err(e), (r, sent, progress))),
                }
            },
        );

        let resp = state
            .client
            .post(&upload_url)
            .header("User-Agent", github::UA)
            .header("Accept", "application/vnd.github+json")
            .header("Content-Type", "application/zip")
            .header("Content-Length", total)
            .bearer_auth(token)
            .body(reqwest::Body::wrap_stream(stream))
            .send()
            .await
            .map_err(|e| format!("上传压缩包失败: {e}"))?;
        if !resp.status().is_success() {
            let code = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!(
                "上传压缩包失败（HTTP {code}）: {}",
                text.chars().take(200).collect::<String>()
            ));
        }
        grand_sent = base + total;
        progress(grand_sent, grand_total);
    }
    Ok(release_id)
}

/// 删除一个 Release（连带其 Asset）。
pub async fn delete(
    state: &AppState,
    token: &str,
    account: &str,
    repo_name: &str,
    release_id: u64,
) -> Result<(), String> {
    let url = format!(
        "{}/repos/{}/{}/releases/{}",
        github::API,
        account,
        repo_name,
        release_id
    );
    github::send_json(&state.client, reqwest::Method::DELETE, &url, token, None).await?;
    Ok(())
}

/// 列出某个 Release 的全部附件（按名字排序，保证分卷顺序）。
pub async fn list_assets(
    state: &AppState,
    token: &str,
    account: &str,
    repo_name: &str,
    release_id: u64,
) -> Result<Vec<(u64, u64, String)>, String> {
    let url = format!(
        "{}/repos/{}/{}/releases/{}",
        github::API,
        account,
        repo_name,
        release_id
    );
    let Some(v) = github::get(&state.client, &url, token).await? else {
        return Err("快照不存在或已被删除".into());
    };
    let mut assets: Vec<(u64, u64, String)> = v
        .get("assets")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    Some((
                        x.get("id").and_then(|v| v.as_u64())?,
                        x.get("size").and_then(|v| v.as_u64())?,
                        x.get("name").and_then(|v| v.as_str())?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    assets.sort_by(|a, b| a.2.cmp(&b.2));
    Ok(assets)
}

/// 下载 Asset 到本地文件（私有仓库需带令牌）。
/// `progress(received, total)` 会在下载过程中被回调。
pub async fn download_asset(
    state: &AppState,
    token: &str,
    account: &str,
    repo_name: &str,
    asset_id: u64,
    dest: &Path,
    progress: impl Fn(u64, u64) + Send,
) -> Result<(), String> {
    let url = format!(
        "{}/repos/{}/{}/releases/assets/{}",
        github::API,
        account,
        repo_name,
        asset_id
    );
    let resp = state
        .client
        .get(&url)
        .header("User-Agent", github::UA)
        .header("Accept", "application/octet-stream")
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("下载失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("下载失败：HTTP {}", resp.status().as_u16()));
    }
    let total = resp.content_length().unwrap_or(0);
    let mut file = std::fs::File::create(dest).map_err(|e| format!("创建文件失败: {e}"))?;
    let mut received = 0u64;
    let mut resp = resp;
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("读取下载内容失败: {e}"))?
    {
        use std::io::Write;
        file.write_all(&chunk).map_err(|e| format!("写入文件失败: {e}"))?;
        received += chunk.len() as u64;
        progress(received, total);
    }
    Ok(())
}

/// 配额清理：某世界的快照超过上限时，从最旧的开始删除。
/// `snapshots` 为 list() 的结果（最新在前）。
pub async fn cleanup_quota(
    state: &AppState,
    token: &str,
    account: &str,
    repo_name: &str,
    world_id: &str,
    snapshots: &[Value],
    keep: u32,
) -> Result<u32, String> {
    let mine: Vec<&Value> = snapshots
        .iter()
        .filter(|s| s.get("worldId").and_then(|v| v.as_str()) == Some(world_id))
        .collect();
    let keep = keep.max(1) as usize;
    if mine.len() <= keep {
        return Ok(0);
    }
    let mut deleted = 0u32;
    for s in mine.iter().skip(keep) {
        if let Some(id) = s.get("releaseId").and_then(|v| v.as_u64()) {
            if delete(state, token, account, repo_name, id).await.is_ok() {
                deleted += 1;
            }
        }
    }
    Ok(deleted)
}

/// 读取本地保存的每个世界快照上限（默认 5）。
pub fn keep_per_world(state: &AppState) -> u32 {
    let cs = store::load(state);
    if cs.keep_per_world == 0 {
        5
    } else {
        cs.keep_per_world
    }
}
