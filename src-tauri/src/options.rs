//! `options.txt` 里的按键绑定读写。
//!
//! 只改提交的字段，其余行（顺序、未知配置项）原样保留。文件由游戏首次运行时
//! 生成，不存在时直接报错——凭空创建可能写出游戏不认识的行。

use std::path::Path;

/// 未绑定的按键码（游戏自己的表示）
pub const UNBOUND: &str = "key.keyboard.unknown";

/// 一条按键绑定：options.txt 里的 `key_key.<action>:<key>` 行
#[derive(Clone, Debug, serde::Serialize)]
pub struct KeyBind {
    /// 动作 id（不含 `key_key.` 前缀），如 `forward`、`hotbar.1`
    pub action: String,
    /// 按键码，如 `key.keyboard.w`、`key.mouse.left`
    pub key: String,
}

/// options.txt 路径（实例根目录下）
pub fn path(instance_dir: &Path) -> std::path::PathBuf {
    instance_dir.join("options.txt")
}

/// 按键码合法性：只认 `key.keyboard.*` / `key.mouse.*`，防止写入任意内容
fn valid_key(key: &str) -> bool {
    (key.starts_with("key.keyboard.") || key.starts_with("key.mouse."))
        && key.len() <= 64
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_')
}

/// 解析一行按键绑定；`extra` 兜住老格式潜在的第三段，写回时原样带上
fn parse_line(line: &str) -> Option<(String, String, Option<String>)> {
    let rest = line.strip_prefix("key_key.")?;
    let mut parts = rest.splitn(3, ':');
    let action = parts.next()?.to_string();
    if action.is_empty() {
        return None;
    }
    let key = parts.next()?.to_string();
    let extra = parts.next().map(str::to_string);
    Some((action, key, extra))
}

/// 读取全部按键绑定。返回 (文件是否存在, 绑定列表)
pub fn read_keybinds(instance_dir: &Path) -> (bool, Vec<KeyBind>) {
    let Ok(text) = std::fs::read_to_string(path(instance_dir)) else {
        return (false, Vec::new());
    };
    let binds = text
        .lines()
        .filter_map(|l| parse_line(l).map(|(action, key, _)| KeyBind { action, key }))
        .collect();
    (true, binds)
}

/// 应用按键修改：`changes` 是 `动作 → 新按键码`（`null` = 未绑定）。
/// 只写提交的动作；文件里没有的动作追加到末尾。返回实际改动条数。
pub fn apply_keybinds(
    instance_dir: &Path,
    changes: &serde_json::Map<String, serde_json::Value>,
) -> Result<usize, String> {
    if changes.is_empty() {
        return Ok(0);
    }
    // 先整体校验，避免写一半才发现有非法值
    let mut wanted: Vec<(String, String)> = Vec::with_capacity(changes.len());
    for (action, value) in changes {
        let key = value.as_str().unwrap_or(UNBOUND);
        if !valid_key(key) {
            return Err(format!("不认识的按键码: {key}"));
        }
        wanted.push((action.clone(), key.to_string()));
    }

    let file = path(instance_dir);
    let text = std::fs::read_to_string(&file)
        .map_err(|_| "options.txt 不存在，请先启动一次游戏再修改按键".to_string())?;
    let trailing_newline = text.ends_with('\n');
    // 换行符跟随原文件（Windows 是 CRLF），否则整个文件的换行都会被改掉
    let terminator: &str = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();

    let mut changed = 0usize;
    for line in lines.iter_mut() {
        let Some((action, _, extra)) = parse_line(line) else {
            continue;
        };
        let Some((_, new_key)) = wanted.iter().find(|(a, _)| *a == action) else {
            continue;
        };
        let new_line = match &extra {
            Some(e) => format!("key_key.{action}:{new_key}:{e}"),
            None => format!("key_key.{action}:{new_key}"),
        };
        if *line != new_line {
            *line = new_line;
            changed += 1;
        }
    }

    // 文件里没有的动作（该版本游戏没写过这行）追加到末尾
    let known: std::collections::HashSet<String> = lines
        .iter()
        .filter_map(|l| parse_line(l).map(|(a, _, _)| a))
        .collect();
    for (action, key) in &wanted {
        if !known.contains(action) {
            lines.push(format!("key_key.{action}:{key}"));
            changed += 1;
        }
    }

    let mut out = lines.join(terminator);
    if trailing_newline && !out.is_empty() {
        out.push_str(terminator);
    }
    std::fs::write(&file, out).map_err(|e| format!("写入 options.txt 失败: {e}"))?;
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("qookix-options-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // smoothCamera 初始有绑定，这样解绑（→null）才是一次真实改动
    const SAMPLE: &str = "version:3565\nkey_key.forward:key.keyboard.w\nkey_key.jump:key.keyboard.space\nkey_key.smoothCamera:key.keyboard.f8\nsensitivity:0.5\n";

    #[test]
    fn parses_only_keybind_lines() {
        let dir = temp_dir("parse");
        std::fs::write(dir.join("options.txt"), SAMPLE).unwrap();
        let (exists, binds) = read_keybinds(&dir);
        assert!(exists);
        assert_eq!(binds.len(), 3, "只解析 key_ 开头的行");
        assert_eq!(binds[0].action, "forward");
        assert_eq!(binds[0].key, "key.keyboard.w");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_reports_not_exists_and_refuses_to_write() {
        let dir = temp_dir("missing");
        let (exists, binds) = read_keybinds(&dir);
        assert!(!exists);
        assert!(binds.is_empty());
        let mut changes = serde_json::Map::new();
        changes.insert("forward".into(), json!("key.keyboard.w"));
        assert!(apply_keybinds(&dir, &changes).is_err(), "不该凭空创建文件");
    }

    #[test]
    fn apply_touches_only_submitted_actions() {
        let dir = temp_dir("apply");
        std::fs::write(dir.join("options.txt"), SAMPLE).unwrap();
        let mut changes = serde_json::Map::new();
        changes.insert("jump".into(), json!("key.mouse.4"));
        changes.insert("smoothCamera".into(), json!(null));
        changes.insert("sprint".into(), json!("key.keyboard.left.control")); // 文件里没有 → 追加
        assert_eq!(apply_keybinds(&dir, &changes).unwrap(), 3);

        let text = std::fs::read_to_string(dir.join("options.txt")).unwrap();
        assert!(text.starts_with("version:3565\n"), "其余行必须原样保留");
        assert!(text.contains("key_key.forward:key.keyboard.w"), "未提交的绑定不动");
        assert!(text.contains("key_key.jump:key.mouse.4"));
        assert!(text.contains("key_key.smoothCamera:key.keyboard.unknown"), "null = 未绑定");
        assert!(text.contains("key_key.sprint:key.keyboard.left.control"));
        assert!(text.contains("sensitivity:0.5"));
        assert!(text.ends_with('\n'), "结尾换行要保留");

        // 幂等：同样的改动再应用一次，改动数应为 0
        assert_eq!(apply_keybinds(&dir, &changes).unwrap(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn preserves_crlf_line_endings() {
        let dir = temp_dir("crlf");
        // MC 在 Windows 上按 CRLF 写 options.txt，换行风格必须原样保留
        let sample = "version:3565\r\nkey_key.forward:key.keyboard.w\r\nkey_key.jump:key.keyboard.space\r\n";
        std::fs::write(dir.join("options.txt"), sample).unwrap();
        let mut changes = serde_json::Map::new();
        changes.insert("jump".into(), json!("key.mouse.4"));
        apply_keybinds(&dir, &changes).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("options.txt")).unwrap(),
            "version:3565\r\nkey_key.forward:key.keyboard.w\r\nkey_key.jump:key.mouse.4\r\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rejects_bad_key_without_touching_file() {
        let dir = temp_dir("badkey");
        std::fs::write(dir.join("options.txt"), SAMPLE).unwrap();
        let mut changes = serde_json::Map::new();
        changes.insert("jump".into(), json!("../../etc/passwd"));
        assert!(apply_keybinds(&dir, &changes).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join("options.txt")).unwrap(),
            SAMPLE,
            "校验失败时不能动文件"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
