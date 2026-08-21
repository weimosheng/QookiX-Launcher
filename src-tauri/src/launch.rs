use crate::models::*;
use crate::state::AppState;
use crate::util::rules_allow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use tauri::Emitter;

#[derive(Clone)]
pub struct ResolvedAccount {
    pub username: String,
    pub uuid: String,
    pub access_token: String,
    pub user_type: String,
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
}

/// Launch a game instance. Emits `launch://log`, `launch://state` events.
/// `world` (optional) directly joins a singleplayer world via quick play.
pub async fn launch_game(
    app: tauri::AppHandle,
    state: &AppState,
    instance: &Instance,
    account: ResolvedAccount,
    world: Option<String>,
) -> Result<LaunchResult, String> {
    let mut settings = state.settings.read().unwrap().clone();
    // Resolve effective Microsoft Client ID (built-in or user-configured)
    if let Ok(id) = crate::accounts::effective_ms_client_id(state) {
        settings.ms_client_id = id;
    }
    let version_path = state.versions_dir().join(&instance.id).join(format!("{}.json", instance.id));
    if !version_path.exists() {
        return Err("游戏尚未安装，请先点击「安装游戏」".into());
    }
    let text = std::fs::read_to_string(&version_path).map_err(|e| e.to_string())?;
    let mut version_json: VersionJson =
        serde_json::from_str(&text).map_err(|e| format!("版本元数据损坏: {e}"))?;
    // ensure natives-related JVM args point at the natives dir itself
    // (idempotent; also covers instances patched before this fix)
    crate::install::normalize_natives_args(&mut version_json);

    let java = match pick_java(state, instance, &version_json).await {
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
            pick_java(state, instance, &version_json)
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
    set_chinese_lang(&instance_dir);

    // ---- classpath ----
    let features = features_map(resolution.is_some());
    let mut classpath: Vec<String> = Vec::new();
    for lib in &version_json.libraries {
        if !rules_allow(lib.rules.as_deref().unwrap_or(&[]), &features) {
            continue;
        }
        // NOTE: natives jars stay on the classpath — LWJGL 3.4 loads shared
        // libraries from the classpath (`LibraryResource` + SharedLibraryLoader),
        // which is how modern versions (26.x) provide lwjgl.dll etc.
        if let Some(dl) = lib.downloads.as_ref().and_then(|d| d.artifact.as_ref()) {
            if let Some(p) = &dl.path {
                classpath.push(crate::paths::libraries_dir(state, &instance.id).join(p).to_string_lossy().to_string());
            } else if let Some(rel) = crate::models::maven_to_path(&lib.name) {
                classpath.push(crate::paths::libraries_dir(state, &instance.id).join(rel).to_string_lossy().to_string());
            }
        } else if let Some(rel) = crate::models::maven_to_path(&lib.name) {
            classpath.push(crate::paths::libraries_dir(state, &instance.id).join(rel).to_string_lossy().to_string());
        }
    }
    let client_jar = state.versions_dir().join(&instance.id).join(format!("{}.jar", instance.id));
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
        assets_dir: crate::paths::assets_dir(state, &instance.id),
        libraries_dir: crate::paths::libraries_dir(state, &instance.id),
        version_dir: state.versions_dir().join(&instance.id),
        account,
        resolution,
        world,
    };

    let args = build_args(&ctx);
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
    let _ = std::fs::write(&log_path, format!("$ java {}\n", args.join(" ")));

    let child = tokio::process::Command::from(cmd)
        .spawn()
        .map_err(|e| format!("启动 Java 失败: {e}"))?;
    let pid = child.id().unwrap_or(0);
    emit_state(&app, &instance.id, "running", pid, None);
    {
        let mut guard = state.game_process.lock().unwrap();
        *guard = Some(child);
    }
    {
        let mut guard = state.running_instance.lock().unwrap();
        *guard = Some(instance.id.clone());
    }
    let _ = app.emit(
        "launch://pid",
        serde_json::json!({ "instanceId": instance.id, "pid": pid, "logPath": log_path.to_string_lossy() }),
    );

    // stream stdout/stderr in the background
    let app2 = app.clone();
    let inst_id = instance.id.clone();
    let gp = state.game_process.clone();
    let ri = state.running_instance.clone();
    let logs_dir = state.logs_dir();
    tauri::async_runtime::spawn(async move {
        let outcome = stream_output(&app2, gp, logs_dir, inst_id.clone()).await;
        emit_state(&app2, &inst_id, "exited", pid, outcome);
        let _ = app2.emit(
            "launch://exit",
            serde_json::json!({ "instanceId": inst_id, "code": outcome }),
        );
        {
            let mut guard = ri.lock().unwrap();
            *guard = None;
        }
    });

    Ok(LaunchResult {
        pid,
        command: args,
    })
}

async fn stream_output(
    app: &tauri::AppHandle,
    game_process: Arc<Mutex<Option<tokio::process::Child>>>,
    logs_dir: PathBuf,
    instance_id: String,
) -> Option<i32> {
    // Take the child out of state to own its pipes
    let child = {
        let mut guard = game_process.lock().unwrap();
        guard.take()
    };
    let Some(mut child) = child else {
        return None;
    };

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
    // restore for potential re-take (kill)
    {
        let mut guard = game_process.lock().unwrap();
        *guard = Some(child);
    }
    status.map(|s| s.code().unwrap_or(-1))
}

pub async fn kill_game(state: &AppState) -> Result<(), String> {
    let child = {
        let mut guard = state.game_process.lock().unwrap();
        guard.take()
    };
    if let Some(mut child) = child {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    Ok(())
}

pub fn is_running(state: &AppState) -> bool {
    state.running_instance.lock().unwrap().is_some()
}

// ---------------------------------------------------------------------------
// Java selection
// ---------------------------------------------------------------------------

/// Required Java major version for an instance: from the version JSON, with the
/// client jar's class-file version as an authoritative fallback.
pub fn required_java_for(state: &AppState, instance: &Instance) -> u32 {
    let mut required = 8u32;
    let version_path = state.versions_dir().join(&instance.id).join(format!("{}.json", instance.id));
    if let Ok(text) = std::fs::read_to_string(&version_path) {
        if let Ok(vj) = serde_json::from_str::<VersionJson>(&text) {
            if let Some(j) = vj.java_version {
                required = j.major_version;
            }
        }
    }
    let jar = state.versions_dir().join(&instance.id).join(format!("{}.jar", instance.id));
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
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    {
        if let Some((ts, list)) = state.java_cache.lock().unwrap().as_ref() {
            if now.saturating_sub(*ts) < 300 {
                return list.clone();
            }
        }
    }
    let runtimes = state.root.join("runtimes");
    let detected = tokio::task::spawn_blocking(move || {
        crate::java::detect_java(None, Some(&runtimes))
    })
    .await
    .unwrap_or_default();
    *state.java_cache.lock().unwrap() = Some((now, detected.clone()));
    detected
}

/// Best available Java for the required major: exact match preferred, else highest.
pub async fn find_best_java(state: &AppState, required: u32) -> Option<JavaInfo> {
    let detected = get_detected_java(state).await;
    detected
        .iter()
        .find(|j| j.major == required)
        .or_else(|| detected.iter().max_by_key(|j| j.major))
        .cloned()
}

async fn pick_java(
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
    let jar = state.versions_dir().join(&instance.id).join(format!("{}.jar", instance.id));
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
    if detected.is_empty() {
        return Err(format!("NEED_DOWNLOAD:{required}"));
    }
    let exact = detected.iter().find(|j| j.major == required);
    if let Some(j) = exact {
        return Ok(j.clone());
    }
    let best = detected.iter().max_by_key(|j| j.major).unwrap();
    if best.major < required {
        return Err(format!("NEED_DOWNLOAD:{required}"));
    }
    Ok(best.clone())
}

/// Set `lang:zh_CN` in the instance's `options.txt` so the game launches in Chinese.
fn set_chinese_lang(instance_dir: &std::path::Path) {
    let options_path = instance_dir.join("options.txt");
    let target_line = "lang:zh_CN";
    if let Ok(text) = std::fs::read_to_string(&options_path) {
        let mut found = false;
        let updated: String = text
            .lines()
            .map(|line| {
                if line.starts_with("lang:") {
                    found = true;
                    target_line.to_string()
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let result = if found {
            updated
        } else {
            format!("{updated}\n{target_line}")
        };
        let _ = std::fs::write(&options_path, result);
    } else {
        let _ = std::fs::write(&options_path, target_line);
    }
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

fn build_args(ctx: &LaunchContext) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    let settings = &ctx.settings;
    let instance = &ctx.instance;

    // memory
    let max_mem = match instance.memory_mode.as_deref() {
        Some("auto") => {
            crate::settings::total_memory_mb()
                .map(|t| crate::settings::recommended_memory(t).0 as u32)
                .unwrap_or(0)
        }
        _ => 0,
    };
    let max_mem = if max_mem > 0 {
        max_mem
    } else {
        instance.max_memory_mb.or(Some(settings.max_memory_mb)).unwrap_or(2048).max(256)
    };
    let min_mem = settings.min_memory_mb.max(64);
    let (min_mem, _) = if instance.memory_mode.as_deref() == Some("auto") {
        crate::settings::total_memory_mb()
            .map(|t| crate::settings::recommended_memory(t))
            .map(|(_, m)| (m, 0))
            .unwrap_or((min_mem, 0))
    } else {
        (min_mem, 0)
    };
    args.push(format!("-Xmx{max_mem}M"));
    args.push(format!("-Xms{min_mem}M"));

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

    args.extend(game_args);
    args
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
    vars.insert("user_properties".into(), "{}".into());
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
