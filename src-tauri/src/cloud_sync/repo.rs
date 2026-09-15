//! 云存档仓库：检查 / 创建 / 标识文件（qookix.json）读写。

use super::github;
use super::store;
use crate::state::AppState;
use base64::Engine as _;
use serde_json::json;

pub const DEFAULT_REPO: &str = "Qookix-Saves";
pub const MARKER: &str = "qookix.json";
const MARKER_TYPE: &str = "qookix-cloud-save";

/// 确保云端存在云存档仓库（不存在则创建私有仓库并写入标识文件）。
/// 返回 `{ repoName, repositoryId }`。
pub async fn ensure(state: &AppState, token: &str) -> Result<serde_json::Value, String> {
    let mut cs = store::load(state);
    let account = if cs.account.is_empty() {
        return Err("账号信息缺失，请重新授权".into());
    } else {
        cs.account.clone()
    };
    let repo = if cs.repo_name.is_empty() {
        DEFAULT_REPO.to_string()
    } else {
        cs.repo_name.clone()
    };
    let repo_url = format!("{}/repos/{}/{}", github::API, account, repo);

    match github::get(&state.client, &repo_url, token).await? {
        // 不存在 → 创建（auto_init 让仓库可立即写入）
        None => {
            github::send_json(
                &state.client,
                reqwest::Method::POST,
                &format!("{}/user/repos", github::API),
                token,
                Some(&json!({
                    "name": repo,
                    "private": true,
                    "auto_init": true,
                    "description": "Qookix Launcher 云存档（自动创建；请勿手动修改 qookix.json）",
                })),
            )
            .await
            .map_err(|e| format!("创建云存档仓库失败: {e}"))?;
            // 新建仓库的默认分支初始化有延迟，重试写入标识文件
            let rid = uuid::Uuid::new_v4().simple().to_string();
            let mut last_err = String::new();
            for _ in 0..5 {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                match put_marker(state, token, &account, &repo, &rid, None).await {
                    Ok(()) => {
                        cs.repo_name = repo.clone();
                        cs.repository_id = rid.clone();
                        store::save(state, &cs)?;
                        return Ok(json!({ "repoName": repo, "repositoryId": rid }));
                    }
                    Err(e) => last_err = e,
                }
            }
            Err(format!("仓库已创建，但写入标识文件失败: {last_err}"))
        }
        // 已存在 → 读标识文件校验归属
        Some(_) => {
            let marker = read_marker(state, token, &account, &repo).await?;
            let Some(m) = marker else {
                return Err(format!(
                    "已存在仓库 {account}/{repo}，但它不是云存档仓库（缺少 {MARKER}）。请在 GitHub 上删除或改名该仓库后重试。"
                ));
            };
            let ty = m.get("type").and_then(|v| v.as_str()).unwrap_or("");
            let rid = m
                .get("repository_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if ty != MARKER_TYPE {
                return Err("该仓库的标识文件类型不匹配，可能不是本启动器创建的云存档仓库".into());
            }
            if !cs.repository_id.is_empty() && cs.repository_id != rid {
                return Err("该仓库与本地记录的仓库 id 不一致，请确认账号或仓库是否正确".into());
            }
            cs.repo_name = repo.clone();
            cs.repository_id = rid.clone();
            store::save(state, &cs)?;
            Ok(json!({ "repoName": repo, "repositoryId": rid }))
        }
    }
}

async fn read_marker(
    state: &AppState,
    token: &str,
    account: &str,
    repo: &str,
) -> Result<Option<serde_json::Value>, String> {
    let url = format!(
        "{}/repos/{}/{}/contents/{}",
        github::API,
        account,
        repo,
        MARKER
    );
    let Some(v) = github::get(&state.client, &url, token).await? else {
        return Ok(None);
    };
    let content = v.get("content").and_then(|c| c.as_str()).unwrap_or("");
    let cleaned: String = content.chars().filter(|c| !c.is_whitespace()).collect();
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(cleaned) else {
        return Ok(None);
    };
    Ok(serde_json::from_slice(&bytes).ok())
}

async fn put_marker(
    state: &AppState,
    token: &str,
    account: &str,
    repo: &str,
    repository_id: &str,
    sha: Option<&str>,
) -> Result<(), String> {
    let content = json!({
        "type": MARKER_TYPE,
        "schema_version": 1,
        "repository_id": repository_id,
    });
    let encoded = base64::engine::general_purpose::STANDARD.encode(content.to_string());
    let mut body = json!({
        "message": "chore: 初始化 Qookix 云存档仓库",
        "content": encoded,
    });
    if let Some(s) = sha {
        body["sha"] = json!(s);
    }
    let url = format!(
        "{}/repos/{}/{}/contents/{}",
        github::API,
        account,
        repo,
        MARKER
    );
    github::send_json(&state.client, reqwest::Method::PUT, &url, token, Some(&body)).await?;
    Ok(())
}
