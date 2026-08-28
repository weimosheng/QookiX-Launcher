use serde::Serialize;
use serde_json::Value;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// 单个 Minecraft 多人服务器的实时状态（Java 版 Server List Ping 协议返回）
#[derive(Serialize, Clone)]
pub struct ServerStatus {
    pub online: bool,
    pub address: String,
    pub name: Option<String>,
    pub version: Option<String>,
    pub players_online: Option<u32>,
    pub players_max: Option<u32>,
    pub motd: Option<String>,
    pub favicon: Option<String>,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
}

/// 入口：统一返回 ServerStatus，网络/解析失败也会返回一个 offline 状态，绝不抛错
pub async fn ping_server(raw_addr: &str) -> ServerStatus {
    let addr = raw_addr.trim().to_string();
    match do_ping(&addr).await {
        Ok(s) => s,
        Err(e) => ServerStatus {
            online: false,
            address: addr,
            name: None,
            version: None,
            players_online: None,
            players_max: None,
            motd: None,
            favicon: None,
            latency_ms: None,
            error: Some(e),
        },
    }
}

/// 解析 "host"、"host:port" 或 "[ipv6]:port" 为 (host, port)
fn parse_addr(addr: &str) -> Result<(String, u16), String> {
    let addr = addr.trim();
    if addr.starts_with('[') {
        let end = addr.find(']').ok_or("无效的 IPv6 地址")?;
        let host = addr[1..end].to_string();
        let rest = &addr[end + 1..];
        let port = if let Some(stripped) = rest.strip_prefix(':') {
            stripped.parse::<u16>().map_err(|_| "端口无效".to_string())?
        } else {
            25565
        };
        return Ok((host, port));
    }
    if let Some(idx) = addr.rfind(':') {
        let (h, p) = addr.split_at(idx);
        if h.is_empty() {
            return Err("地址无效".into());
        }
        let port = p[1..].parse::<u16>().map_err(|_| "端口无效".to_string())?;
        Ok((h.to_string(), port))
    } else {
        Ok((addr.to_string(), 25565))
    }
}

async fn do_ping(addr: &str) -> Result<ServerStatus, String> {
    let (host, port) = parse_addr(addr)?;

    let connect = TcpStream::connect((host.as_str(), port));
    let stream = match tokio::time::timeout(Duration::from_secs(5), connect).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Err(format!("连接失败: {e}")),
        Err(_) => return Err("连接超时".into()),
    };
    let _ = stream.set_nodelay(true);
    let (mut reader, mut writer) = stream.into_split();

    // 1) Handshake: 协议版本(VarInt) + 地址(VarInt len + utf8) + 端口(u16) + nextState(1)
    let mut payload = Vec::new();
    write_varint(&mut payload, 767); // 使用现代协议号即可，服务器会回它自己的版本
    write_varint(&mut payload, host.len() as i32);
    payload.extend_from_slice(host.as_bytes());
    payload.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut payload, 1); // next state = status

    let mut packet = Vec::new();
    packet.push(0x00); // packet id
    packet.extend_from_slice(&payload);
    let mut full = Vec::new();
    write_varint(&mut full, packet.len() as i32);
    full.extend_from_slice(&packet);
    write_all_timeout(&mut writer, &full, Duration::from_secs(5)).await?;

    // 2) Status Request: 仅 packet id 0x00
    let mut req = Vec::new();
    write_varint(&mut req, 1);
    req.push(0x00);
    write_all_timeout(&mut writer, &req, Duration::from_secs(5)).await?;

    // 3) 读取 Status Response
    let t0 = Instant::now();
    let len = read_varint_timeout(&mut reader, Duration::from_secs(6)).await?;
    if len <= 0 || len > 10_000_000 {
        return Err("状态响应过大或无效".into());
    }
    let mut buf = vec![0u8; len as usize];
    read_exact_timeout(&mut reader, &mut buf, Duration::from_secs(6)).await?;
    let elapsed = t0.elapsed();
    // buf[0] = packet id (0x00)，随后是 VarInt 长度的 JSON 字符串
    let mut pos = 1usize;
    let json_len = read_varint_at(&buf, &mut pos)? as usize;
    if pos + json_len > buf.len() {
        return Err("状态响应格式错误".into());
    }
    let json = String::from_utf8_lossy(&buf[pos..pos + json_len]).to_string();
    let v: Value = serde_json::from_str(&json).map_err(|e| format!("JSON 解析失败: {e}"))?;

    // 4) Ping / Pong 用于测量更精确的延迟
    let latency_ms: Option<u64> = match send_ping_pong(&mut reader, &mut writer, Duration::from_secs(5)).await {
        Ok(rtt) => Some(rtt),
        Err(_) => Some(elapsed.as_millis() as u64),
    };

    let version = v
        .get("version")
        .and_then(|x| x.get("name"))
        .and_then(|x| x.as_str())
        .map(|s| s.to_string());
    let players_online = v
        .get("players")
        .and_then(|x| x.get("online"))
        .and_then(|x| x.as_u64())
        .map(|x| x as u32);
    let raw_max = v
        .get("players")
        .and_then(|x| x.get("max"))
        .and_then(|x| x.as_u64())
        .map(|x| x as u32);
    // 部分服务器（BungeeCord 网关、插件服等）会把 max 写成 1 或小于在线人数，
    // 这种上限无意义，统一隐藏，避免显示成 "260/1" 之类的奇怪数字
    let players_max = match (players_online, raw_max) {
        (Some(o), Some(m)) if m >= o && m > 0 => Some(m),
        _ => None,
    };
    let favicon = v.get("favicon").and_then(|x| x.as_str()).map(|s| s.to_string());
    let motd = v.get("description").map(flatten_chat);

    Ok(ServerStatus {
        online: true,
        address: addr.to_string(),
        name: None,
        version,
        players_online,
        players_max,
        motd,
        favicon,
        latency_ms,
        error: None,
    })
}

/// 发送 0x01 Ping（payload = 时间戳），读取 0x01 Pong，返回往返耗时(ms)
async fn send_ping_pong<R, W>(reader: &mut R, writer: &mut W, timeout: Duration) -> Result<u64, String>
where
    R: AsyncReadExt + Unpin,
    W: AsyncWriteExt + Unpin,
{
    let payload = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut body = Vec::new();
    body.push(0x01u8);
    body.extend_from_slice(&payload.to_be_bytes());
    let mut packet = Vec::new();
    write_varint(&mut packet, body.len() as i32);
    packet.extend_from_slice(&body);
    write_all_timeout(writer, &packet, timeout).await?;
    let t0 = Instant::now();
    let len = read_varint_timeout(reader, timeout).await?;
    if len != 9 {
        return Err("pong 长度异常".into());
    }
    let mut buf = [0u8; 9];
    read_exact_timeout(reader, &mut buf, timeout).await?;
    if buf[0] != 0x01 {
        return Err("pong 类型异常".into());
    }
    Ok(t0.elapsed().as_millis() as u64)
}

fn write_varint(buf: &mut Vec<u8>, mut value: i32) {
    loop {
        let mut temp = (value & 0b0111_1111) as u8;
        value >>= 7;
        if value != 0 {
            temp |= 0b1000_0000;
        }
        buf.push(temp);
        if value == 0 {
            break;
        }
    }
}

async fn write_all_timeout<W: AsyncWriteExt + Unpin>(w: &mut W, buf: &[u8], timeout: Duration) -> Result<(), String> {
    match tokio::time::timeout(timeout, w.write_all(buf)).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(format!("发送失败: {e}")),
        Err(_) => Err("发送超时".into()),
    }
}

async fn read_varint_timeout<R: AsyncReadExt + Unpin>(r: &mut R, timeout: Duration) -> Result<i32, String> {
    let mut num_read = 0;
    let mut result = 0i32;
    loop {
        let mut byte = [0u8; 1];
        read_exact_timeout(r, &mut byte, timeout).await?;
        let b = byte[0];
        result |= ((b & 0b0111_1111) as i32) << (7 * num_read);
        num_read += 1;
        if num_read > 5 {
            return Err("VarInt 过长".into());
        }
        if (b & 0b1000_0000) == 0 {
            break;
        }
    }
    Ok(result)
}

async fn read_exact_timeout<R: AsyncReadExt + Unpin>(r: &mut R, buf: &mut [u8], timeout: Duration) -> Result<(), String> {
    match tokio::time::timeout(timeout, r.read_exact(buf)).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(format!("读取失败: {e}")),
        Err(_) => Err("读取超时".into()),
    }
}

fn read_varint_at(buf: &[u8], pos: &mut usize) -> Result<i32, String> {
    let mut num_read = 0;
    let mut result = 0i32;
    loop {
        if *pos >= buf.len() {
            return Err("VarInt 越界".into());
        }
        let b = buf[*pos];
        *pos += 1;
        result |= ((b & 0b0111_1111) as i32) << (7 * num_read);
        num_read += 1;
        if num_read > 5 {
            return Err("VarInt 过长".into());
        }
        if (b & 0b1000_0000) == 0 {
            break;
        }
    }
    Ok(result)
}

/// 把 MC 的聊天组件（字符串 / 带 text+extra 的对象 / 数组）压平为带 § 颜色码的纯文本，
/// 方便前端解析 §x 后渲染成彩色。
fn flatten_chat(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Object(o) => {
            let mut out = String::new();
            // 颜色
            if let Some(Value::String(c)) = o.get("color") {
                if let Some(code) = mc_color_code(c) {
                    out.push('§');
                    out.push(code);
                }
            }
            // 格式（粗体/斜体/下划线/删除线/乱码）
            for f in ["bold", "italic", "underlined", "strikethrough", "obfuscated"] {
                if o.get(f).and_then(|x| x.as_bool()).unwrap_or(false) {
                    if let Some(code) = mc_color_code(f) {
                        out.push('§');
                        out.push(code);
                    }
                }
            }
            if let Some(Value::String(t)) = o.get("text") {
                out.push_str(t);
            }
            if let Some(Value::Array(extra)) = o.get("extra") {
                for e in extra {
                    out.push_str(&flatten_chat(e));
                }
            }
            out
        }
        Value::Array(a) => a.iter().map(flatten_chat).collect::<String>(),
        _ => String::new(),
    }
}

/// MC 颜色/格式名 -> § 颜色码
fn mc_color_code(name: &str) -> Option<char> {
    Some(match name {
        "black" => '0',
        "dark_blue" => '1',
        "dark_green" => '2',
        "dark_aqua" => '3',
        "dark_red" => '4',
        "dark_purple" => '5',
        "gold" => '6',
        "gray" => '7',
        "dark_gray" => '8',
        "blue" => '9',
        "green" => 'a',
        "aqua" => 'b',
        "red" => 'c',
        "light_purple" => 'd',
        "yellow" => 'e',
        "white" => 'f',
        "obfuscated" => 'k',
        "bold" => 'l',
        "italic" => 'o',
        "underlined" => 'n',
        "strikethrough" => 'm',
        "reset" => 'r',
        _ => return None,
    })
}
