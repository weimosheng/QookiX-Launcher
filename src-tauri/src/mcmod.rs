use std::collections::HashMap;
use std::sync::OnceLock;

const WIKI_ENTRIES_DATA: &str = include_str!("../WikiEntries.txt");

struct WikiIndex {
    modrinth: HashMap<String, (u32, Option<String>)>,
    curseforge: HashMap<String, (u32, Option<String>)>,
    /// 按“英文原名/标题”反查中文名（不区分平台），用于没有 slug 的已装模组
    by_title: HashMap<String, String>,
}

static INDEX: OnceLock<WikiIndex> = OnceLock::new();

fn index() -> &'static WikiIndex {
    INDEX.get_or_init(build_index)
}

/// 将 "钠 (Sodium)" / "铁路 (Railcraft)" 解析为 (中文名, 英文名)
fn split_cn_en(name: &str) -> Option<(String, String)> {
    let start = name.find('(')?;
    let end = name.rfind(')')?;
    if end <= start {
        return None;
    }
    let cn = name[..start].trim().to_string();
    let en = name[start + 1..end].trim().to_string();
    if cn.is_empty() || en.is_empty() {
        return None;
    }
    Some((cn, en))
}

fn build_index() -> WikiIndex {
    let mut modrinth = HashMap::new();
    let mut curseforge = HashMap::new();
    let mut by_title = HashMap::new();
    let lines: Vec<&str> = WIKI_ENTRIES_DATA.lines().collect();
    let entry_lines = if lines.is_empty() { &lines[..] } else { &lines[..lines.len() - 1] };
    for (i, line) in entry_lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        let wiki_id = (i as u32) + 1;
        for raw_entry in line.split('¨') {
            let mut parts = raw_entry.split('|');
            let slugs = parts.next().unwrap_or_default();
            let final_part = parts.next_back();
            let (cf, mr) = parse_slugs(slugs);
            let name = final_part.map(|n| resolve_name(n, cf.as_deref().or(mr.as_deref())));
            if let Some(s) = mr {
                modrinth.entry(s.to_lowercase()).or_insert((wiki_id, name.clone()));
            }
            if let Some(s) = cf {
                curseforge.entry(s.to_lowercase()).or_insert((wiki_id, name.clone()));
            }
            if let Some(n) = &name {
                if let Some((cn, en)) = split_cn_en(n) {
                    by_title.entry(en.to_lowercase()).or_insert(cn);
                }
            }
        }
    }
    WikiIndex { modrinth, curseforge, by_title }
}

/// 按英文原名/标题反查中文名（不区分平台）。先整串匹配，再退而取首词匹配。
pub fn lookup_chinese_name_by_title(title: &str) -> Option<String> {
    let idx = index();
    let t = title.trim().to_lowercase();
    if t.is_empty() {
        return None;
    }
    if let Some(cn) = idx.by_title.get(&t) {
        return Some(cn.clone());
    }
    if let Some(first) = t.split_whitespace().next() {
        if let Some(cn) = idx.by_title.get(first) {
            return Some(cn.clone());
        }
    }
    None
}

/// 综合中文名：优先按 slug（平台精确映射），否则按标题反查。
/// 用于实例模组列表，复用内容中心相同的 WikiEntries 映射。
pub fn cn_name_for_record(source: &str, slug: Option<&str>, title: Option<&str>) -> Option<String> {
    if let Some(s) = slug.filter(|s| !s.is_empty()) {
        if let Some(cn) = lookup_chinese_name(s, source) {
            return Some(cn);
        }
    }
    if let Some(t) = title.filter(|t| !t.is_empty()) {
        return lookup_chinese_name_by_title(t);
    }
    None
}

fn resolve_name(name: &str, slug: Option<&str>) -> String {
    if name.contains('*') {
        let english = slug.unwrap_or_default().replace('-', " ");
        let capitalized: String = english
            .split_whitespace()
            .map(|w| {
                let mut chars = w.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().chain(chars).collect(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        name.replace('*', &format!(" ({capitalized})"))
    } else {
        name.to_string()
    }
}

fn parse_slugs(slugs: &str) -> (Option<&str>, Option<&str>) {
    if let Some(mr) = slugs.strip_prefix('@') {
        return (None, non_empty(mr));
    }
    if let Some(shared) = slugs.strip_suffix('@') {
        let s = non_empty(shared);
        return (s, s);
    }
    if let Some((cf, mr)) = slugs.split_once('@') {
        return (non_empty(cf), non_empty(mr));
    }
    (non_empty(slugs), None)
}

fn non_empty(v: &str) -> Option<&str> {
    (!v.is_empty()).then_some(v)
}

/// Look up MC wiki class ID by platform slug.
pub fn lookup_wiki_id(slug: &str, provider: &str) -> Option<u32> {
    let idx = index();
    let map = match provider {
        "modrinth" => &idx.modrinth,
        "curseforge" => &idx.curseforge,
        _ => return None,
    };
    map.get(&slug.to_lowercase()).map(|(id, _)| *id)
}

/// Look up Chinese name by platform slug.
pub fn lookup_chinese_name(slug: &str, provider: &str) -> Option<String> {
    let idx = index();
    let map = match provider {
        "modrinth" => &idx.modrinth,
        "curseforge" => &idx.curseforge,
        _ => return None,
    };
    map.get(&slug.to_lowercase()).and_then(|(_, name)| name.clone())
}
