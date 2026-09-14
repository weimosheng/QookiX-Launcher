//! 内容描述翻译：对接自建翻译服务（trans.zhayi.cc）。
//! 带本地磁盘缓存（`cache/translations.json`，TTL 7 天），命中缓存零配额消耗。

use crate::state::AppState;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

const BASE: &str = "https://trans.zhayi.cc";
const LANG: &str = "zh";
/// 缓存有效期：7 天
const CACHE_TTL: u64 = 7 * 24 * 3600;
/// 服务端批量接口单次上限
const BATCH: usize = 5;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cache_file(root: &std::path::Path) -> PathBuf {
    root.join("cache").join("translations.json")
}

type CacheMap = BTreeMap<String, Value>;

fn load_cache(root: &std::path::Path) -> CacheMap {
    std::fs::read_to_string(cache_file(root))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_cache(root: &std::path::Path, cache: &CacheMap) {
    let path = cache_file(root);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(s) = serde_json::to_string(cache) {
        let _ = std::fs::write(path, s);
    }
}

fn cache_key(provider: &str, slug: &str) -> String {
    format!("{provider}:{slug}:{LANG}")
}

/// 批量翻译模组描述：先查本地缓存，未命中的按 5 个一组请求服务端。
///
/// 返回 `{ translations: {slug: 中文}, failed: [slug], rateLimited: bool }`；
/// 失败的 slug 不出现在 `translations` 里，前端回退显示原文。
/// 触发 429 时停止本轮剩余请求，避免继续白烧配额。
pub async fn translate_descriptions(
    state: &AppState,
    provider: &str,
    slugs: Vec<String>,
) -> Result<Value, String> {
    // 选择翻译服务：默认（自建服务）或自定义（用户自己的 OpenAI 兼容 AI API）
    let (custom_ready, api_base, api_key, api_model) = {
        let s = state.settings.read().unwrap();
        (
            s.translate_provider == "custom",
            s.translate_api_base.trim().trim_end_matches('/').to_string(),
            s.translate_api_key.clone().unwrap_or_default(),
            s.translate_api_model.trim().to_string(),
        )
    };
    let use_custom = custom_ready && !api_base.is_empty() && !api_key.is_empty() && !api_model.is_empty();
    // 内置服务已支持 CurseForge 资源，两套服务覆盖相同的平台范围
    // 两套服务的翻译结果质量不同，缓存分开存放，来回切换互不覆盖
    let service_tag = if use_custom { "custom" } else { provider };
    let mut cache = load_cache(&state.root);
    let now = now_secs();
    let mut translations = BTreeMap::new();
    let mut failed: Vec<String> = Vec::new();
    let mut queue: Vec<String> = Vec::new();

    for slug in slugs {
        let key = cache_key(provider, &slug);
        let hit = cache.get(&key).and_then(|e| {
            let ts = e.get("ts").and_then(|v| v.as_u64()).unwrap_or(0);
            let text = e.get("text").and_then(|v| v.as_str()).unwrap_or("");
            (now.saturating_sub(ts) < CACHE_TTL && !text.is_empty()).then(|| text.to_string())
        });
        match hit {
            Some(text) => {
                translations.insert(slug, json!(text));
            }
            None => queue.push(slug),
        }
    }

    let mut rate_limited = false;
    // ---- 自定义服务：先从 Modrinth 取英文原文，再逐条交给用户的 AI 翻译 ----
    if use_custom {
        while !queue.is_empty() {
            let slug = queue.remove(0);
            // 两个平台的 project_info 都返回统一的 description 字段
            // （Modrinth 用 project id/slug 查询，CurseForge 用数字 mod id）
            let info = match provider {
                "curseforge" => crate::curseforge::project_info(state, &slug).await,
                _ => crate::modrinth::project_info(state, &slug).await,
            };
            let desc = info
                .ok()
                .and_then(|p| {
                    p.get("description")
                        .and_then(|d| d.as_str())
                        .map(|s| s.trim().to_string())
                })
                .filter(|d| !d.is_empty());
            let Some(desc) = desc else {
                failed.push(slug);
                continue;
            };
            match chat_translate(state, &api_base, &api_key, &api_model, &desc).await {
                Ok(text) => {
                    cache.insert(cache_key(service_tag, &slug), json!({ "text": text, "ts": now }));
                    translations.insert(slug, json!(text));
                }
                Err(e) => {
                    // 鉴权失败 / 限流时继续请求没有意义，剩余全部记为失败
                    let fatal = e.contains("鉴权失败") || e.contains("429");
                    failed.push(slug);
                    if fatal {
                        failed.extend(queue.drain(..));
                        break;
                    }
                }
            }
        }
        save_cache(&state.root, &cache);
        return Ok(json!({
            "translations": translations,
            "failed": failed,
            "rateLimited": rate_limited,
        }));
    }
    // ---- 默认服务：按 5 个一组请求自建接口 ----
    while !queue.is_empty() {
        let group: Vec<String> = queue.drain(..queue.len().min(BATCH)).collect();
        let body = json!({ "platform": provider, "lang": LANG, "mod_ids": group });
        let resp = state
            .client
            .post(format!("{BASE}/translate/mods"))
            .json(&body)
            .send()
            .await;

        let parsed = match resp {
            Ok(r) if r.status().as_u16() == 200 => r.json::<Value>().await.ok(),
            Ok(r) if r.status().as_u16() == 429 => {
                rate_limited = true;
                None
            }
            _ => None,
        };
        let Some(body) = parsed else {
            // 限流 / 服务不可达 / 响应异常：本组与剩余全部记为失败，停止请求
            failed.extend(group);
            failed.extend(queue.drain(..));
            break;
        };
        let results = body
            .get("results")
            .and_then(|r| r.as_object())
            .cloned()
            .unwrap_or_default();
        for slug in group {
            let text = results
                .get(&slug)
                .and_then(|e| e.get("text"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            if text.is_empty() {
                failed.push(slug);
                continue;
            }
            cache.insert(cache_key(service_tag, &slug), json!({ "text": text, "ts": now }));
            translations.insert(slug, json!(text));
        }
    }

    save_cache(&state.root, &cache);
    Ok(json!({
        "translations": translations,
        "failed": failed,
        "rateLimited": rate_limited,
    }))
}

/// 翻译资源正文（body）：详情弹窗右侧展示，译文优先、原文兜底。
///
/// - 内置服务与自定义 AI 均支持两平台（CurseForge 正文为 HTML）
/// - 返回 `{ body: 译文|null, bodyCached, original, supported, error? }`
pub async fn translate_body(
    state: &AppState,
    provider: &str,
    slug: &str,
    translate: bool,
) -> Result<Value, String> {
    // 拉原文（无译文时的兜底显示内容）
    let original = match provider {
        "curseforge" => crate::curseforge::project_description(state, slug).await,
        _ => {
            let info = crate::modrinth::project_info(state, slug).await?;
            Ok(info
                .get("body")
                .and_then(|b| b.as_str())
                .unwrap_or("")
                .to_string())
        }
    }
    .unwrap_or_default();

    let (custom_ready, api_base, api_key, api_model) = {
        let s = state.settings.read().unwrap();
        (
            s.translate_provider == "custom",
            s.translate_api_base.trim().trim_end_matches('/').to_string(),
            s.translate_api_key.clone().unwrap_or_default(),
            s.translate_api_model.trim().to_string(),
        )
    };
    let use_custom = custom_ready && !api_base.is_empty() && !api_key.is_empty() && !api_model.is_empty();
    let service_tag = if use_custom { "custom" } else { provider };
    // 正文缓存加格式版本：服务端 2026-09-14 起 CF 正文改为 Markdown，图片内联在原文位置。
    // 旧缓存里的译文没有图片，会一直被 withImages 兜底堆到文末，必须让它失效重取。
    let key = format!("{provider}:{slug}:{LANG}:body:v2:{service_tag}");
    let mut cache = load_cache(&state.root);
    let now = now_secs();
    // 缓存优先：手动翻译过的正文再次打开详情时直接展示译文（translate=false 也命中）。
    // 用户不想要译文时点「显示原文」切换即可；清空缓存可彻底回到未翻译态。
    if let Some(hit) = cache.get(&key).and_then(|e| {
        let ts = e.get("ts").and_then(|v| v.as_u64()).unwrap_or(0);
        let text = e.get("text").and_then(|v| v.as_str()).unwrap_or("");
        (now.saturating_sub(ts) < CACHE_TTL && !text.is_empty()).then(|| text.to_string())
    }) {
        return Ok(json!({ "body": hit, "bodyCached": true, "original": original, "supported": true }));
    }
    if original.trim().is_empty() {
        return Ok(json!({ "body": null, "bodyCached": false, "original": original, "supported": false }));
    }
    // translate=false 且无缓存：只返回原文，不调翻译服务
    if !translate {
        return Ok(json!({ "body": null, "bodyCached": false, "original": original, "supported": true }));
    }
    // 正文超长时截断（与服务端 maxBodyTranslateChars 对齐），避免请求被拒
    const MAX_BODY_CHARS: usize = 12000;
    let clipped: String = original.chars().take(MAX_BODY_CHARS).collect();

    // 自定义 AI：两平台通用
    if use_custom {
        return match chat_translate(state, &api_base, &api_key, &api_model, &clipped).await {
            Ok(text) => {
                cache.insert(key, json!({ "text": text, "ts": now }));
                save_cache(&state.root, &cache);
                Ok(json!({ "body": text, "bodyCached": false, "original": original, "supported": true }))
            }
            Err(e) => Ok(json!({ "body": null, "bodyCached": false, "original": original, "supported": true, "error": e })),
        };
    }
    // 内置服务已支持 CurseForge 资源，platform 原样传给服务端
    // 正文翻译同步耗时 10~60s+，用独立长超时 client（全局 client 是 60s 会掐断）
    let slow = reqwest::Client::builder()
        .user_agent(concat!("QookiX-Launcher/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(150))
        .build()
        .map_err(|e| e.to_string())?;
    let req = json!({ "platform": provider, "lang": LANG, "mod_id": slug, "include_body": true });
    let resp = slow
        .post(format!("{BASE}/translate/mod"))
        .json(&req)
        .send()
        .await
        .map_err(|e| format!("请求翻译服务失败: {e}"))?;
    let status = resp.status().as_u16();
    // 400 等错误回包可能是纯文本，先按文本读再尝试 JSON 解析
    let text = resp.text().await.map_err(|e| e.to_string())?;
    let parsed: Value = serde_json::from_str(&text)
        .unwrap_or_else(|_| json!({ "error": text.trim().chars().take(200).collect::<String>() }));
    if status != 200 {
        let msg = parsed.get("error").and_then(|v| v.as_str()).unwrap_or("翻译失败");
        return Err(format!("翻译服务返回 HTTP {status}: {msg}"));
    }
    match parsed
        .get("body")
        .and_then(|b| b.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        Some(text) => {
            cache.insert(key, json!({ "text": text, "ts": now }));
            save_cache(&state.root, &cache);
            Ok(json!({
                "body": text,
                "bodyCached": parsed.get("body_cached").and_then(|v| v.as_bool()).unwrap_or(false),
                "original": original,
                "supported": true,
            }))
        }
        None => {
            let err = parsed
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("服务未返回正文译文")
                .to_string();
            Ok(json!({ "body": null, "bodyCached": false, "original": original, "supported": true, "error": err }))
        }
    }
}

/// 反馈某条翻译已过期。返回服务端 status；`updated` 时清掉本地缓存，
/// 下次请求即可拿到重新翻译的结果。
pub async fn report_stale(state: &AppState, provider: &str, slug: &str) -> Result<String, String> {
    let body = json!({ "platform": provider, "mod_id": slug, "lang": LANG });
    let resp = state
        .client
        .post(format!("{BASE}/feedback/stale"))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求失败: {e}"))?;
    let v: Value = resp.json().await.map_err(|e| format!("解析响应失败: {e}"))?;
    let status = v
        .get("status")
        .and_then(|s| s.as_str())
        .unwrap_or("received")
        .to_string();
    if status == "updated" {
        let mut cache = load_cache(&state.root);
        cache.remove(&cache_key(provider, slug));
        save_cache(&state.root, &cache);
    }
    Ok(status)
}

/// 反馈翻译质量问题（issue_type + 可选的建议翻译与补充说明）。
/// 服务端限流 5 次/天/IP，超限静默返回 received，客户端无需特殊处理。
pub async fn report_quality(
    state: &AppState,
    provider: &str,
    slug: &str,
    issue_type: &str,
    suggestion: Option<String>,
    comment: Option<String>,
) -> Result<String, String> {
    match issue_type {
        "wrong_translation" | "unnatural" | "missing" | "other" => {}
        _ => return Err("未知的问题类型".into()),
    }
    let mut body = json!({
        "platform": provider,
        "mod_id": slug,
        "lang": LANG,
        "issue_type": issue_type,
    });
    if let Some(s) = suggestion.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        body["user_suggestion"] = json!(s);
    }
    if let Some(c) = comment.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        body["user_comment"] = json!(c);
    }
    let resp = state
        .client
        .post(format!("{BASE}/feedback/quality"))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求失败: {e}"))?;
    let v: Value = resp.json().await.map_err(|e| format!("解析响应失败: {e}"))?;
    Ok(v.get("status")
        .and_then(|s| s.as_str())
        .unwrap_or("received")
        .to_string())
}

/// 清空翻译缓存，返回释放的字节数。
///
/// `service`：
/// - `Some("custom")`  只清自定义 AI 服务的翻译
/// - `Some("modrinth")` 只清内置服务的翻译
/// - `None` 全部清空（直接删除缓存文件）
pub fn clear_cache(state: &AppState, service: Option<&str>) -> Result<u64, String> {
    let path = cache_file(&state.root);
    if !path.exists() {
        return Ok(0);
    }
    let Some(tag) = service else {
        let freed = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        std::fs::remove_file(&path).map_err(|e| format!("清理翻译缓存失败: {e}"))?;
        return Ok(freed);
    };
    // 前端传 "default" 表示内置服务，缓存 key 用的是 provider（"modrinth"）开头
    let tag = if tag == "default" { "modrinth" } else { &tag };
    // 按服务标签前缀过滤，另一套服务的缓存原样保留
    let prefix = format!("{tag}:");
    let mut freed = 0u64;
    let mut kept: CacheMap = BTreeMap::new();
    for (key, value) in load_cache(&state.root) {
        if key.starts_with(&prefix) {
            freed += serde_json::to_string(&value).map(|s| s.len() as u64).unwrap_or(0);
        } else {
            kept.insert(key, value);
        }
    }
    save_cache(&state.root, &kept);
    Ok(freed)
}

/// 调用用户的 OpenAI 兼容接口翻译一段文本。
pub async fn chat_translate(
    state: &AppState,
    base: &str,
    key: &str,
    model: &str,
    text: &str,
) -> Result<String, String> {
    let url = format!("{}/chat/completions", base.trim_end_matches('/'));
    let body = json!({
        "model": model,
        "temperature": 0.2,
        "messages": [
            {
                "role": "system",
                "content": "你是 Minecraft 模组描述的翻译器。把用户提供的英文模组描述准确翻译成简体中文：保留模组名、专有名词与技术术语；只输出译文本身，不要任何解释、引号或前后缀。"
            },
            { "role": "user", "content": text }
        ],
    });
    let resp = state
        .client
        .post(url)
        .bearer_auth(key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求失败: {e}"))?;
    let status = resp.status().as_u16();
    if status == 401 || status == 403 {
        return Err(format!("鉴权失败（HTTP {status}），请检查 API Key"));
    }
    if status == 429 {
        return Err("自定义服务限流（429）".into());
    }
    if !(200..300).contains(&status) {
        return Err(format!("自定义翻译服务返回 HTTP {status}"));
    }
    let v: Value = resp.json().await.map_err(|e| format!("解析响应失败: {e}"))?;
    let content = v
        .pointer("/choices/0/message/content")
        .and_then(|c| c.as_str())
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty());
    content.ok_or_else(|| "自定义服务未返回翻译内容".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_includes_provider_and_lang() {
        assert_eq!(cache_key("modrinth", "sodium"), "modrinth:sodium:zh");
        assert_ne!(cache_key("modrinth", "a"), cache_key("curseforge", "a"));
    }

    #[test]
    fn cache_roundtrip_and_expiry_field() {
        let dir = std::env::temp_dir().join(format!(
            "qkx-translate-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let key = cache_key("modrinth", "sodium");
        let mut cache = load_cache(&dir);
        assert!(cache.is_empty());

        cache.insert(key.clone(), json!({ "text": "新翻译", "ts": now_secs() }));
        save_cache(&dir, &cache);

        let loaded = load_cache(&dir);
        let entry = loaded.get(&key).unwrap();
        assert_eq!(entry["text"], "新翻译");
        let ts = entry["ts"].as_u64().unwrap();
        assert!(now_secs().saturating_sub(ts) < CACHE_TTL);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn stale_entry_recognized_by_ttl() {
        let now = now_secs();
        let old = now - CACHE_TTL - 10;
        // 与 translate_descriptions 相同的命中判断逻辑
        let fresh_hit = |ts: u64, text: &str| {
            (now.saturating_sub(ts) < CACHE_TTL && !text.is_empty()).then(|| text.to_string())
        };
        assert!(fresh_hit(now, "新翻译").is_some());
        assert!(fresh_hit(old, "旧翻译").is_none());
        // 空文本不允许命中
        assert!(fresh_hit(now, "").is_none());
    }
}
