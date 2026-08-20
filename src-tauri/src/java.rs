use crate::models::JavaInfo;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Detect Java installations on this machine, including any runtimes the
/// launcher itself downloaded under `runtime_root`.
///
/// Sources:
///  - user-selected path from settings
///  - `JAVA_HOME` / `JRE_HOME` env
///  - `java` / `javaw` on PATH
///  - Windows registry (HKLM/HKCU `Software\JavaSoft\...` and Adoptium)
///  - common install directories
///  - launcher-managed runtimes under `<root>/runtimes/java`
pub fn detect_java(custom: Option<&str>, runtime_root: Option<&Path>) -> Vec<JavaInfo> {
    let mut paths: Vec<PathBuf> = Vec::new();

    if let Some(c) = custom {
        if !c.trim().is_empty() {
            paths.push(PathBuf::from(c.trim()));
        }
    }
    for var in ["JAVA_HOME", "JRE_HOME", "JDK_HOME"] {
        if let Ok(v) = std::env::var(var) {
            if !v.is_empty() {
                paths.push(PathBuf::from(v).join("bin").join(java_exe()));
            }
        }
    }
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let cand = dir.join(java_exe());
            if cand.is_file() {
                paths.push(cand);
            }
        }
    }
    paths.extend(registry_java_paths());
    paths.extend(fs_scan_paths());
    paths.extend(runtime_java_paths(runtime_root));

    // de-dupe preserving order (also collapse java.exe / javaw.exe pairs
    // from the same bin directory)
    let mut seen = std::collections::HashSet::new();
    let mut seen_dirs = std::collections::HashSet::new();
    let mut out: Vec<JavaInfo> = Vec::new();
    for p in paths {
        let canon = p.canonicalize().unwrap_or_else(|_| p.clone());
        let key = canon.to_string_lossy().to_lowercase();
        if !seen.insert(key) {
            continue;
        }
        let dir = canon.parent().unwrap_or(&canon).to_string_lossy().to_lowercase();
        if !seen_dirs.insert(dir) {
            continue;
        }
        if let Some(info) = probe_java(&canon) {
            out.push(info);
        }
    }
    out
}

/// Find `bin/javaw.exe` (or `java`) under `<root>/runtimes/java/**`.
fn runtime_java_paths(root: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Some(base) = root.map(|r| r.join("runtimes").join("java")) else {
        return out;
    };
    if !base.is_dir() {
        return out;
    }
    fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
        if depth > 4 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                let bin = p.join("bin");
                let exe = bin.join(java_exe());
                if exe.is_file() {
                    out.push(exe);
                } else {
                    walk(&p, out, depth + 1);
                }
            }
        }
    }
    walk(&base, &mut out, 0);
    out
}

/// Download (if needed) a JRE of the given major version from Adoptium,
/// extract it under `runtimes/java/<major>/`, and return its JavaInfo.
pub async fn download_java_runtime(
    app: tauri::AppHandle,
    state: &crate::state::AppState,
    major: u32,
) -> Result<JavaInfo, String> {
    let api_url = format!(
        "https://api.adoptium.net/v3/assets/latest/{major}/hotspot?os=windows&architecture=x64&image_type=jre"
    );
    let body: serde_json::Value = crate::download::get_json(&state.client, &api_url).await?;
    let arr = body.as_array().ok_or("Adoptium 响应格式错误")?;
    let first = arr.first().ok_or(format!("没有可用的 Java {major} JRE"))?;
    let pkg = first
        .get("binary")
        .and_then(|b| b.get("package"))
        .ok_or("缺少包信息")?;
    let url = pkg.get("link").and_then(|l| l.as_str()).ok_or("缺少下载链接")?;
    let name = pkg.get("name").and_then(|n| n.as_str()).unwrap_or("runtime.zip");
    let size = pkg.get("size").and_then(|s| s.as_u64()).unwrap_or(0);

    let dl_dir = state.root.join("runtimes").join("downloads");
    std::fs::create_dir_all(&dl_dir).map_err(|e| e.to_string())?;
    let zip_path = dl_dir.join(name);

    if !zip_path.exists() {
        let task_id = state.next_task_id();
        let source = format!("Java 运行时 {major}");
        crate::install::emit_progress(
            &app,
            task_id,
            "runtime",
            &format!("正在下载 Java {major} JRE（{name}）…"),
            0,
            1,
            &instance_placeholder(state),
            &source,
        );
        let items = vec![crate::download::DownloadItem {
            url: url.to_string(),
            dest: zip_path.clone(),
            sha1: None,
            size: if size > 0 { Some(size) } else { None },
            label: name.to_string(),
        }];
        crate::download::download_many(app.clone(), state, task_id, "runtime", items).await?;
    }

    // extract into runtimes/java/<major>/ (zip contains one top-level folder)
    let dest = state.root.join("runtimes").join("java").join(major.to_string());
    if dest.exists() {
        let _ = std::fs::remove_dir_all(&dest);
    }
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    crate::util::extract_zip(&zip_path, &dest, &["META-INF/"])
        .map_err(|e| format!("解压 Java 运行时失败: {e}"))?;

    // locate the executable
    let found = detect_java(None, Some(&state.root.join("runtimes")))
        .into_iter()
        .find(|j| j.major == major)
        .ok_or(format!("Java {major} 下载完成但未找到可执行文件"))?;
    Ok(found)
}

/// Minimal instance used only for progress events of runtime downloads.
fn instance_placeholder(state: &crate::state::AppState) -> crate::models::Instance {
    crate::models::Instance {
        id: "runtime".into(),
        name: "Java 运行时".into(),
        mc_version: String::new(),
        loader: crate::models::LoaderType::Vanilla,
        loader_version: None,
        created: 0,
        last_played: None,
        installed: false,
        icon: None,
        max_memory_mb: None,
        jvm_args: None,
        game_args: None,
        java_path: None,
        account_id: None,
        resolution: None,
        mods: Vec::new(),
        resource_packs: Vec::new(),
        shaders: Vec::new(),
    }
}

pub fn java_exe() -> &'static str {
    if cfg!(windows) {
        "javaw.exe"
    } else {
        "java"
    }
}

/// Run `java -version` and parse the first line, e.g.
/// `openjdk version "21.0.2" 2024-01-16` or `java version "1.8.0_392"`.
pub fn probe_java(path: &Path) -> Option<JavaInfo> {
    let output = Command::new(path).arg("-version").output().ok()?;
    let text = String::from_utf8_lossy(&output.stderr).to_string();
    let first = text.lines().next().unwrap_or("").to_string();

    let mut version = String::new();
    let mut major: u32 = 0;
    let mut vendor = String::new();

    if let Some(idx) = first.find("version \"") {
        let rest = &first[idx + 9..];
        let v = rest.split('"').next().unwrap_or("");
        version = v.to_string();
        if let Some(dot) = v.find('.') {
            // 1.8.0_392 -> 8 ; 21.0.2 -> 21
            let head = &v[..dot];
            if head == "1" {
                let after = &v[dot + 1..];
                if let Some(dot2) = after.find('.') {
                    major = after[..dot2].parse().unwrap_or(0);
                } else {
                    major = after.parse().unwrap_or(0);
                }
            } else {
                major = head.parse().unwrap_or(0);
            }
        } else {
            major = v.parse().unwrap_or(0);
        }
    }
    for line in text.lines().skip(1).take(2) {
        let l = line.trim();
        if l.contains("OpenJDK") || l.contains("Eclipse") || l.contains("Oracle") || l.contains("Microsoft") || l.contains("Temurin") || l.contains("Zulu") {
            vendor = l.to_string();
            break;
        }
    }
    if vendor.is_empty() {
        let p = path.to_string_lossy().to_lowercase();
        if p.contains("temurin") || p.contains("adoptium") {
            vendor = "Eclipse Temurin".into();
        } else if p.contains("zulu") || p.contains("azul") {
            vendor = "Azul Zulu".into();
        } else if p.contains("microsoft") || p.contains("openjdk") {
            vendor = "Microsoft OpenJDK".into();
        } else if p.contains("oracle") {
            vendor = "Oracle".into();
        } else {
            vendor = "Unknown".into();
        }
    }
    let arch = probe_arch(path);
    if version.is_empty() {
        return None;
    }
    Some(JavaInfo {
        path: clean_path(path),
        version,
        major,
        vendor,
        arch,
    })
}

/// Strip the Windows verbatim prefix (`\\?\`) from canonicalized paths.
fn clean_path(path: &Path) -> String {
    let s = path.to_string_lossy().to_string();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s
    }
}

fn probe_arch(path: &Path) -> String {
    let output = Command::new(path)
        .args(["-XshowSettings:properties", "-version"])
        .output();
    if let Ok(o) = output {
        let text = String::from_utf8_lossy(&o.stderr).to_string();
        for line in text.lines() {
            let t = line.trim();
            if let Some(v) = t.strip_prefix("os.arch =") {
                return v.trim().to_string();
            }
        }
    }
    if cfg!(target_arch = "aarch64") {
        "aarch64".into()
    } else {
        "x86_64".into()
    }
}

/// Read Java install paths from the Windows registry.
///
/// Mirrors what PrismLauncher / PCL search: JavaSoft (old + new key names),
/// AdoptOpenJDK, Eclipse Foundation, Eclipse Adoptium, Semeru, Microsoft JDK,
/// Azul Zulu and BellSoft Liberica, in both HKLM/HKCU and 32/64-bit views.
#[cfg(windows)]
fn registry_java_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let roots = [
        "SOFTWARE\\JavaSoft\\Java Runtime Environment",
        "SOFTWARE\\JavaSoft\\Java Development Kit",
        "SOFTWARE\\JavaSoft\\JRE",
        "SOFTWARE\\JavaSoft\\JDK",
        "SOFTWARE\\AdoptOpenJDK",
        "SOFTWARE\\Eclipse Foundation",
        "SOFTWARE\\Eclipse Adoptium",
        "SOFTWARE\\Semeru",
        "SOFTWARE\\Microsoft\\JDK",
        "SOFTWARE\\Microsoft\\JavaSoft",
        "SOFTWARE\\Azul Systems",
        "SOFTWARE\\BellSoft",
        "SOFTWARE\\Amazon Corretto",
    ];
    for hive in ["HKLM", "HKCU"] {
        for root in roots {
            let key = format!("{hive}\\{root}");
            for reg_view in ["", "/reg:32"] {
                let mut args = vec!["query", &key, "/s"];
                if !reg_view.is_empty() {
                    args.push(reg_view);
                }
                if let Ok(o) = Command::new("reg").args(&args).output() {
                    let text = String::from_utf8_lossy(&o.stdout).to_string();
                    for line in text.lines() {
                        let t = line.trim();
                        // value lines look like:  JavaHome  REG_SZ  C:\...
                        let parts: Vec<&str> = t.split_whitespace().collect();
                        if parts.len() >= 3
                            && (parts[1] == "REG_SZ" || parts[1] == "REG_EXPAND_SZ")
                            && matches!(parts[0], "JavaHome" | "InstallPath" | "Path" | "Home")
                        {
                            let mut v = parts[2..].join(" ");
                            v = v.trim_matches('"').to_string();
                            if !v.is_empty() {
                                out.push(PathBuf::from(v).join("bin").join(java_exe()));
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

#[cfg(not(windows))]
fn registry_java_paths() -> Vec<PathBuf> {
    Vec::new()
}

/// Deep filesystem search for `javaw.exe` / `java.exe` in every place Java
/// installs can hide: Program Files (recursively), IDE-bundled JBRs, Oracle
/// javapath, user JDK dirs (~/.jdks, ~/.gradle/jdks, sdkman), the official
/// launcher's runtimes and launcher-managed runtimes.
fn fs_scan_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok();
    let appdata = std::env::var("APPDATA").ok();
    let local = std::env::var("LOCALAPPDATA").ok();
    let program_files = std::env::var("ProgramFiles").ok();
    let program_files_x86 = std::env::var("ProgramFiles(x86)").ok();

    let mut bases: Vec<PathBuf> = Vec::new();
    if let Some(p) = &program_files {
        bases.push(PathBuf::from(p));
    }
    if let Some(p) = &program_files_x86 {
        bases.push(PathBuf::from(p));
    }
    if let Some(p) = &local {
        bases.push(PathBuf::from(p).join("Programs"));
        bases.push(PathBuf::from(p).join("JetBrains")); // Toolbox JBRs
    }
    if let Some(p) = &program_files {
        // Oracle auto-update entry point
        bases.push(PathBuf::from(p).join("Common Files").join("Oracle").join("Java").join("javapath"));
    }
    if let Some(h) = &home {
        bases.push(PathBuf::from(h).join(".jdks"));
        bases.push(PathBuf::from(h).join(".gradle").join("jdks"));
        bases.push(PathBuf::from(h).join(".sdkman").join("candidates").join("java"));
        bases.push(PathBuf::from(h).join(".minecraft").join("runtime"));
    }
    if let Some(a) = &appdata {
        bases.push(PathBuf::from(a).join(".minecraft").join("runtime"));
    }

    // deny-list of huge unrelated dirs inside Program Files scans
    let deny = [
        "windows kits",
        "dotnet",
        "microsoft visual studio",
        "windowsapps",
        "common files",
        "reference assemblies",
        "nuget",
        "windows defender",
        "internet explorer",
        "windows nt",
        "windows multimedia platform",
        "microsoft office",
        "windows security",
        "windows portable devices",
        "windows sidebar",
    ];

    for base in bases {
        if !base.exists() {
            continue;
        }
        scan_java_exe(&base, &mut out, 0, &deny);
    }
    out
}

fn scan_java_exe(dir: &std::path::Path, out: &mut Vec<PathBuf>, depth: usize, deny: &[&str]) {
    if depth > 5 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let Ok(md) = e.metadata() else { continue };
        if md.is_dir() {
            let name = e.file_name().to_string_lossy().to_lowercase();
            if deny.contains(&name.as_str()) {
                continue;
            }
            scan_java_exe(&p, out, depth + 1, deny);
        } else {
            let n = e.file_name().to_string_lossy().to_lowercase();
            if n == "javaw.exe" || n == "java.exe" {
                out.push(p);
            }
        }
    }
}
