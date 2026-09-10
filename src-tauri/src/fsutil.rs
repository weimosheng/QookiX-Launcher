//! 实例目录与托管服务器目录共用的文件操作助手：路径越界防护、目录列举、
//! 文本读写与系统定位。两处文件管理器的命令层统一走这里，避免各写一份。

use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

/// 内置编辑器愿意加载的最大文件字节数。
pub const MAX_EDIT_BYTES: u64 = 4 * 1024 * 1024;

pub fn fmt_bytes(n: u64) -> String {
    if n >= 1024 * 1024 {
        format!("{:.1} MB", n as f64 / 1024.0 / 1024.0)
    } else if n >= 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{n} B")
    }
}

pub fn modified_secs(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn ext_of(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase()
}

#[derive(Serialize)]
pub struct FsEntry {
    pub name: String,
    pub rel: String,
    pub size: u64,
    pub modified: u64,
    pub is_dir: bool,
    pub ext: String,
}

/// 目录 ID 只允许单层名称，防止拼出 `..` 或绝对路径。
pub fn validate_id(id: &str, label: &str) -> Result<(), String> {
    if id.is_empty()
        || id == "."
        || id.contains("..")
        || id.contains('/')
        || id.contains('\\')
    {
        return Err(format!("非法{label} ID"));
    }
    Ok(())
}

/// 拒绝会拼出嵌套路径或逃出父目录的名称。
pub fn validate_name(name: &str) -> Result<(), String> {
    let t = name.trim();
    if t.is_empty() || t == "." || t == ".." {
        return Err("名称不能为空".into());
    }
    if t.contains('/') || t.contains('\\') {
        return Err("名称不能包含路径分隔符".into());
    }
    Ok(())
}

/// 把 `rel` 解析到 `dir` 之内，拒绝 `..`、绝对路径与指向外部的符号链接；
/// 目标尚不存在时（新建 / 重命名）改为逐段词法校验。`label` 用于错误文案。
pub fn resolve_in_dir(dir: &Path, rel: &str, label: &str) -> Result<PathBuf, String> {
    let root = dir
        .canonicalize()
        .map_err(|e| format!("{label}目录不可用: {e}"))?;
    let cleaned = rel.replace('\\', "/");
    let cleaned = cleaned.trim_start_matches('/');
    let target = if cleaned.is_empty() {
        root.clone()
    } else {
        root.join(cleaned)
    };
    match target.canonicalize() {
        Ok(c) => {
            if c != root && !c.starts_with(&root) {
                return Err(format!("路径超出{label}目录范围"));
            }
            Ok(c)
        }
        Err(_) => {
            let mut depth = 0i32;
            for part in Path::new(cleaned).components() {
                match part {
                    Component::Normal(_) => depth += 1,
                    Component::ParentDir => depth -= 1,
                    Component::CurDir => {}
                    other => {
                        return Err(format!("非法路径: {}", other.as_os_str().to_string_lossy()))
                    }
                }
                if depth < 0 {
                    return Err(format!("路径超出{label}目录范围"));
                }
            }
            Ok(target)
        }
    }
}

/// 列举目录内容：目录在前，同类型按名称（不区分大小写）排序。
pub fn list_dir(dir: &Path, base_rel: &str) -> Result<Vec<FsEntry>, String> {
    let base = base_rel.trim_end_matches('/').to_string();
    let mut out: Vec<FsEntry> = Vec::new();
    let rd = std::fs::read_dir(dir).map_err(|e| format!("读取目录失败: {e}"))?;
    for e in rd.flatten() {
        let meta = match e.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let name = e.file_name().to_string_lossy().to_string();
        let is_dir = meta.is_dir();
        let child_rel = if base.is_empty() {
            name.clone()
        } else {
            format!("{base}/{name}")
        };
        out.push(FsEntry {
            ext: if is_dir { String::new() } else { ext_of(&name) },
            name,
            rel: child_rel,
            size: if is_dir { 0 } else { meta.len() },
            modified: modified_secs(&meta),
            is_dir,
        });
    }
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

/// 读取文本文件：限制体积、拒绝二进制与非 UTF-8 内容。
pub fn read_text(path: &Path, rel: &str) -> Result<Value, String> {
    if !path.is_file() {
        return Err("不是一个文件".into());
    }
    let meta = std::fs::metadata(path).map_err(|e| format!("读取文件失败: {e}"))?;
    if meta.len() > MAX_EDIT_BYTES {
        return Err(format!(
            "文件过大（{}），内置编辑器最多支持 {}",
            fmt_bytes(meta.len()),
            fmt_bytes(MAX_EDIT_BYTES)
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("读取文件失败: {e}"))?;
    if bytes.iter().take(4096).any(|b| *b == 0) {
        return Err("这是二进制文件，无法在内置编辑器中打开".into());
    }
    let content = String::from_utf8(bytes)
        .map_err(|_| String::from("文件不是 UTF-8 编码，无法在内置编辑器中打开"))?;
    let meta2 = std::fs::metadata(path).ok();
    Ok(json!({
        "rel": rel,
        "content": content,
        "size": meta.len(),
        "modified": meta2.as_ref().map(modified_secs).unwrap_or(0),
    }))
}

/// 写入文本文件，必要时创建父目录。
pub fn write_text(path: &Path, rel: &str, content: String) -> Result<Value, String> {
    if path.is_dir() {
        return Err("目标是一个目录".into());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let len = content.len() as u64;
    std::fs::write(path, content).map_err(|e| format!("写入文件失败: {e}"))?;
    let meta = std::fs::metadata(path).ok();
    Ok(json!({
        "rel": rel,
        "size": meta.as_ref().map(|m| m.len()).unwrap_or(len),
        "modified": meta.as_ref().map(modified_secs).unwrap_or(0),
    }))
}

/// 在系统文件管理器中定位路径；目标是文件时打开其所在目录。
pub fn reveal(app: &tauri::AppHandle, path: &Path) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let target = if path.is_file() {
        path.parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| path.to_path_buf())
    } else {
        path.to_path_buf()
    };
    app.opener()
        .open_path(target.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个用例一个独立的临时根目录：root/sub/a.txt
    fn tmp_root(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("qkx-fsutil-{tag}-{nanos}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/a.txt"), "hi").unwrap();
        dir.canonicalize().unwrap()
    }

    #[test]
    fn resolve_accepts_paths_inside_root() {
        let root = tmp_root("inside");
        assert_eq!(
            resolve_in_dir(&root, "sub/a.txt", "实例").unwrap(),
            root.join("sub/a.txt")
        );
        // 空 rel 指向根目录本身
        assert_eq!(resolve_in_dir(&root, "", "实例").unwrap(), root);
        // 反斜杠写法与多余的前导斜杠都要能归一化
        assert_eq!(
            resolve_in_dir(&root, "/sub\\a.txt", "实例").unwrap(),
            root.join("sub").join("a.txt")
        );
    }

    #[test]
    fn resolve_rejects_parent_escape() {
        let root = tmp_root("escape");
        let err = resolve_in_dir(&root, "../outside.txt", "实例").unwrap_err();
        assert!(err.contains("超出实例目录范围"), "unexpected: {err}");
        let err = resolve_in_dir(&root, "sub/../../outside.txt", "实例").unwrap_err();
        assert!(err.contains("超出实例目录范围"), "unexpected: {err}");
    }

    #[test]
    fn resolve_rejects_absolute_paths() {
        let root = tmp_root("absolute");
        if cfg!(windows) {
            // Windows 盘符路径会在 join 时替换掉 root，必须被拒绝
            let err = resolve_in_dir(&root, "C:/Windows/win.ini", "实例").unwrap_err();
            assert!(err.contains("超出实例目录范围"), "unexpected: {err}");
        } else {
            // Unix 绝对路径的前导 / 会被剥掉、降级为 root 内的相对路径；
            // 无论目标存在与否，结果都不允许落在 root 之外
            let p = resolve_in_dir(&root, "/etc/hosts", "实例").unwrap();
            assert!(p.starts_with(&root), "escaped root: {p:?}");
        }
    }

    #[test]
    fn resolve_allows_nonexistent_target_within_root() {
        let root = tmp_root("newfile");
        // 目标不存在时走词法校验，合法路径直接返回
        let target = resolve_in_dir(&root, "sub/new.txt", "实例").unwrap();
        assert_eq!(target, root.join("sub/new.txt"));
        // 但逃逸到根之外的新路径仍要拒绝
        assert!(resolve_in_dir(&root, "../new.txt", "实例").is_err());
    }

    #[test]
    fn resolve_reports_missing_root() {
        let missing = std::env::temp_dir().join("qkx-fsutil-definitely-missing");
        let err = resolve_in_dir(&missing, "a.txt", "服务器").unwrap_err();
        assert!(err.contains("服务器目录不可用"), "unexpected: {err}");
    }

    #[test]
    fn validate_id_rejects_traversal_and_separators() {
        assert!(validate_id("abc-123", "实例").is_ok());
        for bad in ["", ".", "..", "../etc", "a/b", "a\\b", "a..b"] {
            assert!(validate_id(bad, "实例").is_err(), "should reject: {bad}");
        }
        assert_eq!(
            validate_id("..", "服务器").unwrap_err(),
            "非法服务器 ID"
        );
    }

    #[test]
    fn validate_name_rejects_separators_and_blank() {
        assert!(validate_name("a.txt").is_ok());
        for bad in ["", "  ", ".", "..", "a/b", "a\\b"] {
            assert!(validate_name(bad).is_err(), "should reject: {bad}");
        }
    }

    #[test]
    fn ext_of_is_lowercase_without_dot() {
        assert_eq!(ext_of("a.TXT"), "txt");
        assert_eq!(ext_of("noext"), "");
        assert_eq!(ext_of(".hidden"), "");
    }

    #[test]
    fn fmt_bytes_switches_units() {
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(2048), "2.0 KB");
        assert_eq!(fmt_bytes(3 * 1024 * 1024), "3.0 MB");
    }

    #[test]
    fn list_dir_lists_directories_first_with_relative_paths() {
        let root = tmp_root("list");
        std::fs::write(root.join("b.txt"), "x").unwrap();
        std::fs::create_dir_all(root.join("aaa")).unwrap();

        let entries = list_dir(&root, "").unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["aaa", "sub", "b.txt"]);
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].size, 0);
        assert_eq!(entries.last().unwrap().ext, "txt");

        // 带 base_rel 时 rel 要拼上前缀
        let nested = list_dir(&root.join("sub"), "sub").unwrap();
        assert_eq!(nested[0].rel, "sub/a.txt");
    }

    #[test]
    fn read_text_rejects_binary_and_oversized_files() {
        let root = tmp_root("read");
        let binary = root.join("bin.dat");
        std::fs::write(&binary, [0u8, 1, 2, 3]).unwrap();
        assert!(read_text(&binary, "bin.dat").is_err());

        let big = root.join("big.txt");
        std::fs::write(&big, vec![b'a'; (MAX_EDIT_BYTES + 1) as usize]).unwrap();
        assert!(read_text(&big, "big.txt").is_err());

        let ok = root.join("ok.txt");
        std::fs::write(&ok, "hello").unwrap();
        let v = read_text(&ok, "ok.txt").unwrap();
        assert_eq!(v["content"], "hello");
        assert_eq!(v["size"], 5);
    }

    #[test]
    fn write_text_creates_parent_dirs_and_reports_size() {
        let root = tmp_root("write");
        let target = root.join("deep/nested/out.txt");
        let v = write_text(&target, "deep/nested/out.txt", "abc".to_string()).unwrap();
        assert_eq!(v["size"], 3);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "abc");
        // 目标已是目录时要报错
        assert!(write_text(&root.join("sub"), "sub", "x".to_string()).is_err());
    }
}
