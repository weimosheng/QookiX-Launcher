use crate::mcping;
use crate::state::AppState;
use serde_json::{json, Value};
use tauri::State;

// Multiplayer servers
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn ping_mc_server(address: String) -> mcping::ServerStatus {
    mcping::ping_server(&address).await
}

#[tauri::command]
pub fn list_servers(state: State<AppState>, instance_id: String) -> Result<Value, String> {
    let dir = state.instances_dir().join(&instance_id);

    // 现代 Minecraft (1.20.5+) 使用 servers.json
    let json_path = dir.join("servers.json");
    if json_path.is_file() {
        if let Ok(text) = std::fs::read_to_string(&json_path) {
            if let Ok(v) = serde_json::from_str::<Value>(&text) {
                if let Some(servers) = v.get("servers").and_then(|x| x.as_array()) {
                    let list: Vec<Value> = servers
                        .iter()
                        .filter_map(|s| {
                            let name = s.get("name").and_then(|x| x.as_str())?.to_string();
                            let ip = s
                                .get("ip")
                                .and_then(|x| x.as_str())
                                .unwrap_or("")
                                .to_string();
                            if ip.is_empty() {
                                return None;
                            }
                            let icon = s.get("icon").and_then(|x| x.as_str()).map(|x| x.to_string());
                            Some(json!({ "name": name, "address": ip, "icon": icon }))
                        })
                        .collect();
                    return Ok(json!({ "servers": list }));
                }
            }
        }
    }

    // 旧版 Minecraft 使用 servers.dat (GZIP 压缩的 NBT)
    let dat_path = dir.join("servers.dat");
    if dat_path.is_file() {
        if let Ok(bytes) = std::fs::read(&dat_path) {
            use std::io::Read;
            // Minecraft 1.12 及更早的 servers.dat 是 gzip 压缩的 NBT，
            // 1.13+ 改为未压缩的纯 NBT。根据魔数判断是否需要先解压。
            let raw: Vec<u8> = if bytes.starts_with(&[0x1f, 0x8b]) {
                use flate2::read::GzDecoder;
                let mut decompressed = Vec::new();
                match GzDecoder::new(&bytes[..]).read_to_end(&mut decompressed) {
                    Ok(_) => decompressed,
                    Err(_) => bytes,
                }
            } else {
                bytes
            };
            if let Ok(root) = fastnbt::from_bytes::<ServersDat>(&raw) {
                let list: Vec<Value> = root
                    .servers
                    .into_iter()
                    .filter_map(|s| {
                        if s.ip.trim().is_empty() {
                            return None;
                        }
                        // NBT 中的 icon 是裸 base64（无 data: 前缀），补全以便前端渲染
                        let icon = s.icon.filter(|i| !i.trim().is_empty()).map(|i| {
                            if i.starts_with("data:") {
                                i
                            } else {
                                format!("data:image/png;base64,{}", i)
                            }
                        });
                        Some(json!({ "name": s.name, "address": s.ip, "icon": icon }))
                    })
                    .collect();
                return Ok(json!({ "servers": list }));
            }
        }
    }

    Ok(json!({ "servers": [] }))
}

#[derive(serde::Deserialize)]
struct ServersDat {
    servers: Vec<ServerNbt>,
}

#[derive(serde::Deserialize)]
struct ServerNbt {
    name: String,
    ip: String,
    #[serde(default)]
    icon: Option<String>,
}

