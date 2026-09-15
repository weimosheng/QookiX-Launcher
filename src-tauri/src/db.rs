//! 应用数据库（SQLite）。
//!
//! 存放"核心应用数据"：账号、全局设置、首页固定、按天游玩时长。
//! 实例目录 / 版本 JSON / assets 等保持文件形式（MC 生态约定），缓存不进库。
//!
//! 兼容策略：首次打开时若发现旧的 JSON 文件（accounts.json / settings.json /
//! pins.json / playtime.json）则自动导入，并把旧文件改名为 `*.migrated.bak`
//! 保留备份；之后一律只读写 `qookix.db`。

use rusqlite::Connection;
use std::path::Path;

pub fn db_path(root: &Path) -> std::path::PathBuf {
    root.join("qookix.db")
}

/// 打开（或创建）数据库并确保表结构与 JSON 迁移完成。
pub fn open(root: &Path) -> Result<Connection, String> {
    let conn = Connection::open(db_path(root)).map_err(|e| format!("打开数据库失败: {e}"))?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| format!("设置 WAL 失败: {e}"))?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|e| format!("设置 synchronous 失败: {e}"))?;
    create_tables(&conn)?;
    migrate_json_files(root, &conn)?;
    Ok(conn)
}

fn create_tables(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS accounts (
            id   TEXT PRIMARY KEY,
            idx  INTEGER NOT NULL DEFAULT 0,
            data TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS pins (
            id   TEXT PRIMARY KEY,
            idx  INTEGER NOT NULL DEFAULT 0,
            data TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS playtime (
            day  TEXT PRIMARY KEY,
            secs INTEGER NOT NULL
        );
        "#,
    )
    .map_err(|e| format!("建表失败: {e}"))
}

/// 把旧的 JSON 文件导入数据库（仅在库为空时），旧文件改名留备份。
fn migrate_json_files(root: &Path, conn: &Connection) -> Result<(), String> {
    // settings.json
    let sp = root.join("settings.json");
    if sp.exists() {
        let has = count(conn, "settings")? > 0;
        if !has {
            if let Ok(text) = std::fs::read_to_string(&sp) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                    set_setting(conn, "app", &v.to_string())?;
                }
            }
        }
        rename_migrated(&sp);
    }
    // accounts.json（含登录令牌，导入即加密；迁移后直接删除而非留明文备份，
    // 令牌丢失的代价只是重新登录一次）
    let ap = root.join("accounts.json");
    if ap.exists() {
        if count(conn, "accounts")? == 0 {
            if let Ok(text) = std::fs::read_to_string(&ap) {
                if let Ok(list) = serde_json::from_str::<Vec<serde_json::Value>>(&text) {
                    save_rows_encrypted(conn, "accounts", &list)?;
                }
            }
        }
        let _ = std::fs::remove_file(&ap);
    }
    // 清掉历史遗留的明文账号备份（旧版迁移产物）
    let _ = std::fs::remove_file(root.join("accounts.json.migrated.bak"));
    // 历史明文账号行重加密
    ensure_accounts_encrypted(conn)?;
    // pins.json
    let pp = root.join("pins.json");
    if pp.exists() {
        if count(conn, "pins")? == 0 {
            if let Ok(text) = std::fs::read_to_string(&pp) {
                if let Ok(list) = serde_json::from_str::<Vec<serde_json::Value>>(&text) {
                    for (i, item) in list.iter().enumerate() {
                        let id = item
                            .get("id")
                            .and_then(|x| x.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| format!("row-{i}"));
                        insert_row(conn, "pins", &id, i as i64, item)?;
                    }
                }
            }
        }
        rename_migrated(&pp);
    }
    // playtime.json：{ "<day>": secs }
    let tp = root.join("playtime.json");
    if tp.exists() {
        if count(conn, "playtime")? == 0 {
            if let Ok(text) = std::fs::read_to_string(&tp) {
                if let Ok(map) =
                    serde_json::from_str::<std::collections::HashMap<String, u64>>(&text)
                {
                    for (day, secs) in map {
                        conn.execute(
                            "INSERT OR REPLACE INTO playtime(day, secs) VALUES (?1, ?2)",
                            rusqlite::params![day, secs as i64],
                        )
                        .map_err(|e| format!("导入游玩时长失败: {e}"))?;
                    }
                }
            }
        }
        rename_migrated(&tp);
    }
    Ok(())
}

fn rename_migrated(path: &Path) {
    let mut bak = path.as_os_str().to_owned();
    bak.push(".migrated.bak");
    let _ = std::fs::rename(path, std::path::PathBuf::from(bak));
}

/// 把 accounts 表中遗留的明文行整体重写为密文（幂等）。
fn ensure_accounts_encrypted(conn: &Connection) -> Result<(), String> {
    let rows = load_rows_encrypted(conn, "accounts");
    if rows.is_empty() {
        return Ok(());
    }
    let any_plain = conn
        .prepare("SELECT data FROM accounts")
        .and_then(|mut s| {
            s.query_map([], |r| r.get::<_, String>(0))
                .map(|it| it.flatten().collect::<Vec<String>>())
        })
        .map_err(|e| e.to_string())?
        .iter()
        .any(|s| crate::secret::unprotect(s).is_err());
    if any_plain {
        save_rows_encrypted(conn, "accounts", &rows)?;
    }
    Ok(())
}

fn count(conn: &Connection, table: &str) -> Result<i64, String> {
    conn.query_row(
        &format!("SELECT COUNT(*) FROM {table}"),
        [],
        |r| r.get::<_, i64>(0),
    )
    .map_err(|e| format!("查询 {table} 失败: {e}"))
}

fn insert_row(
    conn: &Connection,
    table: &str,
    id: &str,
    idx: i64,
    value: &serde_json::Value,
) -> Result<(), String> {
    conn.execute(
        &format!("INSERT OR REPLACE INTO {table}(id, idx, data) VALUES (?1, ?2, ?3)"),
        rusqlite::params![id, idx, value.to_string()],
    )
    .map_err(|e| format!("写入 {table} 失败: {e}"))?;
    Ok(())
}

// ---- settings ----

/// 读取设置（key 固定为 "app"；不存在返回 None）。
pub fn get_setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        [key],
        |r| r.get::<_, String>(0),
    )
    .ok()
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO settings(key, value) VALUES (?1, ?2)",
        rusqlite::params![key, value],
    )
    .map_err(|e| format!("写入设置失败: {e}"))?;
    Ok(())
}

// ---- rows（accounts / pins 通用：整行存 JSON，按 idx 保序）----

pub fn load_rows(conn: &Connection, table: &str) -> Vec<serde_json::Value> {
    let mut stmt = match conn.prepare(&format!(
        "SELECT data FROM {table} ORDER BY idx, id"
    )) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map([], |r| r.get::<_, String>(0));
    match rows {
        Ok(iter) => iter
            .flatten()
            .filter_map(|s| serde_json::from_str(&s).ok())
            .collect(),
        Err(_) => Vec::new(),
    }
}

pub fn save_rows(
    conn: &Connection,
    table: &str,
    items: &[serde_json::Value],
) -> Result<(), String> {
    conn.execute(&format!("DELETE FROM {table}"), [])
        .map_err(|e| format!("清空 {table} 失败: {e}"))?;
    for (i, item) in items.iter().enumerate() {
        let id = item
            .get("id")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("row-{i}"));
        insert_row(conn, table, &id, i as i64, item)?;
    }
    Ok(())
}

/// 加密版行存取：data 列整体 DPAPI 加密（base64 密文），用于存放
/// 含登录令牌的账号数据。id/idx 保持明文以便查询排序。
/// 读取兼容明文行（历史数据），下次写入自动转为密文。
pub fn load_rows_encrypted(conn: &Connection, table: &str) -> Vec<serde_json::Value> {
    let mut stmt = match conn.prepare(&format!("SELECT data FROM {table} ORDER BY idx, id")) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map([], |r| r.get::<_, String>(0));
    match rows {
        Ok(iter) => iter
            .flatten()
            .filter_map(|s| {
                // 优先按密文解；失败（明文历史行）按明文解析
                let plain = crate::secret::unprotect(&s).unwrap_or(s);
                serde_json::from_str(&plain).ok()
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

pub fn save_rows_encrypted(
    conn: &Connection,
    table: &str,
    items: &[serde_json::Value],
) -> Result<(), String> {
    conn.execute(&format!("DELETE FROM {table}"), [])
        .map_err(|e| format!("清空 {table} 失败: {e}"))?;
    for (i, item) in items.iter().enumerate() {
        let id = item
            .get("id")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("row-{i}"));
        let json = serde_json::to_string(item).map_err(|e| e.to_string())?;
        let data = crate::secret::protect(&json).unwrap_or(json);
        conn.execute(
            &format!("INSERT OR REPLACE INTO {table}(id, idx, data) VALUES (?1, ?2, ?3)"),
            rusqlite::params![id, i as i64, data],
        )
        .map_err(|e| format!("写入 {table} 失败: {e}"))?;
    }
    Ok(())
}

// ---- playtime ----

pub fn add_playtime(conn: &Connection, day: &str, secs: u64) -> Result<(), String> {
    conn.execute(
        "INSERT INTO playtime(day, secs) VALUES (?1, ?2)
         ON CONFLICT(day) DO UPDATE SET secs = secs + ?2",
        rusqlite::params![day, secs as i64],
    )
    .map_err(|e| format!("写入游玩时长失败: {e}"))?;
    Ok(())
}

pub fn load_playtime(conn: &Connection) -> std::collections::HashMap<String, u64> {
    let mut out = std::collections::HashMap::new();
    if let Ok(mut stmt) = conn.prepare("SELECT day, secs FROM playtime") {
        if let Ok(iter) = stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        }) {
            for (day, secs) in iter.flatten() {
                out.insert(day, secs.max(0) as u64);
            }
        }
    }
    out
}
