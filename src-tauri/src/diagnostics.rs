//! 诊断中心：一键收集运行环境与实例状态，生成可分享的诊断报告。
//!
//! 采集内容：
//!   - 系统：OS / 架构 / 内存 / 数据目录可用空间 / 启动器版本
//!   - Java：检测到的全部 JDK + 目标实例需要的版本与是否满足
//!   - GPU：显卡型号与驱动（Windows 读注册表，其它平台不采集）
//!   - 网络：镜像与代理配置、各上游（Mojang / Modrinth / CurseForge / CDN）延迟
//!   - 实例自检：版本 json / client jar / natives / 模组数量
//!   - 崩溃：最近几份 crash-report 的诊断摘要
//!   - 日志：最近一次启动日志尾部 + 安装追踪日志
//!
//! 报告生成前会做**脱敏**：系统用户名、账号 UUID、access token、服务器地址等。

use crate::state::AppState;
use serde::Serialize;
use std::path::Path;
use std::time::Duration;

/// 采集超时：单个网络探测最长等待（报告整体控制在 10 秒内）
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// 日志尾部保留行数
const LOG_TAIL_LINES: usize = 120;

#[derive(Serialize, Clone, Debug)]
pub struct DiagnosticSection {
    pub key: String,
    pub title: String,
    /// 每行一条 `label: value`（前端按原样展示）
    pub lines: Vec<String>,
    /// 该节的长文本（日志等），前端折叠展示
    #[serde(default)]
    pub block: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct DiagnosticReport {
    pub generated_at: u64,
    pub sections: Vec<DiagnosticSection>,
    /// 完整的 Markdown 报告（复制 / 另存用）
    pub markdown: String,
    /// 脱敏替换次数（提示用户报告已脱敏）
    pub redactions: usize,
}

// ---------------------------------------------------------------------------
// 脱敏
// ---------------------------------------------------------------------------

/// 报告脱敏：替换系统用户名、UUID、token、服务器地址。
/// 返回 (清洗后的文本, 替换次数)。
pub fn redact(text: &str) -> (String, usize) {
    let mut count = 0usize;
    let mut out = text.to_string();

    // 1) 用户目录中的用户名：C:\Users\<name>\  /  /home/<name>/  /  /Users/<name>/
    for (re_src, _) in [
        (r"(?i)([A-Z]:\\Users\\)([^\\\r\n]+)", ""),
        (r"(?i)(/home/)([^/\r\n]+)", ""),
        (r"(?i)(/Users/)([^/\r\n]+)", ""),
    ] {
        if let Ok(re) = regex::Regex::new(re_src) {
            let before = out.clone();
            out = re.replace_all(&out, "${1}<user>").to_string();
            if out != before {
                count += 1;
            }
        }
    }

    // 2) UUID（账号 / 实例标识）
    if let Ok(re) = regex::Regex::new(
        r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}",
    ) {
        let before = out.clone();
        out = re.replace_all(&out, "<uuid>").to_string();
        if out != before {
            count += 1;
        }
    }

    // 3) 访问令牌（命令行 / 日志中的 --accessToken xxx）
    if let Ok(re) = regex::Regex::new(r"(?i)(--accessToken[= ]+)(\S+)") {
        let before = out.clone();
        out = re.replace_all(&out, "${1}<token>").to_string();
        if out != before {
            count += 1;
        }
    }
    if let Ok(re) = regex::Regex::new(r#"(?i)"(accessToken|msa_access_token|refresh_token)"\s*:\s*"[^"]*""#) {
        let before = out.clone();
        out = re.replace_all(&out, r#""$1": "<token>""#).to_string();
        if out != before {
            count += 1;
        }
    }

    (out, count)
}

// ---------------------------------------------------------------------------
// 采集
// ---------------------------------------------------------------------------

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 读取文件尾部 N 行（大日志只取尾部，避免报告膨胀）
fn tail_lines(path: &Path, n: usize) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(n);
    let tail = lines[start..].join("\n");
    let truncated = start > 0;
    let mut out = String::new();
    if truncated {
        out.push_str(&format!("…（仅显示最后 {n} 行，共 {} 行）\n", lines.len()));
    }
    out.push_str(&tail);
    Some(out)
}

fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    format!("{:.1} {}", v, UNITS[i])
}

/// 系统信息（OS / 内存 / 磁盘）
fn collect_system(state: &AppState) -> DiagnosticSection {
    let mut s = sysinfo::System::new();
    s.refresh_memory();

    let mut lines = vec![
        format!("启动器版本: {}", env!("CARGO_PKG_VERSION")),
        format!("操作系统: {} {}", std::env::consts::OS, std::env::consts::ARCH),
        format!(
            "内存: 已用 {} / 共 {}",
            human_bytes(s.used_memory()),
            human_bytes(s.total_memory())
        ),
        format!("数据目录: {}", state.root.display()),
    ];

    // 数据目录所在盘的可用空间
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let root = &state.root;
    let disk = disks
        .list()
        .iter()
        .filter(|d| root.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len());
    if let Some(d) = disk {
        lines.push(format!(
            "数据目录磁盘可用: {} / {}（{}）",
            human_bytes(d.available_space()),
            human_bytes(d.total_space()),
            d.mount_point().display()
        ));
    }

    DiagnosticSection {
        key: "system".into(),
        title: "系统环境".into(),
        lines,
        block: None,
    }
}

/// 显卡信息（Windows 读注册表：显卡类设备的 DriverDesc / DriverVersion）
#[cfg(target_os = "windows")]
fn collect_gpu() -> DiagnosticSection {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ,
        REG_SZ,
    };

    unsafe fn read_value(hkey: HKEY, name: &str) -> Option<String> {
        let name_w: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let mut ty = 0u32;
        let mut size = 0u32;
        // 第一次调用拿长度
        let rc = RegQueryValueExW(
            hkey,
            name_w.as_ptr(),
            std::ptr::null(),
            &mut ty,
            std::ptr::null_mut(),
            &mut size,
        );
        if rc != 0 || size == 0 || ty != REG_SZ {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        let rc = RegQueryValueExW(
            hkey,
            name_w.as_ptr(),
            std::ptr::null(),
            &mut ty,
            buf.as_mut_ptr(),
            &mut size,
        );
        if rc != 0 {
            return None;
        }
        let wide: Vec<u16> = buf
            .chunks_exact(2)
            .map(|c| u16::from_ne_bytes([c[0], c[1]]))
            .take_while(|c| *c != 0)
            .collect();
        Some(OsString::from_wide(&wide).to_string_lossy().to_string())
    }

    unsafe fn list_subkeys(hkey: HKEY) -> Vec<String> {
        let mut out = Vec::new();
        let mut idx = 0u32;
        loop {
            let mut name = [0u16; 256];
            let mut len = name.len() as u32;
            let rc = RegEnumKeyExW(
                hkey,
                idx,
                name.as_mut_ptr(),
                &mut len,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            if rc != 0 {
                break;
            }
            out.push(String::from_utf16_lossy(&name[..len as usize]));
            idx += 1;
        }
        out
    }

    let mut lines: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    unsafe {
        let path: Vec<u16> = "SYSTEM\\CurrentControlSet\\Control\\Class\\{4d36e968-e325-11ce-bfc1-08002be10318}"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut hkey: HKEY = std::ptr::null_mut();
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, path.as_ptr(), 0, KEY_READ, &mut hkey) == 0 {
            for sub in list_subkeys(hkey) {
                // 只处理 0000/0001/... 形式的实例键
                if !sub.chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }
                let sub_w: Vec<u16> = sub.encode_utf16().chain(std::iter::once(0)).collect();
                let mut child: HKEY = std::ptr::null_mut();
                if RegOpenKeyExW(hkey, sub_w.as_ptr(), 0, KEY_READ, &mut child) != 0 {
                    continue;
                }
                let desc = read_value(child, "DriverDesc");
                let ver = read_value(child, "DriverVersion");
                let date = read_value(child, "DriverDate");
                RegCloseKey(child);
                if let Some(d) = desc {
                    // 同一显卡可能有多个实例键（多显示器/多输出），按型号+驱动去重
                    let dedup_key = format!("{d}|{}", ver.clone().unwrap_or_default());
                    if !seen.contains(&dedup_key) {
                        seen.push(dedup_key);
                        let mut line = format!("显卡: {d}");
                        if let Some(v) = ver {
                            line.push_str(&format!("（驱动 {v}"));
                            if let Some(dt) = date {
                                line.push_str(&format!(", {dt}"));
                            }
                            line.push('）');
                        }
                        // 远程控制/串流软件会装虚拟显示适配器，标出来避免误判为真实显卡
                        let lower = d.to_lowercase();
                        if ["virtual", "todesk", "oray", "gameviewer", "parsec", "sunlogin", "anydesk", "teamviewer", "idd"]
                            .iter()
                            .any(|k| lower.contains(k))
                        {
                            line.push_str("（疑似虚拟显示器）");
                        }
                        lines.push(line);
                    }
                }
            }
            RegCloseKey(hkey);
        }
    }
    if lines.is_empty() {
        lines.push("显卡: 未能读取（注册表无可用信息）".into());
    }
    DiagnosticSection {
        key: "gpu".into(),
        title: "显卡".into(),
        lines,
        block: None,
    }
}

#[cfg(not(target_os = "windows"))]
fn collect_gpu() -> DiagnosticSection {
    DiagnosticSection {
        key: "gpu".into(),
        title: "显卡".into(),
        lines: vec!["显卡信息采集仅支持 Windows".into()],
        block: None,
    }
}

/// Java 环境：全部候选 JDK + 目标实例需要的版本与是否满足
fn collect_java(state: &AppState, instance: Option<&crate::models::Instance>) -> DiagnosticSection {
    let now = now_secs();
    let (.., list) = crate::java::cached_detect(&state.root, &state.root.join("runtimes"), now, false);
    let mut lines: Vec<String> = Vec::new();

    if list.is_empty() {
        lines.push("未检测到可用 Java".into());
    } else {
        for j in &list {
            lines.push(format!(
                "Java {}（{}，{}）: {}",
                j.version, j.vendor, j.arch, j.path
            ));
        }
    }

    if let Some(inst) = instance {
        let required = instance_required_java(&state, inst);
        match required {
            Some(major) => {
                lines.push(format!("实例 {}({}) 需要: Java {}", inst.name, inst.id, major));
                let matched = list.iter().find(|j| j.major == major);
                match matched {
                    Some(j) => lines.push(format!("匹配情况: 已满足（{}）", j.path)),
                    None => lines.push(format!(
                        "匹配情况: 未找到 Java {major}，启动时可能自动下载或失败"
                    )),
                }
            }
            None => lines.push("实例所需 Java 版本: 未能确定（版本 json 缺失）".into()),
        }
    }

    DiagnosticSection {
        key: "java".into(),
        title: "Java 环境".into(),
        lines,
        block: None,
    }
}

/// 从实例已安装的版本 json 读 javaVersion.majorVersion（不联网）
fn instance_required_java(state: &AppState, inst: &crate::models::Instance) -> Option<u32> {
    let json_path = crate::paths::resolve_version_dir(state, &inst.id).join(format!("{}.json", inst.id));
    let text = std::fs::read_to_string(&json_path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("javaVersion")?.get("majorVersion")?.as_u64().map(|n| n as u32)
}

/// 实例自检：版本 json / 客户端 jar / natives / 模组数量
fn collect_instance(state: &AppState, inst: &crate::models::Instance) -> DiagnosticSection {
    let dir = state.instances_dir().join(&inst.id);
    let mut lines = vec![
        format!("实例: {}（{}）", inst.name, inst.id),
        format!(
            "版本: Minecraft {} · {}{}",
            inst.mc_version,
            inst.loader.as_str(),
            inst.loader_version
                .as_ref()
                .map(|v| format!(" {v}"))
                .unwrap_or_default()
        ),
    ];

    let json_path = crate::paths::resolve_version_dir(state, &inst.id).join(format!("{}.json", inst.id));
    lines.push(format!(
        "版本 json: {}",
        if json_path.is_file() {
            match std::fs::metadata(&json_path) {
                Ok(m) => format!("存在（{}）", human_bytes(m.len())),
                Err(_) => "存在".into(),
            }
        } else {
            "缺失（未安装游戏本体）".into()
        }
    ));

    let client = crate::paths::resolve_version_dir(state, &inst.id).join(format!("{}.jar", inst.id));
    lines.push(format!(
        "客户端 jar: {}",
        match std::fs::metadata(&client) {
            Ok(m) if m.len() > 0 => format!("存在（{}）", human_bytes(m.len())),
            Ok(_) => "存在但为空（损坏）".into(),
            Err(_) => "缺失".into(),
        }
    ));

    let natives = dir.join("natives");
    let dll_count = std::fs::read_dir(&natives)
        .map(|rd| rd.flatten().filter(|e| e.path().is_file()).count())
        .unwrap_or(0);
    lines.push(format!(
        "natives 目录: {}",
        if natives.is_dir() {
            format!("存在（{dll_count} 个文件）")
        } else {
            "缺失（启动会报「缺少 natives 目录」）".into()
        }
    ));

    // 依赖库完整性：libraries 缺失时游戏启动会崩在模组入口点，Fabric 却报成
    // 「某个模组启动时崩溃」——这里是唯一能看清真相的地方。
    if json_path.is_file() {
        match std::fs::read_to_string(&json_path)
            .ok()
            .and_then(|t| serde_json::from_str::<crate::models::VersionJson>(&t).ok())
        {
            Some(mut v) => {
                crate::install::normalize_natives_args(&mut v);
                let entries = crate::launch::resolve_classpath_entries(
                    state,
                    &v,
                    &crate::launch::features_map(inst.resolution.is_some()),
                );
                let problems: Vec<String> = entries
                    .iter()
                    .filter_map(|e| {
                        crate::launch::entry_state(e)
                            .err()
                            .map(|why| format!("{}（{why}）", e.name))
                    })
                    .collect();
                lines.push(format!(
                    "依赖库(libraries): {} 个条目，{}",
                    entries.len(),
                    if problems.is_empty() {
                        "全部就位".to_string()
                    } else {
                        format!(
                            "有 {} 个文件缺失或损坏（{}{}）",
                            problems.len(),
                            problems.iter().take(3).cloned().collect::<Vec<_>>().join("、"),
                            if problems.len() > 3 { " 等" } else { "" }
                        )
                    }
                ));
            }
            None => lines.push("依赖库(libraries): 版本 json 无法解析，跳过检查".into()),
        }
    }

    let mods_dir = dir.join("mods");
    let (mut jars, mut disabled) = (0usize, 0usize);
    if let Ok(rd) = std::fs::read_dir(&mods_dir) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_lowercase();
            if n.ends_with(".jar.disabled") {
                disabled += 1;
            } else if n.ends_with(".jar") {
                jars += 1;
            }
        }
    }
    lines.push(format!("模组: {jars} 个启用 / {disabled} 个禁用"));
    if !state.instances_dir().join(&inst.id).join("options.txt").is_file() {
        lines.push("options.txt: 不存在（首次启动会生成）".into());
    }

    DiagnosticSection {
        key: "instance".into(),
        title: "实例自检".into(),
        lines,
        block: None,
    }
}

/// 网络：镜像 / 代理配置 + 上游可达性与延迟
async fn collect_network(state: &AppState, instance: Option<&crate::models::Instance>) -> DiagnosticSection {
    let (mirror, mirror_custom, proxy_mode, proxy) = {
        let s = state.settings.read().unwrap();
        (
            s.mirror.clone(),
            s.mirror_custom.clone(),
            s.proxy_mode.clone(),
            s.proxy.clone().unwrap_or_default(),
        )
    };

    let mut lines = vec![
        format!("镜像源: {mirror}{}", if mirror == "custom" { format!("（{mirror_custom}）") } else { String::new() }),
        format!(
            "代理模式: {proxy_mode}{}",
            if proxy_mode == "custom" && !proxy.is_empty() { format!("（{proxy}）") } else { String::new() }
        ),
        format!(
            "CurseForge API Key: {}",
            if state.settings.read().unwrap().curseforge_api_key.as_deref().map(|k| !k.is_empty()).unwrap_or(false) {
                "已配置"
            } else {
                "未配置（CurseForge 功能不可用）"
            }
        ),
    ];

    // 上游探测：并发 + 逐个超时，避免拖慢报告
    let mut probes: Vec<(String, String)> = vec![
        ("Mojang 元数据".into(), "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json".into()),
        ("Modrinth API".into(), "https://api.modrinth.com/v2/tag/game_version".into()),
        ("CurseForge API".into(), "https://api.curseforge.com/v1/games".into()),
        ("Forge CDN".into(), "https://edge.forgecdn.net/files/4612/979/".into()),
    ];
    if mirror != "official" {
        probes.push(("镜像站".into(), crate::mirror::rewrite(state, "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")));
    }

    use futures_util::StreamExt;
    let results: Vec<(String, Result<(u16, u128), String>)> = futures_util::stream::iter(probes)
        .map(|(label, url)| {
            let client = state.client.clone();
            let key = state
                .settings
                .read()
                .unwrap()
                .curseforge_api_key
                .clone()
                .unwrap_or_default();
            async move {
                let started = std::time::Instant::now();
                let mut req = client.get(&url).timeout(PROBE_TIMEOUT);
                if url.contains("curseforge.com") && !key.is_empty() {
                    req = req.header("x-api-key", key);
                }
                let r = match req.send().await {
                    Ok(resp) => Ok((resp.status().as_u16(), started.elapsed().as_millis())),
                    Err(e) => Err(e.to_string()),
                };
                (label, r)
            }
        })
        .buffered(4)
        .collect()
        .await;

    for (label, r) in results {
        match r {
            Ok((status, ms)) => lines.push(format!("{label}: HTTP {status}（{ms} ms）")),
            Err(e) => lines.push(format!("{label}: 连接失败（{e}）")),
        }
    }

    // 目标实例的目标资源（便于判断是网络还是内容问题）
    if let Some(inst) = instance {
        lines.push(format!("实例目标版本: Minecraft {} / {}", inst.mc_version, inst.loader.as_str()));
    }

    DiagnosticSection {
        key: "network".into(),
        title: "网络与镜像".into(),
        lines,
        block: None,
    }
}

/// 崩溃报告：最近几份的诊断摘要
fn collect_crashes(state: &AppState, inst: Option<&crate::models::Instance>) -> Option<DiagnosticSection> {
    let inst = inst?;
    let crash_dir = state.instances_dir().join(&inst.id).join("crash-reports");
    let mut entries: Vec<(String, std::time::SystemTime, u64)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&crash_dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("crash-") && name.ends_with(".txt") {
                if let Ok(m) = e.metadata() {
                    entries.push((name, m.modified().unwrap_or(std::time::UNIX_EPOCH), m.len()));
                }
            }
        }
    }
    if entries.is_empty() {
        return None;
    }
    entries.sort_by(|a, b| b.1.cmp(&a.1));
    let mut lines: Vec<String> = Vec::new();
    let mut block = String::new();
    for (name, _, size) in entries.iter().take(3) {
        let path = crash_dir.join(name);
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let d = crate::crash::analyze_text(&text, None);
                lines.push(format!(
                    "{name}（{}）: {} — {}",
                    human_bytes(*size),
                    d.title,
                    d.reason.chars().take(160).collect::<String>()
                ));
                block.push_str(&format!("\n===== {name} =====\n{}\n", d.advice));
            }
            Err(_) => lines.push(format!("{name}: 读取失败")),
        }
    }
    Some(DiagnosticSection {
        key: "crash".into(),
        title: "最近崩溃".into(),
        lines,
        block: if block.is_empty() { None } else { Some(block) },
    })
}

/// 日志：最近一次启动日志尾部 + 安装追踪日志
fn collect_logs(state: &AppState, inst: Option<&crate::models::Instance>) -> Vec<DiagnosticSection> {
    let mut out = Vec::new();
    if let Some(inst) = inst {
        let latest = state.instances_dir().join(&inst.id).join("logs").join("latest.log");
        if let Some(tail) = tail_lines(&latest, LOG_TAIL_LINES) {
            out.push(DiagnosticSection {
                key: "game_log".into(),
                title: "最近游戏日志（尾部）".into(),
                lines: vec![format!("来源: {}", latest.display())],
                block: Some(tail),
            });
        }
    }
    let trace = std::env::temp_dir().join("qookix-install-debug.log");
    if let Some(tail) = tail_lines(&trace, 80) {
        out.push(DiagnosticSection {
            key: "install_log".into(),
            title: "安装 / 启动追踪日志（尾部）".into(),
            lines: vec![format!("来源: {}", trace.display())],
            block: Some(tail),
        });
    }
    out
}

/// 生成 Markdown 报告文本
fn render_markdown(
    sections: &[DiagnosticSection],
    generated_at: u64,
    redactions: usize,
    instance: Option<&crate::models::Instance>,
) -> String {
    let mut md = String::new();
    md.push_str("# QookiX Launcher 诊断报告\n\n");
    md.push_str(&format!(
        "- 生成时间（unix）: {generated_at}\n- 启动器版本: {}\n- 已脱敏替换: {redactions} 处\n",
        env!("CARGO_PKG_VERSION")
    ));
    if let Some(inst) = instance {
        md.push_str(&format!(
            "- 关联实例: {}（Minecraft {} · {}）\n",
            inst.name,
            inst.mc_version,
            inst.loader.as_str()
        ));
    }
    md.push('\n');
    for s in sections {
        md.push_str(&format!("## {}\n\n", s.title));
        for l in &s.lines {
            md.push_str(&format!("- {l}\n"));
        }
        if let Some(b) = &s.block {
            md.push_str(&format!("\n```\n{}\n```\n", b.trim_end()));
        }
        md.push('\n');
    }
    md
}

/// 一键采集（可选指定实例）。
/// 报告会自动落盘到 `<数据目录>/diagnostics/`，保留最近 [`KEEP_REPORTS`] 份，
/// 便于事后回溯（用户反馈时不必重新复现）。
pub async fn collect(state: &AppState, instance_id: Option<String>) -> Result<DiagnosticReport, String> {
    let instance = match instance_id.as_deref() {
        Some(id) if !id.is_empty() => Some(crate::instances::get_instance(state, id)?),
        _ => None,
    };
    let inst_ref = instance.as_ref();

    let mut sections = vec![
        collect_system(state),
        collect_gpu(),
        collect_java(state, inst_ref),
    ];
    sections.push(collect_network(state, inst_ref).await);
    if let Some(inst) = inst_ref {
        sections.push(collect_instance(state, inst));
    }
    if let Some(c) = collect_crashes(state, inst_ref) {
        sections.push(c);
    }
    sections.extend(collect_logs(state, inst_ref));

    // 整体脱敏：路径里的用户名、UUID、token
    let generated_at = now_secs();
    let raw_md = render_markdown(&sections, generated_at, 0, inst_ref);
    let (_, redactions) = redact(&raw_md);

    // 结构化字段同样脱敏（前端展示用）
    let mut clean_sections = Vec::with_capacity(sections.len());
    let mut total = redactions;
    for s in sections {
        let (mut lines, c1) = redact(&s.lines.join("\n"));
        total += c1;
        let (block, c2) = match &s.block {
            Some(b) => {
                let (cb, c) = redact(b);
                (Some(cb), c)
            }
            None => (None, 0),
        };
        total += c2;
        if c1 > 0 {
            lines = lines.replace("<user>", "<user>");
        }
        clean_sections.push(DiagnosticSection {
            key: s.key,
            title: s.title,
            lines: lines.lines().map(|l| l.to_string()).collect(),
            block,
        });
    }

    let markdown = render_markdown(&clean_sections, generated_at, total, inst_ref);
    // 自动落盘（失败不影响本次报告返回，仅记录日志）
    if let Err(e) = save_report(state, &markdown, generated_at) {
        crate::util::log_line(&format!("[diagnostics] 报告落盘失败: {e}"));
    }

    Ok(DiagnosticReport {
        generated_at,
        sections: clean_sections,
        markdown,
        redactions: total,
    })
}

// ---------------------------------------------------------------------------
// 持久化：报告落盘 / 列表 / 读取 / 删除
// ---------------------------------------------------------------------------

/// 数据目录下存放历史诊断报告的文件夹
pub const REPORTS_DIR: &str = "diagnostics";
/// 自动保留的历史报告份数（超出后删除最旧的）
pub const KEEP_REPORTS: usize = 20;

#[derive(Serialize, Clone, Debug)]
pub struct DiagnosticReportEntry {
    pub filename: String,
    /// 生成时间（unix 秒，取自文件名 / mtime）
    pub generated_at: u64,
    pub size: u64,
}

fn reports_dir(state: &AppState) -> std::path::PathBuf {
    state.root.join(REPORTS_DIR)
}

/// 校验报告文件名：仅允许 `report-<数字>.md`，防止路径穿越
fn valid_report_filename(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("report-") else { return false };
    let Some(stem) = rest.strip_suffix(".md") else { return false };
    !stem.is_empty() && stem.chars().all(|c| c.is_ascii_digit())
}

/// 报告写入 `<数据目录>/diagnostics/report-<时间戳>.md`，并清理超额旧报告
pub fn save_report(state: &AppState, markdown: &str, generated_at: u64) -> Result<String, String> {
    let dir = reports_dir(state);
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建诊断目录失败: {e}"))?;
    let path = dir.join(format!("report-{generated_at}.md"));
    std::fs::write(&path, markdown).map_err(|e| format!("写入诊断报告失败: {e}"))?;
    prune_reports(state, KEEP_REPORTS);
    Ok(path.to_string_lossy().to_string())
}

/// 历史报告列表（新的在前）
pub fn list_reports(state: &AppState) -> Vec<DiagnosticReportEntry> {
    let dir = reports_dir(state);
    let mut out: Vec<DiagnosticReportEntry> = Vec::new();
    let Ok(rd) = std::fs::read_dir(&dir) else { return out };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !valid_report_filename(&name) {
            continue;
        }
        let meta = e.metadata().ok();
        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let generated_at = name
            .strip_prefix("report-")
            .and_then(|s| s.strip_suffix(".md"))
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or_else(|| {
                meta.as_ref()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            });
        out.push(DiagnosticReportEntry { filename: name, generated_at, size });
    }
    out.sort_by(|a, b| b.generated_at.cmp(&a.generated_at));
    out
}

/// 读取某份历史报告全文
pub fn read_report(state: &AppState, filename: &str) -> Result<String, String> {
    if !valid_report_filename(filename) {
        return Err("非法的报告文件名".into());
    }
    std::fs::read_to_string(reports_dir(state).join(filename))
        .map_err(|e| format!("读取诊断报告失败: {e}"))
}

/// 删除某份历史报告
pub fn delete_report(state: &AppState, filename: &str) -> Result<(), String> {
    if !valid_report_filename(filename) {
        return Err("非法的报告文件名".into());
    }
    let path = reports_dir(state).join(filename);
    if !path.is_file() {
        return Err("报告不存在".into());
    }
    std::fs::remove_file(&path).map_err(|e| format!("删除诊断报告失败: {e}"))
}

/// 只保留最近 `keep` 份报告，其余删除
pub fn prune_reports(state: &AppState, keep: usize) {
    let list = list_reports(state);
    for entry in list.into_iter().skip(keep) {
        let path = reports_dir(state).join(&entry.filename);
        crate::util::fs_best_effort("remove_file", &path, std::fs::remove_file(&path));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_user_paths_uuid_and_tokens() {
        let raw = "路径 C:\\Users\\ZhaYi\\AppData\\Roaming\\QookiX-Launcher\n\
                   家目录 /home/zayi/.minecraft\n\
                   uuid 123e4567-e89b-12d3-a456-426614174000\n\
                   --accessToken eyJhbGciOi.payload.sig\n\
                   {\"accessToken\": \"secret-value\"}";
        let (out, n) = redact(raw);
        assert!(n >= 4, "应至少命中 4 类脱敏，实际 {n}");
        assert!(out.contains(r"C:\Users\<user>"), "用户名未脱敏: {out}");
        assert!(out.contains("/home/<user>"));
        assert!(out.contains("<uuid>"));
        assert!(!out.contains("eyJhbGciOi.payload.sig"), "token 未脱敏: {out}");
        assert!(!out.contains("secret-value"));
        assert!(!out.contains("ZhaYi"));
    }

    #[test]
    fn tail_lines_keeps_last_n() {
        let dir = std::env::temp_dir().join("qookix-diag-tail");
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("test.log");
        let content: String = (1..=300).map(|i| format!("line{i}\n")).collect();
        std::fs::write(&p, content).unwrap();
        let tail = tail_lines(&p, 10).unwrap();
        assert!(tail.contains("line300"));
        assert!(!tail.contains("line289\n"));
        assert!(tail.contains("仅显示最后 10 行"));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn human_bytes_formats() {
        assert_eq!(human_bytes(512), "512.0 B");
        assert_eq!(human_bytes(1024), "1.0 KB");
        assert_eq!(human_bytes(1536 * 1024), "1.5 MB");
    }

    fn test_state(root: &Path) -> AppState {
        use std::sync::{Arc, Mutex, RwLock};
        AppState {
            root: root.to_path_buf(),
            settings: RwLock::new(Default::default()),
            db: Mutex::new(rusqlite::Connection::open_in_memory().unwrap()),
            client: crate::settings::http_client("system", None),
            semaphore: Arc::new(tokio::sync::Semaphore::new(4)),
            game_pids: Arc::new(Mutex::new(std::collections::HashMap::new())),
            server_pids: Arc::new(Mutex::new(std::collections::HashMap::new())),
            server_senders: Arc::new(Mutex::new(std::collections::HashMap::new())),
            task_counter: std::sync::atomic::AtomicU64::new(1),
            install_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            ms_flow: Arc::new(Mutex::new(None)),
            java_cache: Mutex::new(None),
            terracotta: Mutex::new(None),
            pending_update: Mutex::new(None),
        }
    }

    /// 报告落盘 / 列表 / 读取 / 删除 / 超额清理，以及文件名穿越防护
    #[test]
    fn reports_persist_list_read_delete_and_prune() {
        let root = std::env::temp_dir().join("qookix-diag-reports");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let state = test_state(&root);

        for i in 0..5u64 {
            save_report(&state, &format!("# 报告 {i}"), 1_700_000_000 + i).unwrap();
        }
        let list = list_reports(&state);
        assert_eq!(list.len(), 5);
        assert_eq!(list[0].generated_at, 1_700_000_004, "列表应按时间倒序");

        let content = read_report(&state, &list[0].filename).unwrap();
        assert!(content.contains("报告 4"));

        // 路径穿越 / 非法文件名必须被拒绝
        assert!(read_report(&state, "../instance.json").is_err());
        assert!(read_report(&state, "report-abc.md").is_err());
        assert!(delete_report(&state, "evil.md").is_err());

        delete_report(&state, &list[0].filename).unwrap();
        assert_eq!(list_reports(&state).len(), 4);

        // 超额自动清理：只保留最近 2 份
        prune_reports(&state, 2);
        let rest = list_reports(&state);
        assert_eq!(rest.len(), 2);
        assert!(rest[0].generated_at > rest[1].generated_at);

        let _ = std::fs::remove_dir_all(&root);
    }

    /// classpath 推导：classifier 必须参与去重。
    ///
    /// MC 26.2 的 json 里 `org.lwjgl:lwjgl:3.4.1` 只列了 `:unsafe`（真正的类 jar，
    /// 含 org.lwjgl.Version）与 `:natives-*`（只有 DLL）。按 group:artifact 去重会
    /// 把 `:unsafe` 丢掉，classpath 上只剩 DLL jar → org.lwjgl.* 全部 ClassNotFound。
    #[test]
    fn classpath_keeps_unsafe_and_natives_classifiers() {
        let root = std::env::temp_dir().join("qookix-diag-classpath");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let state = test_state(&root);
        let v: crate::models::VersionJson = serde_json::from_str(
            r#"{
              "id": "t",
              "libraries": [
                {"name":"org.lwjgl:lwjgl:3.4.1:natives-linux","downloads":{"artifact":{"sha1":"","size":1,"url":"u","path":"org/lwjgl/lwjgl/3.4.1/lwjgl-3.4.1-natives-linux.jar"}},"rules":[{"action":"allow","os":{"name":"linux"}}]},
                {"name":"org.lwjgl:lwjgl:3.4.1:natives-windows","downloads":{"artifact":{"sha1":"","size":2,"url":"u","path":"org/lwjgl/lwjgl/3.4.1/lwjgl-3.4.1-natives-windows.jar"}},"rules":[{"action":"allow","os":{"name":"windows"}}]},
                {"name":"org.lwjgl:lwjgl:3.4.1:natives-windows-arm64","downloads":{"artifact":{"sha1":"","size":3,"url":"u","path":"org/lwjgl/lwjgl/3.4.1/lwjgl-3.4.1-natives-windows-arm64.jar"}},"rules":[{"action":"allow","os":{"name":"windows"}}]},
                {"name":"org.lwjgl:lwjgl:3.4.1:unsafe","downloads":{"artifact":{"sha1":"","size":4,"url":"u","path":"org/lwjgl/lwjgl/3.4.1/lwjgl-3.4.1-unsafe.jar"}}},
                {"name":"org.lwjgl:lwjgl-glfw:3.4.1","downloads":{"artifact":{"sha1":"","size":5,"url":"u","path":"org/lwjgl/lwjgl-glfw/3.4.1/lwjgl-glfw-3.4.1.jar"}}},
                {"name":"org.lwjgl:lwjgl-glfw:3.4.1:natives-windows","downloads":{"artifact":{"sha1":"","size":6,"url":"u","path":"org/lwjgl/lwjgl-glfw/3.4.1/lwjgl-glfw-3.4.1-natives-windows.jar"}},"rules":[{"action":"allow","os":{"name":"windows"}}]},
                {"name":"org.ow2.asm:asm:9.6"},
                {"name":"org.ow2.asm:asm:9.10.1"},
                {"name":"com.example:disabled:1.0","rules":[{"action":"disallow"}]}
              ]
            }"#,
        )
        .unwrap();
        let names: Vec<String> = crate::launch::resolve_classpath_entries(
            &state,
            &v,
            &crate::launch::features_map(false),
        )
        .into_iter()
        .map(|e| e.name)
        .collect();

        assert!(
            names.contains(&"org.lwjgl:lwjgl:3.4.1:unsafe".to_string()),
            "类 jar（:unsafe）必须留在 classpath：{names:?}"
        );
        assert!(
            !names.iter().any(|n| n.ends_with("natives-linux")),
            "其它系统的 natives 不该进 classpath：{names:?}"
        );
        assert!(
            !names.iter().any(|n| n.ends_with("natives-windows-arm64")),
            "非本机架构的 natives 不该进 classpath（安装期也不会下载）：{names:?}"
        );
        if cfg!(windows) && cfg!(target_arch = "x86_64") {
            assert!(
                names.contains(&"org.lwjgl:lwjgl:3.4.1:natives-windows".to_string()),
                "本机 natives 仍需在 classpath 上（LWJGL 3.4 从 classpath 取 DLL）：{names:?}"
            );
        }
        // 同 classifier 的重复条目仍要去重，且保留最高版本
        assert!(names.contains(&"org.ow2.asm:asm:9.10.1".to_string()));
        assert!(!names.contains(&"org.ow2.asm:asm:9.6".to_string()));
        assert!(!names.iter().any(|n| n.starts_with("com.example:")));
        let _ = std::fs::remove_dir_all(&root);
    }
}
