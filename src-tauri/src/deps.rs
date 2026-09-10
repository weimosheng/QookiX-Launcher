//! 依赖体检：扫描实例 mods 目录，解析每个 jar 声明的依赖，
//! 报告缺失的前置模组与重复的 mod id；缺失项可再联网解析成可安装的项目。

use crate::state::AppState;
use crate::util::{self, ModJarMeta};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

/// 由加载器 / 游戏运行时本身提供、无需用户安装的 mod id。
const ENV_MOD_IDS: &[&str] = &[
    "minecraft",
    "java",
    "fabricloader",
    "fabric-loader",
    "fabric_loader",
    "forge",
    "neoforge",
    "quilt_loader",
    "quiltloader",
    "quilt_base",
    "openloader",
    "mixinextras",
];

fn is_env_mod(id: &str) -> bool {
    ENV_MOD_IDS.iter().any(|e| e.eq_ignore_ascii_case(id))
}

/// mods 目录下的全部 jar（`.jar` 与被禁用的 `.jar.disabled`）。
/// 返回 (展示文件名, 路径, 是否启用)；被禁用的文件名还原为 `.jar` 结尾。
fn collect_jars(dir: &std::path::Path) -> Vec<(String, PathBuf, bool)> {
    let mut out: Vec<(String, PathBuf, bool)> = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else { continue };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Some(base) = name.strip_suffix(".jar.disabled") {
            out.push((base.to_string() + ".jar", path, false));
        } else if name.ends_with(".jar") {
            out.push((name, path, true));
        }
    }
    out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    out
}

/// 对实例的 mods 做依赖体检。
///
/// 返回：
/// - `missing`：缺失的必选前置（按 mod id 聚合，含被谁依赖、以及是否存在同名但被禁用的文件）
/// - `duplicates`：同一 mod id 被多个启用中的文件提供
pub fn check(state: &AppState, instance_id: &str) -> Result<Value, String> {
    let instance = crate::instances::get_instance(state, instance_id)?;
    let dir = state.instances_dir().join(&instance.id).join("mods");
    let jars = collect_jars(&dir);

    // mod id（小写） -> (启用中的文件, 禁用中的文件)
    let mut provided: HashMap<String, (Vec<String>, Vec<String>)> = HashMap::new();
    let mut metas: Vec<(String, bool, ModJarMeta)> = Vec::new();
    let mut unreadable = 0usize;

    for (name, path, enabled) in &jars {
        match util::parse_mod_jar(path) {
            Some(meta) => {
                if let Some(id) = &meta.mod_id {
                    let entry = provided.entry(id.to_lowercase()).or_default();
                    if *enabled {
                        entry.0.push(name.clone());
                    } else {
                        entry.1.push(name.clone());
                    }
                }
                metas.push((name.clone(), *enabled, meta));
            }
            None => unreadable += 1,
        }
    }

    // 缺失依赖：按 mod id 聚合；BTreeMap 保证输出顺序稳定。
    let mut missing: BTreeMap<String, Value> = BTreeMap::new();
    for (filename, enabled, meta) in &metas {
        if !enabled {
            // 被禁用的 mod 自身不生效，其依赖声明不参与校验
            continue;
        }
        for dep in &meta.depends {
            if dep.kind != "required" || is_env_mod(&dep.mod_id) {
                continue;
            }
            let key = dep.mod_id.to_lowercase();
            if provided
                .get(&key)
                .map(|(enabled_files, _)| !enabled_files.is_empty())
                .unwrap_or(false)
            {
                continue; // 已有启用中的实现
            }
            let entry = missing.entry(key.clone()).or_insert_with(|| {
                json!({
                    "modId": dep.mod_id,
                    "requirement": dep.requirement,
                    "kind": dep.kind,
                    "requiredBy": [],
                    "disabledFile": Value::Null,
                })
            });
            if let Some(arr) = entry.get_mut("requiredBy").and_then(|v| v.as_array_mut()) {
                if !arr.iter().any(|v| v.as_str() == Some(filename.as_str())) {
                    arr.push(json!(filename));
                }
            }
            if entry
                .get("disabledFile")
                .map(|v| v.is_null())
                .unwrap_or(false)
            {
                if let Some((_, disabled)) = provided.get(&key) {
                    if let Some(f) = disabled.first() {
                        entry["disabledFile"] = json!(f);
                    }
                }
            }
        }
    }

    // 重复 mod id：同一 mod id 被多个「启用中」的文件提供
    let mut duplicates = Vec::new();
    let mut ids: Vec<&String> = provided.keys().collect();
    ids.sort();
    for id in ids {
        let (enabled_files, _) = &provided[id];
        if enabled_files.len() > 1 {
            duplicates.push(json!({ "modId": id, "files": enabled_files }));
        }
    }

    Ok(json!({
        "instanceId": instance.id,
        "checkedMods": metas.iter().filter(|(_, en, _)| *en).count(),
        "unreadable": unreadable,
        "missing": missing.into_values().collect::<Vec<_>>(),
        "duplicates": duplicates,
    }))
}

/// 把缺失的 mod id 在 Modrinth 上解析成可安装的项目。
///
/// 优先取 slug / title 与 mod id 精确一致（忽略大小写与 `-`/`_` 差异）的条目，
/// 否则回退到相关性第一的结果并标记 `exact: false`。
/// `latestVersionId` 直接来自搜索结果，可交给安装接口使用。
pub async fn resolve_missing(
    state: &AppState,
    instance_id: &str,
    mod_ids: Vec<String>,
) -> Result<Vec<Value>, String> {
    let instance = crate::instances::get_instance(state, instance_id)?;
    let loader = if instance.loader == crate::models::LoaderType::Vanilla {
        String::new()
    } else {
        instance.loader.as_str().to_string()
    };
    let mc = instance.mc_version.clone();

    let tasks = mod_ids.into_iter().map(|raw_id| {
        let mc = mc.clone();
        let loader = loader.clone();
        async move {
            let id = raw_id.trim().to_string();
            let mut best: Option<(Value, bool)> = None;
            if let Ok(res) =
                crate::modrinth::search(state, &id, "mod", "", "relevance", 0, 8, &mc, &loader)
                    .await
            {
                let hits = res
                    .get("hits")
                    .and_then(|h| h.as_array())
                    .cloned()
                    .unwrap_or_default();
                let norm =
                    |s: &str| s.to_lowercase().replace(['-', '_'], "");
                let want = norm(&id);
                let exact = hits.iter().position(|h| {
                    let slug = h.get("slug").and_then(|v| v.as_str()).unwrap_or("");
                    let title = h.get("title").and_then(|v| v.as_str()).unwrap_or("");
                    norm(slug) == want || norm(title) == want
                });
                best = match exact {
                    Some(i) => hits.get(i).cloned().map(|h| (h, true)),
                    None => hits.first().cloned().map(|h| (h, false)),
                };
            }
            match best {
                Some((h, exact)) => Some(json!({
                    "modId": id,
                    "provider": "modrinth",
                    "projectId": h.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                    "slug": h.get("slug").and_then(|v| v.as_str()).unwrap_or(""),
                    "title": h.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                    "icon": h.get("icon_url").and_then(|v| v.as_str()).unwrap_or(""),
                    "downloads": h.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0),
                    "latestVersionId": h.get("latest_version").and_then(|v| v.as_str()).unwrap_or(""),
                    "exact": exact,
                })),
                None => Some(json!({
                    "modId": id,
                    "provider": "modrinth",
                    "projectId": "",
                    "exact": false,
                })),
            }
        }
    });

    let results = futures_util::future::join_all(tasks).await;
    Ok(results.into_iter().flatten().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_ids_are_matched_case_insensitively() {
        assert!(is_env_mod("Minecraft"));
        assert!(is_env_mod("FabricLoader"));
        assert!(is_env_mod("quilt_loader"));
        assert!(!is_env_mod("fabric-api"));
        assert!(!is_env_mod("sodium"));
    }

    #[test]
    fn collect_jars_treats_disabled_files_as_base_names() {
        let dir = std::env::temp_dir().join(format!(
            "qkx-deps-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.jar"), b"x").unwrap();
        std::fs::write(dir.join("b.jar.disabled"), b"x").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();

        let jars = collect_jars(&dir);
        assert_eq!(jars.len(), 2);
        assert_eq!(jars[0].0, "a.jar");
        assert!(jars[0].2);
        assert_eq!(jars[1].0, "b.jar");
        assert!(!jars[1].2);
        std::fs::remove_dir_all(&dir).ok();
    }
}
