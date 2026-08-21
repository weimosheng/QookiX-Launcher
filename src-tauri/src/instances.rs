use crate::models::{InstalledContent, Instance, LoaderType};
use crate::state::AppState;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn instance_path(state: &AppState, id: &str) -> std::path::PathBuf {
    state.instances_dir().join(id).join("qookix.json")
}

pub fn load_instances(state: &AppState) -> Vec<Instance> {
    let dir = state.instances_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let meta = p.join("qookix.json");
        if let Ok(text) = std::fs::read_to_string(&meta) {
            if let Ok(inst) = serde_json::from_str::<Instance>(&text) {
                out.push(inst);
            }
        }
    }
    out.sort_by(|a, b| b.created.cmp(&a.created));
    out
}

pub fn get_instance(state: &AppState, id: &str) -> Result<Instance, String> {
    let text = std::fs::read_to_string(instance_path(state, id))
        .map_err(|_| format!("实例 {id} 不存在"))?;
    serde_json::from_str(&text).map_err(|e| format!("实例数据损坏: {e}"))
}

pub fn save_instance(state: &AppState, instance: &Instance) -> Result<(), String> {
    let path = instance_path(state, &instance.id);
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(instance).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())
}

pub fn create_instance(
    state: &AppState,
    name: String,
    mc_version: String,
    loader: LoaderType,
    loader_version: Option<String>,
) -> Result<Instance, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("实例名称不能为空".into());
    }
    if mc_version.is_empty() {
        return Err("请选择 Minecraft 版本".into());
    }
    // unique short id
    let mut id = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    while instance_path(state, &id).exists() {
        id = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    }
    let instance = Instance {
        id,
        name,
        mc_version,
        loader,
        loader_version,
        created: now(),
        last_played: None,
        installed: true,
        icon: None,
        max_memory_mb: None,
        memory_mode: None,
        jvm_args: None,
        game_args: None,
        java_path: None,
        account_id: None,
        resolution: None,
        mods: Vec::new(),
        resource_packs: Vec::new(),
        shaders: Vec::new(),
    };
    // game dir
    std::fs::create_dir_all(state.instances_dir().join(&instance.id)).map_err(|e| e.to_string())?;
    save_instance(state, &instance)?;
    Ok(instance)
}

pub fn delete_instance(state: &AppState, id: &str) -> Result<(), String> {
    let dir = state.instances_dir().join(id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("删除实例目录失败: {e}"))?;
    }
    // remove cached version json/jar
    let ver_dir = state.versions_dir().join(id);
    if ver_dir.exists() {
        let _ = std::fs::remove_dir_all(&ver_dir);
    }
    Ok(())
}

pub fn mark_installed(state: &AppState, id: &str) -> Result<(), String> {
    let mut inst = get_instance(state, id)?;
    inst.installed = true;
    save_instance(state, &inst)
}

pub fn update_instance(state: &AppState, patch: serde_json::Value) -> Result<Instance, String> {
    let id = patch
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("缺少实例 id")?;
    let mut inst = get_instance(state, id)?;
    if let Some(v) = patch.get("name").and_then(|v| v.as_str()) {
        inst.name = v.to_string();
    }
    if let Some(v) = patch.get("icon").and_then(|v| v.as_str()) {
        inst.icon = if v.is_empty() { None } else { Some(v.to_string()) };
    }
    if let Some(v) = patch.get("max_memory_mb").and_then(|v| v.as_u64()) {
        inst.max_memory_mb = if v > 0 { Some(v as u32) } else { None };
    }
    if let Some(v) = patch.get("memory_mode").and_then(|v| v.as_str()) {
        inst.memory_mode = if v.is_empty() { None } else { Some(v.to_string()) };
    }
    if let Some(v) = patch.get("jvm_args").and_then(|v| v.as_str()) {
        inst.jvm_args = if v.trim().is_empty() { None } else { Some(v.to_string()) };
    }
    if let Some(v) = patch.get("game_args").and_then(|v| v.as_str()) {
        inst.game_args = if v.trim().is_empty() { None } else { Some(v.to_string()) };
    }
    if let Some(v) = patch.get("java_path") {
        inst.java_path = v.as_str().map(|s| s.to_string()).filter(|s| !s.is_empty());
    }
    if let Some(v) = patch.get("account_id") {
        inst.account_id = v.as_str().map(|s| s.to_string()).filter(|s| !s.is_empty());
    }
    if let Some(v) = patch.get("resolution") {
        if let Some(arr) = v.as_array() {
            if arr.len() == 2 {
                let w = arr[0].as_u64().unwrap_or(0) as u32;
                let h = arr[1].as_u64().unwrap_or(0) as u32;
                inst.resolution = if w > 0 && h > 0 { Some((w, h)) } else { None };
            }
        }
    }
    save_instance(state, &inst)?;
    Ok(inst)
}

pub fn touch_last_played(state: &AppState, id: &str) {
    if let Ok(mut inst) = get_instance(state, id) {
        inst.last_played = Some(now());
        let _ = save_instance(state, &inst);
    }
}

// ---------------------------------------------------------------------------
// Installed content tracking
// ---------------------------------------------------------------------------

pub fn list_content(state: &AppState, id: &str, kind: &str) -> Vec<InstalledContent> {
    let Ok(inst) = get_instance(state, id) else {
        return Vec::new();
    };
    match kind {
        "resourcepack" => inst.resource_packs,
        "shader" => inst.shaders,
        _ => inst.mods,
    }
}

pub fn add_content(state: &AppState, id: &str, kind: &str, record: InstalledContent) -> Result<(), String> {
    let mut inst = get_instance(state, id)?;
    let list = match kind {
        "resourcepack" => &mut inst.resource_packs,
        "shader" => &mut inst.shaders,
        _ => &mut inst.mods,
    };
    list.retain(|c| c.filename != record.filename);
    list.push(record);
    save_instance(state, &inst)
}

pub fn add_content_batch(state: &AppState, id: &str, kind: &str, records: Vec<InstalledContent>) -> Result<(), String> {
    let mut inst = get_instance(state, id)?;
    let list = match kind {
        "resourcepack" => &mut inst.resource_packs,
        "shader" => &mut inst.shaders,
        _ => &mut inst.mods,
    };
    for rec in records {
        list.retain(|c| c.filename != rec.filename);
        list.push(rec);
    }
    save_instance(state, &inst)
}

pub fn remove_content(state: &AppState, id: &str, kind: &str, filename: &str) -> Result<(), String> {
    let mut inst = get_instance(state, id)?;
    let list = match kind {
        "resourcepack" => &mut inst.resource_packs,
        "shader" => &mut inst.shaders,
        _ => &mut inst.mods,
    };
    list.retain(|c| c.filename != filename);
    save_instance(state, &inst)
}

pub fn set_content_enabled(state: &AppState, id: &str, kind: &str, filename: &str, enabled: bool) -> Result<(), String> {
    let mut inst = get_instance(state, id)?;
    let list = match kind {
        "resourcepack" => &mut inst.resource_packs,
        "shader" => &mut inst.shaders,
        _ => &mut inst.mods,
    };
    let Some(item) = list.iter_mut().find(|c| c.filename == filename) else {
        return Err("内容记录不存在".into());
    };
    let folder = crate::modrinth::kind_folder(kind);
    let dir = state.instances_dir().join(id).join(folder);
    let active = dir.join(filename);
    let disabled = dir.join(format!("{filename}.disabled"));
    if enabled {
        if !active.is_file() && disabled.is_file() {
            std::fs::rename(&disabled, &active).map_err(|e| e.to_string())?;
        }
    } else {
        if active.is_file() {
            std::fs::rename(&active, &disabled).map_err(|e| e.to_string())?;
        }
    }
    item.enabled = enabled;
    save_instance(state, &inst)
}
