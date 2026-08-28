use crate::download::{download_many, DownloadItem};
use crate::mcmeta;
use crate::models::*;
use crate::state::AppState;
use crate::util::{extract_zip, rules_allow, sort_mc_versions};
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::Emitter;

/// Entry point: install (or repair) the game files for an instance.
pub async fn install_game(
    app: tauri::AppHandle,
    state: &AppState,
    instance: &Instance,
) -> Result<InstallPlan, String> {
    let task_id = state.next_task_id();
    let source = format!(
        "游戏安装 {} {}",
        instance.mc_version,
        if instance.loader == LoaderType::Vanilla { "" } else { instance.loader.as_str() }
    );
    emit_progress(&app, task_id, "manifest", "获取 Minecraft 版本信息…", 0, 0, instance, &source);

    let existing_json = crate::paths::resolve_version_dir(state, &instance.id).join(format!("{}.json", instance.id));
    let (patched, patched_path) = match std::fs::read_to_string(&existing_json)
        .ok()
        .and_then(|t| serde_json::from_str::<VersionJson>(&t).ok())
    {
        Some(json) => (json, existing_json),
        None => {
            let vanilla = mcmeta::fetch_version_json(state, &instance.mc_version).await?;
            let patched = patch_version(&app, state, &vanilla, instance).await?;
            let patched_path = mcmeta::cache_version_json(state, &patched).await?;
            (patched, patched_path)
        }
    };

    // ---- client jar ----
    emit_progress(&app, task_id, "client", "下载游戏客户端…", 0, 0, instance, &source);
    let mut items: Vec<DownloadItem> = Vec::new();
    let client_jar_path = crate::paths::resolve_version_dir(state, &instance.id).join(format!("{}.jar", instance.id));
    if let Some(client) = &patched.downloads.client {
        items.push(DownloadItem {
            url: client.url.clone(),
            dest: client_jar_path.clone(),
            sha1: Some(client.sha1.clone()),
            sha512: None,
            size: Some(client.size),
            label: format!("{}.jar", instance.id),
        });
    }
    if !items.is_empty() {
        download_many(app.clone(), state, task_id, "client", items).await?;
    }

    // ---- libraries + natives ----
    let features = HashMap::new();
    let mut lib_items: Vec<DownloadItem> = Vec::new();
    let mut native_jars: Vec<(PathBuf, Vec<String>)> = Vec::new(); // (jar path, exclude)
    for lib in &patched.libraries {
        if !rules_allow(lib.rules.as_deref().unwrap_or(&[]), &features) {
            continue;
        }
        if crate::install::is_native_entry(lib) {
            // this library carries native binaries (old style: `natives` field +
            // classifiers map; modern style: separate `...:natives-<os>` entries).
            // NOTE: 4-part names with other classifiers (e.g. `:unsafe`) are
            // regular classpath jars and must NOT land in this branch.
            let name_has_classifier = lib.name.split(':').count() > 3;
            if let Some(classifier) = platform_native_classifier(lib) {
                let dl = lib.downloads.as_ref();
                let file = dl
                    .and_then(|d| d.classifiers.as_ref())
                    .and_then(|c| c.get(&classifier))
                    .cloned()
                    .or_else(|| dl.and_then(|d| d.artifact.clone()));
                if let Some(file) = file {
                    let dest = if name_has_classifier {
                        libraries_path(state, &lib.name, None)
                    } else {
                        libraries_path(state, &lib.name, Some(&classifier))
                    };
                    lib_items.push(DownloadItem {
                        url: file.url.clone(),
                        dest: dest.clone(),
                        sha1: Some(file.sha1.clone()),
                        sha512: None,
                        size: Some(file.size),
                        label: format!("{} ({})", lib.name, classifier),
                    });
                    let exclude = lib.extract.as_ref().map(|e| e.exclude.clone()).unwrap_or_default();
                    native_jars.push((dest, exclude));
                }
            }
            // old-style native library: the main artifact still goes on the classpath
            if !name_has_classifier {
                if let Some(dl) = lib.downloads.as_ref().and_then(|d| d.artifact.as_ref()) {
                    let dest = libraries_path(state, &lib.name, None);
                    lib_items.push(DownloadItem {
                        url: dl.url.clone(),
                        dest,
                        sha1: Some(dl.sha1.clone()),
                        sha512: None,
                        size: Some(dl.size),
                        label: lib.name.clone(),
                    });
                }
            }
            continue;
        }
        if let Some(dl) = lib.downloads.as_ref().and_then(|d| d.artifact.as_ref()) {
            let dest = libraries_path(state, &lib.name, None);
            lib_items.push(DownloadItem {
                url: dl.url.clone(),
                dest,
                sha1: Some(dl.sha1.clone()),
                sha512: None,
                size: Some(dl.size),
                label: lib.name.clone(),
            });
        } else if let Some(url) = &lib.url {
            if let Some(rel) = crate::models::maven_to_path(&lib.name) {
                let dest = crate::paths::libraries_dir(state).join(&rel);
                let file_url = format!("{}/{}", url.trim_end_matches('/'), rel.to_string_lossy().replace('\\', "/"));
                lib_items.push(DownloadItem {
                    url: file_url,
                    dest,
                    sha1: None,
                    sha512: None,
                    size: None,
                    label: lib.name.clone(),
                });
            }
        }
    }
    if !lib_items.is_empty() {
        emit_progress(&app, task_id, "libraries", "下载依赖库…", 0, lib_items.len(), instance, &source);
        download_many(app.clone(), state, task_id, "libraries", lib_items).await?;
    }

    // ---- natives extraction ----
    if !native_jars.is_empty() {
        emit_progress(&app, task_id, "natives", "解压运行库 (natives)…", 0, native_jars.len(), instance, &source);
        let natives_dir = state.instances_dir().join(&instance.id).join("natives");
        std::fs::create_dir_all(&natives_dir).map_err(|e| e.to_string())?;
        // clean old natives (keep dir)
        for entry in std::fs::read_dir(&natives_dir).map_err(|e| e.to_string())? {
            let p = entry.map_err(|e| e.to_string())?.path();
            if p.is_file() {
                let _ = std::fs::remove_file(&p);
            }
        }
        for (i, (jar, exclude)) in native_jars.iter().enumerate() {
            if !jar.exists() {
                continue;
            }
            let mut skip: Vec<&str> = vec!["META-INF/"];
            skip.extend(exclude.iter().map(|s| s.as_str()));
            extract_zip(jar, &natives_dir, &skip)
                .map_err(|e| format!("解压 natives 失败: {e}"))?;
            emit_progress(&app, task_id, "natives", "解压运行库 (natives)…", i + 1, native_jars.len(), instance, &source);
        }
    }

    // ---- assets ----
    if let Some(index) = &patched.asset_index {
        emit_progress(&app, task_id, "assets", "下载资源文件…", 0, 0, instance, &source);
        let index_path = crate::paths::assets_indexes_dir(state).join(format!("{}.json", index.id));
        if !index_path.exists() {
            let item = DownloadItem {
                url: index.url.clone(),
                dest: index_path.clone(),
                sha1: Some(index.sha1.clone()),
                sha512: None,
                size: Some(index.size),
                label: format!("asset index {}", index.id),
            };
            download_many(app.clone(), state, task_id, "assets", vec![item]).await?;
        }
        let index_text = std::fs::read_to_string(&index_path).map_err(|e| e.to_string())?;
        let asset_index: AssetIndexFile = serde_json::from_str(&index_text).map_err(|e| e.to_string())?;
        let mut asset_items: Vec<DownloadItem> = Vec::new();
        for (name, obj) in &asset_index.objects {
            let dest = crate::paths::assets_objects_dir(state)
                .join(&obj.hash[0..2])
                .join(&obj.hash);
            if dest.exists() && std::fs::metadata(&dest).map(|m| m.len() == obj.size).unwrap_or(false) {
                continue;
            }
            asset_items.push(DownloadItem {
                url: format!(
                    "https://resources.download.minecraft.net/{}/{}",
                    &obj.hash[0..2],
                    &obj.hash
                ),
                dest,
                sha1: Some(obj.hash.clone()),
                sha512: None,
                size: Some(obj.size),
                label: name.clone(),
            });
        }
        if !asset_items.is_empty() {
            download_many(app.clone(), state, task_id, "assets", asset_items).await?;
        }
    }

    // ---- logging config ----
    if let Some(logging) = &patched.logging {
        if let Some(client) = &logging.client {
            emit_progress(&app, task_id, "logging", "下载日志配置…", 0, 0, instance, &source);
            let dest = crate::paths::resolve_version_dir(state, &instance.id).join("log4j2.xml");
            if !dest.exists() {
                let item = DownloadItem {
                    url: client.file.url.clone(),
                    dest,
                    sha1: Some(client.file.sha1.clone()),
                    sha512: None,
                    size: Some(client.file.size),
                    label: "log4j2.xml".into(),
                };
                download_many(app.clone(), state, task_id, "logging", vec![item]).await?;
            }
        }
    }

    let total = state
        .instances_dir()
        .join(&instance.id)
        .read_dir()
        .map(|d| d.count())
        .unwrap_or(0);
    // create standard game folders so the instance detail tabs show up
    for sub in ["mods", "shaderpacks", "resourcepacks", "saves", "screenshots", "config"] {
        let _ = std::fs::create_dir_all(state.instances_dir().join(&instance.id).join(sub));
    }
    let plan = InstallPlan {
        instance_id: instance.id.clone(),
        total_bytes: 0,
        file_count: total.max(1),
        symlink_fallback: false,
    };
    let _ = patched_path;
    emit_progress(&app, task_id, "done", "安装完成", 1, 1, instance, &source);
    Ok(plan)
}

fn os_native() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "osx",
        "linux" => "linux",
        _ => "unknown",
    }
}

/// Which natives classifier this platform needs for the given library
/// (old style: from the `natives` map; modern style: from a
/// `...:natives-<os>` name segment).
pub fn platform_native_classifier(lib: &Library) -> Option<String> {
    let os = os_native();
    let arch = std::env::consts::ARCH;
    if let Some(natives) = &lib.natives {
        return natives.get(os).cloned();
    }
    let seg = lib.name.split(':').nth(3)?;
    if !seg.starts_with("natives-") {
        return None;
    }
    let bases: Vec<String> = if os == "osx" {
        vec!["natives-osx".to_string(), "natives-macos".to_string()]
    } else {
        vec![format!("natives-{os}")]
    };
    for b in &bases {
        if seg == b {
            return Some(seg.to_string());
        }
    }
    if arch != "x86_64" {
        for b in &bases {
            let candidate = format!("{b}-{arch}");
            if seg == candidate {
                return Some(seg.to_string());
            }
        }
    }
    None
}

/// True when the library carries platform natives (either style).
pub fn is_native_entry(lib: &Library) -> bool {
    if lib.natives.is_some() {
        return true;
    }
    lib.name
        .split(':')
        .nth(3)
        .map(|s| s.starts_with("natives-"))
        .unwrap_or(false)
}

/// Local library path under `libraries/` with optional classifier suffix.
fn libraries_path(state: &AppState, name: &str, classifier: Option<&str>) -> PathBuf {
    if let Some(rel) = crate::models::maven_to_path(name) {
        let mut p = rel;
        if let Some(c) = classifier {
            // replace `-version.jar` with `-version-classifier.jar`
            let stem = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
            let ext = p.extension().unwrap_or_default().to_string_lossy().to_string();
            if let Some(dash) = stem.rfind('-') {
                let base = &stem[..dash];
                p.set_file_name(format!("{base}-{c}.{ext}"));
            }
        }
        crate::paths::libraries_dir(state).join(p)
    } else {
        crate::paths::libraries_dir(state).join(name.replace(':', "-"))
    }
}

// ---------------------------------------------------------------------------
// Loader patching
// ---------------------------------------------------------------------------

/// Produce the final version JSON used to launch this instance.
pub(crate) async fn patch_version(
    app: &tauri::AppHandle,
    state: &AppState,
    vanilla: &VersionJson,
    instance: &Instance,
) -> Result<VersionJson, String> {
    let mut patched = vanilla.clone();
    match instance.loader {
        LoaderType::Vanilla => {}
        LoaderType::Fabric => {
            patched = fabric_patch(state, vanilla, instance).await?;
        }
        LoaderType::Quilt => {
            patched = quilt_patch(state, vanilla, instance).await?;
        }
        LoaderType::Forge => {
            patched = forge_patch(app, state, vanilla, instance, false).await?;
        }
        LoaderType::NeoForge => {
            patched = forge_patch(app, state, vanilla, instance, true).await?;
        }
    }
    patched.id = instance.id.clone();
    normalize_natives_args(&mut patched);
    Ok(patched)
}

/// Modern versions point the natives-related JVM args at subdirectories
/// (`${natives_directory}/java` etc.). Point them all at the natives dir
/// itself — like PCL does — so java.library.path actually contains the DLLs
/// and LWJGL's classpath extraction lands in the same place.
pub(crate) fn normalize_natives_args(json: &mut VersionJson) {
    let Some(args) = &mut json.arguments else { return };
    let Some(jvm) = &mut args.jvm else { return };
    for av in jvm.iter_mut() {
        let targets: Vec<&mut String> = match av {
            ArgumentValue::Str(s) => vec![s],
            ArgumentValue::Rule(r) => match &mut r.value {
                ArgumentValueInner::Str(s) => vec![s],
                ArgumentValueInner::List(l) => l.iter_mut().collect(),
            },
        };
        for s in targets {
            for (prop, sub) in [
                ("java.library.path", "java"),
                ("jna.tmpdir", "jna"),
                ("org.lwjgl.system.SharedLibraryExtractPath", "lwjgl"),
                ("io.netty.native.workdir", "netty"),
            ] {
                let prefix = format!("-D{prop}=${{natives_directory}}/");
                if let Some(rest) = s.strip_prefix(&prefix) {
                    if rest == sub {
                        *s = format!("-D{prop}=${{natives_directory}}");
                    }
                }
            }
        }
    }
}

pub(crate) async fn fabric_patch(
    state: &AppState,
    vanilla: &VersionJson,
    instance: &Instance,
) -> Result<VersionJson, String> {
    let loader_ver = match &instance.loader_version {
        Some(v) if !v.is_empty() => v.clone(),
        Some(_) | None => {
            latest_stable_loader(state, "https://meta.fabricmc.net/v2/versions/loader", &instance.mc_version).await?
        }
    };
    let url = format!(
        "https://meta.fabricmc.net/v2/versions/loader/{}/{}",
        instance.mc_version, loader_ver
    );
    let meta: LoaderMetaEntry = crate::download::get_json(&state.client, &url).await?;
    let mut patched = vanilla.clone();
    patched.main_class = Some(meta.launcher_meta.main_class.clone().unwrap_or_else(|| "net.fabricmc.loader.impl.launch.knot.KnotClient".into()));
    let mut libs: Vec<Library> = Vec::new();
    if let Some(lm) = &meta.launcher_meta.libraries {
        for (_, list) in lm {
            for ml in list {
                libs.push(meta_library_to_library(ml));
            }
        }
    }
    // loader + intermediary must come first
    let mut ordered = vec![
        Library {
            name: format!("net.fabricmc:fabric-loader:{}", meta.loader.version),
            url: Some("https://maven.fabricmc.net/".into()),
            downloads: None,
            rules: None,
            natives: None,
            extract: None,
        },
        Library {
            name: format!("net.fabricmc:intermediary:{}", meta.intermediary.version),
            url: Some("https://maven.fabricmc.net/".into()),
            downloads: None,
            rules: None,
            natives: None,
            extract: None,
        },
    ];
    ordered.extend(libs);
    ordered.extend(vanilla.libraries.clone());
    dedupe_libraries(&mut ordered);
    patched.libraries = ordered;
    Ok(patched)
}

/// Remove duplicate libraries by maven name (keep first occurrence).
fn dedupe_libraries(libs: &mut Vec<Library>) {
    let mut seen = std::collections::HashSet::new();
    libs.retain(|l| seen.insert(l.name.clone()));
}

async fn quilt_patch(
    state: &AppState,
    vanilla: &VersionJson,
    instance: &Instance,
) -> Result<VersionJson, String> {
    let loader_ver = match &instance.loader_version {
        Some(v) if !v.is_empty() => v.clone(),
        Some(_) | None => {
            latest_stable_loader(state, "https://meta.quiltmc.org/v3/versions/loader", &instance.mc_version).await?
        }
    };
    let url = format!(
        "https://meta.quiltmc.org/v3/versions/loader/{}/{}",
        instance.mc_version, loader_ver
    );
    let meta: LoaderMetaEntry = crate::download::get_json(&state.client, &url).await?;
    let mut patched = vanilla.clone();
    patched.main_class = Some(meta.launcher_meta.main_class.clone().unwrap_or_else(|| "org.quiltmc.loader.impl.launch.knot.KnotClient".into()));
    let mut libs: Vec<Library> = Vec::new();
    if let Some(lm) = &meta.launcher_meta.libraries {
        for (_, list) in lm {
            for ml in list {
                libs.push(meta_library_to_library(ml));
            }
        }
    }
    let mut ordered = vec![Library {
        name: format!("org.quiltmc:quilt-loader:{}", meta.loader.version),
        url: Some("https://maven.quiltmc.org/repository/release/".into()),
        downloads: None,
        rules: None,
        natives: None,
        extract: None,
    }];
    ordered.extend(libs);
    ordered.extend(vanilla.libraries.clone());
    dedupe_libraries(&mut ordered);
    patched.libraries = ordered;
    Ok(patched)
}

/// Forge / NeoForge: download the installer jar, extract `install_profile.json`,
/// use its `versionInfo` as the patched version JSON.
async fn forge_patch(
    app: &tauri::AppHandle,
    state: &AppState,
    vanilla: &VersionJson,
    instance: &Instance,
    is_neoforge: bool,
) -> Result<VersionJson, String> {
    let version = instance
        .loader_version
        .clone()
        .ok_or("缺少加载器版本")?;

    let (base_url, artifact, installer_name) = if is_neoforge {
        (
            "https://maven.neoforged.net/releases/",
            "net/neoforged/neoforge",
            "neoforge",
        )
    } else {
        (
            "https://maven.minecraftforge.net/",
            "net/minecraftforge/forge",
            "forge",
        )
    };
    let full_ver = if is_neoforge {
        version.clone()
    } else {
        format!("{}-{}", instance.mc_version, version)
    };
    let installer_url = format!("{base_url}{artifact}/{full_ver}/{installer_name}-{full_ver}-installer.jar");
    let installer_path = state.root.join("runtimes").join(format!("{installer_name}-{full_ver}-installer.jar"));
    std::fs::create_dir_all(installer_path.parent().unwrap()).map_err(|e| e.to_string())?;

    if !installer_path.exists() {
        let item = DownloadItem {
            url: installer_url.clone(),
            dest: installer_path.clone(),
            sha1: None,
            sha512: None,
            size: None,
            label: format!("{installer_name} 安装器"),
        };
        let task_id = state.next_task_id();
        let source = format!("加载器：{installer_name} {full_ver}");
        emit_progress(
            app,
            task_id,
            "loader",
            &format!("正在下载 {installer_name} {full_ver} 安装器…"),
            0,
            1,
            instance,
            &source,
        );
        download_many(app.clone(), state, task_id, "loader", vec![item]).await?;
    }

    let profile_bytes = crate::util::read_zip_entry(&installer_path, "install_profile.json")
        .map_err(|e| format!("读取 install_profile.json 失败: {e}"))?;
    let profile_text = String::from_utf8_lossy(&profile_bytes).to_string();
    // substitute common placeholders
    let replaced = profile_text
        .replace("${version}", &full_ver)
        .replace("${mcVersion}", &instance.mc_version)
        .replace("${forgeVersion}", &version)
        .replace("${neoForgeVersion}", &version);
    let profile: serde_json::Value = serde_json::from_str(&replaced).map_err(|e| e.to_string())?;
    let version_info = profile
        .get("versionInfo")
        .ok_or("install_profile.json 中缺少 versionInfo")?;
    let mut patched: VersionJson = serde_json::from_value(version_info.clone())
        .map_err(|e| format!("解析 versionInfo 失败: {e}"))?;

    // ensure essential vanilla pieces exist
    if patched.asset_index.is_none() {
        patched.asset_index = vanilla.asset_index.clone();
    }
    if patched.downloads.client.is_none() {
        patched.downloads.client = vanilla.downloads.client.clone();
    }
    if patched.java_version.is_none() {
        patched.java_version = vanilla.java_version.clone();
    }
    if patched.logging.is_none() {
        patched.logging = vanilla.logging.clone();
    }
    Ok(patched)
}

fn meta_library_to_library(ml: &MetaLibrary) -> Library {
    Library {
        name: ml.name.clone(),
        url: ml.url.clone(),
        downloads: None,
        rules: None,
        natives: None,
        extract: None,
    }
}

async fn latest_stable_loader(
    state: &AppState,
    base: &str,
    mc_version: &str,
) -> Result<String, String> {
    let url = format!("{base}/{mc_version}");
    let list: Vec<serde_json::Value> = crate::download::get_json(&state.client, &url).await?;
    for entry in &list {
        let stable = entry
            .get("loader")
            .and_then(|l| l.get("stable"))
            .and_then(|s| s.as_bool())
            .unwrap_or(false);
        if stable {
            if let Some(v) = entry.get("loader").and_then(|l| l.get("version")).and_then(|v| v.as_str()) {
                return Ok(v.to_string());
            }
        }
    }
    if let Some(v) = list
        .first()
        .and_then(|e| e.get("loader"))
        .and_then(|l| l.get("version"))
        .and_then(|v| v.as_str())
    {
        return Ok(v.to_string());
    }
    Err("没有可用的加载器版本".into())
}

/// Fetch available loader versions for a Minecraft version.
pub async fn loader_versions(
    state: &AppState,
    loader: LoaderType,
    mc_version: &str,
) -> Result<Vec<String>, String> {
    match loader {
        LoaderType::Fabric | LoaderType::Quilt => {
            let base = match loader {
                LoaderType::Fabric => "https://meta.fabricmc.net/v2/versions/loader",
                _ => "https://meta.quiltmc.org/v3/versions/loader",
            };
            let url = format!("{base}/{mc_version}");
            let list: Vec<serde_json::Value> = crate::download::get_json(&state.client, &url).await?;
            let versions: Vec<String> = list
                .into_iter()
                .filter_map(|e| e.get("loader")?.get("version")?.as_str().map(|s| s.to_string()))
                .collect();
            Ok(crate::util::sort_version_desc(versions))
        }
        LoaderType::Forge | LoaderType::NeoForge => {
            let (url, prefix) = match loader {
                LoaderType::Forge => (
                    "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml",
                    format!("{}-", mc_version),
                ),
                _ => (
                    "https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml",
                    {
                        let stripped = mc_version.strip_prefix("1.").unwrap_or(mc_version);
                        format!("{stripped}.")
                    },
                ),
            };
            let xml = crate::download::get_text(&state.client, url).await?;
            let versions = crate::util::parse_maven_versions(&xml);
            if loader == LoaderType::Forge {
                Ok(sort_mc_versions(versions, &prefix))
            } else {
                // neoforge: strip the mc prefix from the version string, newest first
                let stripped: Vec<String> = versions
                    .into_iter()
                    .filter(|v| v.starts_with(&prefix))
                    .map(|v| v[prefix.len()..].to_string())
                    .collect();
                Ok(crate::util::sort_version_desc(stripped))
            }
        }
        LoaderType::Vanilla => Ok(vec![]),
    }
}

/// Emit an install task progress event carrying instance + source context.
pub fn emit_progress(
    app: &tauri::AppHandle,
    task_id: u64,
    stage: &str,
    message: &str,
    done: usize,
    total: usize,
    instance: &Instance,
    source: &str,
) {
    let _ = app.emit(
        "install://progress",
        serde_json::json!({
            "taskId": task_id,
            "stage": stage,
            "message": message,
            "done": done,
            "total": total,
            "instanceId": instance.id,
            "instanceName": instance.name,
            "source": source,
        }),
    );
}
