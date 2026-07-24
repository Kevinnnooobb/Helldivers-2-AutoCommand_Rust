// Wiki 战备数据自动拉取 — 从 Stratagem Hero Trainer JS 数据源解析
use crate::stratagems::PluginStratagem;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

pub fn cache_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("wiki_cache")
}

pub fn stratagem_cache_path() -> PathBuf {
    cache_dir().join("stratagems.json")
}

/// 权威数据源：Stratagem Hero Trainer 的 JS 数据文件
pub const STRATAGEM_DATA_URL: &str =
    "https://raw.githubusercontent.com/nvigneux/Stratagem-Hero-Trainer/master/app/lib/placeholder-data-helldivers.js";

/// 部门 ID → 显示名映射（与图标包目录名一致）
const CATEGORY_MAP: &[(&str, &str)] = &[
    ("3958dc9e-712f-4377-85e9-fec4b6a6442a", "Patriotic Administration Center"),
    ("3958dc9e-742f-4377-85e9-fec4b6a6442a", "Orbital Cannons"),
    ("3958dc9e-737f-4377-85e9-fec4b6a6442a", "Hangar"),
    ("50ca3e18-62cd-11ee-8c99-0242ac120002", "Bridge"),
    ("3958dc9e-787f-4377-85e9-fec4b6a6442a", "Engineering Bay"),
    ("76d65c26-f784-44a2-ac19-586678f7c2f2", "Robotics Workshop"),
    ("06a89b98-cc7a-46ac-a8fb-7bbf12d5cb78", "Chemical Agents"),
    ("de1fb4c0-9ae8-4690-af44-90325cf11978", "Urban Legends"),
    ("f701e133-1fff-466c-8c84-3e99154ff778", "Servants of Freedom"),
    ("ee78a618-c92b-48e4-a514-b849a8ad0859", "Borderline Justice"),
    ("d6e15727-9fe1-4961-8c5b-ea44a9bd81aa", "General Stratagems"),
    ("86a708cf-8def-4244-a86a-7e7680632807", "Masters of Ceremony"),
    ("aa878f90-85b0-4ea6-b7ef-3097bc0effd8", "Force of Law"),
    ("347b11f0-ef3e-49ae-af7e-f16d02a0f8eb", "Control Group"),
    ("1fbf16b0-2726-4188-9f9d-11cd31224168", "Dust Devils"),
    ("77fda1c3-b8c9-4d13-a1ba-4f6e036b65f1", "Python Commandos"),
    ("8877a3a0-668a-4495-9606-e35a6d719cb4", "Redacted Regiment"),
    ("d4f10c99-fe1b-4357-a927-343f216dc4c0", "Siege Breakers"),
    ("a63978b8-a411-4b06-bce7-abc2ffef418c", "Entrenched Division"),
];

fn category_name(id: &str) -> String {
    let dept = CATEGORY_MAP
        .iter()
        .find(|(cid, _)| *cid == id)
        .map(|(_, n)| n.to_string())
        .unwrap_or_else(|| id.to_string());
    // 部门名 → Wiki 8 大功能分类
    match dept.as_str() {
        "Patriotic Administration Center" => "Support Weapons",
        "Orbital Cannons" => "Orbital Strikes",
        "Hangar" => "Eagle Strikes",
        "Bridge" => "Orbital Strikes",
        "Engineering Bay" => "Support Weapons",
        "Robotics Workshop" => "Sentries",
        "Chemical Agents" => "Support Weapons",
        "Urban Legends" => "Emplacements",
        "Servants of Freedom" => "Backpacks",
        "Borderline Justice" => "Backpacks",
        "General Stratagems" => "Mission Stratagems",
        "Masters of Ceremony" => "Mission Stratagems",
        "Force of Law" => "Support Weapons",
        "Control Group" => "Sentries",
        "Dust Devils" => "Support Weapons",
        "Python Commandos" => "Support Weapons",
        "Redacted Regiment" => "Backpacks",
        "Siege Breakers" => "Support Weapons",
        "Entrenched Division" => "Emplacements",
        _ => &dept,
    }.to_string()
}

/// 从远端拉取并解析 JS 战备数据
pub fn fetch_stratagems(
    on_progress: impl Fn(String) + Send + 'static,
    on_done: impl FnOnce(Result<Vec<PluginStratagem>, String>) + Send + 'static,
) {
    let url = STRATAGEM_DATA_URL.to_string();
    thread::spawn(move || {
        on_progress("正在连接数据源…".into());

        let resp = match ureq::get(&url).timeout(Duration::from_secs(30)).call() {
            Ok(r) => r,
            Err(e) => {
                on_done(Err(format!("网络错误: {e}")));
                return;
            }
        };

        on_progress("正在下载…".into());

        let mut body = String::new();
        if let Err(e) = resp.into_reader().read_to_string(&mut body) {
            on_done(Err(format!("读取失败: {e}")));
            return;
        }

        on_progress("正在解析…".into());

        let stratagems = match parse_js_data(&body) {
            Ok(s) => s,
            Err(e) => {
                on_done(Err(format!("解析失败: {e}")));
                return;
            }
        };

        on_progress(format!("已解析 {} 条战备，正在比对…", stratagems.len()));

        on_done(Ok(stratagems));
    });
}

/// 解析 Stratagem Hero Trainer JS 格式（手动状态机，零正则依赖）
fn parse_js_data(body: &str) -> Result<Vec<PluginStratagem>, String> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    let chars: Vec<char> = body.chars().collect();
    let len = chars.len();

    while pos < len {
        // 查找 "name: '"（不能加 `{ ` 前缀，因为 JS 中 { 和 name 之间有换行缩进）
        let start_pattern = "name: '";
        if let Some(p) = find_pattern(&chars, pos, start_pattern) {
            pos = p + start_pattern.len();
            // 提取 name
            let name_end = find_char(&chars, pos, '\'');
            if name_end.is_none() { break; }
            let name_end = name_end.unwrap();
            let name = chars[pos..name_end].iter().collect::<String>();
            pos = name_end + 1;

            // 查找 "code: [" — 只在 stratagem entries 中存在，CATEGORIES entries 会跳过
            if let Some(p) = find_pattern(&chars, pos, "code: [") {
                pos = p + "code: [".len();
                let bracket_end = find_char(&chars, pos, ']');
                if bracket_end.is_none() { break; }
                let bracket_end = bracket_end.unwrap();
                let code_str = chars[pos..bracket_end].iter().collect::<String>();
                let code: Vec<String> = code_str
                    .split(',')
                    .map(|s| s.trim().trim_matches('\'').to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                pos = bracket_end + 1;

                if let Some(p) = find_pattern(&chars, pos, "category_id: '") {
                    pos = p + "category_id: '".len();
                    let cat_end = find_char(&chars, pos, '\'');
                    if cat_end.is_none() { break; }
                    let cat_end = cat_end.unwrap();
                    let cat_id = chars[pos..cat_end].iter().collect::<String>();
                    pos = cat_end + 1;

                    let category = category_name(&cat_id);
                    let icon = name_to_snake(&name);

                    out.push(PluginStratagem {
                        name: format!("{} (Wiki)", name),
                        category,
                        model: String::new(),
                        command: code,
                        description: String::new(),
                        icon,
                    });
                } else {
                    pos += 1;
                }
            } else {
                // 没有 code: [ → 这是 CATEGORIES 条目，快速跳过
                // 跳到下一个 } 或 name: ' 位置
                if let Some(skip) = find_pattern(&chars, pos, "name: '") {
                    pos = skip;
                } else {
                    pos += 1;
                }
            }
        } else {
            pos += 1;
        }
    }

    if out.is_empty() {
        return Err("未解析到任何战备数据".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_local_file() {
        let body = std::fs::read_to_string(
            "C:/Users/Rin/AppData/Local/Temp/opencode/test_data.js"
        ).expect("test data file not found");
        let result = parse_js_data(&body);
        assert!(result.is_ok(), "parse failed: {:?}", result.err());
        let items = result.unwrap();
        assert!(items.len() >= 90, "expected >=90 stratagems, got {}", items.len());
    }
}

fn find_pattern(chars: &[char], start: usize, pat: &str) -> Option<usize> {
    let pat_chars: Vec<char> = pat.chars().collect();
    if start + pat_chars.len() > chars.len() { return None; }
    for i in start..=chars.len() - pat_chars.len() {
        if chars[i..i+pat_chars.len()] == pat_chars[..] {
            return Some(i);
        }
    }
    None
}

fn find_char(chars: &[char], start: usize, target: char) -> Option<usize> {
    for i in start..chars.len() {
        if chars[i] == target { return Some(i); }
    }
    None
}

/// 英文名 → snake_case 图标 key
fn name_to_snake(name: &str) -> String {
    let s = name
        .chars()
        .map(|c| {
            if c == ' ' || c == '-' || c == '–' || c == ',' || c == '.' {
                '_'
            } else if c == '"' || c == '“' || c == '”' {
                ' '
            } else {
                c
            }
        })
        .collect::<String>()
        .to_lowercase();
    let parts: Vec<&str> = s.split('_').filter(|p| !p.is_empty()).collect();
    parts.join("_")
}

// ─── 异步刷新接口 ───

pub struct FetchProgress {
    pub stage: String,
    pub done: bool,
    pub result: Option<Result<Vec<PluginStratagem>, String>>,
}

pub fn start_fetch() -> (mpsc::Receiver<FetchProgress>, bool) {
    let (tx, rx) = mpsc::channel();
    let tx_progress = tx.clone();
    let tx_done = tx;
    let has_cache = crate::plugin::plugins_dir().join("_wiki_new.json").exists();

    fetch_stratagems(
        move |msg| {
            let _ = tx_progress.send(FetchProgress { stage: msg, done: false, result: None });
        },
        move |result| {
            let _ = tx_done.send(FetchProgress { stage: String::new(), done: true, result: Some(result) });
        },
    );

    (rx, has_cache)
}
