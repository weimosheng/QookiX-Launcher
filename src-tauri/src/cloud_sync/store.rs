//! 云同步本地状态：GitHub 凭据、仓库信息、世界映射与自动同步设置。
//! 存放于数据目录 `cloud_sync/state.json`。令牌做简单混淆后落盘
//! （与项目既有做法一致；后续可平滑升级为系统钥匙串）。

use crate::state::AppState;
use base64::Engine as _;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct WorldLink {
    /// 实例 id
    pub instance_id: String,
    /// 世界目录名
    pub world_dir: String,
    /// 云端世界 id（uuid）
    pub world_id: String,
    /// 游戏退出后自动上传
    pub auto_sync: bool,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct CloudState {
    /// 混淆后的 access_token
    pub access_token: String,
    /// 混淆后的 refresh_token
    pub refresh_token: String,
    /// access_token 过期时间（unix 秒）
    pub expires_at: u64,
    /// GitHub 登录名
    pub account: String,
    /// 仓库名
    pub repo_name: String,
    /// 仓库唯一 id
    pub repository_id: String,
    /// 每个世界保留的快照数上限
    pub keep_per_world: u32,
    /// 世界映射（本地目录 ↔ 云端世界 id / 自动同步开关）
    pub worlds: Vec<WorldLink>,
}

impl CloudState {
    /// 是否已授权（拿到令牌）。仓库是否就绪须单独检查 `repo_name`，
    /// 否则刚授权完、仓库还没建时会误判为未连接。
    pub fn connected(&self) -> bool {
        !self.access_token.is_empty()
    }
}

fn state_path(state: &AppState) -> std::path::PathBuf {
    state.root.join("cloud_sync").join("state.json")
}

/// 简单可逆混淆（非强加密）：仅作为 DPAPI 不可用时的降级手段。
pub fn obfuscate(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let bytes: Vec<u8> = s.bytes().enumerate().map(|(i, b)| b ^ (0x5A ^ (i as u8 & 0x3F))).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub fn deobfuscate(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(s) else {
        return String::new();
    };
    String::from_utf8(
        bytes
            .into_iter()
            .enumerate()
            .map(|(i, b)| b ^ (0x5A ^ (i as u8 & 0x3F)))
            .collect(),
    )
    .unwrap_or_default()
}

/// 加密令牌：优先 DPAPI（Windows，密钥由系统按当前用户派生）；失败降级为混淆。
pub fn encrypt_secret(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    crate::secret::protect(s).unwrap_or_else(|_| obfuscate(s))
}

/// 解密令牌：先按 DPAPI 解，失败则按旧版混淆解（兼容升级前的存量数据）。
pub fn decrypt_secret(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    crate::secret::unprotect(s).unwrap_or_else(|_| deobfuscate(s))
}

pub fn load(state: &AppState) -> CloudState {
    std::fs::read_to_string(state_path(state))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(state: &AppState, cs: &CloudState) -> Result<(), String> {
    let path = state_path(state);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let text = serde_json::to_string_pretty(cs).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("写入失败: {e}"))?;
    println!("[cloud_sync] state saved: {}", path.display());
    Ok(())
}

pub fn access_token(cs: &CloudState) -> String {
    decrypt_secret(&cs.access_token)
}
