use crate::download::{download_many, DownloadItem};
use crate::mcmeta;
use crate::models::*;
use crate::state::AppState;
use crate::util::{extract_zip, rules_allow, sort_mc_versions};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tauri::Emitter;
use tokio::time::timeout;

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
    let result = install_game_inner(&app, state, instance, task_id, &source).await;
    // 每个任务必须以 done 收尾，否则下载中心的卡片永远停在「进行中」
    let ok = result.is_ok();
    emit_progress(
        &app,
        task_id,
        "done",
        if ok { "游戏文件就绪" } else { "游戏安装失败" },
        1,
        1,
        instance,
        &source,
    );
    result
}

async fn install_game_inner(
    app: &tauri::AppHandle,
    state: &AppState,
    instance: &Instance,
    task_id: u64,
    source: &str,
) -> Result<InstallPlan, String> {
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
            url: crate::mirror::client_jar_url(state, &client.url, &instance.mc_version),
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

// ---- Forge 1.17+ 新版安装器：原版 jar 就位后执行 processors 任务链 ----
    if matches!(instance.loader, LoaderType::Forge | LoaderType::NeoForge) {
        let full_ver = if instance.loader == LoaderType::NeoForge {
            instance.loader_version.clone().unwrap_or_default()
        } else {
            format!(
                "{}-{}",
                instance.mc_version,
                instance.loader_version.clone().unwrap_or_default()
            )
        };
        let tool_name = if instance.loader == LoaderType::NeoForge { "neoforge" } else { "forge" };
        let installer_path =
            state.root.join("runtimes").join(format!("{tool_name}-{full_ver}-installer.jar"));
        if installer_path.exists() {
            emit_progress(&app, task_id, "forge-install", "正在安装 Forge…", 0, 0, instance, &source);
            let profile: serde_json::Value = {
                let file = std::fs::File::open(&installer_path).map_err(|e| e.to_string())?;
                let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
                let mut entry = archive
                    .by_name("install_profile.json")
                    .map_err(|e| e.to_string())?;
                let mut buf = Vec::new();
                std::io::Read::read_to_end(&mut entry, &mut buf).map_err(|e| e.to_string())?;
                serde_json::from_slice(&buf).map_err(|e| e.to_string())?
            };
            // Forge processors 失败不应阻止后续 natives 提取——打印警告但继续
            if let Err(e) = run_forge_processors(&app, state, instance, &installer_path, &profile, task_id, &source).await {
                crate::util::log_line(&format!("[install] Forge processors 警告: {e}"));
            }
            emit_progress(&app, task_id, "forge-install", "Forge 安装完成", 1, 1, instance, &source);
        } else {
            emit_progress(&app, task_id, "forge-install", "Forge 安装器未找到，跳过", 1, 1, instance, &source);
        }
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
                let meta = dl
                    .and_then(|d| d.classifiers.as_ref())
                    .and_then(|c| c.get(&classifier))
                    .cloned()
                    .or_else(|| dl.and_then(|d| d.artifact.clone()));
                // 老式版本 json（1.12 前后的 Forge versionInfo 等）没有 downloads
                // 元数据，只有 maven 仓库基址 `url` + 坐标——按 classifier 拼出
                // 真实下载地址，否则 natives 条目被整段丢弃，启动报
                // 「缺少 natives 目录」。
                let has_meta = meta.is_some();
                let file = match meta {
                    Some(f) => f,
                    None => {
                        let base = lib.url.clone().unwrap_or_else(|| "https://libraries.minecraft.net/".into());
                        let cls_name = format!("{}:{}", lib.name, classifier);
                        let Some(rel) = crate::models::maven_to_path(&cls_name) else { continue };
                        DownloadFile {
                            url: format!("{}/{}", base.trim_end_matches('/'), rel.to_string_lossy().replace('\\', "/")),
                            sha1: String::new(),
                            size: 0,
                            path: None,
                        }
                    }
                };
                let dest = if name_has_classifier {
                    libraries_path(state, &lib.name, None)
                } else {
                    libraries_path(state, &lib.name, Some(&classifier))
                };
                lib_items.push(DownloadItem {
                    url: file.url.clone(),
                    dest: dest.clone(),
                    sha1: if has_meta && !file.sha1.is_empty() { Some(file.sha1.clone()) } else { None },
                    sha512: None,
                    size: if has_meta && file.size > 0 { Some(file.size) } else { None },
                    label: format!("{} ({})", lib.name, classifier),
                });
                let exclude = lib.extract.as_ref().map(|e| e.exclude.clone()).unwrap_or_default();
                native_jars.push((dest, exclude));
            }
            // old-style native library: the main artifact still goes on the classpath
            if !name_has_classifier {
                let main = lib
                    .downloads
                    .as_ref()
                    .and_then(|d| d.artifact.as_ref())
                    .cloned()
                    .or_else(|| {
                        // 老式 json：主构件同样只有 url 基址 + 坐标
                        lib.url.clone().map(|base| {
                            let rel = crate::models::maven_to_path(&lib.name);
                            DownloadFile {
                                url: match rel {
                                    Some(rel) => format!(
                                        "{}/{}",
                                        base.trim_end_matches('/'),
                                        rel.to_string_lossy().replace('\\', "/")
                                    ),
                                    None => String::new(),
                                },
                                sha1: String::new(),
                                size: 0,
                                path: None,
                            }
                        })
                    })
                    .filter(|f| !f.url.is_empty());
                if let Some(dl) = main {
                    let dest = libraries_path(state, &lib.name, None);
                    // 已存在（processor 产物 / installer 内嵌解压产物）就不重复下载
                    if !dest.exists() {
                        lib_items.push(DownloadItem {
                            url: dl.url.clone(),
                            dest,
                            sha1: if dl.sha1.is_empty() { None } else { Some(dl.sha1) },
                            sha512: None,
                            size: if dl.size > 0 { Some(dl.size) } else { None },
                            label: lib.name.clone(),
                        });
                    }
                }
            }
            continue;
        }
        if let Some(dl) = lib.downloads.as_ref().and_then(|d| d.artifact.as_ref()) {
            let dest = libraries_path(state, &lib.name, None);
            // 同上：老式安装器的 forge 本体条目的 url 往往为空（产物由本地生成），
            // 或文件已就位（processor / 内嵌解压），都不该再去请求。
            if dl.url.trim().is_empty() || dest.exists() {
                continue;
            }
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
                // 无 sha1 校验的条目（如 processors 生成的 forge client jar）：
                // 文件已存在则跳过，避免覆盖本地产物或重复下载
                if dest.exists() {
                    continue;
                }
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
                crate::util::fs_best_effort("remove_file", &p, std::fs::remove_file(&p));
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
        flatten_natives(&natives_dir);
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
                url: crate::mirror::asset_url(state, &obj.hash),
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
        let dir = state.instances_dir().join(&instance.id).join(sub);
        crate::util::fs_best_effort("create_dir_all", &dir, std::fs::create_dir_all(&dir));
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
/// 把 natives 目录深层子目录里的 dll 复制到根目录。
/// Mojang 2023+ 重打包的 natives jar（如 1.20.1 的 lwjgl-3.3.1-natives-windows）
/// 内部结构是 `windows/x64/org/lwjgl/lwjgl.dll` 这种深层路径，解压保留相对
/// 路径后 `-Djava.library.path`（指向 natives 根）找不到 dll，启动报
/// 「Failed to locate library: lwjgl.dll」。拍平后各版本加载逻辑都能命中。
fn flatten_natives(dir: &std::path::Path) {
    fn walk(src: &std::path::Path, root: &std::path::Path) {
        let Ok(rd) = std::fs::read_dir(src) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, root);
            } else if p.extension().map(|x| x == "dll").unwrap_or(false) {
                if let Some(name) = p.file_name() {
                    let target = root.join(name);
                    if !target.exists() {
                        let _ = std::fs::copy(&p, &target);
                    }
                }
            }
        }
    }
    walk(dir, dir);
}

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
            let base = crate::mirror::rewrite(state, "https://meta.fabricmc.net/v2/versions/loader");
            latest_stable_loader(state, &base, &instance.mc_version).await?
        }
    };
    let url = crate::mirror::rewrite(
        state,
        &format!(
            "https://meta.fabricmc.net/v2/versions/loader/{}/{}",
            instance.mc_version, loader_ver
        ),
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

/// Remove duplicate libraries (keep first occurrence per name + natives kind).
///
/// 注意：不能只按 maven name 去重！Mojang 对旧版本（≤1.18，LWJGL 3.2.x 时代）
/// 会把同一个坐标列两条——一条是普通 classpath 构件，另一条带 `natives` 和
/// `extract` 字段（用于 natives 的下载与解压）。两条语义不同、都必须保留；
/// 曾经按 name 去重把带 natives 的那条整段丢掉，导致 ≤1.18 的 Fabric/Quilt/
/// Forge 实例安装后没有 natives 目录，启动时报「缺少 natives 目录」。
fn dedupe_libraries(libs: &mut Vec<Library>) {
    let mut seen = std::collections::HashSet::new();
    libs.retain(|l| seen.insert((l.name.clone(), l.natives.is_some())));
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
/// 执行新版 Forge 安装器的 processors 任务链：解压安装器 data/、下载工具库、
/// 逐条运行（mappings 合并 → jar 拆分 → 重命名 → binpatch），最终在 libraries
/// 目录生成打完补丁的 forge client jar。
async fn run_forge_processors(
    app: &tauri::AppHandle,
    state: &AppState,
    instance: &Instance,
    installer_path: &std::path::Path,
    profile: &serde_json::Value,
    task_id: u64,
    source: &str,
) -> Result<(), String> {
    let settings = state.settings.read().unwrap().clone();
    let java: String = match settings.java_path.as_deref().filter(|s| !s.is_empty()) {
        Some(p) => p.to_string(),
        None => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let (.., list) = crate::java::cached_detect(&state.root, &state.root.join("runtimes"), now, false);
            list.first()
                .map(|j| j.path.clone())
                .ok_or("执行 Forge 安装任务需要 Java，但未检测到可用的 Java")?
        }
    };

    let work_dir = crate::paths::resolve_version_dir(state, &instance.id).join("forge_processors");
    std::fs::create_dir_all(&work_dir).map_err(|e| e.to_string())?;

    // 1. 解压安装器内 data/ 目录（binpatch、args 模板等）
    {
        let file = std::fs::File::open(installer_path).map_err(|e| e.to_string())?;
        let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
            let name = entry.name().to_string();
            if !name.starts_with("data/") || entry.is_dir() {
                continue;
            }
            let dest = work_dir.join(&name);
            if let Some(p) = dest.parent() {
                std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
            }
            let mut out = std::fs::File::create(&dest).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        }
    }

    // 2. 下载 processors 的依赖库（仅供安装期 classpath 使用）
    if let Some(installer_libs) = profile.get("libraries").and_then(|v| v.as_array()) {
        let mut items: Vec<DownloadItem> = Vec::new();
        for lib in installer_libs {
            let Ok(l) = serde_json::from_value::<Library>(lib.clone()) else { continue };
            if let Some(dl) = l.downloads.as_ref().and_then(|d| d.artifact.as_ref()) {
                if let Some(rel) = crate::models::maven_to_path(&l.name) {
                    items.push(DownloadItem {
                        url: dl.url.clone(),
                        dest: crate::paths::libraries_dir(state).join(&rel),
                        sha1: Some(dl.sha1.clone()),
                        sha512: None,
                        size: Some(dl.size),
                        label: l.name.clone(),
                    });
                }
            }
        }
        if !items.is_empty() {
            let total = items.len();
            emit_progress(app, task_id, "forge-processors", "下载 Forge 安装依赖…", 0, total, instance, source);
            download_many(app.clone(), state, task_id, "forge-processors", items).await?;
            emit_progress(app, task_id, "forge-processors", "Forge 安装依赖就绪", total, total, instance, source);
        }
    }

    // 3. 变量表：data[VAR].client；[maven 坐标] → libraries 路径；/data/x → 解压目录
    let libs_dir = crate::paths::libraries_dir(state);
    let data_val = profile.get("data").cloned().unwrap_or(serde_json::Value::Null);
    // install_profile.libraries 的实际下载条目（name 可能带 @ext 等后缀），
    // data 变量的 [坐标] 解析必须按条目真实 path 对齐，不能用坐标反推
    let mut installer_libs_parsed: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if let Some(installer_libs) = profile.get("libraries").and_then(|v| v.as_array()) {
        for lib in installer_libs {
            let Ok(l) = serde_json::from_value::<Library>(lib.clone()) else { continue };
            if let Some(dl) = l.downloads.as_ref().and_then(|d| d.artifact.as_ref()) {
                if let Some(rel_path) = dl.path.as_deref() {
                    let dest = crate::paths::libraries_dir(state).join(rel_path).to_string_lossy().to_string();
                    installer_libs_parsed.insert(l.name.clone(), dest);
                }
            }
        }
    }
    let mc_jar = crate::paths::resolve_version_dir(state, &instance.id).join(format!("{}.jar", instance.id));
    let data_value = |var: &str| -> Option<String> {
        let v = data_val.get(var)?.get("client")?.as_str()?.to_string();
        if let Some(coord) = v.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            // 优先按 install_profile 条目的真实下载路径匹配（含 @ 变体），
            // mcp_config 这类非 jar 构件的扩展名与坐标反推结果不同
            if let Some(p) = installer_libs_parsed.get(coord) {
                return Some(p.clone());
            }
            if let Some(p) = installer_libs_parsed.get(&format!("{coord}@jar")) {
                return Some(p.clone());
            }
            let coord_no_ext = coord.split('@').next().unwrap_or(coord);
            if let Some(p) = installer_libs_parsed.get(coord_no_ext) {
                return Some(p.clone());
            }
            let rel = crate::models::maven_to_path(coord_no_ext)?;
            Some(libs_dir.join(rel).to_string_lossy().to_string())
        } else if v.starts_with('/') {
            Some(work_dir.join(v.trim_start_matches('/')).to_string_lossy().to_string())
        } else if v.starts_with('\'') && v.ends_with('\'') && v.len() >= 2 {
            Some(v[1..v.len() - 1].to_string())
        } else {
            Some(v)
        }
    };
    let instance_dir = state.instances_dir().join(&instance.id);
    let replace = |raw: &str| -> Result<String, String> {
        let mut s = raw.to_string();
        let mut i = 0;
        while let Some(st) = s[i..].find('{') {
            let abs = i + st;
            let Some(en_rel) = s[abs..].find('}') else { break };
            let en = abs + en_rel;
            let var = s[abs + 1..en].to_string();
            let val = match var.as_str() {
                "SIDE" => "client".to_string(),
                "ROOT" => instance_dir.to_string_lossy().to_string(),
                "INSTALLER" => installer_path.to_string_lossy().to_string(),
                "MINECRAFT_JAR" => mc_jar.to_string_lossy().to_string(),
                _ => data_value(&var).ok_or(format!("未知变量 {{{var}}}"))?,
            };
            s.replace_range(abs..=en, &val);
            i = abs + val.len();
        }
        // [maven artifact] → install_profile.libraries 对应条目的库文件路径
        let mut j = 0;
        while let Some(st) = s[j..].find('[') {
            let abs = j + st;
            let Some(en_rel) = s[abs..].find(']') else { break };
            let en = abs + en_rel;
            let coord = s[abs + 1..en].to_string();
            let val = installer_libs_parsed
                .get(&coord)
                .cloned()
                .or_else(|| {
                    let no_ext = coord.split('@').next().unwrap_or(&coord).to_string();
                    installer_libs_parsed.get(&no_ext).cloned()
                })
                .unwrap_or_else(|| {
                    crate::models::maven_to_path(&coord)
                        .map(|p| libs_dir.join(p).to_string_lossy().to_string())
                        .unwrap_or_else(|| coord.clone())
                });
            s.replace_range(abs..=en, &val);
            j = abs + val.len();
        }
        Ok(s)
    };

    // 4. 逐条执行 client 侧 processors（老版 Forge ≤1.16 可能没有 processors）
    let Some(processors) = profile.get("processors").and_then(|v| v.as_array()) else {
        // 老版 Forge：没有 processors，仅靠 versionInfo 已经完成了版本信息配置
        return Ok(());
    };
    if processors.is_empty() {
        return Ok(());
    }
    let total = processors.len();
    for (i, proc_entry) in processors.iter().enumerate() {
        if let Some(sides) = proc_entry.get("sides").and_then(|v| v.as_array()) {
            if !sides.iter().any(|s| s.as_str() == Some("client")) {
                continue;
            }
        }
        let main_jar = proc_entry.get("jar").and_then(|v| v.as_str()).ok_or("processor 缺少 jar")?;
        let main_class = proc_entry.get("class").and_then(|v| v.as_str()).map(|s| s.to_string());
        let main_jar_path = libs_dir.join(maven_artifact_path(main_jar).ok_or("processor jar 坐标无效")?);
        let mut cp: Vec<String> = vec![main_jar_path.to_string_lossy().to_string()];
        if let Some(cls) = proc_entry.get("classpath").and_then(|v| v.as_array()) {
            for c in cls {
                if let Some(cn) = c.as_str() {
                    if let Some(rel) = maven_artifact_path(cn) {
                        cp.push(libs_dir.join(rel).to_string_lossy().to_string());
                    }
                }
            }
        }
        let mut cmd_args: Vec<String> = match &main_class {
            Some(cls) => {
                let mut v = vec!["-cp".into(), cp.join(";"), cls.clone()];
                v
            }
            None => {
                let mc = read_jar_main_class(&main_jar_path)?;
                let mut v = vec!["-cp".into(), cp.join(";"), mc];
                v
            }
        };
        if let Some(args) = proc_entry.get("args").and_then(|v| v.as_array()) {
            for a in args {
                let raw = a.as_str().ok_or("processor 参数必须为字符串")?;
                cmd_args.push(replace(raw)?);
            }
        }
        emit_progress(app, task_id, "forge-processors", &format!("Forge 安装任务 {}/{}", i + 1, total), i + 1, total, instance, source);
        let processor_future = tokio::process::Command::new(&java)
            .args(&cmd_args)
            .current_dir(&work_dir)
            .output();
        let output = match timeout(Duration::from_secs(300), processor_future).await {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => return Err(format!("运行 processor 失败: {e}")),
            Err(_) => return Err(format!("processor {main_jar} 执行超时（5 分钟）")),
        };
        if !output.status.success() {
            return Err(format!(
                "processor {main_jar} 执行失败: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        emit_progress(app, task_id, "forge-processors", &format!("Forge 安装任务 {}/{} 完成", i + 1, total), total, total, instance, source);
    }
    Ok(())
}

/// 读取 jar 的 MANIFEST.MF 中的 Main-Class 属性
fn read_jar_main_class(jar: &std::path::Path) -> Result<String, String> {
    let file = std::fs::File::open(jar).map_err(|e| format!("打开 {} 失败: {e}", jar.display()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut entry = archive
        .by_name("META-INF/MANIFEST.MF")
        .map_err(|e| format!("缺少 MANIFEST.MF: {e}"))?;
    let mut text = Vec::new();
    std::io::Read::read_to_end(&mut entry, &mut text).map_err(|e| e.to_string())?;
    for line in String::from_utf8_lossy(&text).lines() {
        if let Some(v) = line.strip_prefix("Main-Class: ") {
            return Ok(v.trim().to_string());
        }
        // MANIFEST 续行以空格开头
        if let Some(rest) = line.strip_prefix(' ') {
            let _ = rest;
        }
    }
    Err(format!("{} 的 MANIFEST 缺少 Main-Class", jar.display()))
}

/// maven 坐标 → 库文件路径，支持 `@ext` 后缀（如 mappings@txt）
fn maven_artifact_path(coord: &str) -> Option<PathBuf> {
    let (coord, ext) = match coord.split_once('@') {
        Some((c, e)) => (c, e),
        None => (coord, "jar"),
    };
    let mut p = crate::models::maven_to_path(coord)?;
    let fname = p.file_name()?.to_string_lossy().to_string();
    let stem = fname.strip_suffix(".jar")?;
    p.set_file_name(format!("{stem}.{ext}"));
    Some(p)
}

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
    // Forge / NeoForge 安装器走 maven 仓库，镜像站统一挂在 `/maven/` 下
    let base_url = crate::mirror::rewrite(state, base_url);
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
        emit_progress(
            app,
            task_id,
            "done",
            &format!("{installer_name} {full_ver} 安装器就绪"),
            1,
            1,
            instance,
            &source,
        );
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

    // 两种安装器格式：
    //   旧版（≤1.16）：install_profile.json 顶层有 versionInfo，即版本 json；
    //   新版（≥1.17）：install_profile.json 只有 processors 等安装任务，
    //     版本信息在 zip 内独立的 version.json（libraries 含 forge client/universal，
    //     带各自的 maven url，后续库下载流程会按 url 拉取——同 HMCL 的处理方式）。
    // 老式安装器（spec 0，典型如 1.12.2）：既没有 versionInfo，也没有 processors
    // 任务链——forge 本体与部分依赖直接内嵌在 installer 的 `maven/` 目录里。
    // 把它解压到 libraries 就等价于官方安装器的安装动作。
    let is_legacy_spec0 = profile.get("spec").and_then(|v| v.as_i64()) == Some(0);
    if is_legacy_spec0 {
        let libs_root = crate::paths::libraries_dir(state);
        if let Err(e) = crate::util::extract_zip_strip(&installer_path, &libs_root, "maven/", &[]) {
            crate::util::log_line(&format!("[forge_patch] 解压内嵌 maven 依赖失败: {e}"));
        }
    }
    let version_info = if let Some(v) = profile.get("versionInfo") {
        v.clone()
    } else {
        // 版本 json 路径由 `json` 字段给出（老式多为 "/version.json"）
        let entry = profile
            .get("json")
            .and_then(|v| v.as_str())
            .unwrap_or("/version.json")
            .trim_start_matches('/');
        let bytes = crate::util::read_zip_entry(&installer_path, entry)
            .map_err(|e| format!("读取版本 json（{entry}）失败: {e}"))?;
        serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|e| format!("解析版本 json 失败: {e}"))?
    };
    let mut patched: VersionJson = serde_json::from_value(version_info)
        .map_err(|e| format!("解析 Forge 版本信息失败: {e}"))?;

    // 新版安装器：forge 本体 jar（universal / client）不在 version.json 的库列表里。
    //   - universal：install_profile.libraries 提供，downloads 元数据齐全；
    //   - client：maven 上有现成的 {path}:client jar，与 universal 同 maven base
    //     （安装器的 processors 链只是"从原版 jar 补丁生成"它的另一条路，
    //     直接用 maven 现成产物，与 HMCL 的处理一致）。
    // 注意：install_profile.libraries 的其余条目（jopt-simple 6.x、lzma-java 等）
    // 是 processors 安装期工具链，不能进运行时 classpath——例如 jopt-simple
    // 6.x 的模块名是 joptsimple，而 modlauncher 要求的模块名是 jopt.simple
    // （5.0.4 的自动推导名），全量合并会直接导致模块解析失败。
    let mut forge_maven_base: Option<String> = None;
    if let Some(installer_libs) = profile.get("libraries").and_then(|v| v.as_array()) {
        for lib in installer_libs {
            let Ok(l) = serde_json::from_value::<Library>(lib.clone()) else { continue };
            if !l.name.ends_with(":universal") {
                continue;
            }
            if let Some(dl) = l.downloads.as_ref().and_then(|d| d.artifact.as_ref()) {
                if let Some(rel) = crate::models::maven_to_path(&l.name) {
                    let rel_s = rel.to_string_lossy().replace('\\', "/");
                    if let Some(idx) = dl.url.find(&rel_s) {
                        forge_maven_base = Some(dl.url[..idx].to_string());
                    }
                }
            }
            patched.libraries.push(l);
        }
    }
    // 老式安装器没有 `:client` 产物（forge 本体内嵌在 installer 里），
    // 按新版那样拼 `:client` 去 maven 取必然 404。
    if !is_legacy_spec0 {
      if let Some(path_str) = profile.get("path").and_then(|v| v.as_str()) {
        let client_name = format!("{path_str}:client");
        if crate::models::maven_to_path(&client_name).is_some() {
            // url 字段必须是 maven base URL（如 https://maven.minecraftforge.net/），
            // 而非完整 jar 路径——下载代码会用 url + maven_to_path(name) 拼出完整 URL。
            let url = forge_maven_base
                .unwrap_or_else(|| "https://maven.minecraftforge.net/".to_string());
            patched.libraries.push(Library {
                name: client_name,
                url: Some(url),
                downloads: None,
                rules: None,
                natives: None,
                extract: None,
            });
        }
      }
    }

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
    // forge 的 version.json 只带它自己的 game args（--launchTarget/--fml.*），
    // vanilla 的用户参数（--username/--version/--accessToken/--gameDir 等）
    // 在 vanilla json 里——不合并的话 MC Main 报
    // Missing required option(s) [accessToken, version]。
    // 合并顺序：vanilla 用户参数在前，forge 追加在后。
    if let Some(vanilla_args) = &vanilla.arguments {
        match &mut patched.arguments {
            Some(pa) => {
                let mut merged = vanilla_args.game.clone().unwrap_or_default();
                merged.extend(pa.game.take().unwrap_or_default());
                pa.game = Some(merged);
            }
            None => patched.arguments = Some(vanilla_args.clone()),
        }
    }
    // 新版 version.json 只含 forge 自身库，不带 vanilla 的 LWJGL natives 库；
    // 不合并的话 install_game 没有任何带 natives 的条目可解压，启动时
    // 报「缺少 natives 目录」。vanilla 库追加在后，重名交给 dedupe 去重。
    patched.libraries.extend(vanilla.libraries.clone());
    dedupe_libraries(&mut patched.libraries);
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
            let url = format!("{}/{mc_version}", crate::mirror::rewrite(state, base));
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
            let url = crate::mirror::rewrite(state, url);
            let xml = crate::download::get_text(&state.client, &url).await?;
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
