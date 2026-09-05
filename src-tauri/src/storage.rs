use crate::state::AppState;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// 单个存储分类的统计结果
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct StorageCategory {
    pub key: String,
    pub label: String,
    pub size: u64,
    pub files: u64,
}

/// 单个游戏实例的大小
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct InstanceStorage {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub files: u64,
}

/// 存储统计整体结果
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct StorageStats {
    pub categories: Vec<StorageCategory>,
    /// 每个游戏实例的单独大小
    pub instances: Vec<InstanceStorage>,
    /// 每个托管服务器实例的单独大小
    pub servers: Vec<InstanceStorage>,
    pub total: u64,
    pub instance_count: u64,
    pub server_count: u64,
    /// 上次更新时间（unix 秒）
    pub updated_at: u64,
    /// 是否为磁盘缓存数据（而非本次实时扫描）
    pub cached: bool,
}

/// 清除缓存的结果
#[derive(Serialize)]
pub struct CacheClearResult {
    /// 释放的空间（字节）
    pub freed: u64,
}

/// 统计结果缓存文件名（位于数据根目录）
const CACHE_FILE: &str = "storage-cache.json";

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 递归统计目录大小与文件数。
/// `exclude` 中的路径（含其子树）会被跳过 —— 用于统计启动器程序时排除数据目录。
fn dir_size(path: &Path, files: &mut u64, exclude: &[PathBuf]) -> u64 {
    let mut total = 0u64;
    let Ok(rd) = std::fs::read_dir(path) else {
        return 0;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if exclude.iter().any(|e| p.starts_with(e)) {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            total += dir_size(&p, files, exclude);
        } else if meta.is_file() {
            *files += 1;
            total += meta.len();
        }
    }
    total
}

/// 扫描数据目录，得到各分类的大小统计
pub fn scan(state: &AppState) -> StorageStats {
    let root = &state.root;
    let mut categories: Vec<StorageCategory> = Vec::new();
    let mut total = 0u64;

    let mut add_dir = |key: &str, label: &str, dir: PathBuf, exclude: &[PathBuf]| {
        let mut files = 0u64;
        let size = dir_size(&dir, &mut files, exclude);
        categories.push(StorageCategory {
            key: key.into(),
            label: label.into(),
            size,
            files,
        });
        total += size;
    };

    // 游戏实例（含每个实例的单独大小）
    let inst_dir = state.instances_dir();
    let instance_count = std::fs::read_dir(&inst_dir)
        .map(|rd| rd.flatten().filter(|e| e.path().is_dir()).count() as u64)
        .unwrap_or(0);
    add_dir("instances", "游戏实例", inst_dir.clone(), &[]);

    let mut instances: Vec<InstanceStorage> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&inst_dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let id = entry
                .file_name()
                .into_string()
                .unwrap_or_default();
            // 优先从实例元数据读取名称，读不到时回退到目录名
            let name = std::fs::read_to_string(path.join("qookix.json"))
                .ok()
                .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(String::from))
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| id.clone());
            let mut files = 0u64;
            let size = dir_size(&path, &mut files, &[]);
            instances.push(InstanceStorage { id, name, size, files });
        }
    }
    instances.sort_by(|a, b| b.size.cmp(&a.size));

    // 托管服务器实例（含每个服务器的单独大小）
    let srv_dir = state.servers_dir();
    let server_count = std::fs::read_dir(&srv_dir)
        .map(|rd| rd.flatten().filter(|e| e.path().is_dir()).count() as u64)
        .unwrap_or(0);
    add_dir("servers", "服务器实例", srv_dir.clone(), &[]);

    let mut servers: Vec<InstanceStorage> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&srv_dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let id = entry
                .file_name()
                .into_string()
                .unwrap_or_default();
            // 优先从 server.json 读取名称，读不到时回退到目录名
            let name = std::fs::read_to_string(path.join("server.json"))
                .ok()
                .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(String::from))
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| id.clone());
            let mut files = 0u64;
            let size = dir_size(&path, &mut files, &[]);
            servers.push(InstanceStorage { id, name, size, files });
        }
    }
    servers.sort_by(|a, b| b.size.cmp(&a.size));

    // 启动器共享数据
    add_dir("libraries", "库文件", state.libraries_dir(), &[]);
    add_dir("assets", "资源文件", state.assets_dir(), &[]);
    add_dir("versions", "版本文件", state.versions_dir(), &[]);
    // Java 运行时：排除其中的下载临时目录（downloads 归入"缓存"分类）
    let runtime_dl = root.join("runtimes").join("downloads");
    add_dir("runtime", "Java 运行时", root.join("runtimes"), &[runtime_dl.clone()]);
    add_dir("logs", "日志", state.logs_dir(), &[]);

    // 缓存：可安全清理的内容（Java 下载临时目录 + Java 检测缓存 + 本统计缓存）
    let mut cache_files = 0u64;
    let mut cache_size = 0u64;
    if runtime_dl.exists() {
        cache_size += dir_size(&runtime_dl, &mut cache_files, &[]);
    }
    for cf in [root.join("java-cache.json"), cache_path(root)] {
        if cf.exists() {
            if let Ok(meta) = cf.metadata() {
                cache_files += 1;
                cache_size += meta.len();
            }
        }
    }
    categories.push(StorageCategory {
        key: "cache".into(),
        label: "缓存".into(),
        size: cache_size,
        files: cache_files,
    });
    total += cache_size;

    // 其他：数据根下未被上面覆盖的条目（设置、账号、皮肤等杂项文件/目录）
    let mut other_files = 0u64;
    let mut other_size = 0u64;
    if let Ok(rd) = std::fs::read_dir(root) {
        for entry in rd.flatten() {
            let p = entry.path();
            let covered = [
                state.instances_dir(),
                state.libraries_dir(),
                state.assets_dir(),
                state.versions_dir(),
                root.join("runtimes"),
                state.logs_dir(),
                state.servers_dir(),
            ]
            .iter()
            .any(|d| p.starts_with(d));
            let fname = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // 排除统计缓存与 Java 检测缓存（均计入"缓存"分类）
            if covered || fname == CACHE_FILE || fname == "java-cache.json" {
                continue;
            }
            if let Ok(meta) = entry.metadata() {
                if meta.is_dir() {
                    other_size += dir_size(&p, &mut other_files, &[]);
                } else if meta.is_file() {
                    other_files += 1;
                    other_size += meta.len();
                }
            }
        }
    }
    categories.push(StorageCategory {
        key: "other".into(),
        label: "其他数据".into(),
        size: other_size,
        files: other_files,
    });
    total += other_size;

    // 启动器程序本体：只统计当前 exe 文件本身。
    // 绝不能递归统计 exe 所在目录——开发模式下那是整个 Rust 的 target 构建缓存，
    // 安装模式下目录里可能还包含/是数据根，会把所有游戏数据错误地计入"启动器程序"。
    let mut launcher_files = 0u64;
    let launcher_size = std::env::current_exe()
        .ok()
        .filter(|exe| exe.is_file())
        .map(|exe| {
            launcher_files += 1;
            exe.metadata().map(|m| m.len()).unwrap_or(0)
        })
        .unwrap_or(0);
    categories.push(StorageCategory {
        key: "launcher".into(),
        label: "启动器程序".into(),
        size: launcher_size,
        files: launcher_files,
    });
    total += launcher_size;

    // 去掉无数据的分类，保持列表干净
    categories.retain(|c| c.size > 0);

    StorageStats {
        categories,
        instances,
        servers,
        total,
        instance_count,
        server_count,
        updated_at: now_secs(),
        cached: false,
    }
}

fn cache_path(root: &Path) -> PathBuf {
    root.join(CACHE_FILE)
}

fn load_cache(root: &Path) -> Option<StorageStats> {
    let data = std::fs::read_to_string(cache_path(root)).ok()?;
    serde_json::from_str::<StorageStats>(&data)
        .ok()
        .map(|mut s| {
            s.cached = true;
            s
        })
}

fn save_cache(root: &Path, stats: &StorageStats) {
    if let Ok(json) = serde_json::to_string(stats) {
        let p = cache_path(root);
        crate::util::fs_best_effort("write", &p, std::fs::write(&p, json));
    }
}

/// 获取存储统计：优先返回上次扫描的缓存，无缓存时执行实时扫描
pub fn get_storage_stats(state: &AppState) -> StorageStats {
    if let Some(cached) = load_cache(&state.root) {
        return cached;
    }
    let stats = scan(state);
    save_cache(&state.root, &stats);
    stats
}

/// 强制重新扫描并保存缓存
pub fn refresh_storage_stats(state: &AppState) -> StorageStats {
    let stats = scan(state);
    save_cache(&state.root, &stats);
    stats
}

/// 清除可安全清理的缓存（Java 下载临时文件、Java 检测缓存、本统计缓存）。
/// 不触碰任何实例、库、资源、版本等游戏数据。
pub fn clear_cache(state: &AppState) -> Result<CacheClearResult, String> {
    let mut freed = 0u64;

    // 1. Java 运行时下载临时目录
    let dl_dir = state.root.join("runtimes").join("downloads");
    if dl_dir.exists() {
        let mut files = 0u64;
        freed += dir_size(&dl_dir, &mut files, &[]);
        std::fs::remove_dir_all(&dl_dir)
            .map_err(|e| format!("清理 Java 下载缓存失败: {e}"))?;
    }

    // 2. Java 检测缓存
    let java_cache = state.root.join("java-cache.json");
    if java_cache.exists() {
        freed += java_cache.metadata().map(|m| m.len()).unwrap_or(0);
        crate::util::fs_best_effort("remove_file", &java_cache, std::fs::remove_file(&java_cache));
    }

    // 3. 本统计缓存（删除后下次访问会重新扫描）
    let stat_cache = cache_path(&state.root);
    if stat_cache.exists() {
        freed += stat_cache.metadata().map(|m| m.len()).unwrap_or(0);
        crate::util::fs_best_effort("remove_file", &stat_cache, std::fs::remove_file(&stat_cache));
    }

    Ok(CacheClearResult { freed })
}
