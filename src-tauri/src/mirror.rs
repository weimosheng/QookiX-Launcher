//! 下载镜像源
//!
//! 直连 Mojang / Fabric / Forge 官方 CDN 在部分地区经常超时或极慢。本模块按规则
//! 把官方地址改写到镜像站。镜像配置保存在 `settings.json`，**每次请求都重新读
//! 取设置**，所以切换镜像后立即生效，无需重启启动器。
//!
//! 规则面向 BMCLAPI / OpenBMCLAPI 的公开接口，任何兼容该接口的镜像站都可作为
//! 「自定义镜像」直接填写根地址使用：
//!   /mc/game/version_manifest_v2.json   版本清单
//!   /version/{version}/json             版本 JSON
//!   /version/{version}/client           客户端 jar
//!   /version/{version}/server           服务端 jar
//!   /assets/{hash[0..2]}/{hash}         资源文件
//!   /maven/{path}                       依赖库 / Forge / Fabric / NeoForge
//!   /fabric-meta/{path}                 Fabric Meta
//!
//! 镜像缺失文件时，下载器会自动回退到官方地址（见 `download.rs`）。

use crate::state::AppState;
use serde_json::{json, Value};

/// 官方源 id（不做任何改写）
pub const OFFICIAL: &str = "official";
/// 自定义镜像 id（使用 `mirror_custom` 里的根地址）
pub const CUSTOM: &str = "custom";

/// 官方版本清单地址
pub const OFFICIAL_MANIFEST: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

pub struct MirrorPreset {
    pub id: &'static str,
    pub label: &'static str,
    /// 镜像站根地址；官方源为空串
    pub base: &'static str,
    pub desc: &'static str,
}

/// 内置镜像预设（顺序即前端展示顺序）
pub const PRESETS: &[MirrorPreset] = &[
    MirrorPreset {
        id: OFFICIAL,
        label: "官方源",
        base: "",
        desc: "Mojang / Fabric / Forge 官方地址，海外网络推荐",
    },
    MirrorPreset {
        id: "bmclapi",
        label: "BMCLAPI",
        base: "https://bmclapi2.bangbang93.com",
        desc: "国内公益镜像，覆盖游戏本体、资源文件与依赖库",
    },
];

/// 由设置解析出实际生效的镜像根地址（官方源返回空串）。
/// 公开版本便于在已持有 settings 读锁的地方复用，避免重复加锁。
pub fn resolve_from(id: &str, custom: &str) -> String {
    if id == CUSTOM {
        return custom.trim().trim_end_matches('/').to_string();
    }
    PRESETS
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.base.to_string())
        .unwrap_or_default()
}

/// 当前生效的镜像根地址；空串表示官方源（不做任何改写）
pub fn base_url(state: &AppState) -> String {
    let s = state.settings.read().unwrap();
    resolve_from(&s.mirror, &s.mirror_custom)
}

/// 当前是否使用官方源
#[allow(dead_code)]
pub fn is_official(state: &AppState) -> bool {
    base_url(state).is_empty()
}

/// 镜像预设列表（供前端渲染）
pub fn presets() -> Value {
    Value::Array(
        PRESETS
            .iter()
            .map(|p| json!({ "id": p.id, "label": p.label, "base": p.base, "desc": p.desc }))
            .collect(),
    )
}

struct Rule {
    /// 官方地址前缀
    prefix: &'static str,
    /// 镜像侧对应的路径前缀（以 `/` 开头）
    target: &'static str,
}

/// 前缀越长越靠前，避免被更短的通用前缀抢先匹配
const RULES: &[Rule] = &[
    Rule {
        prefix: OFFICIAL_MANIFEST,
        target: "/mc/game/version_manifest_v2.json",
    },
    Rule {
        prefix: "https://files.minecraftforge.net/maven/",
        target: "/maven/",
    },
    Rule {
        prefix: "https://maven.neoforged.net/releases/",
        target: "/maven/",
    },
    Rule {
        prefix: "https://maven.neoforged.net/",
        target: "/maven/",
    },
    Rule {
        prefix: "https://maven.minecraftforge.net/",
        target: "/maven/",
    },
    Rule {
        prefix: "https://maven.quiltmc.org/repository/release/",
        target: "/maven/",
    },
    Rule {
        prefix: "https://maven.fabricmc.net/",
        target: "/maven/",
    },
    Rule {
        prefix: "https://libraries.minecraft.net/",
        target: "/maven/",
    },
    Rule {
        prefix: "https://repo1.maven.org/maven2/",
        target: "/maven/",
    },
    Rule {
        prefix: "https://resources.download.minecraft.net/",
        target: "/assets/",
    },
    Rule {
        prefix: "https://meta.fabricmc.net/",
        target: "/fabric-meta/",
    },
];

/// 把单个官方地址改写到镜像；无法识别或官方源时原样返回。
/// 无状态，可在下载线程中安全复用。
pub fn map(base: &str, url: &str) -> String {
    if base.is_empty() || url.is_empty() {
        return url.to_string();
    }
    for r in RULES {
        if let Some(rest) = url.strip_prefix(r.prefix) {
            return format!("{}{}{}", base, r.target, rest);
        }
    }
    url.to_string()
}

/// 按当前设置改写地址
pub fn rewrite(state: &AppState, url: &str) -> String {
    map(&base_url(state), url)
}

/// 版本清单地址
pub fn manifest_url(state: &AppState) -> String {
    let base = base_url(state);
    if base.is_empty() {
        OFFICIAL_MANIFEST.to_string()
    } else {
        format!("{base}/mc/game/version_manifest_v2.json")
    }
}

/// 版本 JSON：镜像源用 `/version/{id}/json`，官方源沿用版本清单里的地址
pub fn version_json_url(state: &AppState, official_url: &str, version_id: &str) -> String {
    let base = base_url(state);
    if base.is_empty() {
        official_url.to_string()
    } else {
        format!("{base}/version/{version_id}/json")
    }
}

/// 客户端 jar：镜像源用 `/version/{id}/client`，官方源沿用版本 JSON 里的地址
pub fn client_jar_url(state: &AppState, official_url: &str, version_id: &str) -> String {
    let base = base_url(state);
    if base.is_empty() {
        official_url.to_string()
    } else {
        format!("{base}/version/{version_id}/client")
    }
}

/// 服务端 jar：镜像源用 `/version/{id}/server`，官方源沿用版本 JSON 里的地址
pub fn server_jar_url(state: &AppState, official_url: &str, version_id: &str) -> String {
    let base = base_url(state);
    if base.is_empty() {
        official_url.to_string()
    } else {
        format!("{base}/version/{version_id}/server")
    }
}

/// 资源文件（assets object）
pub fn asset_url(state: &AppState, hash: &str) -> String {
    let head = hash.get(0..2).unwrap_or(hash);
    let base = base_url(state);
    if base.is_empty() {
        format!("https://resources.download.minecraft.net/{head}/{hash}")
    } else {
        format!("{base}/assets/{head}/{hash}")
    }
}
