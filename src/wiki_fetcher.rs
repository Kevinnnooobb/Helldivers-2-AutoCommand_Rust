// 战备数据自动获取 — 从 helldivers.wiki.gg/wiki/Stratagems 页面表格解析
// 页面结构参考：本地保存的 "Stratagems - The Helldivers Wiki.html"（仓库根目录，
// 仅作为结构参照与测试输入，不作为运行时数据源）。
//
// 目标页面使用 MediaWiki 的 wikitable 展示战备：
//   Icon | Name | Stratagem Code | Base Cooldown
// 方向序列以 `<img alt="Stratagem Arrow Up/Down/Left/Right.svg">` 的形式内嵌在
// 表格中，因此这里手写一个零依赖的 HTML 表格提取器（不引入 scraper/regex）。
//
// 页面结构（战备分类与位置以页面为准）：
//   <h3>Offensive Permit</h3>
//     <details><summary>Orbital Strikes</summary>…<table>…</details>
//     <details><summary>Eagle Strikes</summary>…<table>…</details>
//   …
//   <details><summary>Mission Stratagems</summary>
//     <big><b>Ship</b></big><table>               ← 块内第一张表之前的子标签，沿用 summary 分类
//     <p><big><b>Objective</b></big><table>       ← 出现在前一张表之后的标签，视为新分类
//     <p><big><b>Unavailable</b></big><table>
// 解析出的战备直接归属页面上的分类，不再附加任何 new / wiki 标签。
//
// 关键约束：页面上还有 "Urban Legends" 等债券（Warbond）名称，它们出现在普通
// 文本或没有 Stratagem Code 列的表格里。只解析同时包含 Name 与 Stratagem Code
// 列的表格，并且每行必须解析出 >= 3 个方向，避免把债券名/导航表误当成战备。

use crate::stratagems::PluginStratagem;
use std::collections::HashSet;
use std::io::Read;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

/// 权威数据源：helldivers.wiki.gg 的战备总览页
pub const STRATAGEM_DATA_URL: &str = "https://helldivers.wiki.gg/wiki/Stratagems";

const API_ROOT: &str = "https://helldivers.wiki.gg/api.php";
const USER_AGENT: &str =
    "h2ac-rs/1.0 (Helldivers 2 Auto Stratagem Caller; wiki.gg Stratagems fetcher)";

/// 部门 ID / 部门名 → 显示名映射（与图标包目录名一致）
const CATEGORY_MAP: &[(&str, &str)] = &[
    (
        "3958dc9e-712f-4377-85e9-fec4b6a6442a",
        "Patriotic Administration Center",
    ),
    ("3958dc9e-742f-4377-85e9-fec4b6a6442a", "Orbital Cannons"),
    ("3958dc9e-737f-4377-85e9-fec4b6a6442a", "Hangar"),
    ("50ca3e18-62cd-11ee-8c99-0242ac120002", "Bridge"),
    ("3958dc9e-787f-4377-85e9-fec4b6a6442a", "Engineering Bay"),
    ("76d65c26-f784-44a2-ac19-586678f7c2f2", "Robotics Workshop"),
    ("06a89b98-cc7a-46ac-a8fb-7bbf12d5cb78", "Chemical Agents"),
    ("de1fb4c0-9ae8-4690-af44-90325cf11978", "Urban Legends"),
    (
        "f701e133-1fff-466c-8c84-3e99154ff778",
        "Servants of Freedom",
    ),
    ("ee78a618-c92b-48e4-a514-b849a8ad0859", "Borderline Justice"),
    ("d6e15727-9fe1-4961-8c5b-ea44a9bd81aa", "General Stratagems"),
    (
        "86a708cf-8def-4244-a86a-7e7680632807",
        "Masters of Ceremony",
    ),
    ("aa878f90-85b0-4ea6-b7ef-3097bc0effd8", "Force of Law"),
    ("347b11f0-ef3e-49ae-af7e-f16d02a0f8eb", "Control Group"),
    ("1fbf16b0-2726-4188-9f9d-11cd31224168", "Dust Devils"),
    ("77fda1c3-b8c9-4d13-a1ba-4f6e036b65f1", "Python Commandos"),
    ("8877a3a0-668a-4495-9606-e35a6d719cb4", "Redacted Regiment"),
    ("d4f10c99-fe1b-4357-a927-343f216dc4c0", "Siege Breakers"),
    (
        "a63978b8-a411-4b06-bce7-abc2ffef418c",
        "Entrenched Division",
    ),
];

/// 部门 ID / 部门名 / 债券名 → App 的 8 大功能分类。
/// 实际总览页的 H2 章节为 Offensive Permit / Supply Permit / Defensive Permit / Other；
/// 债券名（Urban Legends 等）只出现在「获取途径」类表格里，不属于战备表格。
fn category_name(id_or_name: &str) -> String {
    let raw = id_or_name.trim();
    let dept = CATEGORY_MAP
        .iter()
        .find(|(cid, _)| *cid == raw)
        .map(|(_, n)| *n)
        .unwrap_or(raw);
    let norm = dept.to_lowercase().replace('_', " ");

    let mapped = match norm.as_str() {
        "patriotic administration center"
        | "support"
        | "support weapon"
        | "support weapons"
        | "engineering bay" => "Support Weapons",
        "orbital cannons" | "orbital" | "orbital strike" | "orbital strikes" | "bridge" => {
            "Orbital Strikes"
        }
        "hangar" | "eagle" | "eagle strike" | "eagle strikes" => "Eagle Strikes",
        "robotics workshop" | "sentry" | "sentries" => "Sentries",
        "urban legends" | "emplacement" | "emplacements" => "Emplacements",
        "servants of freedom" | "backpack" | "backpacks" => "Backpacks",
        "borderline justice" => "Backpacks",
        "chemical agents" | "force of law" | "dust devils" | "python commandos"
        | "siege breakers" => "Support Weapons",
        "redacted regiment" => "Backpacks",
        "control group" => "Sentries",
        "entrenched division" => "Emplacements",
        "general stratagems"
        | "general"
        | "mission"
        | "missions"
        | "mission stratagems"
        | "masters of ceremony" => "Mission Stratagems",
        "vehicle" | "vehicles" | "mech" | "mechs" | "exosuit" | "exosuits" => "Vehicles",
        _ => dept,
    };
    mapped.to_string()
}

// ─── 网络 ───

fn build_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .user_agent(USER_AGENT)
        .build()
}

fn http_get(agent: &ureq::Agent, url: &str) -> Result<String, String> {
    let resp = agent
        .get(url)
        .call()
        .map_err(|e| format!("网络错误: {e}"))?;
    let mut body = String::new();
    resp.into_reader()
        .read_to_string(&mut body)
        .map_err(|e| format!("读取响应失败: {e}"))?;
    Ok(body)
}

fn api_page_url() -> String {
    format!("{API_ROOT}?action=parse&page=Stratagems&prop=text&format=json&formatversion=2")
}

/// 从 MediaWiki action=parse 的 JSON 响应中取出 content-only HTML
fn extract_api_html(body: &str) -> Result<String, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("API JSON 解析失败: {e}"))?;
    if let Some(info) = value.pointer("/error/info").and_then(|v| v.as_str()) {
        return Err(format!("Wiki API 返回错误: {info}"));
    }
    value
        .pointer("/parse/text")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| "Wiki API 响应中没有 parse.text".into())
}

/// 从远端拉取并解析战备数据（仅在线数据源；本地 HTML 快照只作结构参考，不参与运行时数据）
pub fn fetch_stratagems(
    on_progress: impl Fn(String) + Send + 'static,
    on_done: impl FnOnce(Result<Vec<PluginStratagem>, String>) + Send + 'static,
) {
    thread::spawn(move || {
        let agent = build_agent();

        on_progress("正在连接 helldivers.wiki.gg…".into());
        let mut last_error: Option<String> = None;
        let mut items: Vec<PluginStratagem> = Vec::new();

        // 首选：直接拉取战备总览页 HTML
        match http_get(&agent, STRATAGEM_DATA_URL) {
            Ok(html) => {
                on_progress("正在解析战备表格…".into());
                match parse_stratagems_html(&html) {
                    Ok(parsed) => items = parsed,
                    Err(e) => last_error = Some(e),
                }
            }
            Err(e) => {
                last_error = Some(e);
                on_progress("页面直连失败，改用 MediaWiki API…".into());
            }
        }

        // 回退：MediaWiki parse API 返回的 content-only HTML（结构相同）
        if items.is_empty() {
            on_progress("正在通过 MediaWiki API 获取页面…".into());
            match http_get(&agent, &api_page_url()).and_then(|body| extract_api_html(&body)) {
                Ok(html) => match parse_stratagems_html(&html) {
                    Ok(parsed) => items = parsed,
                    Err(e) => last_error = Some(e),
                },
                Err(e) => last_error = Some(e),
            }
        }

        if items.is_empty() {
            on_done(Err(
                last_error.unwrap_or_else(|| "未在页面中找到战备数据".into())
            ));
            return;
        }

        on_progress(format!("已解析 {} 条战备，正在比对…", items.len()));
        on_done(Ok(items));
    });
}

// ─── HTML 表格解析（零正则依赖） ───

#[derive(Debug)]
struct TableBlock {
    html: String,
    start: usize,
}

#[derive(Debug, Clone)]
struct StratagemRow {
    name: String,
    command: Vec<String>,
    cooldown: String,
    icon_hint: String,
}

/// 解析页面 HTML 中形如
/// `Icon | Name | Stratagem Code | Base Cooldown` 的战备表格。
/// 其他表格（债券列表、导航表）没有 Stratagem Code 列，会被忽略。
pub fn parse_stratagems_html(html: &str) -> Result<Vec<PluginStratagem>, String> {
    // 分类以页面结构为准（结构参考：本地保存的页面快照）：
    // - <details><summary>分类</summary> 内的表格 → 该分类；
    // - details 块内出现于前一张表之后的 <big><b>分类</b></big> 标签 → 新分类
    //   （如 Mission Stratagems 块内的 Objective / Unavailable）；
    // - 无 details 结构时回退到表格之前的最近章节标题映射。
    let cleaned = strip_non_content(html);
    let tables = extract_tables(&cleaned);
    let details = extract_details_blocks(&cleaned);

    let mut rows: Vec<(String, StratagemRow)> = Vec::new();
    let mut qualifying_tables = 0usize;

    for table in tables {
        if !table_qualifies(&table.html) {
            continue;
        }
        qualifying_tables += 1;
        let category = table_group_category(&cleaned, &details, table.start)
            .or_else(|| {
                let heading = last_heading_before(&cleaned, table.start);
                (!heading.is_empty()).then(|| category_name(&heading))
            })
            .unwrap_or_else(|| "Mission Stratagems".to_string());
        for row in parse_table_rows(&table.html) {
            rows.push((category.clone(), row));
        }
    }

    // 页面结构变化时的最后防线：全文扫描所有 tr，但每行仍必须带 >= 3 个方向图标
    if qualifying_tables == 0 {
        for row in parse_table_rows(&cleaned) {
            rows.push((String::new(), row));
        }
    }

    if rows.is_empty() {
        return Err("未在页面中找到包含 Name / Stratagem Code 列的战备表格".into());
    }

    let mut out: Vec<PluginStratagem> = Vec::new();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    for (category, row) in rows {
        let description = if row.cooldown.is_empty() {
            String::new()
        } else {
            format!("基础冷却 {}", row.cooldown)
        };
        let icon = icon_key(&row.name, &row.icon_hint);
        let icon_url = icon_download_url(&row.name, &row.icon_hint);
        let item = PluginStratagem {
            name: row.name,
            category,
            model: String::new(),
            command: row.command,
            description,
            icon,
            source: crate::plugin::WIKI_SOURCE.to_string(),
            icon_url,
        };

        let dedup_key = (item.command.join(","), item.name.to_lowercase());
        if seen.insert(dedup_key) {
            out.push(item);
        }
    }

    if out.is_empty() {
        return Err("未在页面中解析到任何战备数据".into());
    }
    Ok(out)
}

// ─── 页面分组（details / big-b 标签）→ 分类 ───

#[derive(Debug)]
struct DetailsBlock {
    start: usize,
    end: usize,
    summary: String,
}

/// 提取顶层 <details> ... </details> 块（含嵌套深度追踪）
fn extract_details_blocks(html: &str) -> Vec<DetailsBlock> {
    let mut out = Vec::new();
    let mut pos = 0usize;

    while let Some(start) = find_open_tag_ci(html, "details", pos) {
        let Some(open_end) = find_tag_end(html, start) else {
            break;
        };
        let mut cursor = open_end + 1;
        let mut depth = 1usize;
        let mut close: Option<usize> = None;

        while cursor < html.len() {
            let next_open = find_open_tag_ci(html, "details", cursor);
            let next_close = find_tag_ci(html, "</details", cursor);
            match (next_open, next_close) {
                (Some(open), Some(close_pos)) if open < close_pos => {
                    depth += 1;
                    let Some(open_end) = find_tag_end(html, open) else {
                        break;
                    };
                    cursor = open_end + 1;
                }
                (_, Some(close_pos)) => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(close_pos);
                        break;
                    }
                    cursor = close_pos + "</details".len();
                }
                _ => break,
            }
        }

        let Some(close_pos) = close else { break };
        let block = &html[start..close_pos];
        out.push(DetailsBlock {
            start,
            end: close_pos,
            summary: details_summary_text(block),
        });
        pos = close_pos + "</details".len();
    }

    out
}

/// 提取 <summary>文本</summary> 的内容文本
fn details_summary_text(block: &str) -> String {
    let Some(open) = find_open_tag_ci(block, "summary", 0) else {
        return String::new();
    };
    let Some(tag_end) = find_tag_end(block, open) else {
        return String::new();
    };
    let content_start = tag_end + 1;
    let Some(close) = find_tag_ci(block, "</summary", content_start) else {
        return String::new();
    };
    clean_text(&strip_tags(&block[content_start..close]))
}

/// 段内出现的 <big><b>标签文本</b></big>（返回段内字节偏移 + 文本）
fn big_b_labels(segment: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut pos = 0usize;

    while let Some(open) = find_open_tag_ci(segment, "big", pos) {
        let Some(tag_end) = find_tag_end(segment, open) else {
            break;
        };
        let content_start = tag_end + 1;
        let Some(close) = find_tag_ci(segment, "</big", content_start) else {
            break;
        };
        let text = clean_text(&strip_tags(&segment[content_start..close]));
        if !text.is_empty() {
            out.push((open, text));
        }
        pos = close + "</big".len();
    }

    out
}

/// 表格所属的页面分组分类：
/// 1. 表格位于 <details> 块内 → 取 summary 文本；
///    但若最近一个 <big><b> 标签出现在块内第一张表之后，说明它开启了新分组
///    （Objective / Unavailable），以标签文本为分类。
/// 2. 无 details 块时返回 None（调用方回退到章节标题映射）。
fn table_group_category(
    html: &str,
    details: &[DetailsBlock],
    table_start: usize,
) -> Option<String> {
    let block = details
        .iter()
        .find(|d| d.start < table_start && table_start < d.end)?;

    // 最近一个位于表格之前、details 块之内的 <big><b> 标签
    let labels = big_b_labels(&html[block.start..table_start]);
    if let Some((label_pos, text)) = labels.last() {
        // 标签之前若已有 </table>（即块内已出现过一张表），该标签即为新分组
        let before_label = &html[block.start..block.start + label_pos];
        if find_tag_ci(before_label, "</table", 0).is_some() {
            return Some(text.clone());
        }
    }

    if block.summary.is_empty() {
        None
    } else {
        Some(block.summary.clone())
    }
}

fn strip_non_content(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while !rest.is_empty() {
        if rest.starts_with("<!--") {
            if let Some(end) = rest.find("-->") {
                rest = &rest[end + 3..];
                continue;
            }
        } else if starts_with_ci(rest, "<script") {
            if let Some(end) = find_tag_ci(rest, "</script", 0) {
                rest = &rest[end + "</script".len()..];
                continue;
            }
        } else if starts_with_ci(rest, "<style") {
            if let Some(end) = find_tag_ci(rest, "</style", 0) {
                rest = &rest[end + "</style".len()..];
                continue;
            }
        }
        let ch = rest.chars().next().unwrap();
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    out
}

fn starts_with_ci(s: &str, prefix: &str) -> bool {
    s.get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

/// 大小写不敏感的字节串查找。返回的是字节偏移；匹配串是 ASCII 标签，
/// 因此匹配位置必然落在 UTF-8 字符边界上，可安全切片。
fn find_tag_ci(haystack: &str, needle: &str, from: usize) -> Option<usize> {
    if from > haystack.len() {
        return None;
    }
    haystack.as_bytes()[from..]
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
        .map(|pos| from + pos)
}

/// 查找 `<tag ...>` 开标签（避免把 `<track` 当成 `<tr`、把 `<image` 当成 `<img`）
fn find_open_tag_ci(html: &str, tag: &str, from: usize) -> Option<usize> {
    let needle = format!("<{tag}");
    let mut pos = from;
    while let Some(index) = find_tag_ci(html, &needle, pos) {
        let after = index + needle.len();
        let boundary = html.as_bytes().get(after).copied().unwrap_or(b'>');
        if boundary == b'>' || boundary == b'/' || boundary.is_ascii_whitespace() {
            return Some(index);
        }
        pos = after;
    }
    None
}

fn find_tag_end(html: &str, tag_start: usize) -> Option<usize> {
    html[tag_start..].find('>').map(|pos| tag_start + pos)
}

/// 提取顶层 `<table> ... </table>` 块（含嵌套深度追踪）
fn extract_tables(html: &str) -> Vec<TableBlock> {
    let mut out = Vec::new();
    let mut pos = 0usize;

    while let Some(start) = find_open_tag_ci(html, "table", pos) {
        let Some(open_end) = find_tag_end(html, start) else {
            break;
        };
        let mut cursor = open_end + 1;
        let mut depth = 1usize;
        let mut close: Option<usize> = None;

        while cursor < html.len() {
            let next_open = find_open_tag_ci(html, "table", cursor);
            let next_close = find_tag_ci(html, "</table", cursor);
            match (next_open, next_close) {
                (Some(open), Some(close_pos)) if open < close_pos => {
                    depth += 1;
                    let Some(open_end) = find_tag_end(html, open) else {
                        break;
                    };
                    cursor = open_end + 1;
                }
                (_, Some(close_pos)) => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(close_pos);
                        break;
                    }
                    cursor = close_pos + "</table".len();
                }
                _ => break,
            }
        }

        let Some(close_pos) = close else { break };
        out.push(TableBlock {
            html: html[start..close_pos].to_string(),
            start,
        });
        pos = close_pos + "</table".len();
    }

    out
}

fn table_qualifies(table: &str) -> bool {
    let lower = table.to_lowercase();
    lower.contains("stratagem code") && lower.contains("name") && lower.contains("<th")
}

/// 表格块中逐行提取；跳过表头行（含 `<th>` 的 tr）
fn parse_table_rows(block: &str) -> Vec<StratagemRow> {
    let mut rows = Vec::new();
    let mut pos = 0usize;

    while let Some(tr_start) = find_open_tag_ci(block, "tr", pos) {
        let Some(tag_end) = find_tag_end(block, tr_start) else {
            break;
        };
        let content_start = tag_end + 1;
        let Some(row_end) = find_matching_tr_end(block, content_start) else {
            break;
        };
        let row = &block[content_start..row_end];

        // 跳过纯表头行（只有 th、没有 td）。若 wikitable 把行首名称写成
        // `<th scope="row">`，行内仍含 td 与方向图标，照常解析。
        let lower_row = row.to_lowercase();
        if lower_row.contains("<th") && !lower_row.contains("<td") {
            pos = row_end + 5;
            continue;
        }
        if let Some(data) = parse_row(row) {
            rows.push(data);
        }
        pos = row_end + 5;
    }

    rows
}

fn find_matching_tr_end(block: &str, content_start: usize) -> Option<usize> {
    let mut cursor = content_start;
    let mut depth = 1usize;

    while cursor < block.len() {
        let next_open = find_open_tag_ci(block, "tr", cursor);
        let next_close = find_tag_ci(block, "</tr", cursor);
        match (next_open, next_close) {
            (_, Some(close)) if next_open.is_none_or(|open| close < open) => {
                depth -= 1;
                if depth == 0 {
                    return Some(close);
                }
                cursor = close + 5;
            }
            (Some(open), _) => {
                depth += 1;
                let open_end = find_tag_end(block, open)?;
                cursor = open_end + 1;
            }
            _ => return None,
        }
    }
    None
}

fn split_cells(row: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut pos = 0usize;

    while let Some(start) = next_cell_start(row, pos) {
        let Some(tag_end) = find_tag_end(row, start) else {
            break;
        };
        let content_start = tag_end + 1;
        let next_cell = next_cell_start(row, content_start);
        let next_tr_end = find_tag_ci(row, "</tr", content_start);
        let end = match (next_cell, next_tr_end) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => row.len(),
        };

        cells.push(row[content_start..end].to_string());
        if end == row.len() || next_tr_end == Some(end) {
            break;
        }
        pos = end;
    }

    cells
}

/// 行内下一个 `<td>` / `<th>` 开标签的起始位置
fn next_cell_start(row: &str, from: usize) -> Option<usize> {
    let td = find_open_tag_ci(row, "td", from);
    let th = find_open_tag_ci(row, "th", from);
    match (td, th) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn parse_row(row: &str) -> Option<StratagemRow> {
    let cells = split_cells(row);
    if cells.len() < 3 {
        return None;
    }

    // 1) 找到包含完整方向序列的单元格（3 个及以上方向图标）
    let mut code_idx: Option<usize> = None;
    let mut command: Vec<String> = Vec::new();
    for (idx, cell) in cells.iter().enumerate() {
        let dirs = directions_in_cell(cell);
        if dirs.len() >= 3 {
            code_idx = Some(idx);
            command = dirs;
            break;
        }
    }
    let code_idx = code_idx?;

    // 2) 名称列：优先取代码列之前的非 File 链接；名称通常在第 2 列
    let name = cells
        .iter()
        .take(code_idx)
        .rev()
        .find_map(|cell| non_file_anchor_text(cell))
        .or_else(|| {
            cells
                .iter()
                .enumerate()
                .filter(|(idx, _)| *idx != code_idx)
                .find_map(|(_, cell)| non_file_anchor_text(cell))
        })
        .or_else(|| {
            cells
                .iter()
                .take(code_idx)
                .rev()
                .find_map(|cell| plain_cell_text(cell))
        })?;

    let name = clean_text(&name);
    if name.is_empty() || is_non_stratagem_name(&name) {
        return None;
    }

    // 3) 图标提示：代码列之前第一个带 alt/src 的 <img>（通常是 Icon 列）
    let icon_hint = cells
        .iter()
        .take(code_idx)
        .find_map(|cell| img_alt_text(cell))
        .unwrap_or_default();

    // 4) 冷却：代码列之后的第一个非空单元格文本
    let cooldown = cells
        .iter()
        .enumerate()
        .skip(code_idx + 1)
        .find_map(|(_, cell)| {
            let text = clean_text(&strip_tags(cell));
            if text.is_empty() || text.eq_ignore_ascii_case("n/a") {
                None
            } else {
                Some(text)
            }
        })
        .unwrap_or_default();

    Some(StratagemRow {
        name,
        command,
        cooldown,
        icon_hint,
    })
}

fn non_file_anchor_text(cell: &str) -> Option<String> {
    let mut pos = 0usize;
    while let Some(anchor) = find_open_tag_ci(cell, "a", pos) {
        let Some(tag_end) = find_tag_end(cell, anchor) else {
            break;
        };
        let tag = &cell[anchor..=tag_end];
        let href = extract_attr(tag, "href").unwrap_or_default();
        if is_non_article_href(&href) {
            pos = tag_end + 1;
            continue;
        }

        if let Some(title) = extract_attr(tag, "title") {
            let title = clean_text(&title);
            if !title.is_empty() {
                return Some(title);
            }
        }

        let Some(close) = find_tag_ci(cell, "</a", tag_end + 1) else {
            break;
        };
        let text = clean_text(&strip_tags(&cell[tag_end + 1..close]));
        if !text.is_empty() {
            return Some(text);
        }
        pos = close + 4;
    }
    None
}

fn is_non_article_href(href: &str) -> bool {
    let lower = href.trim().to_lowercase();
    lower.starts_with("/wiki/file:")
        || lower.starts_with("/wiki/category:")
        || lower.starts_with("/wiki/template:")
        || lower.starts_with("/wiki/special:")
        || lower.starts_with("/wiki/help:")
        || lower.starts_with("file:")
}

fn img_alt_text(cell: &str) -> Option<String> {
    let mut pos = 0usize;
    while let Some(img) = find_open_tag_ci(cell, "img", pos) {
        let Some(tag_end) = find_tag_end(cell, img) else {
            break;
        };
        let tag = &cell[img..=tag_end];
        if let Some(alt) = extract_attr(tag, "alt") {
            return Some(alt);
        }
        if let Some(src) = extract_attr(tag, "src") {
            return Some(src);
        }
        pos = tag_end + 1;
    }
    None
}

fn plain_cell_text(cell: &str) -> Option<String> {
    let text = clean_text(&strip_tags(cell));
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn directions_in_cell(cell: &str) -> Vec<String> {
    let mut dirs: Vec<String> = Vec::new();
    let mut pos = 0usize;

    while let Some(img) = find_open_tag_ci(cell, "img", pos) {
        let Some(tag_end) = find_tag_end(cell, img) else {
            break;
        };
        let tag = &cell[img..=tag_end];
        let hint = extract_attr(tag, "alt")
            .or_else(|| extract_attr(tag, "src"))
            .unwrap_or_default();
        if let Some(dir) = direction_from_text(&hint) {
            dirs.push(dir.to_string());
        }
        pos = tag_end + 1;
    }

    // 无 <img> 时退回纯文本方向解析（兼容 ↑↓←→ 或 up/down/left/right）
    if dirs.len() < 3 {
        dirs = parse_command_text(&strip_tags(cell));
    }
    dirs
}

/// 从图片 alt/src 之类的标记文本中识别方向。
/// "Stratagem Arrow Up.svg" → up；"arrow-down.png" → down；"→" → right。
fn direction_from_text(text: &str) -> Option<&'static str> {
    let decoded = html_unescape(text);
    let mut token = String::new();

    for ch in decoded.chars() {
        match ch {
            '↑' => return Some("up"),
            '↓' => return Some("down"),
            '←' => return Some("left"),
            '→' => return Some("right"),
            _ => {}
        }

        if ch.is_ascii_alphanumeric() {
            token.push(ch);
        } else {
            if let Some(dir) = direction_from_token(&token) {
                return Some(dir);
            }
            token.clear();
        }
    }
    direction_from_token(&token)
}

/// 顺序扫描文本中的方向记号（箭头字符 / up / down / left / right / UDLR 单字母）
fn parse_command_text(text: &str) -> Vec<String> {
    let decoded = html_unescape(text);
    let mut out = Vec::new();
    let mut token = String::new();

    for ch in decoded.chars() {
        let direct = match ch {
            '↑' => Some("up"),
            '↓' => Some("down"),
            '←' => Some("left"),
            '→' => Some("right"),
            _ => None,
        };
        if let Some(dir) = direct {
            out.push(dir.to_string());
            token.clear();
            continue;
        }

        if ch.is_ascii_alphanumeric() {
            token.push(ch);
        } else {
            if let Some(dir) = direction_from_token(&token) {
                out.push(dir.to_string());
            }
            token.clear();
        }
    }
    if let Some(dir) = direction_from_token(&token) {
        out.push(dir.to_string());
    }

    out
}

fn direction_from_token(token: &str) -> Option<&'static str> {
    let lower = token.to_lowercase();
    match lower.as_str() {
        "up" | "u" => Some("up"),
        "down" | "d" => Some("down"),
        "left" | "l" => Some("left"),
        "right" | "r" => Some("right"),
        _ => None,
    }
}

/// 从 `<td>0s</td>` 之类的片段剥掉标签并还原实体
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while !rest.is_empty() {
        if rest.starts_with('<') {
            if let Some(end) = rest.find('>') {
                rest = &rest[end + 1..];
                continue;
            }
        }
        let ch = rest.chars().next().unwrap();
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    html_unescape(&out)
}

fn clean_text(s: &str) -> String {
    let mut text = html_unescape(s);
    if let Some(pos) = text.find("[edit]") {
        text.truncate(pos);
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn html_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;

    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        if let Some(semi) = after.find(';') {
            let entity = &after[..semi];
            out.push_str(&decode_entity(entity));
            rest = &after[semi + 1..];
        } else {
            out.push('&');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

fn decode_entity(entity: &str) -> String {
    match entity {
        "amp" => "&".into(),
        "lt" => "<".into(),
        "gt" => ">".into(),
        "quot" => "\"".into(),
        "apos" => "'".into(),
        "nbsp" => " ".into(),
        "uarr" => "↑".into(),
        "darr" => "↓".into(),
        "larr" => "←".into(),
        "rarr" => "→".into(),
        "harr" => "↔".into(),
        "ndash" => "–".into(),
        "mdash" => "—".into(),
        "hellip" => "…".into(),
        _ => {
            if let Some(hex) = entity
                .strip_prefix("#x")
                .or_else(|| entity.strip_prefix("#X"))
                .and_then(|digits| u32::from_str_radix(digits, 16).ok())
            {
                return char::from_u32(hex)
                    .map(|c| c.to_string())
                    .unwrap_or_default();
            }
            if let Some(dec) = entity
                .strip_prefix('#')
                .and_then(|digits| digits.parse::<u32>().ok())
            {
                return char::from_u32(dec)
                    .map(|c| c.to_string())
                    .unwrap_or_default();
            }
            format!("&{entity};")
        }
    }
}

fn extract_attr(tag: &str, name: &str) -> Option<String> {
    let mut pos = 0usize;
    while pos < tag.len() {
        let Some(start) = find_tag_ci(tag, name, pos) else {
            break;
        };
        let end = start + name.len();

        let before_ok = start == 0 || {
            let before = tag.as_bytes()[start - 1];
            !before.is_ascii_alphanumeric()
        };
        let after = tag.as_bytes().get(end).copied().unwrap_or(b' ');
        let after_ok = !after.is_ascii_alphanumeric();
        if before_ok && after_ok {
            let mut cursor = end;
            while cursor < tag.len() && tag.as_bytes()[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            if cursor < tag.len() && tag.as_bytes()[cursor] == b'=' {
                cursor += 1;
                while cursor < tag.len() && tag.as_bytes()[cursor].is_ascii_whitespace() {
                    cursor += 1;
                }
                if cursor >= tag.len() {
                    return Some(String::new());
                }
                let quote = tag.as_bytes()[cursor];
                if quote == b'"' || quote == b'\'' {
                    let value_start = cursor + 1;
                    let rest = &tag[value_start..];
                    let len = rest.find(quote as char)?;
                    return Some(html_unescape(&rest[..len]));
                }
                let value_start = cursor;
                let rest = &tag[value_start..];
                let len = rest
                    .find(|c: char| c == '>' || c.is_ascii_whitespace())
                    .unwrap_or(rest.len());
                return Some(html_unescape(&rest[..len]));
            }
            return Some(String::new());
        }
        pos = end;
    }
    None
}

/// 表格开始位置之前最近的一个章节标题文本（用于分类：债券名 → 功能分类）
fn last_heading_before(html: &str, before: usize) -> String {
    let prefix = &html[..before.min(html.len())];
    let mut last = String::new();
    let mut pos = 0usize;

    while pos < prefix.len() {
        let mut found: Option<(usize, usize)> = None;
        for level in 1..=6usize {
            let tag = format!("h{level}");
            if let Some(index) = find_open_tag_ci(prefix, &tag, pos) {
                if found.is_none_or(|(_, current)| index < current) {
                    found = Some((index, level));
                }
            }
        }

        let Some((open, level)) = found else { break };
        let Some(tag_end) = find_tag_end(prefix, open) else {
            break;
        };
        let content_start = tag_end + 1;
        let close_tag = format!("</h{level}");
        let Some(close) = find_tag_ci(prefix, &close_tag, content_start) else {
            break;
        };
        last = clean_text(&strip_tags(&prefix[content_start..close]));
        pos = close + close_tag.len();
    }

    last
}

fn is_non_stratagem_name(name: &str) -> bool {
    const NON_STRATAGEMS: &[&str] = &[
        "patriotic administration center",
        "orbital cannons",
        "hangar",
        "bridge",
        "engineering bay",
        "robotics workshop",
        "general stratagems",
        "urban legends",
        "servants of freedom",
        "borderline justice",
        "chemical agents",
        "masters of ceremony",
        "force of law",
        "control group",
        "dust devils",
        "python commandos",
        "redacted regiment",
        "siege breakers",
        "entrenched division",
    ];
    let lower = name.trim().to_lowercase();
    NON_STRATAGEMS.contains(&lower.as_str())
        || lower.ends_with(".svg")
        || lower.ends_with(".png")
        || lower.starts_with("file:")
}

/// helldivers.wiki.gg 的图标文件都在 /images/ 扁平目录下，
/// 文件全名与页面图标格的 alt/src 一致（空格 → 下划线，其余字符 percent 编码）
const ICON_FILE_BASE: &str = "https://helldivers.wiki.gg/images/";

/// 图标源图地址推导：
/// 1. hint 为绝对 URL（http(s)/协议相对）→ 直接使用；
/// 2. hint 是带图片扩展名的文件名（如 "Eagle Rearm Stratagem Icon Background.svg"）
///    → 拼成 wiki /images/ 直链；
/// 3. 兜底：按页面惯例 "<名称> Stratagem Icon Background.svg" 生成候选地址。
/// 本地已有该 icon 键时不会触发下载，因此候选地址允许保守宽松。
fn icon_download_url(name: &str, hint: &str) -> Option<String> {
    let t = hint.trim();
    if !t.is_empty() {
        if t.starts_with("http://") || t.starts_with("https://") {
            return Some(t.to_string());
        }
        if t.starts_with("//") {
            return Some(format!("https:{t}"));
        }
        let lower = t.to_lowercase();
        let has_img_ext = [".svg", ".png", ".webp", ".jpg", ".jpeg", ".gif"]
            .iter()
            .any(|ext| lower.ends_with(ext));
        if has_img_ext && !t.contains('/') {
            return Some(format!("{ICON_FILE_BASE}{}", file_path(t)));
        }
    }
    let candidate = format!("{name} Stratagem Icon Background.svg");
    Some(format!("{ICON_FILE_BASE}{}", file_path(&candidate)))
}

/// MediaWiki 文件路径化：空格 → 下划线，其余非 unreserved 字节 percent 编码
fn file_path(title: &str) -> String {
    let mut out = String::with_capacity(title.len() + 8);
    for b in title.bytes() {
        match b {
            b' ' => out.push('_'),
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 英文名 → snake_case 图标 key
fn name_to_snake(name: &str) -> String {
    let s = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    let parts: Vec<&str> = s.split('_').filter(|p| !p.is_empty()).collect();
    parts.join("_")
}

fn icon_key(name: &str, icon_hint: &str) -> String {
    if let Some(key) = icon_hint_key(icon_hint) {
        return key;
    }
    name_to_snake(name)
}

/// "Eagle Rearm Stratagem Icon Background.svg" → "eagle_rearm"
fn icon_hint_key(hint: &str) -> Option<String> {
    let mut text = clean_text(hint);
    let lower = text.to_lowercase();
    for ext in [".svg", ".png", ".webp", ".jpg"] {
        if lower.ends_with(ext) {
            text.truncate(text.len() - ext.len());
            break;
        }
    }

    let lower = text.to_lowercase();
    for suffix in [
        "stratagem icon background",
        "stratagem icon",
        "icon background",
        "icon",
    ] {
        if lower.ends_with(suffix) {
            let keep = text.len() - suffix.len();
            text = text[..keep].trim().to_string();
            break;
        }
    }

    if text.is_empty() {
        None
    } else {
        Some(name_to_snake(&text))
    }
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
            let _ = tx_progress.send(FetchProgress {
                stage: msg,
                done: false,
                result: None,
            });
        },
        move |result| {
            let _ = tx_done.send(FetchProgress {
                stage: String::new(),
                done: true,
                result: Some(result),
            });
        },
    );

    (rx, has_cache)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("fixtures/wiki_stratagems.html");

    #[test]
    fn parse_fixture() {
        let result = parse_stratagems_html(SAMPLE);
        assert!(result.is_ok(), "parse failed: {:?}", result.err());
        let items = result.unwrap();
        assert_eq!(items.len(), 5);

        assert_eq!(items[0].name, "Eagle Rearm");
        assert_eq!(items[0].category, "Mission Stratagems");
        assert_eq!(items[0].command, vec!["up", "up", "left", "up", "right"]);
        assert_eq!(items[0].icon, "eagle_rearm");
        assert_eq!(items[0].description, "基础冷却 0s");
        assert_eq!(items[0].source, crate::plugin::WIKI_SOURCE);

        assert_eq!(items[1].name, "Resupply");
        assert_eq!(items[1].command, vec!["down", "down", "up", "right"]);
        assert_eq!(items[1].description, "基础冷却 180s");

        assert_eq!(items[2].name, "Call In Super Destroyer");
        assert_eq!(
            items[2].command,
            vec!["up", "up", "down", "down", "left", "right", "left", "right"]
        );
        assert_eq!(items[2].description, "");

        assert_eq!(items[3].name, "Reinforce");
        assert_eq!(items[3].command, vec!["up", "down", "right", "left", "up"]);

        assert_eq!(items[4].name, "SoS Beacon");
        assert_eq!(items[4].command, vec!["up", "down", "right", "up"]);
        assert_eq!(items[4].icon, "sos_beacon");
        // 不再附加任何 new / wiki 标签
        assert!(items.iter().all(|i| !i.name.contains("(Wiki)")));
        assert!(items.iter().all(|i| i.category != "NEW (Wiki)"));
    }

    #[test]
    fn ignores_warbond_tables_without_stratagem_code() {
        // fixture 中第二个表格是 Urban Legends 债券表：没有 Stratagem Code 列，
        // 解析结果必须只有 5 条战备，不能把债券名算进去。
        let items = parse_stratagems_html(SAMPLE).unwrap();
        assert_eq!(items.len(), 5);
        assert!(items.iter().all(|item| item.name != "Urban Legends"));
    }

    #[test]
    fn details_summary_and_big_b_labels_drive_categories() {
        // 结构参考自本地保存的页面快照：details/summary 为分类，
        // 块内第一张表之前的 <big><b> 子标签沿用 summary 分类，
        // 前一张表之后的 <big><b> 标签开启新分类（Objective / Unavailable）。
        let html = r#"
        <h2><span class="mw-headline">List of Stratagems</span></h2>
        <h3><span class="mw-headline">Offensive Permit</span></h3>
        <details><summary>Orbital Strikes</summary>
        <table class="wikitable"><thead><tr><th>Icon</th><th>Name</th><th>Stratagem Code</th><th>Base Cooldown</th></tr></thead><tbody>
        <tr>
        <td><img alt="Orbital Precision Strike Stratagem Icon Background.svg"></td>
        <td><a href="/wiki/Orbital_Precision_Strike" title="Orbital Precision Strike">Orbital Precision Strike</a></td>
        <td><img alt="Stratagem Arrow Right.svg"><img alt="Stratagem Arrow Right.svg"><img alt="Stratagem Arrow Down.svg"></td>
        <td>100s</td>
        </tr>
        </tbody></table>
        </details>
        <h3><span class="mw-headline">Other</span></h3>
        <details><summary>Mission Stratagems</summary>
        <big><b>Ship</b></big>
        <table class="wikitable"><thead><tr><th>Icon</th><th>Name</th><th>Stratagem Code</th><th>Base Cooldown</th></tr></thead><tbody>
        <tr>
        <td><img alt="Reinforce Stratagem Icon Background.svg"></td>
        <td><a href="/wiki/Reinforce" title="Reinforce">Reinforce</a></td>
        <td><img alt="Stratagem Arrow Up.svg"><img alt="Stratagem Arrow Down.svg"><img alt="Stratagem Arrow Right.svg"><img alt="Stratagem Arrow Left.svg"><img alt="Stratagem Arrow Up.svg"></td>
        <td>0s</td>
        </tr>
        </tbody></table>
        <p><big><b>Objective</b></big></p>
        <table class="wikitable"><thead><tr><th>Icon</th><th>Name</th><th>Stratagem Code</th><th>Base Cooldown</th></tr></thead><tbody>
        <tr>
        <td><img alt="SEAF Artillery Stratagem Icon Background.svg"></td>
        <td><a href="/wiki/SEAF_Artillery" title="SEAF Artillery">SEAF Artillery</a></td>
        <td><img alt="Stratagem Arrow Right.svg"><img alt="Stratagem Arrow Up.svg"><img alt="Stratagem Arrow Up.svg"><img alt="Stratagem Arrow Down.svg"></td>
        <td>0s</td>
        </tr>
        </tbody></table>
        <p><big><b>Unavailable</b></big></p>
        <table class="wikitable"><thead><tr><th>Icon</th><th>Name</th><th>Stratagem Code</th><th>Base Cooldown</th></tr></thead><tbody>
        <tr>
        <td><img alt="Orbital Illumination Flare Stratagem Icon Background.svg"></td>
        <td><a href="/wiki/Orbital_Illumination_Flare" title="Orbital Illumination Flare">Orbital Illumination Flare</a></td>
        <td><img alt="Stratagem Arrow Right.svg"><img alt="Stratagem Arrow Right.svg"><img alt="Stratagem Arrow Up.svg"><img alt="Stratagem Arrow Left.svg"></td>
        <td>0s</td>
        </tr>
        </tbody></table>
        </details>
        "#;
        let items = parse_stratagems_html(html).unwrap();
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].name, "Orbital Precision Strike");
        assert_eq!(items[0].category, "Orbital Strikes");
        assert_eq!(items[0].description, "基础冷却 100s");
        assert_eq!(items[1].name, "Reinforce");
        assert_eq!(items[1].category, "Mission Stratagems");
        assert_eq!(items[2].name, "SEAF Artillery");
        assert_eq!(items[2].category, "Objective");
        assert_eq!(items[3].name, "Orbital Illumination Flare");
        assert_eq!(items[3].category, "Unavailable");
    }

    /// 在线连通性验证（默认忽略）：直接拉取线上页面并解析，
    /// 用于在页面结构变化时第一时间发现解析器失效。
    #[test]
    #[ignore]
    fn fetch_live_page_parses() {
        let agent = build_agent();
        let html = http_get(&agent, STRATAGEM_DATA_URL).expect("拉取线上页面失败");
        let items = parse_stratagems_html(&html).expect("线上页面解析失败");
        assert!(!items.is_empty());
        assert!(items.iter().all(|i| !i.name.contains("(Wiki)")));
        eprintln!("线上页面解析出 {} 条战备", items.len());
        let mut counts = std::collections::HashMap::new();
        for i in &items {
            *counts.entry(i.category.clone()).or_insert(0usize) += 1;
        }
        eprintln!("分类分布: {counts:?}");
    }

    #[test]
    fn parse_local_snapshot_if_present() {
        // 结构参考测试：仓库根目录的本地页面快照用于验证解析器与真实页面结构一致；
        // 快照只作结构与解析验证，运行时数据一律在线获取，绝不读取本地快照。
        const SNAPSHOT: &str = "Stratagems - The Helldivers Wiki.html";
        let Ok(html) = std::fs::read_to_string(SNAPSHOT) else {
            eprintln!("skip: 本地页面快照不存在（仅作结构参考）");
            return;
        };
        let items =
            parse_stratagems_html(&html).unwrap_or_else(|e| panic!("快照解析失败: {e}"));
        assert!(items.len() > 100, "快照应解析出上百条战备，实际 {}", items.len());
        let valid: HashSet<&str> = [
            "Orbital Strikes", "Eagle Strikes", "Support Weapons", "Backpacks",
            "Vehicles", "Sentries", "Emplacements", "Mission Stratagems",
            "Objective", "Unavailable",
        ]
        .into_iter()
        .collect();
        let unknown: Vec<&String> = items
            .iter()
            .map(|i| &i.category)
            .filter(|c| !valid.contains(c.as_str()))
            .collect();
        assert!(unknown.is_empty(), "存在未知分类: {unknown:?}");
        assert!(items.iter().all(|i| !i.name.contains("(Wiki)")));
        assert_eq!(items[0].category, "Orbital Strikes");
        assert!(items.iter().any(|i| i.name == "Eagle Rearm" && i.category == "Mission Stratagems"));
        assert!(items.iter().any(|i| i.name == "SEAF Artillery" && i.category == "Objective"));
        assert!(items.iter().any(|i| i.name == "Orbital Illumination Flare" && i.category == "Unavailable"));
    }

    #[test]
    fn parse_empty_or_garbage_returns_err() {
        assert!(parse_stratagems_html("").is_err());
        assert!(parse_stratagems_html("<html><body>no tables here</body></html>").is_err());
    }

    #[test]
    fn ignores_other_tables_even_when_they_contain_arrows() {
        let html = r#"
        <h2><span class="mw-headline">Urban Legends</span></h2>
        <table class="wikitable"><thead><tr><th>Name</th><th>Arrows</th></tr></thead><tbody>
        <tr><td><a href="/wiki/Urban_Legends" title="Urban Legends">Urban Legends</a></td>
        <td><img alt="Stratagem Arrow Up.svg"><img alt="Stratagem Arrow Down.svg"><img alt="Stratagem Arrow Left.svg"></td></tr>
        </tbody></table>
        <h2><span class="mw-headline">General Stratagems</span></h2>
        <table class="wikitable"><thead><tr><th>Icon</th><th>Name</th><th>Stratagem Code</th><th>Base Cooldown</th></tr></thead><tbody>
        <tr>
        <td><img alt="Test Strike Stratagem Icon Background.svg"></td>
        <td><a href="/wiki/Test_Strike" title="Test Strike">Test Strike</a></td>
        <td><img alt="Stratagem Arrow Right.svg"><img alt="Stratagem Arrow Up.svg"><img alt="Stratagem Arrow Down.svg"></td>
        <td>5s</td>
        </tr>
        </tbody></table>
        "#;
        let items = parse_stratagems_html(html).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "Test Strike");
    }

    #[test]
    fn extract_api_html_reads_parse_text() {
        let json = r#"{"parse":{"text":"<table class=\"wikitable\"><thead><tr><th>Name</th><th>Stratagem Code</th></tr></thead><tbody></tbody></table>"}}"#;
        assert!(extract_api_html(json).unwrap().contains("Stratagem Code"));
    }

    #[test]
    fn category_name_maps_departments_and_warbonds() {
        assert_eq!(
            category_name("d6e15727-9fe1-4961-8c5b-ea44a9bd81aa"),
            "Mission Stratagems"
        );
        assert_eq!(
            category_name("3958dc9e-737f-4377-85e9-fec4b6a6442a"),
            "Eagle Strikes"
        );
        assert_eq!(
            category_name("3958dc9e-742f-4377-85e9-fec4b6a6442a"),
            "Orbital Strikes"
        );
        assert_eq!(category_name("General Stratagems"), "Mission Stratagems");
        assert_eq!(category_name("Urban Legends"), "Emplacements");
        assert_eq!(category_name("Servants of Freedom"), "Backpacks");
        assert_eq!(category_name("unknown-id"), "unknown-id");
    }

    #[test]
    fn icon_download_url_derives_images_link() {
        // 页面 alt/src 直接是文件名 → /images/ 直链（空格 → 下划线）
        let url = icon_download_url(
            "Eagle Rearm",
            "Eagle Rearm Stratagem Icon Background.svg",
        );
        assert_eq!(
            url.as_deref(),
            Some("https://helldivers.wiki.gg/images/Eagle_Rearm_Stratagem_Icon_Background.svg")
        );
        // 绝对 URL 原样返回
        assert_eq!(
            icon_download_url("X", "https://cdn.example.com/a%20b.png").as_deref(),
            Some("https://cdn.example.com/a%20b.png")
        );
        // 协议相对 URL → 补 https
        assert_eq!(
            icon_download_url("X", "//img.example/x.png").as_deref(),
            Some("https://img.example/x.png")
        );
        // hint 为空 → 名称惯例兜底
        let fallback = icon_download_url("Reinforce", "");
        assert_eq!(
            fallback.as_deref(),
            Some("https://helldivers.wiki.gg/images/Reinforce_Stratagem_Icon_Background.svg")
        );
    }

    #[test]
    fn file_path_encodes_like_mediawiki() {
        assert_eq!(file_path("SOS Beacon Icon.svg"), "SOS_Beacon_Icon.svg");
        assert_eq!(file_path("A\"B.svg"), "A%22B.svg");
        assert_eq!(file_path("K-9 Rover.svg"), "K-9_Rover.svg");
    }

    #[test]
    fn name_to_snake_maps_specials() {
        assert_eq!(name_to_snake("Eagle 500KG Bomb"), "eagle_500kg_bomb");
        assert_eq!(name_to_snake("RX-1 Railgun"), "rx_1_railgun");
        assert_eq!(name_to_snake("  Spaced  Out "), "spaced_out");
        assert_eq!(name_to_snake("“Guard Dog” K-9"), "guard_dog_k_9");
    }

    #[test]
    fn direction_detection_from_markup() {
        assert_eq!(direction_from_text("Stratagem Arrow Up.svg"), Some("up"));
        assert_eq!(direction_from_text("arrow-down.png"), Some("down"));
        assert_eq!(direction_from_text("→"), Some("right"));
        assert_eq!(direction_from_text("background"), None);
        assert_eq!(direction_from_text("&uarr;"), Some("up"));
        assert_eq!(
            parse_command_text("up right down down down"),
            vec!["up", "right", "down", "down", "down"]
        );
        assert_eq!(
            parse_command_text("↑→↓↓↓"),
            vec!["up", "right", "down", "down", "down"]
        );
    }

    #[test]
    fn html_unescape_decodes_entities() {
        assert_eq!(html_unescape("&amp;&lt;&gt;&quot;&apos;"), "&<>\"'");
        assert_eq!(html_unescape("&uarr;&darr;&larr;&rarr;"), "↑↓←→");
        assert_eq!(html_unescape("&#8593;&#x2192;"), "↑→");
        assert_eq!(html_unescape("&unknown;"), "&unknown;");
    }

    #[test]
    fn extract_attr_reads_quoted_and_unquoted_values() {
        let tag = r#"<img alt="Stratagem Arrow Up.svg" src='/x.png' width=50>"#;
        assert_eq!(
            extract_attr(tag, "alt").as_deref(),
            Some("Stratagem Arrow Up.svg")
        );
        assert_eq!(extract_attr(tag, "src").as_deref(), Some("/x.png"));
        assert_eq!(extract_attr(tag, "width").as_deref(), Some("50"));
        assert_eq!(extract_attr(tag, "height"), None);
    }

    #[test]
    fn strip_tags_keeps_text_content() {
        assert_eq!(
            strip_tags(r#"<a href="/wiki/Reinforce" title="Reinforce">Reinforce</a>"#),
            "Reinforce"
        );
        assert_eq!(clean_text(&strip_tags("0s\n")), "0s");
    }
}
