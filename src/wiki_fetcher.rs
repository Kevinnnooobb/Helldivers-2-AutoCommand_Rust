// Wiki 战备数据自动拉取 — 从 Stratagem Hero Trainer JS 数据源解析
use crate::stratagems::PluginStratagem;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

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

/// 解析 Stratagem Hero Trainer JS 格式（手动状态机，零正则依赖）。
/// 真实文件结构：CATEGORIES 数组在前、战备数组在后；每个战备条目为
/// `{ name: '...', code: [...], category_id: '...' }`。
/// 关键约束：code/category_id 的搜索必须限定在「下一个 name 之前」，
/// 否则分类名会与后续战备的 code 错误配对（生产数据中的潜伏 bug）。
fn parse_js_data(body: &str) -> Result<Vec<PluginStratagem>, String> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    let chars: Vec<char> = body.chars().collect();
    let len = chars.len();

    while pos < len {
        let start_pattern = "name: '";
        let Some(name_pos) = find_pattern(&chars, pos, start_pattern) else { break; };
        pos = name_pos + start_pattern.len();
        let Some(name_end) = find_char(&chars, pos, '\'') else { break; };
        let name: String = chars[pos..name_end].iter().collect();
        let after_name = name_end + 1;

        // 本条目边界：下一个 name 的位置（条目之间以此分隔）
        let next_name = find_pattern(&chars, after_name, start_pattern);
        let in_entry = |i: usize| next_name.is_none_or(|n| i < n);

        // code 必须属于当前条目；否则这是 CATEGORIES 条目，直接跳到下一个 name
        let Some(code_pos) = find_pattern(&chars, after_name, "code: [").filter(|&i| in_entry(i)) else {
            match next_name {
                Some(n) => pos = n,
                None => break,
            }
            continue;
        };

        pos = code_pos + "code: [".len();
        let Some(bracket_end) = find_char(&chars, pos, ']') else { break; };
        let code_str: String = chars[pos..bracket_end].iter().collect();
        let code: Vec<String> = code_str
            .split(',')
            .map(|s| s.trim().trim_matches('\'').to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let after_code = bracket_end + 1;

        // category_id 同样必须属于当前条目
        let Some(cat_pos) = find_pattern(&chars, after_code, "category_id: '").filter(|&i| in_entry(i)) else {
            // 有 code 但无 category：跳到下一个 name 继续扫描
            pos = next_name.unwrap_or(len);
            continue;
        };
        pos = cat_pos + "category_id: '".len();
        let Some(cat_end) = find_char(&chars, pos, '\'') else { break; };
        let cat_id: String = chars[pos..cat_end].iter().collect();
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
    }

    if out.is_empty() {
        return Err("未解析到任何战备数据".into());
    }
    Ok(out)
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
    (start..chars.len()).find(|&i| chars[i] == target)
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
    let has_cache = crate::plugin::wiki_plugin_path().exists();

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

#[cfg(test)]
mod tests {
    use super::*;

    /// 仓库内 fixture，替代原依赖外部绝对路径的测试数据文件
    const SAMPLE: &str = include_str!("fixtures/wiki_sample.js");

    #[test]
    fn parse_fixture() {
        let result = parse_js_data(SAMPLE);
        assert!(result.is_ok(), "parse failed: {:?}", result.err());
        let items = result.unwrap();
        assert_eq!(items.len(), 3);

        assert_eq!(items[0].name, "Reinforce (Wiki)");
        assert_eq!(items[0].category, "Mission Stratagems");
        assert_eq!(items[0].icon, "reinforce");
        assert_eq!(items[0].command, vec!["up", "down", "right", "left", "up"]);

        assert_eq!(items[1].name, "Eagle 500KG Bomb (Wiki)");
        assert_eq!(items[1].category, "Eagle Strikes");
        assert_eq!(items[1].icon, "eagle_500kg_bomb");

        assert_eq!(items[2].name, "Railcannon Strike (Wiki)");
        assert_eq!(items[2].category, "Orbital Strikes");
    }

    #[test]
    fn parse_empty_or_garbage_returns_err() {
        assert!(parse_js_data("").is_err());
        assert!(parse_js_data("no stratagems here").is_err());
    }

    #[test]
    fn category_name_maps_departments_and_falls_back() {
        assert_eq!(category_name("d6e15727-9fe1-4961-8c5b-ea44a9bd81aa"), "Mission Stratagems");
        assert_eq!(category_name("3958dc9e-737f-4377-85e9-fec4b6a6442a"), "Eagle Strikes");
        assert_eq!(category_name("3958dc9e-742f-4377-85e9-fec4b6a6442a"), "Orbital Strikes");
        assert_eq!(category_name("unknown-id"), "unknown-id");
    }

    #[test]
    fn name_to_snake_maps_specials() {
        assert_eq!(name_to_snake("Eagle 500KG Bomb"), "eagle_500kg_bomb");
        assert_eq!(name_to_snake("RX-1 Railgun"), "rx_1_railgun");
        assert_eq!(name_to_snake("  Spaced  Out "), "spaced_out");
    }

    #[test]
    fn find_pattern_bounds() {
        let chars: Vec<char> = "abc abc".chars().collect();
        assert_eq!(find_pattern(&chars, 0, "abc"), Some(0));
        assert_eq!(find_pattern(&chars, 1, "abc"), Some(4));
        assert_eq!(find_pattern(&chars, 0, "xyz"), None);
        assert_eq!(find_pattern(&chars, 0, "abcabcabc"), None);
    }

    #[test]
    fn find_char_bounds() {
        let chars: Vec<char> = "hello".chars().collect();
        assert_eq!(find_char(&chars, 0, 'l'), Some(2));
        assert_eq!(find_char(&chars, 3, 'l'), Some(3));
        assert_eq!(find_char(&chars, 0, 'z'), None);
    }
}
