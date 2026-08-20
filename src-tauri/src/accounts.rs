use crate::models::{Account, MsFlow};
use crate::state::AppState;
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

pub const MS_AUTH_SCOPE: &str = "XboxLive.signin offline_access";
pub const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
pub const DEVICE_CODE_URL: &str =
    "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";

/// Legacy Live SDK client ID — works without Azure AD app registration.
pub const LEGACY_CLIENT_ID: &str = "00000000402b5328";
pub const LEGACY_TOKEN_URL: &str = "https://login.live.com/oauth20_token.srf";
pub const LEGACY_DEVICE_CODE_URL: &str = "https://login.live.com/oauth20_devicecode.srf";
pub const LEGACY_AUTH_SCOPE: &str = "service::user.auth.xboxlive.com::MBI_SSL";

fn is_legacy_client_id(client_id: &str) -> bool {
    client_id == LEGACY_CLIENT_ID
}

/// Built-in Microsoft Client ID. Injected at compile time via MS_CLIENT_ID env var.
/// Falls back to placeholder if not set (build with `MS_CLIENT_ID=your-id cargo build`).
pub const BUILTIN_MS_CLIENT_ID: &str = match option_env!("MS_CLIENT_ID") {
    Some(id) => id,
    None => "",
};

/// Resolve the effective Microsoft Client ID: user setting > built-in.
pub fn effective_ms_client_id(state: &AppState) -> Result<String, String> {
    let s = state.settings.read().unwrap();
    let user_id = s.ms_client_id.trim();
    if !user_id.is_empty() && user_id != "00000000-0000-0000-0000-000000000000" {
        return Ok(user_id.to_string());
    }
    if !BUILTIN_MS_CLIENT_ID.is_empty() {
        return Ok(BUILTIN_MS_CLIENT_ID.to_string());
    }
    // Fallback to legacy Live SDK client ID (works without Azure AD config)
    Ok(LEGACY_CLIENT_ID.to_string())
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Offline accounts
// ---------------------------------------------------------------------------

/// Create an offline account: UUID v3 of "OfflinePlayer:<name>" (same as the
/// official launcher / Java's `nameUUIDFromBytes` with nil namespace).
pub fn create_offline(state: &AppState, username: &str) -> Result<Account, String> {
    let name = username.trim();
    if name.is_empty() {
        return Err("用户名不能为空".into());
    }
    if name.len() > 16 {
        return Err("用户名不能超过 16 个字符".into());
    }
    let uuid = uuid::Uuid::new_v3(&uuid::Uuid::nil(), format!("OfflinePlayer:{name}").as_bytes());
    let account = Account::Offline {
        uuid: uuid.to_string(),
        username: name.to_string(),
        created: now(),
    };
    save_account(state, &account)?;
    Ok(account)
}

// ---------------------------------------------------------------------------
// Microsoft device-code flow
// ---------------------------------------------------------------------------

/// Step 1: request a device code. Returns the URL + code the user must visit.
pub async fn ms_start(state: &AppState) -> Result<serde_json::Value, String> {
    let client_id = effective_ms_client_id(state)?;
    let legacy = is_legacy_client_id(&client_id);
    let dc_url = if legacy { LEGACY_DEVICE_CODE_URL } else { DEVICE_CODE_URL };
    let scope = if legacy { LEGACY_AUTH_SCOPE } else { MS_AUTH_SCOPE };
    let resp = state
        .client
        .post(dc_url)
        .form(&[("client_id", client_id.as_str()), ("scope", scope)])
        .send()
        .await
        .map_err(|e| format!("请求设备码失败: {e}"))?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("设备码请求失败 (HTTP {status}): {}", body));
    }
    let device_code = body
        .get("device_code")
        .and_then(|v| v.as_str())
        .ok_or("响应缺少 device_code")?
        .to_string();
    let interval = body.get("interval").and_then(|v| v.as_u64()).unwrap_or(5);
    let expires_in = body.get("expires_in").and_then(|v| v.as_u64()).unwrap_or(900);
    let flow = MsFlow {
        device_code,
        interval,
        expires_at: now() + expires_in,
        client_id,
    };
    *state.ms_flow.lock().unwrap() = Some(flow);
    // Legacy endpoint uses "verification_url", v2.0 uses "verification_uri"
    let verification_uri = body
        .get("verification_uri")
        .and_then(|v| v.as_str())
        .or_else(|| body.get("verification_url").and_then(|v| v.as_str()))
        .unwrap_or("https://microsoft.com/link");
    Ok(json!({
        "userCode": body.get("user_code").and_then(|v| v.as_str()).unwrap_or(""),
        "verificationUri": verification_uri,
        "expiresIn": expires_in,
    }))
}

/// Step 2: poll the token endpoint until the user authorizes.
/// Returns the new Microsoft account on success.
pub async fn ms_poll(state: &AppState) -> Result<Account, String> {
    let flow = state
        .ms_flow
        .lock()
        .unwrap()
        .clone()
        .ok_or("没有进行中的设备码登录")?;
    if now() > flow.expires_at {
        *state.ms_flow.lock().unwrap() = None;
        return Err("设备码已过期，请重新开始登录".into());
    }
    let legacy = is_legacy_client_id(&flow.client_id);
    let tok_url = if legacy { LEGACY_TOKEN_URL } else { TOKEN_URL };
    let resp = state
        .client
        .post(tok_url)
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", flow.client_id.as_str()),
            ("device_code", flow.device_code.as_str()),
        ])
        .send()
        .await
        .map_err(|e| format!("轮询失败: {e}"))?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let err = body
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        return match err {
            "authorization_pending" => Err("pending".into()),
            "authorization_declined" => Err("用户拒绝了授权".into()),
            "expired_token" => Err("设备码已过期，请重新开始登录".into()),
            "slow_down" => Err("请稍后再试".into()),
            other => Err(format!("登录失败: {other}")),
        };
    }
    let access_token = body
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or("缺少 access_token")?
        .to_string();
    let refresh_token = body
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .ok_or("缺少 refresh_token")?
        .to_string();
    let expires_in = body.get("expires_in").and_then(|v| v.as_u64()).unwrap_or(3600);

    *state.ms_flow.lock().unwrap() = None;
    let account = xbox_login(state, &access_token, &refresh_token, expires_in).await?;
    save_account(state, &account)?;
    Ok(account)
}

/// Exchange a Microsoft access token for a Minecraft profile via XBL/XSTS.
async fn xbox_login(
    state: &AppState,
    msa_token: &str,
    refresh_token: &str,
    expires_in: u64,
) -> Result<Account, String> {
    // 1. XBL authenticate
    let xbl_resp = state
        .client
        .post("https://user.auth.xboxlive.com/user/authenticate")
        .json(&json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={msa_token}"),
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT",
        }))
        .send()
        .await
        .map_err(|e| format!("XBL 认证失败: {e}"))?;
    let xbl_status = xbl_resp.status();
    let xbl: serde_json::Value = xbl_resp.json().await.map_err(|e| e.to_string())?;
    if !xbl_status.is_success() {
        let body_str = serde_json::to_string(&xbl).unwrap_or_default();
        return Err(format!(
            "XBL 认证失败 (HTTP {}): {}",
            xbl_status.as_u16(),
            body_str
        ));
    }
    let xbl_token = xbl
        .get("Token")
        .and_then(|v| v.as_str())
        .ok_or("XBL 响应缺少 Token")?;

    // 2. XSTS authorize
    let xsts_resp = state
        .client
        .post("https://xsts.auth.xboxlive.com/xsts/authorize")
        .json(&json!({
            "Properties": {
                "SandboxId": "RETAIL",
                "UserTokens": [xbl_token],
            },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT",
        }))
        .send()
        .await
        .map_err(|e| format!("XSTS 认证失败: {e}"))?;
    let xsts_status = xsts_resp.status();
    let xsts: serde_json::Value = xsts_resp.json().await.map_err(|e| e.to_string())?;
    if !xsts_status.is_success() || xsts.get("XErr").is_some() {
        if let Some(err) = xsts.get("XErr").and_then(|v| v.as_str()) {
            return Err(match err {
                "2148916233" => "该账号没有 Xbox 档案，请先在 xbox.com 注册".into(),
                "2148916235" => "该账号所在地区不支持 Xbox Live".into(),
                "2148916238" => "未成年账号需要家长同意".into(),
                other => format!("XSTS 错误 {other}"),
            });
        }
        let body_str = serde_json::to_string(&xsts).unwrap_or_default();
        return Err(format!(
            "XSTS 认证失败 (HTTP {}): {}",
            xsts_status.as_u16(),
            body_str
        ));
    }
    let xsts_token = xsts
        .get("Token")
        .and_then(|v| v.as_str())
        .ok_or("XSTS 响应缺少 Token")?;
    let uhs = xsts
        .get("DisplayClaims")
        .and_then(|d| d.get("xui"))
        .and_then(|x| x.as_array())
        .and_then(|a| a.first())
        .and_then(|o| o.get("uhs"))
        .and_then(|v| v.as_str())
        .ok_or("XSTS 响应缺少 uhs")?;

    // 3. Minecraft login
    let mc_resp = state
        .client
        .post("https://api.minecraftservices.com/authentication/login_with_xbox")
        .json(&json!({
            "identityToken": format!("XBL3.0 x={uhs};{xsts_token}"),
        }))
        .send()
        .await
        .map_err(|e| format!("Minecraft 登录失败: {e}"))?;
    let mc_status = mc_resp.status();
    let mc: serde_json::Value = mc_resp.json().await.map_err(|e| e.to_string())?;
    if !mc_status.is_success() {
        let body_str = serde_json::to_string(&mc).unwrap_or_default();
        return Err(format!(
            "Minecraft 登录失败 (HTTP {}): {}",
            mc_status.as_u16(),
            body_str
        ));
    }
    let mc_token = mc
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            format!(
                "Minecraft 登录响应缺少 access_token (响应: {})",
                serde_json::to_string(&mc).unwrap_or_default()
            )
        })?;

    // 4. profile
    let prof_resp = state
        .client
        .get("https://api.minecraftservices.com/minecraft/profile")
        .bearer_auth(mc_token)
        .send()
        .await
        .map_err(|e| format!("获取档案失败: {e}"))?;
    let prof_status = prof_resp.status();
    let profile: serde_json::Value = prof_resp.json().await.map_err(|e| e.to_string())?;
    if !prof_status.is_success() {
        let detail = profile
            .get("errorMessage")
            .and_then(|v| v.as_str())
            .or_else(|| profile.get("error").and_then(|v| v.as_str()))
            .unwrap_or("未知错误");
        return Err(format!("获取 Minecraft 档案失败: {detail}"));
    }
    let uuid = profile
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("该 Microsoft 账号没有购买 Minecraft (需要正版账号)")?;
    let username = profile
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("Player");

    Ok(Account::Microsoft {
        uuid: uuid.to_string(),
        username: username.to_string(),
        created: now(),
        msa_refresh_token: refresh_token.to_string(),
        msa_access_token: mc_token.to_string(),
        msa_expires_at: now() + expires_in,
    })
}

/// Refresh a Microsoft account's access token (called before launching).
pub async fn refresh_microsoft(state: &AppState, account: &Account) -> Result<Account, String> {
    let Account::Microsoft {
        msa_refresh_token,
        msa_access_token: _,
        msa_expires_at,
        ..
    } = account
    else {
        return Ok(account.clone());
    };
    // allow 5 min slack
    if now() + 300 < *msa_expires_at {
        return Ok(account.clone());
    }
    let client_id = effective_ms_client_id(state)?;
    let legacy = is_legacy_client_id(&client_id);
    let tok_url = if legacy { LEGACY_TOKEN_URL } else { TOKEN_URL };
    let scope = if legacy { LEGACY_AUTH_SCOPE } else { MS_AUTH_SCOPE };
    let resp = state
        .client
        .post(tok_url)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", client_id.as_str()),
            ("refresh_token", msa_refresh_token.as_str()),
            ("scope", scope),
        ])
        .send()
        .await
        .map_err(|e| format!("刷新令牌失败: {e}"))?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!(
            "刷新令牌失败 (HTTP {status}): {}",
            body.get("error_description").and_then(|v| v.as_str()).unwrap_or("请重新登录")
        ));
    }
    let access_token = body
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or("刷新响应缺少 access_token")?
        .to_string();
    let refresh_token = body
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .unwrap_or(msa_refresh_token)
        .to_string();
    let expires_in = body.get("expires_in").and_then(|v| v.as_u64()).unwrap_or(3600);
    let new = xbox_login(state, &access_token, &refresh_token, expires_in).await?;
    save_account(state, &new)?;
    Ok(new)
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

pub fn load_accounts(state: &AppState) -> Vec<Account> {
    std::fs::read_to_string(state.accounts_path())
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<Account>>(&s).ok())
        .unwrap_or_default()
}

pub fn save_accounts(state: &AppState, accounts: &[Account]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(accounts).map_err(|e| e.to_string())?;
    std::fs::write(state.accounts_path(), json).map_err(|e| e.to_string())
}

fn save_account(state: &AppState, account: &Account) -> Result<(), String> {
    let mut accounts = load_accounts(state);
    accounts.retain(|a| a.uuid() != account.uuid());
    accounts.push(account.clone());
    save_accounts(state, &accounts)
}

pub fn remove_account(state: &AppState, uuid: &str) -> Result<(), String> {
    let mut accounts = load_accounts(state);
    accounts.retain(|a| a.uuid() != uuid);
    save_accounts(state, &accounts)
}
