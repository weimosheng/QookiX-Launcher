//! GitHub OAuth Device Flow：授权、令牌刷新与断连。
//! Client ID 为公开信息，硬编码安全（无 Client Secret）。

use super::github;
use super::store::{self, CloudState};
use crate::state::AppState;
use serde_json::json;

const CLIENT_ID: &str = "Ov23liANpccEHSzQmJs1";

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 设备流程第一步：申请设备码与用户码。
pub async fn start_device_flow(state: &AppState) -> Result<serde_json::Value, String> {
    let resp = state
        .client
        .post("https://github.com/login/device/code")
        .header("User-Agent", github::UA)
        .header("Accept", "application/json")
        .form(&[("client_id", CLIENT_ID), ("scope", "repo")])
        .send()
        .await
        .map_err(|e| format!("请求设备码失败: {e}"))?;
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析设备码响应失败: {e}"))?;
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
        return Err(format!("申请设备码失败: {err}"));
    }
    Ok(json!({
        "deviceCode": v.get("device_code").and_then(|x| x.as_str()).unwrap_or(""),
        "userCode": v.get("user_code").and_then(|x| x.as_str()).unwrap_or(""),
        "verificationUri": v.get("verification_uri").and_then(|x| x.as_str()).unwrap_or("https://github.com/login/device"),
        "interval": v.get("interval").and_then(|x| x.as_u64()).unwrap_or(5),
        "expiresIn": v.get("expires_in").and_then(|x| x.as_u64()).unwrap_or(900),
    }))
}

/// 设备流程第二步：轮询换取令牌。返回 `{ status: "pending" | "ok", ... }`。
pub async fn poll_device_flow(state: &AppState, device_code: &str) -> Result<serde_json::Value, String> {
    let resp = state
        .client
        .post("https://github.com/login/oauth/access_token")
        .header("User-Agent", github::UA)
        .header("Accept", "application/json")
        .form(&[
            ("client_id", CLIENT_ID),
            ("device_code", device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ])
        .send()
        .await
        .map_err(|e| format!("轮询令牌失败: {e}"))?;
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析令牌响应失败: {e}"))?;
    if let Some(err) = v.get("error").and_then(|x| x.as_str()) {
        println!("[cloud_sync] poll resp error: {err}");
    }
    if let Some(access) = v.get("access_token").and_then(|x| x.as_str()) {
        let mut cs = store::load(state);
        cs.access_token = store::encrypt_secret(access);
        cs.refresh_token = store::encrypt_secret(
            v.get("refresh_token").and_then(|x| x.as_str()).unwrap_or(""),
        );
        let expires_in = v.get("expires_in").and_then(|x| x.as_u64()).unwrap_or(0);
        cs.expires_at = if expires_in > 0 { now_secs() + expires_in } else { 0 };
        // 拉取账号名
        cs.account = fetch_account(state, access).await.unwrap_or_default();
        store::save(state, &cs)?;
        return Ok(json!({ "status": "ok", "account": cs.account }));
    }
    let err = v.get("error").and_then(|x| x.as_str()).unwrap_or("unknown");
    match err {
        "authorization_pending" | "slow_down" => Ok(json!({ "status": "pending" })),
        "expired_token" => Err("用户码已过期，请重新开始授权".into()),
        "access_denied" => Err("用户拒绝了授权".into()),
        other => Err(format!("授权失败: {other}")),
    }
}

async fn fetch_account(state: &AppState, token: &str) -> Result<String, String> {
    let Some(v) = github::get(&state.client, &format!("{}/user", github::API), token).await? else {
        return Err("获取账号信息失败".into());
    };
    Ok(v.get("login").and_then(|l| l.as_str()).unwrap_or("").to_string())
}

/// 确保 access_token 未过期；过期则用 refresh_token 换新（轮换后必须保存新的）。
pub async fn ensure_token(state: &AppState) -> Result<String, String> {
    let mut cs = store::load(state);
    let token = store::access_token(&cs);
    if token.is_empty() {
        return Err("尚未连接 GitHub，请先授权".into());
    }
    // 有效期留 5 分钟余量；expires_at == 0 表示令牌不过期（未启用过期选项）
    if cs.expires_at == 0 || now_secs() + 300 < cs.expires_at {
        return Ok(token);
    }
    let refresh = store::decrypt_secret(&cs.refresh_token);
    if refresh.is_empty() {
        return Err("令牌已过期且无刷新令牌，请重新授权".into());
    }
    let resp = state
        .client
        .post("https://github.com/login/oauth/access_token")
        .header("User-Agent", github::UA)
        .header("Accept", "application/json")
        .form(&[
            ("client_id", CLIENT_ID),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh.as_str()),
        ])
        .send()
        .await
        .map_err(|e| format!("刷新令牌失败: {e}"))?;
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析刷新响应失败: {e}"))?;
    let Some(access) = v.get("access_token").and_then(|x| x.as_str()) else {
        let err = v.get("error").and_then(|x| x.as_str()).unwrap_or("unknown");
        return Err(format!("刷新令牌失败: {err}（请重新授权）"));
    };
    cs.access_token = store::encrypt_secret(access);
    if let Some(rt) = v.get("refresh_token").and_then(|x| x.as_str()) {
        cs.refresh_token = store::encrypt_secret(rt); // 旧 refresh_token 立即失效，必须存新的
    }
    let expires_in = v.get("expires_in").and_then(|x| x.as_u64()).unwrap_or(0);
    cs.expires_at = if expires_in > 0 { now_secs() + expires_in } else { 0 };
    store::save(state, &cs)?;
    Ok(access.to_string())
}

/// 断开连接：清空令牌与仓库信息（云端仓库与数据不动，由用户自行处置）。
pub fn disconnect(state: &AppState) -> Result<(), String> {
    let cs = CloudState {
        keep_per_world: store::load(state).keep_per_world,
        ..Default::default()
    };
    store::save(state, &cs)
}
