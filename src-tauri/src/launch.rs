use crate::models::*;
use crate::state::AppState;
use crate::util::rules_allow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tauri::Emitter;

#[derive(Clone)]
pub struct ResolvedAccount {
    pub username: String,
    pub uuid: String,
    pub access_token: String,
    pub user_type: String,
    /// JSON string passed to the game via `${user_properties}`.
    /// For Microsoft accounts this is filled with the `textures` property so
    /// legacy versions (e.g. 1.12.2) render the online skin without relying on
    /// a runtime sessionserver fetch. Defaults to `"{}"` for offline accounts.
    pub user_properties: String,
}

#[derive(Clone)]
struct LaunchContext {
    instance: Instance,
    settings: Settings,
    #[allow(dead_code)]
    java: JavaInfo,
    version_json: VersionJson,
    classpath: String,
    natives_dir: PathBuf,
    instance_dir: PathBuf,
    assets_dir: PathBuf,
    libraries_dir: PathBuf,
    version_dir: PathBuf,
    account: ResolvedAccount,
    resolution: Option<(u32, u32)>,
    world: Option<String>,
    server: Option<String>,
}

/// Launch a game instance. Emits `launch://log`, `launch://state` events.
/// `world` (optional) directly joins a singleplayer world via quick play.
/// `server` (optional) directly joins a multiplayer server.
pub async fn launch_game(
    app: tauri::AppHandle,
    state: &AppState,
    instance: &Instance,
    account: ResolvedAccount,
    world: Option<String>,
    server: Option<String>,
) -> Result<LaunchResult, String> {
    let mut settings = state.settings.read().unwrap().clone();
    // Resolve effective Microsoft Client ID (built-in or user-configured)
    if let Ok(id) = crate::accounts::effective_ms_client_id(state) {
        settings.ms_client_id = id;
    }
    let version_path = crate::paths::resolve_version_dir(state, &instance.id).join(format!("{}.json", instance.id));
    if !version_path.exists() {
        return Err("游戏尚未安装，请先点击「安装游戏」".into());
    }
    let text = std::fs::read_to_string(&version_path).map_err(|e| e.to_string())?;
    let mut version_json: VersionJson =
        serde_json::from_str(&text).map_err(|e| format!("版本元数据损坏: {e}"))?;
    // ensure natives-related JVM args point at the natives dir itself
    // (idempotent; also covers instances patched before this fix)
    crate::install::normalize_natives_args(&mut version_json);

    let _ = app.emit("launch://progress", serde_json::json!({ "step": "正在检查 Java 运行时…", "progress": 40 }));
    let java = match pick_java(&app, state, instance, &version_json).await {
        Ok(j) => j,
        Err(e) if e.starts_with("NEED_DOWNLOAD:") => {
            let major = e
                .trim_start_matches("NEED_DOWNLOAD:")
                .parse::<u32>()
                .unwrap_or(17);
            let _ = app.emit(
                "launch://log",
                serde_json::json!({
                    "instanceId": instance.id,
                    "stream": "out",
                    "line": format!("未找到合适的 Java，正在自动下载 Java {major} JRE…"),
                }),
            );
            crate::java::download_java_runtime(app.clone(), state, major).await?;
            // 下载后清除检测缓存，确保下一次 pick_java 能发现新装的 Java
            *state.java_cache.lock().unwrap() = None;
            pick_java(&app, state, instance, &version_json)
                .await
                .map_err(|e2| format!("Java 下载完成但仍无法选择: {e2}"))?
        }
        Err(e) => return Err(e),
    };

    let instance_dir = state.instances_dir().join(&instance.id);
    let natives_dir = instance_dir.join("natives");
    if !natives_dir.exists() {
        return Err("缺少 natives 目录，请重新安装游戏".into());
    }
    let resolution = instance.resolution;

    // Ensure the game launches in Chinese
    set_chinese_lang(&instance_dir, &instance.mc_version);

    let account = account;

    // Offline skin: build a resource pack and enable it in options.txt.
    // (Previously we injected the PNG directly into client.jar, but that
    // triggered antivirus heuristics — binary patching + META-INF signature
    // removal.  The resource-pack approach mirrors PCL2 and is AV-safe.)
    if account.user_type == "legacy" {
        let skin_file = state.root.join("skins").join("offline").join(format!("{}.png", account.uuid));
        if skin_file.exists() {
            if let Some(skin_bytes) = std::fs::read(&skin_file).ok() {
                let current_hash = crate::util::file_sha1(&skin_file).unwrap_or_default();
                let marker = instance_dir.join(".qookix-skin-pack");
                let already_built = std::fs::read_to_string(&marker)
                    .map(|h| h.trim() == current_hash)
                    .unwrap_or(false);
                if !already_built {
                    let _ = app.emit("launch//log", serde_json::json!({
                        "instanceId": instance.id,
                        "stream": "out",
                        "line": "正在应用离线皮肤…",
                    }));
                    build_skin_resourcepack(&instance_dir, &skin_bytes)?;
                    let _ = std::fs::write(&marker, current_hash);
                }
            }
        }
    } else {
        // Non-offline account: remove any leftover skin pack so it doesn't
        // override the server-provided skin.
        let skin_pack = instance_dir.join("resourcepacks").join("QookiX_Skin.zip");
        if skin_pack.exists() {
            let _ = std::fs::remove_file(&skin_pack);
            disable_resource_pack(&instance_dir, "QookiX_Skin.zip");
        }
    }

    let _ = app.emit("launch://progress", serde_json::json!({ "step": "正在准备启动参数…", "progress": 60 }));

    // ---- classpath ----
    let features = features_map(resolution.is_some());

    // 按 group:artifact 去重，保留最高版本（避免 duplicate ASM classes 等冲突）
    let mut best_libs: std::collections::HashMap<String, &crate::models::Library> = HashMap::new();
    for lib in &version_json.libraries {
        if !rules_allow(lib.rules.as_deref().unwrap_or(&[]), &features) {
            continue;
        }
        // 提取 group:artifact 作为去重键
        let key = lib.name.split(':').take(2).collect::<Vec<_>>().join(":");
        match best_libs.get(&key) {
            Some(existing) => {
                // 比较版本，保留更高的
                let existing_ver = existing.name.split(':').nth(2).unwrap_or("0");
                let new_ver = lib.name.split(':').nth(2).unwrap_or("0");
                if compare_versions(new_ver, existing_ver) > 0 {
                    best_libs.insert(key, lib);
                }
            }
            None => {
                best_libs.insert(key, lib);
            }
        }
    }

    let mut classpath: Vec<String> = Vec::new();
    for lib in best_libs.values() {
        // NOTE: natives jars stay on the classpath — LWJGL 3.4 loads shared
        // libraries from the classpath (`LibraryResource` + SharedLibraryLoader),
        // which is how modern versions (26.x) provide lwjgl.dll etc.
        if let Some(dl) = lib.downloads.as_ref().and_then(|d| d.artifact.as_ref()) {
            if let Some(p) = &dl.path {
                classpath.push(crate::paths::libraries_dir(state).join(p).to_string_lossy().to_string());
            } else if let Some(rel) = crate::models::maven_to_path(&lib.name) {
                classpath.push(crate::paths::libraries_dir(state).join(rel).to_string_lossy().to_string());
            }
        } else if let Some(rel) = crate::models::maven_to_path(&lib.name) {
            classpath.push(crate::paths::libraries_dir(state).join(rel).to_string_lossy().to_string());
        }
    }
    let client_jar = crate::paths::resolve_version_dir(state, &instance.id).join(format!("{}.jar", instance.id));
    if client_jar.exists() {
        classpath.push(client_jar.to_string_lossy().to_string());
    }
    let sep = if cfg!(windows) { ";" } else { ":" };
    let classpath_str = classpath.join(sep);

    let ctx = LaunchContext {
        instance: instance.clone(),
        settings: settings.clone(),
        java: java.clone(),
        version_json: version_json.clone(),
        classpath: classpath_str,
        natives_dir,
        instance_dir,
        assets_dir: crate::paths::assets_dir(state),
        libraries_dir: crate::paths::libraries_dir(state),
        version_dir: crate::paths::resolve_version_dir(state, &instance.id),
        account,
        resolution,
        world,
        server,
    };

    let args = build_args(&ctx);

    // Diagnostic: Java path + authlib version, to compare with PCL.
    let authlib = ctx
        .classpath
        .split(if cfg!(windows) { ';' } else { ':' })
        .find(|p| p.contains("authlib"))
        .unwrap_or("<no authlib>");
    let _ = app.emit("launch://log", serde_json::json!({
        "instanceId": &instance.id, "stream": "out",
        "line": format!("[环境诊断] Java={} (major={}), authlib={}", java.path, java.major, authlib)
    }));

    let mut cmd = Command::new(&java.path);
    cmd.args(&args)
        .current_dir(&ctx.instance_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    // write launch log file
    let log_path = state.logs_dir().join(format!(
        "{}-{}.log",
        instance.id,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(state.logs_dir()).map_err(|e| e.to_string())?;
    // Never write the raw access token into the launch log.
    let logged = mask_secret_args(&args);
    let _ = std::fs::write(&log_path, format!("$ java {}\n", logged.join(" ")));

    let _ = app.emit("launch://progress", serde_json::json!({ "step": "正在启动游戏进程…", "progress": 80 }));

    let child = tokio::process::Command::from(cmd)
        .spawn()
        .map_err(|e| format!("启动 Java 失败: {e}"))?;
    let pid = child.id().unwrap_or(0);
    let _ = app.emit("launch://progress", serde_json::json!({ "step": "启动成功，正在等待游戏窗口…", "progress": 100 }));
    emit_state(&app, &instance.id, "running", pid, None);
    {
        let mut guard = state.game_pids.lock().unwrap();
        guard.insert(instance.id.clone(), pid);
    }
    let _ = app.emit(
        "launch://pid",
        serde_json::json!({ "instanceId": instance.id, "pid": pid, "logPath": log_path.to_string_lossy() }),
    );

    // stream stdout/stderr in the background
    let app2 = app.clone();
    let inst_id = instance.id.clone();
    let gp = state.game_pids.clone();
    let logs_dir = state.logs_dir();
    let inst_dir = ctx.instance_dir.clone();
    tauri::async_runtime::spawn(async move {
        let outcome = stream_output(&app2, child, logs_dir.clone(), inst_id.clone()).await;
        {
            let mut guard = gp.lock().unwrap();
            guard.remove(&inst_id);
        }
        emit_state(&app2, &inst_id, "exited", pid, outcome);
        let _ = app2.emit(
            "launch://exit",
            serde_json::json!({ "instanceId": inst_id, "code": outcome }),
        );
        // 崩溃检查：进程异常退出时分析日志与崩溃文件并弹窗提示
        if outcome != Some(0) {
            if let Some(diag) = diagnose_crash(&inst_dir, &logs_dir, &inst_id, outcome) {
                let mut v = serde_json::to_value(&diag).unwrap_or_default();
                v["instanceId"] = serde_json::json!(inst_id);
                let _ = app2.emit("launch//crash", v);
            }
        }
    });

    Ok(LaunchResult {
        pid,
        command: args,
    })
}

async fn stream_output(
    app: &tauri::AppHandle,
    mut child: tokio::process::Child,
    logs_dir: PathBuf,
    instance_id: String,
) -> Option<i32> {
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let write_log: std::sync::Arc<std::sync::Mutex<std::fs::File>> =
        std::sync::Arc::new(std::sync::Mutex::new(
            std::fs::File::create(logs_dir.join(format!("{instance_id}-live.log"))).ok()?,
        ));

    let mut tasks = Vec::new();
    if let Some(out) = stdout {
        let app = app.clone();
        let id = instance_id.clone();
        let log = write_log.clone();
        tasks.push(tauri::async_runtime::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut reader = tokio::io::BufReader::new(out).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let _ = app.emit("launch://log", serde_json::json!({
                    "instanceId": id, "stream": "out", "line": line
                }));
                if let Ok(mut f) = log.lock() {
                    use std::io::Write;
                    let _ = writeln!(f, "{line}");
                }
            }
        }));
    }
    if let Some(err) = stderr {
        let app = app.clone();
        let id = instance_id.clone();
        let log = write_log.clone();
        tasks.push(tauri::async_runtime::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut reader = tokio::io::BufReader::new(err).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let _ = app.emit("launch://log", serde_json::json!({
                    "instanceId": id, "stream": "err", "line": line
                }));
                if let Ok(mut f) = log.lock() {
                    use std::io::Write;
                    let _ = writeln!(f, "{line}");
                }
            }
        }));
    }

    let status = child.wait().await.ok();
    for t in tasks {
        let _ = t.await;
    }
    status.map(|s| s.code().unwrap_or(-1))
}

pub async fn kill_game(state: &AppState) -> Result<(), String> {
    let pids: Vec<u32> = {
        let mut guard = state.game_pids.lock().unwrap();
        guard.drain().map(|(_, v)| v).collect()
    };
    if pids.is_empty() {
        return Ok(());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // Graceful: send WM_CLOSE via taskkill (no /F) to all, then give them a
        // single shared window to exit cleanly before force-killing the rest.
        for pid in &pids {
            let _ = Command::new("taskkill")
                .args(["/PID", &pid.to_string()])
                .creation_flags(CREATE_NO_WINDOW)
                .output();
        }
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        for pid in &pids {
            // Force kill the entire process tree if still alive
            let _ = Command::new("taskkill")
                .args(["/T", "/F", "/PID", &pid.to_string()])
                .creation_flags(CREATE_NO_WINDOW)
                .output();
        }
    }
    #[cfg(not(windows))]
    {
        // Graceful: SIGTERM all first
        for pid in &pids {
            let _ = Command::new("kill")
                .args(["-15", &pid.to_string()])
                .output();
        }
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        for pid in &pids {
            // Force kill if still alive
            let _ = Command::new("kill")
                .args(["-9", &pid.to_string()])
                .output();
        }
    }
    Ok(())
}

pub fn is_running(state: &AppState) -> bool {
    !state.game_pids.lock().unwrap().is_empty()
}

// ---------------------------------------------------------------------------
// Java selection
// ---------------------------------------------------------------------------

/// Required Java major version for an instance: from the version JSON, with the
/// client jar's class-file version as an authoritative fallback.
pub fn required_java_for(state: &AppState, instance: &Instance) -> u32 {
    let mut required = 8u32;
    let version_path = crate::paths::resolve_version_dir(state, &instance.id).join(format!("{}.json", instance.id));
    if let Ok(text) = std::fs::read_to_string(&version_path) {
        if let Ok(vj) = serde_json::from_str::<VersionJson>(&text) {
            if let Some(j) = vj.java_version {
                required = j.major_version;
            }
        }
    }
    let jar = crate::paths::resolve_version_dir(state, &instance.id).join(format!("{}.jar", instance.id));
    if let Some(class_major) = crate::util::jar_class_version(&jar) {
        let from_jar = class_major.saturating_sub(44);
        if from_jar > required {
            required = from_jar;
        }
    }
    required
}

/// Get detected Java list from cache. Re-scans if cache is older than 5 minutes.
async fn get_detected_java(state: &AppState) -> Vec<JavaInfo> {
    // Prefer in-memory results (loaded from the persisted cache at startup;
    // refreshed on demand or after a runtime download invalidates it).
    if let Some((_, list)) = state.java_cache.lock().unwrap().as_ref() {
        return list.clone();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let root = state.root.clone();
    let runtimes = root.join("runtimes");
    let (ts, detected) = tokio::task::spawn_blocking(move || {
        crate::java::cached_detect(&root, &runtimes, now, false)
    })
    .await
    .unwrap_or_else(|_| (now, Vec::new()));
    *state.java_cache.lock().unwrap() = Some((ts, detected.clone()));
    detected
}

/// Parse the update number from a Java version string for ranking.
/// `1.8.0_51` -> 51, `1.8.0_302` -> 302, `17.0.2` -> 2, `21.0.2` -> 2.
/// Used to prefer newer patch releases within the same major version — this
/// avoids picking the ancient Mojang `jre-legacy` (8u51) whose cacerts are too
/// old to validate Mojang's current TLS certificates, causing skin loading to
/// fail with `PKIX path building failed`.
fn java_update_number(version: &str) -> u32 {
    if let Some(idx) = version.find('_') {
        version[idx + 1..]
            .split(|c: char| !c.is_ascii_digit())
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    } else {
        version
            .split('.')
            .filter_map(|p| p.parse().ok())
            .last()
            .unwrap_or(0)
    }
}

/// Best available Java for the required major: exact match preferred (newest
/// patch release), else highest major.
pub async fn find_best_java(state: &AppState, required: u32) -> Option<JavaInfo> {
    let detected = get_detected_java(state).await;
    detected
        .iter()
        .filter(|j| j.major == required)
        .max_by_key(|j| java_update_number(&j.version))
        .or_else(|| detected.iter().max_by_key(|j| j.major))
        .cloned()
}

async fn pick_java(
    app: &tauri::AppHandle,
    state: &AppState,
    instance: &Instance,
    version_json: &VersionJson,
) -> Result<JavaInfo, String> {
    let mut required = version_json
        .java_version
        .as_ref()
        .map(|j| j.major_version)
        .unwrap_or(8);
    // authoritative fallback: derive the requirement from the client jar itself
    let jar = crate::paths::resolve_version_dir(state, &instance.id).join(format!("{}.jar", instance.id));
    if let Some(class_major) = crate::util::jar_class_version(&jar) {
        let from_jar = class_major.saturating_sub(44);
        if from_jar > required {
            required = from_jar;
        }
    }

    // Java selection is per-instance only
    if let Some(path) = &instance.java_path {
        let cand = crate::java::probe_java(std::path::Path::new(path))
            .ok_or_else(|| format!("指定的 Java 不可用: {path}"))?;
        if cand.major < required {
            return Err(format!(
                "所选 Java {} 版本过低，该游戏需要 Java {required}+（class 版本要求）\n请安装 JDK {required}（推荐 Adoptium Temurin），或在实例设置中更换 Java",
                cand.version
            ));
        }
        return Ok(cand);
    }

    let detected = get_detected_java(state).await;
    let _ = app.emit("launch//log", serde_json::json!({
        "instanceId": instance.id,
        "stream": "out",
        "line": format!("[诊断] 检测到 {} 个 Java: {:?}", detected.len(),
            detected.iter().map(|j| format!("{}(major={},update={})", j.version, j.major, java_update_number(&j.version))).collect::<Vec<_>>()),
    }));
    if detected.is_empty() {
        return Err(format!("NEED_DOWNLOAD:{required}"));
    }
    // Prefer the newest patch release of the required major (e.g. 8u422 over
    // 8u51) — older JREs like Mojang's jre-legacy (8u51) have an outdated
    // cacerts trust store that breaks Mojang TLS (skin/cape lookups).
    let exact = detected
        .iter()
        .filter(|j| j.major == required)
        .max_by_key(|j| java_update_number(&j.version));
    if let Some(j) = exact {
        // Mojang's ancient `jre-legacy` (8u51) has an outdated cacerts trust
        // store that breaks Mojang TLS — skin/cape lookups fail with
        // `PKIX path building failed`. Force a download of a modern Java 8
        // (Adoptium Temurin, current update) instead of using a JRE older
        // than 8u200.
        if j.major == 8 && java_update_number(&j.version) < 200 {
            return Err(format!("NEED_DOWNLOAD:{required}"));
        }
        return Ok(j.clone());
    }
    let best = detected.iter().max_by_key(|j| j.major).unwrap();
    if best.major < required {
        return Err(format!("NEED_DOWNLOAD:{required}"));
    }
    Ok(best.clone())
}

/// Redact sensitive arguments (the Minecraft access token) before logging the
/// launch command line to `logs/<instance>-<ts>.log`.
fn mask_secret_args(args: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(args.len());
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--accessToken" {
            out.push(a.clone());
            if i + 1 < args.len() {
                out.push("***".into());
                i += 2;
                continue;
            }
        } else if a.starts_with("--accessToken=") {
            out.push("--accessToken=***".into());
            i += 1;
            continue;
        }
        out.push(a.clone());
        i += 1;
    }
    out
}

/// Set `lang:zh_CN` (or `zh_cn` for 1.13+) in the instance's `options.txt` so
/// the game launches in Chinese. Only writes the default when the user has not
/// already chosen a language, so their in-game language preference is respected.
fn set_chinese_lang(instance_dir: &std::path::Path, mc_version: &str) {
    let options_path = instance_dir.join("options.txt");
    // Minecraft 1.13+ uses lowercase language codes (zh_cn), older versions use zh_CN
    let parts: Vec<&str> = mc_version.split('.').collect();
    let major: u32 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor: u32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let target_line = if major == 1 && minor < 13 { "lang:zh_CN" } else { "lang:zh_cn" };
    if let Ok(text) = std::fs::read_to_string(&options_path) {
        // Respect an existing language choice instead of forcing zh.
        if text.lines().any(|l| l.starts_with("lang:")) {
            return;
        }
        let result = format!("{text}\n{target_line}");
        let _ = std::fs::write(&options_path, result);
    } else {
        let _ = std::fs::write(&options_path, target_line);
    }
}

/// Build a resource pack zip (`QookiX_Skin.zip`) containing the offline skin
/// PNG and enable it in the instance's `options.txt`.
///
/// This replaces the old jar-injection approach (`inject_skin_into_jar`) which
/// read `client.jar`, stripped its `META-INF` signatures, rewrote it with
/// `ZipWriter` and overwrote the original.  That sequence — binary patching +
/// signature removal + overwrite — matched the heuristic signature of
/// `Trojan/CoinMiner` jar-backdoor injectors and caused persistent antivirus
/// false-positives on the packaged `setup.exe`.
///
/// The resource-pack strategy mirrors PCL2 (`PCL2 Skin.zip`) and is AV-safe:
/// we only *create* a new zip in `resourcepacks/` and toggle one line in
/// `options.txt`; `client.jar` is never touched.
fn build_skin_resourcepack(instance_dir: &std::path::Path, skin: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let rp_dir = instance_dir.join("resourcepacks");
    std::fs::create_dir_all(&rp_dir).map_err(|e| format!("create resourcepacks: {e}"))?;
    let zip_path = rp_dir.join("QookiX_Skin.zip");

    let buf = std::io::Cursor::new(Vec::new());
    let mut zip = zip::write::ZipWriter::new(buf);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    let mcmeta = r#"{"pack":{"pack_format":1,"description":"QookiX Offline Skin"}}"#;
    zip.start_file("pack.mcmeta", opts)
        .map_err(|e| format!("write mcmeta: {e}"))?;
    zip.write_all(mcmeta.as_bytes())
        .map_err(|e| format!("write mcmeta: {e}"))?;

    let skin_paths: &[&str] = &[
        "assets/minecraft/textures/entity/steve.png",
        "assets/minecraft/textures/entity/alex.png",
        "assets/minecraft/textures/entity/player/wide/steve.png",
        "assets/minecraft/textures/entity/player/wide/alex.png",
        "assets/minecraft/textures/entity/player/slim/steve.png",
        "assets/minecraft/textures/entity/player/slim/alex.png",
    ];
    for &p in skin_paths {
        zip.start_file(p, opts).map_err(|e| format!("write entry: {e}"))?;
        zip.write_all(skin).map_err(|e| format!("write skin: {e}"))?;
    }

    let buf = zip.finish().map_err(|e| format!("finish zip: {e}"))?;
    std::fs::write(&zip_path, buf.into_inner()).map_err(|e| format!("save zip: {e}"))?;

    enable_resource_pack(instance_dir, "QookiX_Skin.zip");
    Ok(())
}

/// Add `"file/<pack>"` to the `resourcePacks` line in `options.txt` so the
/// game loads the skin pack on next launch.  Our pack is inserted at the front
/// so it takes priority over vanilla textures.
fn enable_resource_pack(instance_dir: &std::path::Path, pack_file: &str) {
    let options_path = instance_dir.join("options.txt");
    let entry = format!("\"file/{pack_file}\"");
    let mut lines: Vec<String> = Vec::new();
    let mut found = false;
    let mut already_enabled = false;
    if let Ok(text) = std::fs::read_to_string(&options_path) {
        for line in text.lines() {
            if line.starts_with("resourcePacks:") {
                found = true;
                let value = &line["resourcePacks:".len()..];
                if value.contains(&entry) {
                    already_enabled = true;
                    lines.push(line.to_string());
                } else {
                    let inner = value.trim_start_matches('[').trim_end_matches(']');
                    let new_value = if inner.is_empty() {
                        format!("[{entry}]")
                    } else {
                        format!("[{entry},{inner}]")
                    };
                    lines.push(format!("resourcePacks:{new_value}"));
                }
            } else {
                lines.push(line.to_string());
            }
        }
    }
    if !found {
        lines.push(format!("resourcePacks:[{entry}]"));
    }
    if !already_enabled || !found {
        let _ = std::fs::write(&options_path, lines.join("\n"));
    }
}

/// Remove `"file/<pack>"` from the `resourcePacks` line in `options.txt`.
fn disable_resource_pack(instance_dir: &std::path::Path, pack_file: &str) {
    let options_path = instance_dir.join("options.txt");
    let entry = format!("\"file/{pack_file}\"");
    let Ok(text) = std::fs::read_to_string(&options_path) else { return };
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.starts_with("resourcePacks:") {
            let value = &line["resourcePacks:".len()..];
            let inner = value.trim_start_matches('[').trim_end_matches(']');
            let kept: Vec<&str> = inner
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty() && *s != entry)
                .collect();
            if kept.is_empty() {
                lines.push("resourcePacks:[]".to_string());
            } else {
                lines.push(format!("resourcePacks:[{}]", kept.join(",")));
            }
        } else {
            lines.push(line.to_string());
        }
    }
    let _ = std::fs::write(&options_path, lines.join("\n"));
}

// ---------------------------------------------------------------------------
// Argument building
// ---------------------------------------------------------------------------

fn features_map(custom_resolution: bool) -> HashMap<String, bool> {
    let mut m = HashMap::new();
    m.insert("is_demo_user".to_string(), false);
    m.insert("has_custom_resolution".to_string(), custom_resolution);
    m
}

/// 比较两个 Maven 版本号，返回 >0 表示 a 更新，<0 表示 b 更新，0 表示相等
fn compare_versions(a: &str, b: &str) -> i32 {
    let pa: Vec<u32> = a.split(|c: char| c == '.' || c == '-').filter_map(|s| s.parse().ok()).collect();
    let pb: Vec<u32> = b.split(|c: char| c == '.' || c == '-').filter_map(|s| s.parse().ok()).collect();
    for i in 0..pa.len().max(pb.len()) {
        let va = pa.get(i).copied().unwrap_or(0);
        let vb = pb.get(i).copied().unwrap_or(0);
        if va != vb {
            return if va > vb { 1 } else { -1 };
        }
    }
    0
}

fn build_args(ctx: &LaunchContext) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    let settings = &ctx.settings;
    let instance = &ctx.instance;

    // memory — count mods for auto calculation
    let mods_dir = ctx.instance_dir.join("mods");
    let mod_count = std::fs::read_dir(&mods_dir)
        .map(|d| d.filter(|e| e.as_ref().map(|e| e.path().extension().is_some_and(|x| x == "jar")).unwrap_or(false)).count())
        .unwrap_or(0);
    let auto_mode = match instance.memory_mode.as_deref() {
        Some("auto") => true,
        Some("global") | None => settings.memory_mode == "auto",
        _ => false,
    };
    let max_mem = if auto_mode {
        crate::settings::available_memory_mb()
            .map(|a| crate::settings::recommended_memory(a, mod_count).0)
            .unwrap_or(0)
    } else {
        0
    };
    let max_mem = if max_mem > 0 {
        max_mem
    } else {
        instance.max_memory_mb.or(Some(settings.max_memory_mb)).unwrap_or(2048).max(256)
    };
    let min_mem = settings.min_memory_mb.max(64);
    let (min_mem, _) = if auto_mode {
        crate::settings::available_memory_mb()
            .map(|a| crate::settings::recommended_memory(a, mod_count))
            .map(|(_, m)| (m, 0))
            .unwrap_or((min_mem, 0))
    } else {
        (min_mem, 0)
    };
    args.push(format!("-Xmx{max_mem}M"));
    args.push(format!("-Xms{min_mem}M"));
    args.push("-Duser.language=zh".into());
    args.push("-Duser.country=CN".into());

    // jvm args from json
    let mut features = features_map(ctx.resolution.is_some());
    if let Some(arguments) = &ctx.version_json.arguments {
        if let Some(jvm) = &arguments.jvm {
            for av in jvm {
                match av {
                    ArgumentValue::Str(s) => {
                        args.push(substitute(s, ctx, &mut features));
                    }
                    ArgumentValue::Rule(r) => {
                        if rules_allow(&r.rules, &features) {
                            match &r.value {
                                ArgumentValueInner::Str(s) => args.push(substitute(s, ctx, &mut features)),
                                ArgumentValueInner::List(l) => {
                                    for s in l {
                                        args.push(substitute(s, ctx, &mut features));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // legacy jvm args for < 1.13
    if ctx.version_json.arguments.is_none() {
        args.push(substitute("-Djava.library.path=${natives_directory}", ctx, &mut features));
        args.push(substitute("-Dminecraft.launcher.brand=${launcher_name}", ctx, &mut features));
        args.push(substitute("-Dminecraft.launcher.version=${launcher_version}", ctx, &mut features));
        args.push(substitute("-cp", ctx, &mut features));
        args.push(ctx.classpath.clone());
    }

    // logging config
    if let Some(logging) = &ctx.version_json.logging {
        if let Some(client) = &logging.client {
            let path = ctx.version_dir.join("log4j2.xml");
            if path.exists() {
                let arg = client.argument.replace("${path}", &path.to_string_lossy());
                args.push(arg);
            }
        }
    }

    // custom jvm args
    args.extend(split_args(&instance.jvm_args.clone().or(Some(settings.jvm_args.clone())).unwrap_or_default()));

    // main class
    let main_class = ctx
        .version_json
        .main_class
        .clone()
        .unwrap_or_else(|| "net.minecraft.client.main.Main".into());
    args.push(main_class);

    // game args
    let mut game_args: Vec<String> = Vec::new();
    if let Some(arguments) = &ctx.version_json.arguments {
        if let Some(game) = &arguments.game {
            for av in game {
                match av {
                    ArgumentValue::Str(s) => game_args.push(substitute(s, ctx, &mut features)),
                    ArgumentValue::Rule(r) => {
                        if rules_allow(&r.rules, &features) {
                            match &r.value {
                                ArgumentValueInner::Str(s) => game_args.push(substitute(s, ctx, &mut features)),
                                ArgumentValueInner::List(l) => {
                                    for s in l {
                                        game_args.push(substitute(s, ctx, &mut features));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(legacy) = &ctx.version_json.minecraft_arguments {
        game_args.extend(split_args(legacy).into_iter().map(|s| substitute(&s, ctx, &mut features)));
    }
    game_args.extend(split_args(&settings.game_args));

    // quick play: directly join a singleplayer world (Minecraft 1.20.2+)
    if let Some(world) = &ctx.world {
        if supports_quick_play(&ctx.instance.mc_version) {
            args.push("--quickPlaySingleplayer".into());
            args.push(world.clone());
        }
    }

    // quick play: directly join a multiplayer server
    if let Some(server) = &ctx.server {
        if supports_quick_play(&ctx.instance.mc_version) {
            args.push("--quickPlayMultiplayer".into());
            args.push(server.clone());
        } else {
            // 1.20.2 之前的版本使用 --server / --port 参数
            let (host, port) = split_host_port(server);
            args.push("--server".into());
            args.push(host);
            args.push("--port".into());
            args.push(port.to_string());
        }
    }

    args.extend(game_args);
    args
}

/// 把 "host"、"host:port" 或 "[ipv6]:port" 拆成 (host, port)，默认 25565
fn split_host_port(addr: &str) -> (String, u16) {
    if addr.starts_with('[') {
        if let Some(end) = addr.find(']') {
            let host = addr[1..end].to_string();
            let port = addr[end + 1..]
                .strip_prefix(':')
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or(25565);
            return (host, port);
        }
    }
    if let Some(idx) = addr.rfind(':') {
        let (h, p) = addr.split_at(idx);
        if let Ok(port) = p[1..].parse::<u16>() {
            return (h.to_string(), port);
        }
    }
    (addr.to_string(), 25565)
}

/// Minecraft added quick-play (`--quickPlaySingleplayer`) in 1.20.2.
fn supports_quick_play(mc_version: &str) -> bool {
    let nums: Vec<u32> = mc_version
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse().ok())
        .collect();
    match nums.as_slice() {
        [1, 20, minor, ..] => minor >= &2,
        [1, major, ..] => major >= &21,
        [major, ..] => major >= &2,
        _ => false,
    }
}

/// Split a string of args on whitespace, respecting double quotes.
fn split_args(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    for c in input.chars() {
        match c {
            '"' => in_quote = !in_quote,
            c if c.is_whitespace() && !in_quote => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn substitute(input: &str, ctx: &LaunchContext, features: &mut HashMap<String, bool>) -> String {
    let mut s = input.to_string();
    let mut vars: HashMap<String, String> = HashMap::new();
    vars.insert("natives_directory".into(), ctx.natives_dir.to_string_lossy().to_string());
    vars.insert("launcher_name".into(), "QookiX-Launcher".into());
    vars.insert("launcher_version".into(), env!("CARGO_PKG_VERSION").into());
    vars.insert("classpath".into(), ctx.classpath.clone());
    vars.insert("library_directory".into(), ctx.libraries_dir.to_string_lossy().to_string());
    vars.insert("version_name".into(), ctx.instance.id.clone());
    vars.insert("version_type".into(), "release".into());
    vars.insert("assets_root".into(), ctx.assets_dir.to_string_lossy().to_string());
    vars.insert("game_assets".into(), ctx.assets_dir.to_string_lossy().to_string());
    vars.insert("game_directory".into(), ctx.instance_dir.to_string_lossy().to_string());
    vars.insert("auth_player_name".into(), ctx.account.username.clone());
    vars.insert("auth_uuid".into(), ctx.account.uuid.replace('-', ""));
    vars.insert("auth_access_token".into(), ctx.account.access_token.clone());
    vars.insert("user_type".into(), ctx.account.user_type.clone());
    vars.insert("user_properties".into(), ctx.account.user_properties.clone());
    vars.insert("clientid".into(), ctx.settings.ms_client_id.clone());
    vars.insert("auth_xuid".into(), "0".into());
    if let Some((w, h)) = ctx.resolution {
        vars.insert("resolution_width".into(), w.to_string());
        vars.insert("resolution_height".into(), h.to_string());
    }
    if let Some(idx) = &ctx.version_json.asset_index {
        vars.insert("assets_index_name".into(), idx.id.clone());
        vars.insert("assets_root_legacy".into(), ctx.assets_dir.join("virtual").join("legacy").to_string_lossy().to_string());
    }
    for (k, v) in &vars {
        s = s.replace(&format!("${{{k}}}"), v);
    }
    // handle ${path} and ${prompt} leftovers generically
    s = s.replace("${path}", "");
    let _ = features;
    s
}

fn emit_state(app: &tauri::AppHandle, instance_id: &str, state: &str, pid: u32, code: Option<i32>) {
    let _ = app.emit(
        "launch://state",
        serde_json::json!({
            "instanceId": instance_id,
            "state": state,
            "pid": pid,
            "code": code,
        }),
    );
}

// ---------------------------------------------------------------------------
// 崩溃诊断
// ---------------------------------------------------------------------------

#[derive(serde::Serialize, Clone, Default)]
pub struct CrashDiagnosis {
    /// oom | jvm | gl | java_ver | lwjgl | mod | unknown
    pub severity: String,
    /// 诊断结论标题
    pub title: String,
    /// 崩溃原因（中文）
    pub reason: String,
    /// 修复建议（中文，最多 200 字）
    pub advice: String,
    /// 日志/崩溃报告中代表性摘录
    pub excerpt: String,
    /// 游戏进程退出码（非正常退出时非 `0` 或 `null`）
    pub exit_code: Option<i32>,
    /// 触发的崩溃报告文件路径（若有）
    pub crash_report: Option<String>,
    /// PCL2 风格：与本次崩溃相关的模组列表（从崩溃报告解析）
    pub affected_mods: Vec<String>,
}

/// 匹配规则助手：文件全名或路径中包含关键字的全局优先，内容全文匹配
struct CrashRule {
    severity: &'static str,
    title: &'static str,
    reason: &'static str,
    advice: &'static str,
    keys: &'static [&'static str],
}

const CRASH_RULES: &[CrashRule] = &[
    CrashRule {
        severity: "jvm",
        title: "Java 虚拟机崩溃",
        reason: "JVM 发生致命错误（hs_err_pid*.log 已生成）",
        advice: "请检查 Java 运行版本是否匹配，并尝试在实例设置中更换 Java 路径或调低内存分配后重试。",
        keys: &["hs_err_pid", "# there is insufficient memory for the java runtime", "native memory allocation (mmap) failed"],
    },
    CrashRule {
        severity: "oom",
        title: "内存不足",
        reason: "内存分配失败（包含 Java 堆与原生内存）",
        advice: "请进入实例设置调低已分配内存，或关闭其他占用内存的程序后重试。",
        keys: &["outofmemoryerror", "could not reserve enough space", "not enough space", "java heap space", "native memory allocation (mmap) failed", "failed to allocate memory", "insufficient memory"],
    },
    CrashRule {
        severity: "lwjgl",
        title: "LWJGL 依赖缺失",
        reason: "本地库加载失败（可能由 Java/Minecraft 组件缺失引起）",
        advice: "请尝试重装/切换 Java 运行时；或彻底删除后重装该实例。",
        keys: &["no classfound: org/lwjgl", "no lwjgl on java.library.path", "could not initialize class org.lwjgl", "unsatisfiedlinkerror", "failed to locate library"],
    },
    CrashRule {
        severity: "java_ver",
        title: "Java 版本过旧",
        reason: "Java 运行库版本不满足启动要求",
        advice: "请安装较新 JRE 后重试，可在「设置」中选择自动下载 JRE。",
        keys: &["unsupportedclassversionerror", "unsupported major.minor", "class file version", "bad class file", "java version", "has been compiled by a more recent version"],
    },
    CrashRule {
        severity: "gl",
        title: "显卡 / OpenGL 初始化失败",
        reason: "OpenGL / 显卡驱动初始化错误",
        advice: "请检查显卡驱动是否最新、是否满足 Minecraft 对 OpenGL 3.2 的要求，更新驱动后重试。",
        keys: &["pixel format not accelerated", "failed to create gl context", "no matching pixel format", "opengl 32bit", "glfw error", "failed to initialize glfw", "could not create glfw", "probably the driver does not support opengl", "glx genesys"],
    },
    CrashRule {
        severity: "mod",
        title: "模组/资源包加载失败",
        reason: "Fabric/Forge 或某个模组加载时抛出严重异常",
        advice: "请在实例设置中检查并禁用最近安装/更新过的模组后重启游戏，或查阅 Mod 日志确认冲突项。",
        keys: &[
            "mod loading has failed",
            "mod loading was attempted",
            "fatal error during mod loading",
            "there was a severe problem during mod loading",
            "mod launcher failed",
            "mod initializer failed",
            "mixin apply failed",
            "duplicate modifier",
            "mixins are missing dependencies",
            "this is a fatal",
            "incompatible mod",
            "mod crashed",
        ],
    },
];

/// 取目录中最近被修改且文件名匹配 `pred` 的文件
fn newest_match(dir: &std::path::Path, pred: impl Fn(&str) -> bool) -> Option<PathBuf> {
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if !pred(&name) { continue; }
        let meta = entry.metadata().ok()?;
        let t = meta.modified().ok()?;
        best = Some(match best {
            Some((bt, _)) if t > bt => (t, entry.path()),
            _ => (t, entry.path()),
        });
    }
    best.map(|(_, p)| p)
}

/// 日志尾部（长文件可用性优化：只取最后 `n` 行）
fn tail(text: &str, n: usize) -> String {
    text.lines().rev().take(n).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n")
}

/// 崩溃诊断：在游戏进程非正常/被强杀后，结合崩溃现场报告、JVM 致命错误与实时日志粗定位原因。
/// 仅在（report / hs_err / 日志关键词）命中时返回建议。
fn diagnose_crash(instance_dir: &std::path::Path, logs_dir: &std::path::Path, instance_id: &str, code: Option<i32>) -> Option<CrashDiagnosis> {
    use std::fs;

    // ---- 收集崩溃证据 ----
    let crash_dir = instance_dir.join("crash-reports");
    let crash_report = newest_match(&crash_dir, |n| n.starts_with("crash-") && n.ends_with(".txt"));
    let hs_err = newest_match(instance_dir, |n| n.starts_with("hs_err_pid") && n.ends_with(".log"));

    // ---- 日志文本缓冲（用于关键词匹配与摘录） ----
    let mut buf = String::new();
    if let Some(p) = &crash_report {
        if let Ok(t) = fs::read_to_string(p) { buf.push_str(&t); }
    }
    if let Some(p) = &hs_err {
        if let Ok(t) = fs::read_to_string(p) { buf.push_str(&t); }
    }
    let live_log = logs_dir.join(format!("{}-live.log", instance_id));
    if let Ok(t) = fs::read_to_string(&live_log) { buf.push_str(&tail(&t, 200)); }
    let latest_log = instance_dir.join("logs").join("latest.log");
    if let Ok(t) = fs::read_to_string(&latest_log) { buf.push_str(&tail(&t, 300)); }
    let lower = buf.to_lowercase();
    // PCL2 风格：预先解析崩溃报告中涉及的模组列表，供各类诊断使用
    let involved_mods = extract_affected_mods(&buf);

    // 优先摘录崩溃报告中的「Description」行，但忽略 Fabric 无意义的阶段名（如 "Loading: LWJGL system"）。
    let mut excerpt = String::new();
    let mut fabric_note = String::new();
    if let Some(p) = &crash_report {
        if let Ok(t) = fs::read_to_string(p) {
            if let Some(line) = t.lines().find(|l| l.trim_start().starts_with("Description:")) {
                let d = line.trim_start().strip_prefix("Description:").unwrap_or("").trim().replace('\u{a0}', " ");
                // Fabric 崩溃时 Description 常为「Loading library...」这类无用阶段信息
                if !d.is_empty() && !d.to_lowercase().contains("loading library") {
                    excerpt = d.to_string();
                }
            }
            // 摘录异常类型行（优先 Fabric 的 FormattedException、Caused by 链路、主异常类）
            if excerpt.is_empty() {
                for l in t.lines() {
                    let s = l.trim();
                    if s.is_empty() { continue; }
                    if s.starts_with("net.fabricmc") || s.starts_with("java.") || s.starts_with("cpw.mods") {
                        excerpt = s.replace('\u{a0}', " ");
                        break;
                    }
                }
            }
            // Fabric 官方自带解决建议（mod 依赖缺失）
            fabric_note = extract_fabric_solution(&t);
        }
    }

    // Description 描述本身已明确是 Mod 阶段问题时，优先判定为“模组问题”，
    // 避免其堆栈中出现的 lwjgl/opengl/glfw 字样被后续显卡规则误判。
    let desc_lower = excerpt.to_lowercase();
    let mod_desc_hints = [
        "mod loading", "mods loading", "mod launcher", "fabric", "forge", "mixin",
        "duplicate", "mod conflict", "mod crash", "incompatible mod", "shader",
    ];
    if !desc_lower.is_empty() && mod_desc_hints.iter().any(|k| desc_lower.contains(k)) {
        return Some(CrashDiagnosis {
            severity: "mod".into(),
            title: "模组/资源包加载失败".into(),
            reason: "Mod 加载阶段发生异常（依据崩溃报告描述）".into(),
            advice: "请优先检查并禁用最近安装/更新过的模组后重启游戏；多个模组互相冲突时建议逐个启用排查。".into(),
            excerpt: excerpt.clone(),
            exit_code: code,
            crash_report: crash_report.map(|p| p.to_string_lossy().to_string()),
            affected_mods: involved_mods.clone(),
        });
    }

    // Fabric 的“不兼容模组”异常是明确结论（含依赖缺失），其结果优先级高于
    // 后续的 lwjgl/显卡等全文关键词规则，避免误判。
    let fabric_evid = [
        "some of your mods are incompatible",
        "incompatible with the game",
        "formattedexception",
        "net.fabricmc.loader.impl",
        "incompatible mods found",
        "missing required dependency",
        "缺少需要的模组",
        "不兼容的模组",
    ];
    if fabric_evid.iter().any(|k| lower.contains(k)) {
        let missing_dep = lower.contains("missing required dependency")
            || lower.contains("fabric-api")
            || lower.contains("需要 ")
            || lower.contains("缺少需要的");
        let advice = if !fabric_note.is_empty() {
            format!("Mod 加载已被阻止：\n{}", fabric_note)
        } else if !involved_mods.is_empty() {
            format!(
                "以下模组存在依赖或兼容问题：{}。请检查它们的依赖模组（如 fabric-api）是否安装完整。",
                involved_mods.join("、")
            )
        } else if missing_dep {
            "缺少模组依赖（如 fabric-api、fabric-language-kotlin 等），请安装缺失依赖后再启动游戏。".into()
        } else {
            "Mod 与游戏或 Mod 之间存在不兼容，请移除/更新冲突的模组后再试。".into()
        };
        return Some(CrashDiagnosis {
            severity: "mod".into(),
            title: "Mod 不兼容 / 依赖缺失".into(),
            reason: if missing_dep {
                "缺少必需的 Mod 依赖（Fabric 报错：不兼容模组）".into()
            } else {
                "Fabric 检测到 Mod 之间或 Mod 与游戏不兼容".into()
            },
            advice,
            excerpt: excerpt.clone(),
            exit_code: code,
            crash_report: crash_report.map(|p| p.to_string_lossy().to_string()),
            affected_mods: involved_mods.clone(),
        });
    }

    // 关键词规则逐一匹配，非“unknown”规则命中后即可得出结论
    for rule in CRASH_RULES {
        if rule.keys.iter().any(|k| lower.contains(k)) {
            return Some(CrashDiagnosis {
                severity: rule.severity.into(),
                title: rule.title.into(),
                reason: rule.reason.into(),
                advice: rule.advice.into(),
                excerpt: excerpt.clone(),
                exit_code: code,
                crash_report: crash_report.map(|p| p.to_string_lossy().to_string()),
                affected_mods: if involved_mods.is_empty() { Vec::new() } else { involved_mods.clone() },
            });
        }
    }

    // ---- 找不到规则但存在崩溃产物：归类为“未知崩溃” ----
    if crash_report.is_some() || hs_err.is_some() {
        // guidelines instruct: use generic unknown
        return Some(CrashDiagnosis {
            severity: "unknown".into(),
            title: "游戏异常退出".into(),
            reason: "未能定位到具体的崩溃原因".into(),
            advice: "请将崩溃报告（crash-reports 或 hs_err 日志）提交到启动器仓库/社区，以便进一步分析。".into(),
            excerpt: String::new(),
            exit_code: code,
            crash_report: crash_report.map(|p| p.to_string_lossy().to_string()),
            affected_mods: involved_mods.clone(),
        });
    }

    // ---- 一切正常（退出码 0 且无匹配关键信息） ----
    None
}

/// 提取 Fabric 崩溃报告中的官方解决方案片段（如缺失模组依赖提示）。
fn extract_fabric_solution(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut start: Option<usize> = None;
    for (i, l) in lines.iter().enumerate() {
        if l.contains("确定了一种可能的解决方法")
            || l.contains("a possible solution")
            || l.contains("This may fix")
        {
            start = Some(i);
            break;
        }
    }
    let Some(si) = start else { return String::new(); };
    let mut out = String::new();
    for l in lines.iter().skip(si) {
        let s = l.trim();
        if s.is_empty() { break; }
        if s.starts_with("at net.fabricmc") || s.starts_with("at java.") || s.contains("更多信息") {
            break;
        }
        out.push_str(s.trim_start_matches('-').trim());
        out.push('\n');
    }
    out.trim().to_string()
}

/// PCL2 风格：从崩溃报告中解析与本次崩溃相关的模组列表。
/// 优先读 Fabric/Forge 的报告「Mods affected」区块；否则从 `更多信息/More info`
/// 的依赖提示行（`模组 'xxx' (id)` / `Mod 'xxx' (id)`）提取。
fn extract_affected_mods(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut in_block = false;
    for raw in text.lines() {
        let s = raw.trim();
        // -- Mods affected 区块 --
        if s.eq_ignore_ascii_case("Mods affected:")
            || s.starts_with("受影响")
            || (s.contains("Mods affected") && s.contains(':'))
            || s.starts_with("--Mods affected")
        {
            in_block = true;
            continue;
        }
        if in_block {
            if s.is_empty() || s.starts_with("Stacktrace") || s.starts_with("Time:") {
                break;
            }
            let name = s.trim_start_matches(['-', '•', '*']).trim();
            if name.is_empty() || name.starts_with("at ") || name.starts_with("at ") {
                continue;
            }
            // 形如 `appleskin (AppleSkin)` / `sodium (Sodium)` / `minecraft (Minecraft) 1.20.1`
            let pretty = name
                .split_once(" — ")
                .map(|(a, _)| a.trim())
                .unwrap_or(name)
                .split_once(" (")
                .map(|(a, _)| a.trim())
                .unwrap_or(name);
            if pretty.is_empty() || !out.iter().any(|x| x == &pretty) {
                if !pretty.is_empty() {
                    out.push(pretty.to_string());
                }
            }
            continue;
        }
        // More info 依赖提示行：`模组 'AppleSkin' (appleskin) 需要 ...` 或 `Mod 'AppleSkin' (appleskin)`
        if s.starts_with("模组 '") {
            if let Some(rest) = s.strip_prefix("模组 '") {
                if let Some((name, _)) = rest.split_once("'") {
                    let n = name.trim().to_string();
                    if !n.is_empty() && !out.iter().any(|x| x == &n) {
                        out.push(n);
                    }
                }
            }
        }
        if s.starts_with("Mod '") {
            if let Some(rest) = s.strip_prefix("Mod '") {
                if let Some((name, _)) = rest.split_once("'") {
                    let n = name.trim().to_string();
                    if !n.is_empty() && !out.iter().any(|x| x == &n) {
                        out.push(n);
                    }
                }
            }
        }
    }
    out.truncate(8);
    out
}
