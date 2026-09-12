//! Yggdrasil（authlib-injector）皮肤站账号：登录、令牌保活与 authlib-injector 获取。
//!
//! 协议遵循 authlib-injector 规范的 authserver 端点，Blessing Skin 系皮肤站
//! （LittleSkin / 自建站）通用：
//! - `POST /authserver/authenticate` 登录
//! - `POST /authserver/validate` 校验令牌（204 即有效）
//! - `POST /authserver/refresh` 刷新令牌（rotate：旧令牌立即失效）
//! - `GET <root>` 返回站点 meta（serverName / 链接 / skinDomains）
//! - authlib-injector jar 从官方 artifact 接口获取（皮肤站 meta 不提供）

use crate::models::Account;
use crate::state::AppState;
use base64::Engine as _;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 规整用户输入的 API root：去空白与尾部斜杠。
pub fn normalize_root(root: &str) -> String {
    root.trim().trim_end_matches('/').to_string()
}

async fn post_json(state: &AppState, url: &str, body: &Value) -> Result<(u16, Value), String> {
    let resp = state
        .client
        .post(url)
        .json(body)
        .send()
        .await
        .map_err(|e| format!("请求皮肤站失败: {e}"))?;
    let status = resp.status().as_u16();
    let v: Value = resp
        .json()
        .await
        .map_err(|e| format!("解析皮肤站响应失败: {e}"))?;
    Ok((status, v))
}

/// 把皮肤站的错误响应转成用户可读的信息（Yggdrasil 约定 `errorMessage` 字段）。
fn server_error(status: u16, v: &Value, fallback: &str) -> String {
    let msg = v
        .get("errorMessage")
        .and_then(|m| m.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| fallback.to_string());
    format!("{msg}（HTTP {status}）")
}

/// 站点主页 / 注册页链接（meta 的 `links` 字段，登录页展示用）。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct YggLinks {
    pub homepage: String,
    pub register: String,
}

/// 登录结果：令牌 + 站点信息 + 可选角色列表（多角色时由用户挑选）。
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct YggLoginResult {
    pub access_token: String,
    pub client_token: String,
    /// 规整后的 API root
    pub server: String,
    /// 站点 meta 里的 serverName（如 "LittleSkin"）
    pub server_name: String,
    pub profiles: Vec<YggProfile>,
    pub links: YggLinks,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct YggProfile {
    pub id: String,
    pub name: String,
}

/// 拉取皮肤站账号的皮肤/披风纹理，下载为 data URL 供启动器内渲染头像。
/// 角色没有皮肤数据时返回 null。
#[tauri::command]
pub async fn yggdrasil_textures(
    state: tauri::State<'_, AppState>,
    server: String,
    profile_id: String,
) -> Result<Option<Value>, String> {
    let root = normalize_root(&server);
    let pid = profile_id.trim().to_string();
    if root.is_empty() || pid.is_empty() {
        return Ok(None);
    }
    let url = format!("{root}/sessionserver/session/minecraft/profile/{pid}");
    let resp = state
        .client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("访问皮肤站 sessionserver 失败: {e}"))?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None); // 该角色没有纹理数据
    }
    if !resp.status().is_success() {
        return Err(format!("拉取纹理失败（HTTP {}）", resp.status().as_u16()));
    }
    let profile: Value = resp.json().await.map_err(|e| e.to_string())?;
    let encoded = profile
        .get("properties")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|p| p.get("name").and_then(|n| n.as_str()) == Some("textures"))
        })
        .and_then(|p| p.get("value").and_then(|v| v.as_str()))
        .ok_or_else(|| "纹理数据格式异常".to_string())?;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| format!("纹理数据 base64 解码失败: {e}"))?;
    let textures: Value = serde_json::from_slice(&decoded).map_err(|e| e.to_string())?;
    let skin_url = textures.pointer("/textures/SKIN/url").and_then(|v| v.as_str());
    let cape_url = textures.pointer("/textures/CAPE/url").and_then(|v| v.as_str());
    // 皮肤站图片不带 CORS 头，由后端下载转 data URL，前端 canvas 才能裁头
    async fn download(state: &AppState, url: &str) -> Result<Vec<u8>, String> {
        let bytes = state
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| format!("下载纹理失败: {e}"))?
            .bytes()
            .await
            .map_err(|e| e.to_string())?;
        Ok(bytes.to_vec())
    }
    let encode =
        |bytes: &[u8]| format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes));
    let skin = match skin_url {
        Some(u) => Some(encode(&download(&state, u).await?)),
        None => None,
    };
    let cape = match cape_url {
        Some(u) => Some(encode(&download(&state, u).await?)),
        None => None,
    };
    if skin.is_none() && cape.is_none() {
        return Ok(None);
    }
    Ok(Some(json!({ "skin": skin, "cape": cape })))
}

/// 登录皮肤站：验证账号密码，返回令牌与该账号下的全部角色。
/// 顺带拉取站点 meta，带出站点名与主页/注册链接（登录页展示用）。
#[tauri::command]
pub async fn yggdrasil_login(
    state: tauri::State<'_, AppState>,
    server_url: String,
    username: String,
    password: String,
) -> Result<YggLoginResult, String> {
    let root = normalize_root(&server_url);
    if root.is_empty() {
        return Err("请填写皮肤站 API 地址".into());
    }
    if username.trim().is_empty() || password.is_empty() {
        return Err("请填写账号和密码".into());
    }
    // 先拉站点 meta：拿 serverName 与主页/注册链接，顺带验证地址是否可达
    let meta: Value = crate::download::get_json(&state.client, &root)
        .await
        .map_err(|e| format!("无法访问皮肤站（{root}）: {e}"))?;
    let server_name = meta
        .pointer("/meta/serverName")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("自定义皮肤站")
        .to_string();
    let links = YggLinks {
        homepage: meta
            .pointer("/links/homepage")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        register: meta
            .pointer("/links/register")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    };

    let client_token = uuid::Uuid::new_v4().simple().to_string();
    let (status, body) = post_json(
        state.inner(),
        &format!("{root}/authserver/authenticate"),
        &json!({
            // 注意：Yggdrasil 规范要求 agent.name 恰为 "Minecraft"，
            // 写成 "MinecraftClient" 会被 Blessing Skin 系服务端以 400 拒绝
            "agent": { "name": "Minecraft", "version": 1 },
            "username": username.trim(),
            "password": password,
            "clientToken": client_token,
        }),
    )
    .await?;
    if status != 200 {
        return Err(server_error(
            status,
            &body,
            "登录失败，请检查账号、密码或皮肤站地址",
        ));
    }
    let access_token = body
        .get("accessToken")
        .and_then(|v| v.as_str())
        .ok_or_else(|| server_error(status, &body, "皮肤站未返回令牌"))?
        .to_string();
    let profiles: Vec<YggProfile> = body
        .get("availableProfiles")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|p| {
                    Some(YggProfile {
                        id: p.get("id").and_then(|v| v.as_str())?.to_string(),
                        name: p.get("name").and_then(|v| v.as_str())?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    if profiles.is_empty() {
        return Err("该账号下没有角色，请先到皮肤站创建角色".into());
    }
    Ok(YggLoginResult {
        access_token,
        client_token,
        server: root,
        server_name,
        profiles,
        links,
    })
}

/// 把登录结果 + 用户选定的角色登记为一个皮肤站账号（同角色旧记录会被替换）。
#[tauri::command]
pub async fn yggdrasil_add_account(
    state: tauri::State<'_, AppState>,
    server_url: String,
    server_name: String,
    access_token: String,
    client_token: String,
    profile_id: String,
    profile_name: String,
) -> Result<Account, String> {
    if profile_id.is_empty() || profile_name.is_empty() {
        return Err("缺少角色信息".into());
    }
    let account = Account::Yggdrasil {
        uuid: profile_id,
        username: profile_name,
        created: now_secs(),
        access_token,
        client_token,
        server: normalize_root(&server_url),
        server_name: if server_name.trim().is_empty() {
            "自定义皮肤站".into()
        } else {
            server_name.trim().to_string()
        },
    };
    let mut list = load_accounts(state.inner());
    list.retain(|a| a.uuid() != account.uuid());
    list.push(account.clone());
    save_accounts(state.inner(), &list)?;
    Ok(account)
}

/// 校验令牌是否有效（`/authserver/validate` 返回 204 即有效）。
pub async fn validate(
    state: &AppState,
    root: &str,
    access_token: &str,
    client_token: &str,
) -> bool {
    match post_json(
        state,
        &format!("{root}/authserver/validate"),
        &json!({ "accessToken": access_token, "clientToken": client_token }),
    )
    .await
    {
        Ok((status, _)) => status == 204 || status == 200,
        Err(_) => false,
    }
}

/// 刷新令牌（rotate：返回新令牌，旧的立即失效）。
pub async fn refresh(
    state: &AppState,
    root: &str,
    access_token: &str,
    client_token: &str,
) -> Result<String, String> {
    let (status, body) = post_json(
        state,
        &format!("{root}/authserver/refresh"),
        &json!({ "accessToken": access_token, "clientToken": client_token }),
    )
    .await?;
    if status != 200 {
        return Err(server_error(status, &body, "令牌已失效，请重新登录皮肤站"));
    }
    body.get("accessToken")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "皮肤站未返回新令牌".into())
}

/// 启动前调用：令牌有效则原样返回；失效则尝试刷新并持久化新令牌。
/// 刷新也失败时返回错误，提示用户重新登录。
pub async fn ensure_token(state: &AppState, account_uuid: &str) -> Result<String, String> {
    let account = {
        let list = load_accounts(state);
        list.iter()
            .find(|a| a.uuid() == account_uuid)
            .cloned()
            .ok_or("账号不存在")?
    };
    let (root, access_token, client_token) = match &account {
        Account::Yggdrasil {
            server,
            access_token,
            client_token,
            ..
        } => (server.clone(), access_token.clone(), client_token.clone()),
        _ => return Err("该账号不是皮肤站账号".into()),
    };
    if validate(state, &root, &access_token, &client_token).await {
        return Ok(access_token);
    }
    let new_token = refresh(state, &root, &access_token, &client_token).await?;
    let mut list = load_accounts(state);
    if let Some(acc) = list.iter_mut().find(|a| a.uuid() == account_uuid) {
        if let Account::Yggdrasil { access_token, .. } = acc {
            *access_token = new_token.clone();
        }
    }
    save_accounts(state, &list)?;
    Ok(new_token)
}

/// 命令包装：供前端在启动前手动触发令牌保活。
#[tauri::command]
pub async fn yggdrasil_ensure_token(
    state: tauri::State<'_, AppState>,
    account_uuid: String,
) -> Result<String, String> {
    ensure_token(state.inner(), &account_uuid).await
}

/// 确保本地存在 authlib-injector.jar，返回其路径。
///
/// jar 不在皮肤站 meta 里，从官方 artifact 接口（latest.json）获取：
/// 官方源在前，LittleSkin 镜像兜底（国内直连官方源常超时）。
/// 本地已有时按 sha256 决定复用还是重新下载；latest.json 拉不到
/// （如离线）但本地有旧 jar 时宽容复用。
pub async fn ensure_authlib_injector(
    state: &AppState,
    _api_root: &str,
) -> Result<PathBuf, String> {
    const OFFICIAL: &str = "https://authlib-injector.moe.yushi.moe";
    const MIRROR: &str = "https://download.littleskin.cn/moe/yushi/authlibinjector";

    let dest_dir = state.root.join("authlib-injector");
    let dest = dest_dir.join("authlib-injector.jar");

    let mut latest: Option<Value> = None;
    let mut last_err = String::new();
    for base in [OFFICIAL, MIRROR] {
        match crate::download::get_json(&state.client, &format!("{base}/artifact/latest.json"))
            .await
        {
            Ok(v) => {
                latest = Some(v);
                break;
            }
            Err(e) => last_err = e,
        }
    }
    let expected = latest
        .as_ref()
        .and_then(|v| v.pointer("/checksums/sha256"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_ascii_lowercase());

    if dest.exists() {
        // 没有校验值（latest 全挂）时信任本地文件
        let usable = match &expected {
            Some(want) => crate::util::file_sha256(&dest)
                .map(|h| h.eq_ignore_ascii_case(want))
                .unwrap_or(false),
            None => true,
        };
        if usable {
            return Ok(dest);
        }
    }

    let latest = latest.ok_or_else(|| format!("获取 authlib-injector 版本信息失败: {last_err}"))?;
    let url = latest
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or("authlib-injector latest.json 缺少下载地址")?
        .to_string();

    async fn fetch(state: &AppState, url: &str) -> Result<Vec<u8>, String> {
        state
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| e.to_string())
    }
    let bytes = match fetch(state, &url).await {
        Ok(b) => b,
        Err(e) => {
            // 官方源不通时，把路径原样映射到 LittleSkin 镜像重试一次
            let mirror = url.replacen(OFFICIAL, MIRROR, 1);
            fetch(state, &mirror)
                .await
                .map_err(|e2| format!("下载 authlib-injector 失败: {e} / {e2}"))?
        }
    };

    if let Some(want) = &expected {
        use sha2::Digest;
        let actual = sha2::Sha256::digest(&bytes);
        let hex: String = actual.iter().map(|b| format!("{b:02x}")).collect();
        if !hex.eq_ignore_ascii_case(want) {
            return Err("authlib-injector 校验失败（sha256 不匹配），请重试".into());
        }
    }
    std::fs::create_dir_all(&dest_dir).map_err(|e| format!("创建目录失败: {e}"))?;
    std::fs::write(&dest, &bytes).map_err(|e| format!("写入 authlib-injector 失败: {e}"))?;
    Ok(dest)
}

fn load_accounts(state: &AppState) -> Vec<Account> {
    crate::accounts::load_accounts(state)
}

fn save_accounts(state: &AppState, list: &[Account]) -> Result<(), String> {
    crate::accounts::save_accounts(state, list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_root_trims_slash_and_space() {
        assert_eq!(
            normalize_root(" https://littleskin.cn/api/yggdrasil/ "),
            "https://littleskin.cn/api/yggdrasil"
        );
        assert_eq!(normalize_root(""), "");
        assert_eq!(normalize_root("https://skin.example.com/yggdrasil"), "https://skin.example.com/yggdrasil");
    }

    #[test]
    fn littleskin_official_root_survives_normalization() {
        assert_eq!(
            normalize_root("https://littleskin.cn/api/yggdrasil"),
            "https://littleskin.cn/api/yggdrasil"
        );
    }
}
