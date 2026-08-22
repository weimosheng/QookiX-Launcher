use std::collections::HashMap;
use std::sync::OnceLock;

const WIKI_ENTRIES_DATA: &str = include_str!("../WikiEntries.txt");

struct WikiIndex {
    modrinth: HashMap<String, (u32, Option<String>)>,
    curseforge: HashMap<String, (u32, Option<String>)>,
}

static INDEX: OnceLock<WikiIndex> = OnceLock::new();

fn index() -> &'static WikiIndex {
    INDEX.get_or_init(build_index)
}

fn build_index() -> WikiIndex {
    let mut modrinth = HashMap::new();
    let mut curseforge = HashMap::new();
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
                curseforge.entry(s.to_lowercase()).or_insert((wiki_id, name));
            }
        }
    }
    WikiIndex { modrinth, curseforge }
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
