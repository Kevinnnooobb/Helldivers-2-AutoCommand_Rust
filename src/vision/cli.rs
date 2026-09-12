// 最终 CLI / 调试工具（需求 §37 的 `vision-debug`，外加 `vision-dataset` 与 `vision-eval`）。
//
// 用法：
//   cargo run -- vision-debug 截图.png [--target loadout|list] [--debug detailed] [--out 目录]
//   cargo run -- vision-dataset 截图目录 --out dataset [--target loadout] [--preview]
//   cargo run -- vision-eval 截图.png --labels labels.json [--target list]
//
// 说明：release 构建带 `windows_subsystem = "windows"`（无控制台），
// CLI 请在 `cargo run`（debug）或自行编译的 console 构建下使用。
use std::path::PathBuf;

use super::config::{VisionConfig, VisionDebugLevel};
use super::dataset::{self, DatasetOptions};
use super::debug::DebugSession;
use super::error::VisionError;
use super::recognize::{DetectionOutcome, LoadoutRecognition, RecognizeTarget, VisionEngine};
use super::roi::load_frame;

pub const CMD_DEBUG: &str = "vision-debug";
pub const CMD_DATASET: &str = "vision-dataset";
pub const CMD_EVAL: &str = "vision-eval";
pub const CMD_LABELS: &str = "vision-labels";
pub const CMD_CAPTURE: &str = "vision-capture";
pub const CMD_CATALOG: &str = "vision-catalog";
pub const CMD_PAGE: &str = "vision-page";
pub const CMD_NAVPLAN: &str = "vision-navplan";
pub const CMD_SELECTPLAN: &str = "vision-selectplan";

/// 帮助文本。
pub fn usage() -> String {
    [
        "H2AC-RS 视觉识别调试工具",
        "",
        &format!("用法：{CMD_DEBUG} <截图.png> [--target loadout|list] [--debug off|basic|detailed|trace] [--out 目录]"),
        &format!("      {CMD_DATASET} <截图目录|截图.png> --out <目录> [--target loadout|list] [--preview]"),
        &format!("      {CMD_EVAL} <截图.png> --labels <标签.json> [--target loadout|list]"),
        &format!("      {CMD_LABELS} <人类标注.json> [--out <标签.json>]"),
        &format!("      {CMD_CAPTURE} --out <截图.png> [--target loadout|list]   ← 只截图+识别，不发送任何输入"),
        &format!("      {CMD_CATALOG} [--root <参考资产目录>]   ← 校验参考 manifest 并输出迁移覆盖审计"),
        &format!("      {CMD_PAGE} <前.png> <后.png> [--direction up|down] [--target list]   ← 语义分页关系诊断"),
        &format!("      {CMD_NAVPLAN} [--direction up|down]   ← 打印有界翻页计划（只读，不发送任何输入）"),
        &format!("      {CMD_SELECTPLAN} [--profile <名称>]   ← 打印 direct_select 的目标计划（只读，不发送任何输入）"),
        "",
        "标签文件格式（JSON）：",
        r#"  { "target": "list", "cells": [ {"slot": 0, "id": "defoliation_tool"}, {"slot": 1, "empty": true} ] }"#,
        "  cells 中未出现的槽位不参与统计；id = 期望内部 ID；empty = 期望空格。",
        "",
        &format!("{CMD_LABELS} 把 screenshots/list.json 这类「逐行英文名」标注转成上面的 cells 格式："),
        r#"  { "screen": "list", "rows": [["Orbital Precision Strike", "..."], ...] }"#,
    ]
    .join("\n")
}

/// 命令分发：识别到本模块的命令时返回退出码，否则返回 None（由 GUI 继续启动）。
pub fn dispatch(args: &[String]) -> Option<i32> {
    let first = args.first()?.as_str();
    if !matches!(
        first,
        CMD_DEBUG
            | CMD_DATASET
            | CMD_EVAL
            | CMD_LABELS
            | CMD_CAPTURE
            | CMD_CATALOG
            | CMD_PAGE
            | CMD_NAVPLAN
            | CMD_SELECTPLAN
    ) {
        return None;
    }
    let result = match first {
        CMD_DEBUG => run_debug(&args[1..]),
        CMD_DATASET => run_dataset(&args[1..]),
        CMD_EVAL => run_eval(&args[1..]),
        CMD_LABELS => run_labels(&args[1..]),
        CMD_CAPTURE => run_capture(&args[1..]),
        CMD_CATALOG => run_catalog(&args[1..]),
        CMD_PAGE => run_page(&args[1..]),
        CMD_NAVPLAN => run_navplan(&args[1..]),
        CMD_SELECTPLAN => run_selectplan(&args[1..]),
        _ => unreachable!(),
    };
    Some(match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("错误 [{}]: {}", e.code(), e.message());
            eprintln!("\n{}", usage());
            2
        }
    })
}

/// 极简参数解析（位置参数 + `--flag value` + 布尔 flag）。
#[derive(Debug, Default)]
struct Args {
    positional: Vec<String>,
    flags: Vec<String>,
    options: Vec<(String, String)>,
}

fn parse_args(args: &[String]) -> Result<Args, VisionError> {
    let mut parsed = Args::default();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if let Some(name) = arg.strip_prefix("--") {
            let name = name.to_string();
            let next_is_value = args
                .get(index + 1)
                .map(|v| !v.starts_with("--"))
                .unwrap_or(false);
            if matches!(
                name.as_str(),
                "target" | "debug" | "out" | "labels" | "top-k"
            ) {
                let value = args.get(index + 1).filter(|v| !v.starts_with("--"));
                let Some(value) = value else {
                    return Err(VisionError::Usage {
                        detail: format!("--{name} 需要一个取值"),
                    });
                };
                parsed.options.push((name, value.clone()));
                index += 2;
                continue;
            }
            if !next_is_value {
                parsed.flags.push(name);
                index += 1;
                continue;
            }
            parsed.flags.push(name);
            index += 1;
        } else {
            parsed.positional.push(arg.clone());
            index += 1;
        }
    }
    Ok(parsed)
}

impl Args {
    fn option(&self, name: &str) -> Option<&str> {
        self.options
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    fn flag(&self, name: &str) -> bool {
        self.flags.iter().any(|f| f == name)
    }

    fn target(&self) -> Result<RecognizeTarget, VisionError> {
        match self.option("target").unwrap_or("loadout") {
            "loadout" | "home" => Ok(RecognizeTarget::Loadout),
            "list" => Ok(RecognizeTarget::List),
            other => Err(VisionError::Usage {
                detail: format!("未知 --target「{other}」（应为 loadout 或 list）"),
            }),
        }
    }

    fn debug_level(&self) -> Result<VisionDebugLevel, VisionError> {
        match self.option("debug").unwrap_or("basic") {
            "off" => Ok(VisionDebugLevel::Off),
            "basic" => Ok(VisionDebugLevel::Basic),
            "detailed" => Ok(VisionDebugLevel::Detailed),
            "trace" => Ok(VisionDebugLevel::Trace),
            other => Err(VisionError::Usage {
                detail: format!("未知 --debug「{other}」"),
            }),
        }
    }
}

fn build_engine(level: VisionDebugLevel, out: Option<&str>) -> Result<VisionEngine, VisionError> {
    let mut cfg = VisionConfig::default();
    cfg.debug.level = level;
    if let Some(dir) = out {
        cfg.debug.dump_dir = PathBuf::from(dir);
    }
    VisionEngine::new(cfg)
}

/// `vision-debug`：单张截图 → ROI / grid / mask / 识别结果 + 调试产物（需求 §37）。
fn run_debug(args: &[String]) -> Result<(), VisionError> {
    let parsed = parse_args(args)?;
    let Some(path) = parsed.positional.first() else {
        return Err(VisionError::Usage {
            detail: format!("{CMD_DEBUG} 需要一个截图路径"),
        });
    };
    let target = parsed.target()?;
    let level = parsed.debug_level()?;
    let engine = build_engine(level, parsed.option("out"))?;
    let frame = load_frame(std::path::Path::new(path))?;
    let output = engine.run(&frame, target)?;

    println!(
        "屏幕: {}x{}  ROI: ({},{},{},{})  几何来源: {:?}  耗时: {} ms",
        output.recognition.screen.0,
        output.recognition.screen.1,
        output.geometry.roi.x,
        output.geometry.roi.y,
        output.geometry.roi.w,
        output.geometry.roi.h,
        output.recognition.geometry_source,
        output.recognition.elapsed_ms
    );
    println!(
        "槽位: {} 个（{} 行 × {} 列）",
        output.plan.slots.len(),
        output.plan.rows,
        output.plan.cols
    );
    // 模板库加载情况：资源缺失必须显式可见，否则「识别不出」会被误当成算法问题。
    println!("{}", engine.template_report());
    println!();
    for line in output.topk_report() {
        println!("{line}");
    }
    println!();
    for line in recognition_lines(&output.recognition) {
        println!("{line}");
    }

    if level.writes_artifacts() {
        let session = DebugSession::create(&engine.config().debug, "vision-debug")?;
        session.dump_output(&output)?;
        println!("\n调试产物: {}", session.dir().display());
    }
    Ok(())
}

/// `vision-capture`：从**正在运行的游戏窗口**抓一帧 → 落盘 + 识别。
///
/// **只读**：绝不发送鼠标或键盘输入。它的用途是在真实游戏画面下取证，
/// 与 `vision-debug`（读磁盘图片）互补。
fn run_capture(args: &[String]) -> Result<(), VisionError> {
    let parsed = parse_args(args)?;
    let Some(out) = parsed.option("out") else {
        return Err(VisionError::Usage {
            detail: format!("{CMD_CAPTURE} 需要 --out <截图.png>"),
        });
    };
    let win = crate::loadout_sync::window::find_game_window().map_err(|e| VisionError::Io {
        path: "game-window".to_string(),
        detail: format!("找不到游戏窗口: {}", e.message()),
    })?;
    println!(
        "游戏窗口: hwnd={:?} client={}x{} 前台={}",
        win.hwnd,
        win.client.w,
        win.client.h,
        crate::loadout_sync::window::is_foreground(win.hwnd)
    );
    let frame = crate::loadout_sync::capture::capture_client_area(&win, true).map_err(|e| {
        VisionError::Io {
            path: "capture".to_string(),
            detail: format!("截图失败: {}", e.message()),
        }
    })?;
    frame
        .save_png(std::path::Path::new(out))
        .map_err(|e| VisionError::Io {
            path: out.to_string(),
            detail: format!("保存截图失败: {}", e.message()),
        })?;
    println!("已保存 {} ({}x{})", out, frame.width(), frame.height());

    let target = parsed.target()?;
    let level = parsed.debug_level()?;
    let engine = build_engine(level, parsed.option("out-dir"))?;
    let output = engine.run(&frame, target)?;
    println!("{}", engine.template_report());
    for line in output.topk_report() {
        println!("{line}");
    }
    println!();
    for line in recognition_lines(&output.recognition) {
        println!("{line}");
    }
    Ok(())
}

/// `vision-catalog`：校验参考实现图标目录，并输出迁移覆盖审计（plan6 P0.1）。
///
/// **只读**：读 manifest、校验资源存在、比较 ID 覆盖，不发送任何输入。
fn run_catalog(args: &[String]) -> Result<(), VisionError> {
    let parsed = parse_args(args)?;
    let root = parsed
        .option("root")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("assets/reference/icons"));
    let catalog = catalog_mod::IconCatalog::load(&root).map_err(|e| VisionError::Io {
        path: root.display().to_string(),
        detail: e.message(),
    })?;

    println!("参考图标目录: {}", root.display());
    for kind in [
        catalog_mod::ItemKind::Stratagem,
        catalog_mod::ItemKind::Booster,
    ] {
        // 样例路径同时验证 `asset_path` 的解析结果（相对 manifest 目录）
        let sample = catalog
            .by_kind(kind)
            .next()
            .and_then(|e| catalog.asset_path(&e.item_id));
        println!(
            "  {:<9} {} 条{}",
            kind.label(),
            catalog.count_of(kind),
            match sample {
                Some(p) => format!("   样例: {}", p.display()),
                None => String::new(),
            }
        );
    }
    println!("  合计 {} 条（资源全部存在）", catalog.len());

    let current_keys = crate::icons::all_icon_keys();
    println!("\n当前项目图标键: {} 条", current_keys.len());

    let mut mapped = 0usize;
    let mut unmapped: Vec<&str> = Vec::new();
    for entry in catalog.by_kind(catalog_mod::ItemKind::Stratagem) {
        match catalog.resolve_current_key(&entry.item_id, &current_keys) {
            Some(_) => mapped += 1,
            None => unmapped.push(&entry.item_id),
        }
    }
    println!(
        "参考 stratagem 能被当前项目图标覆盖: {mapped} / {}",
        catalog.count_of(catalog_mod::ItemKind::Stratagem)
    );
    if !unmapped.is_empty() {
        println!("  当前项目缺失的参考 stratagem（{} 条）:", unmapped.len());
        for id in &unmapped {
            let name = catalog
                .get(id)
                .map(|e| e.display_name.as_str())
                .unwrap_or("");
            println!("    {id:34} {name}");
        }
    }

    // 反向：当前项目有哪些图标不在参考目录里
    let mut ref_keys: Vec<&str> = Vec::new();
    let mut owned: Vec<String> = Vec::new();
    for entry in catalog.by_kind(catalog_mod::ItemKind::Stratagem) {
        if let Some(k) = catalog.resolve_current_key(&entry.item_id, &current_keys) {
            ref_keys.push(k);
            owned.push(k.to_string());
        }
    }
    let extra: Vec<&&str> = current_keys
        .iter()
        .filter(|k| !owned.iter().any(|o| o == *k))
        .collect();
    println!("\n当前项目有、参考目录没有的图标: {} 条", extra.len());
    for k in &extra {
        println!("    {k}");
    }

    println!(
        "\n注意：参考 manifest **不含**任务战备（增援/补给/地狱炸弹/SSSD 交付/超级地球旗帜/\n\
         上传数据/地震探测器/暗流体容器/蜂巢破碎钻机/SEAF 火炮/呼叫超级驱逐舰/飞鹰重新装填）。\n\
         预设里若含任务战备，在普通战备列表里永远搜不到 —— 必须按「目录缺失」拒答，\n\
         而不是无限滚动（这是真实运行 `SearchingTarget` 超时的直接原因）。"
    );
    Ok(())
}

use crate::loadout_sync::direct_select as ds;
use crate::vision::reference_catalog as catalog_mod;

/// `vision-selectplan`：打印 `direct_select` 的目标计划（plan6 §6.3）。
///
/// **只读**：读 profile、读配置开关、打印计划，不发送任何输入。
/// 用途是在真正运行前核对「打算装哪 4 个 + 哪个 Booster」，
/// 并显示当前由哪条动作路径接管（legacy / direct_select）。
fn run_selectplan(args: &[String]) -> Result<(), VisionError> {
    let parsed = parse_args(args)?;
    let profile = parsed.option("profile").unwrap_or("轮椅").to_string();
    let path = crate::util::app_dir()
        .join("profiles")
        .join(format!("{profile}.json"));
    let raw = std::fs::read_to_string(&path).map_err(|e| VisionError::Io {
        path: path.display().to_string(),
        detail: format!("读取 profile 失败: {e}"),
    })?;
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|e| VisionError::Io {
        path: path.display().to_string(),
        detail: format!("解析 profile 失败: {e}"),
    })?;
    let loadout = value
        .get("loadout")
        .and_then(|v| v.as_array())
        .ok_or_else(|| VisionError::Io {
            path: path.display().to_string(),
            detail: "profile 缺少 loadout 数组".to_string(),
        })?;

    // slots 06~10 → 数组下标 5..=9（见 loadout_sync::selection 的权威定义）
    let id_of = |i: usize| -> Option<String> {
        let idx = loadout.get(i)?.as_u64()? as usize;
        crate::stratagems::STRATAGEMS
            .get(idx)
            .map(|s| s.icon.to_string())
    };
    let stratagems: Vec<Option<String>> = (5..9).map(id_of).collect();
    let booster = id_of(9);

    println!("profile: {profile}  ({})", path.display());
    println!("动作路径: direct_select（plan6）");

    let (strat_targets, boost_targets) = ds::machine::plan_preview(&stratagems, booster.as_ref());
    println!("\n计划装载的战备（{} 个）:", strat_targets.len());
    for (i, t) in strat_targets.iter().enumerate() {
        let known = crate::stratagems::STRATAGEMS.iter().any(|s| s.icon == t);
        println!(
            "  S{}  {t:32} 内置目录={}",
            i + 1,
            if known {
                "是"
            } else {
                "否（任务战备/插件）"
            }
        );
    }
    println!(
        "计划装载的 Booster（{} 个）: {:?}",
        boost_targets.len(),
        boost_targets
    );

    // 目录覆盖检查：参考 manifest 不含任务战备 → 这类目标永远搜不到
    let root = std::path::PathBuf::from("assets/reference/icons");
    match crate::vision::reference_catalog::IconCatalog::load(&root) {
        Ok(cat) => {
            let keys = crate::icons::all_icon_keys();
            let missing: Vec<&String> = strat_targets
                .iter()
                .filter(|t| {
                    !cat.by_kind(crate::vision::reference_catalog::ItemKind::Stratagem)
                        .any(|e| cat.resolve_current_key(&e.item_id, &keys) == Some(t.as_str()))
                })
                .collect();
            if missing.is_empty() {
                println!("\n目录覆盖: 全部目标都在参考目录中");
            } else {
                println!(
                    "\n目录覆盖: {} 个目标**不在**参考目录中 → 在普通战备列表里搜不到，\n\
                     必须按「目录缺失」拒答（不得无限滚动）:",
                    missing.len()
                );
                for m in missing {
                    println!("  {m}");
                }
            }
        }
        Err(e) => println!("\n目录加载失败（跳过覆盖检查）: {}", e.message()),
    }
    Ok(())
}

/// `vision-navplan`：打印一次有界翻页计划（plan6 P3.3 的可解释输出）。
///
/// **只读**：只计算并打印将发出的滚轮量、判定顺序与硬上限，不发送任何输入。
/// 存在的意义是让「翻页为什么停」可解释 —— 每次输入都有确定的量与上限，
/// 而不是靠超时收尾。
fn run_navplan(args: &[String]) -> Result<(), VisionError> {
    let parsed = parse_args(args)?;
    let direction = match parsed.option("direction").unwrap_or("down") {
        "down" => crate::loadout_sync::list_map::ScrollDirection::Down,
        "up" => crate::loadout_sync::list_map::ScrollDirection::Up,
        other => {
            return Err(VisionError::Usage {
                detail: format!("未知 --direction「{other}」（应为 up 或 down）"),
            })
        }
    };
    use ds::page_navigation as nav;

    println!("翻页方向: {direction:?}");
    println!("\n输入序列（每步都有确定的滚轮量）:");
    let plan = [
        ("整页翻动", nav::NavInput::Full(direction)),
        ("边界探测", nav::NavInput::Probe(direction)),
    ];
    for (label, input) in plan {
        println!(
            "  {label:8} kind={:10} wheel_delta={:+5}  (full={} probe={})",
            if input.is_full() { "full" } else { "probe" },
            nav::navigation_delta(input),
            input.is_full(),
            input.is_probe()
        );
    }

    let tracker = nav::NavigationTracker::default();
    println!("\n硬上限（不依赖超时）:");
    println!("  整页翻动次数上限: {}", nav::MAX_FULL_TURNS);
    println!("  边界探测次数上限: {}", nav::MAX_PROBES);
    println!("  单次翻页硬超时:   {} ms", nav::HARD_TIMEOUT_MS);
    println!("  结论前最少语义样本: {}", nav::MIN_SEMANTIC_OBSERVATIONS);
    println!("  判定无位移所需连续同视口帧: {}", nav::NO_MOVEMENT_FRAMES);
    println!("  初始 can_continue: {}", tracker.can_continue(direction));
    println!(
        "  初始 boundary_confirmed: {}（必须探测过且连续无位移才为真）",
        tracker.boundary_confirmed()
    );
    println!(
        "\n判定顺序: 指纹变化 → 触发语义扫描；共同身份垂直位移 → 唯一成功判据。\n\
         - Shifted           → 移动成功（short = 位移 < 整页 {:.0}%）\n\
         - SameViewport ×{} → 无位移；必须再做 probe 才能确认边界\n\
         - DifferentViewport → 内容整体被替换 → Uncertain（不得当成已移动）\n\
         - Uncertain         → 停止当前方向，不得盲滚",
        ds::page_relation::PAGE_TURN_SHORT_THRESHOLD_RATIO * 100.0,
        nav::NO_MOVEMENT_FRAMES
    );
    Ok(())
}

/// `vision-page`：对两张截图比较**语义分页关系**（plan6 P3.2/P3.4 诊断）。
///
/// 输出同时给出「整屏指纹差」与「共同身份垂直位移」两套判据，
/// 便于直接看出为什么某一帧被判定为 moved / same / uncertain。
/// **只读**：不发送任何输入。
fn run_page(args: &[String]) -> Result<(), VisionError> {
    let parsed = parse_args(args)?;
    if parsed.positional.len() < 2 {
        return Err(VisionError::Usage {
            detail: format!("{CMD_PAGE} 需要两张截图：<前.png> <后.png>"),
        });
    }
    let before_path = &parsed.positional[0];
    let after_path = &parsed.positional[1];
    let target = parsed.target()?;
    let direction = match parsed.option("direction").unwrap_or("down") {
        "down" => crate::loadout_sync::list_map::ScrollDirection::Down,
        "up" => crate::loadout_sync::list_map::ScrollDirection::Up,
        other => {
            return Err(VisionError::Usage {
                detail: format!("未知 --direction「{other}」（应为 up 或 down）"),
            })
        }
    };
    let level = parsed.debug_level()?;
    let engine = build_engine(level, parsed.option("out"))?;
    let trace_of = |path: &str| -> Result<ds::page_navigation::PageSnapshot, VisionError> {
        let frame = load_frame(std::path::Path::new(path))?;
        let output = engine.run(&frame, target)?;
        let height = output.geometry.roi.h as f32;
        let slots = output
            .recognition
            .detections
            .iter()
            .map(|d| ds::page_relation::TrackedSlot {
                row: d.row,
                col: d.col,
                rect: d.cell,
                item_id: d.outcome.id().map(|i| i.as_str().to_string()),
            })
            .collect();
        // 指纹用 ROI 图，与参考实现一致（不是整帧）
        let signature = ds::frame::image_fingerprint(&output.roi_image);
        Ok(ds::page_navigation::PageSnapshot::new(
            slots, signature, height,
        ))
    };

    let before = trace_of(before_path)?;
    let after = trace_of(after_path)?;

    let named_before: usize = before.slots.iter().filter(|s| s.item_id.is_some()).count();
    let named_after: usize = after.slots.iter().filter(|s| s.item_id.is_some()).count();
    let distance = ds::frame::fingerprint_distance(&before.signature, &after.signature);
    let relation = ds::page_relation::compare_page_turn(
        &before.slots,
        &after.slots,
        direction,
        after.roi_height,
    );

    println!(
        "前帧: {before_path}  槽位 {}（已分类 {named_before}）",
        before.slots.len()
    );
    println!(
        "后帧: {after_path}  槽位 {}（已分类 {named_after}）",
        after.slots.len()
    );
    println!("滚轮方向: {direction:?}");
    println!();
    println!(
        "整屏指纹平均绝对差: {distance:.3}（阈值 {}，>= 阈值才算画面变了）",
        ds::frame::PAGE_CHANGE_THRESHOLD
    );
    println!(
        "画面是否变化: {}",
        ds::frame::frame_changed(&before.signature, &after.signature)
    );
    println!("语义分页关系: {} -> {relation:?}", relation.label());
    match relation {
        ds::page_relation::PageRelation::Shifted(shift) => {
            println!(
                "  方向一致位移 {:.1}px，占整页 {:.3}（短翻页 = {}）",
                shift.directed_shift,
                shift.shift_ratio,
                shift.is_short()
            );
        }
        ds::page_relation::PageRelation::DifferentViewport => {
            println!("  内容整体被替换 —— 不得当作「已移动」继续搜索");
        }
        ds::page_relation::PageRelation::Uncertain => {
            println!("  无法判定 —— 必须停止当前方向，不得盲滚");
        }
        ds::page_relation::PageRelation::SameViewport => {
            println!("  视口未移动");
        }
    }
    Ok(())
}

/// `vision-labels`：人类标注（逐行英文名）→ CLI 标签（cells + 内部 ID）。///
/// 存在的意义：`screenshots/list.json` 这类文件是**人类标注源**，必须保持原样；
/// evaluation 需要的是按 slot 编号的内部 ID。这个命令是二者之间唯一、可测试的桥。
fn run_labels(args: &[String]) -> Result<(), VisionError> {
    let parsed = parse_args(args)?;
    let Some(path) = parsed.positional.first() else {
        return Err(VisionError::Usage {
            detail: format!("{CMD_LABELS} 需要一个标注 JSON 路径"),
        });
    };
    let raw = std::fs::read_to_string(path).map_err(|e| VisionError::Io {
        path: path.to_string(),
        detail: format!("读取标注失败: {e}"),
    })?;
    let rows: RowLabelFile = serde_json::from_str(&raw).map_err(|e| VisionError::Io {
        path: path.to_string(),
        detail: format!("解析标注失败: {e}"),
    })?;
    let labels = rows_to_label_file(&rows).map_err(|detail| VisionError::Io {
        path: path.to_string(),
        detail,
    })?;
    labels.validate().map_err(|detail| VisionError::Io {
        path: path.to_string(),
        detail: format!("转换结果自检失败: {detail}"),
    })?;
    let json = serde_json::to_string_pretty(&labels).map_err(|e| VisionError::Io {
        path: path.to_string(),
        detail: format!("序列化失败: {e}"),
    })?;
    match parsed.option("out") {
        Some(out) => {
            std::fs::write(out, json).map_err(|e| VisionError::Io {
                path: out.to_string(),
                detail: format!("写入失败: {e}"),
            })?;
            println!("已写入 {out}（{} 个槽位）", labels.cells.len());
        }
        None => println!("{json}"),
    }
    Ok(())
}

/// `vision-dataset`：批量生成数据集（需求 §22）。
fn run_dataset(args: &[String]) -> Result<(), VisionError> {
    let parsed = parse_args(args)?;
    let Some(input) = parsed.positional.first() else {
        return Err(VisionError::Usage {
            detail: format!("{CMD_DATASET} 需要一个截图目录或文件"),
        });
    };
    let Some(out) = parsed.option("out") else {
        return Err(VisionError::Usage {
            detail: format!("{CMD_DATASET} 需要 --out <目录>"),
        });
    };
    let target = parsed.target()?;
    let screenshots = {
        let path = PathBuf::from(input);
        if path.is_dir() {
            dataset::collect_screenshots(&path)?
        } else {
            vec![path]
        }
    };
    if screenshots.is_empty() {
        return Err(VisionError::Usage {
            detail: format!("目录「{input}」里没有 PNG/JPG 截图"),
        });
    }
    let engine = build_engine(VisionDebugLevel::Off, None)?;
    let report = dataset::generate(
        &engine,
        &screenshots,
        &DatasetOptions {
            out_dir: PathBuf::from(out),
            target,
            write_preview: parsed.flag("preview"),
            ..DatasetOptions::default()
        },
    )?;
    println!("{}", report.summary());
    println!("输出目录: {out}");
    Ok(())
}

// ─── 评估（需求 §29 / §30 / §41） ───

/// 标签文件。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LabelFile {
    #[serde(default)]
    pub target: Option<String>,
    pub cells: Vec<LabelCell>,
}

/// 单个槽位的期望值。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LabelCell {
    pub slot: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty: Option<bool>,
}

impl LabelFile {
    /// 校验标签文件的结构（标签本身错就整体拒绝，而不是静默算出一个错的准确率）。
    ///
    /// 规则：
    ///   * `target` 若出现必须是 `loadout` / `home` / `list`；
    ///   * slot 不允许重复；
    ///   * 每条 cell 必须且只能给出 `id` 或 `empty: true` 之一；
    ///   * `id` 必须能映射到已知内部 ID（不接受随手编的字符串）。
    pub fn validate(&self) -> Result<(), String> {
        if let Some(target) = self.target.as_deref() {
            if !matches!(target, "loadout" | "home" | "list") {
                return Err(format!("target「{target}」未知（应为 loadout / list）"));
            }
        }
        let mut seen: Vec<usize> = Vec::with_capacity(self.cells.len());
        for cell in &self.cells {
            if seen.contains(&cell.slot) {
                return Err(format!("槽位 {} 重复出现", cell.slot));
            }
            seen.push(cell.slot);
            match (cell.id.as_deref(), cell.empty) {
                (Some(_), Some(true)) => {
                    return Err(format!("槽位 {} 同时给了 id 与 empty", cell.slot));
                }
                (None, None | Some(false)) => {
                    return Err(format!("槽位 {} 既没有 id 也没有 empty", cell.slot));
                }
                (Some(id), _) => {
                    if crate::vision::id::StratagemId::from_display_name(id).is_none() {
                        return Err(format!(
                            "槽位 {} 的 id「{id}」无法映射到已知战备",
                            cell.slot
                        ));
                    }
                }
                (None, Some(true)) => {}
            }
        }
        Ok(())
    }
}

/// 评估结果（逐槽 —— 需求 §30 要求不要把整体 accuracy 当成唯一指标）。
#[derive(Debug, Clone, Default)]
pub struct EvalReport {
    pub total: usize,
    pub top1: usize,
    pub top3: usize,
    pub empty_expected: usize,
    pub empty_correct: usize,
    pub abstained: usize,
    pub false_positive: usize,
    pub wrong: Vec<String>,
}

impl EvalReport {
    pub fn accuracy(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            self.top1 as f32 / self.total as f32
        }
    }

    pub fn summary(&self) -> String {
        let mut lines = vec![format!(
            "样本 {} | top-1 {}/{} ({:.1}%) | top-3 {}/{} | 空槽 {}/{} | 弃权 {} | 误识别 {}",
            self.total,
            self.top1,
            self.total,
            self.accuracy() * 100.0,
            self.top3,
            self.total,
            self.empty_correct,
            self.empty_expected,
            self.abstained,
            self.false_positive
        )];
        for line in &self.wrong {
            lines.push(format!("  ! {line}"));
        }
        lines.join("\n")
    }
}

/// 按标签评估一张截图。
pub fn evaluate(
    engine: &VisionEngine,
    frame: &crate::loadout_sync::capture::CapturedFrame,
    target: RecognizeTarget,
    labels: &LabelFile,
) -> Result<EvalReport, VisionError> {
    let output = engine.run(frame, target)?;
    let mut report = EvalReport::default();
    for cell in &labels.cells {
        let Some(det) = output
            .recognition
            .detections
            .iter()
            .find(|d| d.slot == cell.slot)
        else {
            report.wrong.push(format!(
                "槽位 {} 不在本次识别范围内（未识别到该槽位）",
                cell.slot
            ));
            continue;
        };
        report.total += 1;
        if cell.empty == Some(true) {
            report.empty_expected += 1;
            if det.outcome.is_empty() {
                report.empty_correct += 1;
            } else {
                report.false_positive += 1;
                report.wrong.push(format!(
                    "槽位 {} 期望空槽，实际 {}",
                    cell.slot,
                    det.outcome.summary()
                ));
            }
            continue;
        }
        let Some(expected) = cell.id.as_deref() else {
            // 未给期望值：只记录是否弃权
            if det.outcome.is_unknown() {
                report.abstained += 1;
            }
            continue;
        };
        match &det.outcome {
            DetectionOutcome::Recognized {
                id, alternatives, ..
            } => {
                if id.as_str() == expected {
                    report.top1 += 1;
                    report.top3 += 1;
                } else if alternatives.iter().any(|a| a.id.as_str() == expected) {
                    report.top3 += 1;
                    report.wrong.push(format!(
                        "槽位 {} top-1 = {}，期望 {expected}（命中 top-3）",
                        cell.slot,
                        id.as_str()
                    ));
                } else {
                    report.false_positive += 1;
                    report.wrong.push(format!(
                        "槽位 {} top-1 = {}，期望 {expected}",
                        cell.slot,
                        id.as_str()
                    ));
                }
            }
            DetectionOutcome::Unknown { reason, best, .. } => {
                report.abstained += 1;
                if best
                    .as_ref()
                    .map(|b| b.as_str() == expected)
                    .unwrap_or(false)
                {
                    report.top3 += 1;
                }
                report.wrong.push(format!(
                    "槽位 {} 弃权（{}），期望 {expected}",
                    cell.slot,
                    reason.label()
                ));
            }
            DetectionOutcome::Empty => {
                report
                    .wrong
                    .push(format!("槽位 {} 判为空槽，期望 {expected}", cell.slot));
            }
        }
    }
    Ok(report)
}

fn run_eval(args: &[String]) -> Result<(), VisionError> {
    let parsed = parse_args(args)?;
    let Some(path) = parsed.positional.first() else {
        return Err(VisionError::Usage {
            detail: format!("{CMD_EVAL} 需要一个截图路径"),
        });
    };
    let Some(labels_path) = parsed.option("labels") else {
        return Err(VisionError::Usage {
            detail: format!("{CMD_EVAL} 需要 --labels <标签.json>"),
        });
    };
    let raw = std::fs::read_to_string(labels_path).map_err(|e| VisionError::Io {
        path: labels_path.to_string(),
        detail: format!("读取标签失败: {e}"),
    })?;
    let labels: LabelFile = serde_json::from_str(&raw).map_err(|e| VisionError::Io {
        path: labels_path.to_string(),
        detail: format!("解析标签失败: {e}"),
    })?;
    labels.validate().map_err(|detail| VisionError::Io {
        path: labels_path.to_string(),
        detail: format!("标签校验失败: {detail}"),
    })?;
    let target = match labels.target.as_deref() {
        Some("list") => RecognizeTarget::List,
        Some("loadout") | Some("home") | None => parsed.target()?,
        Some(other) => {
            return Err(VisionError::Usage {
                detail: format!("标签文件里的 target「{other}」未知"),
            })
        }
    };
    let engine = build_engine(parsed.debug_level()?, parsed.option("out"))?;
    let frame = load_frame(std::path::Path::new(path))?;
    let report = evaluate(&engine, &frame, target, &labels)?;
    println!("{}", report.summary());
    Ok(())
}

/// 人类的结论行（GUI 日志与 CLI 共用）。
pub fn recognition_lines(rec: &LoadoutRecognition) -> Vec<String> {
    rec.summary_lines()
}

// ─── 人类可读标注 → CLI 标签（需求 §28：不要破坏用户的 list.json） ───

/// 人类可读的标注源（`screenshots/list.json` 的 5×4 名称矩阵）。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct RowLabelFile {
    #[serde(default)]
    pub screen: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
    pub rows: Vec<Vec<String>>,
}

/// 逐行名称矩阵 → 按 slot 编号的内部 ID 标签。
///
/// 只做「名称 → 内部 ID」的查表（复用 `StratagemId::from_display_name`，
/// 不复制第二份名称映射表）；任何无法映射的名称、空行、列数不一致都报错。
pub fn rows_to_label_file(rows: &RowLabelFile) -> Result<LabelFile, String> {
    if rows.rows.is_empty() {
        return Err("rows 为空".to_string());
    }
    let columns = rows.rows[0].len();
    if columns == 0 {
        return Err("rows[0] 没有任何列".to_string());
    }
    let target = match rows.target.as_deref() {
        Some(t @ ("loadout" | "home" | "list")) => Some(t.to_string()),
        Some(other) => return Err(format!("target「{other}」未知")),
        None => match rows.screen.as_deref() {
            Some("list") => Some("list".to_string()),
            _ => Some("loadout".to_string()),
        },
    };
    let mut cells = Vec::with_capacity(rows.rows.len() * columns);
    for (row_index, row) in rows.rows.iter().enumerate() {
        if row.len() != columns {
            return Err(format!(
                "第 {} 行有 {} 列，与首行的 {columns} 列不一致",
                row_index + 1,
                row.len()
            ));
        }
        for (col_index, name) in row.iter().enumerate() {
            let slot = row_index * columns + col_index;
            if name.trim().is_empty() {
                cells.push(LabelCell {
                    slot,
                    id: None,
                    empty: Some(true),
                });
                continue;
            }
            let id = crate::vision::id::StratagemId::from_display_name(name).ok_or_else(|| {
                format!(
                    "第 {} 行第 {} 列的名称「{name}」无法映射到内部 ID",
                    row_index + 1,
                    col_index + 1
                )
            })?;
            cells.push(LabelCell {
                slot,
                id: Some(id.as_str().to_string()),
                empty: None,
            });
        }
    }
    Ok(LabelFile { target, cells })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn dispatch_ignores_non_cli_arguments() {
        assert!(dispatch(&args(&[])).is_none());
        assert!(dispatch(&args(&["--help"])).is_none());
        assert!(dispatch(&args(&[CMD_DEBUG])).is_some());
    }

    #[test]
    fn parse_args_handles_options_and_flags() {
        let parsed = parse_args(&args(&[
            "shot.png",
            "--target",
            "list",
            "--preview",
            "--debug",
            "trace",
        ]))
        .expect("解析");
        assert_eq!(parsed.positional, vec!["shot.png".to_string()]);
        assert_eq!(parsed.option("target"), Some("list"));
        assert_eq!(parsed.option("debug"), Some("trace"));
        assert!(parsed.flag("preview"));
        assert!(!parsed.flag("missing"));
    }

    #[test]
    fn unknown_target_is_a_usage_error() {
        let parsed = parse_args(&args(&["shot.png", "--target", "banana"])).expect("解析");
        let err = parsed.target().expect_err("必须报错");
        assert_eq!(err.code(), "VisionUsage");
        assert!(err.message().contains("banana"));
    }

    #[test]
    fn missing_option_value_is_reported() {
        let err = parse_args(&args(&["shot.png", "--out"])).expect_err("必须报错");
        assert!(err.message().contains("--out"));
    }

    #[test]
    fn usage_text_lists_all_commands() {
        let text = usage();
        for cmd in [CMD_DEBUG, CMD_DATASET, CMD_EVAL] {
            assert!(text.contains(cmd));
        }
    }

    #[test]
    fn eval_report_accuracy_is_per_sample() {
        let report = EvalReport {
            total: 4,
            top1: 3,
            top3: 4,
            empty_expected: 1,
            empty_correct: 1,
            abstained: 0,
            false_positive: 1,
            wrong: vec!["x".into()],
        };
        assert!((report.accuracy() - 0.75).abs() < 1e-6);
        assert!(report.summary().contains("top-1 3/4"));
    }

    /// 需求 §28 的验收表：英文显示名必须映射到正确的内部 ID。
    #[test]
    fn english_display_names_map_to_internal_ids() {
        let cases = [
            ("Orbital Precision Strike", "orbital_precision_strike"),
            ("Orbital Gatling Barrage", "orbital_gatling_barrage"),
            ("Orbital 380mm HE Barrage", "orbital_380mm_he_barrage"),
            ("Orbital EMS Strike", "orbital_ems_strike"),
            ("Eagle Smoke Strike", "eagle_smoke_strike"),
            ("Eagle 500kg Bomb", "eagle_500kg_bomb"),
            // 带型号前缀、资源键却去掉前缀的条目（别名表覆盖）
            ("CQC-9 Defoliation Tool", "defoliation_tool"),
            ("B-100 Portable Hellbomb", "hellbomb_portable"),
            ("B/FLAM-80 Cremator", "cremator"),
            ("CQC-20 Breaching Hammer", "cqc_20"),
            ("A/ARC-3 Tesla Tower", "tesla_tower"),
            ("EXO-51 Lumberer Exosuit", "lumberer_exosuit"),
        ];
        for (name, expected) in cases {
            let id = crate::vision::id::StratagemId::from_display_name(name)
                .unwrap_or_else(|| panic!("「{name}」应当能映射"));
            assert_eq!(id.as_str(), expected, "「{name}」映射错误");
        }
        assert!(crate::vision::id::StratagemId::from_display_name("Nope Not Real").is_none());
    }

    #[test]
    fn rows_convert_to_slot_labels() {
        let rows = RowLabelFile {
            screen: Some("home1".into()),
            target: None,
            rows: vec![
                vec![
                    "CQC-9 Defoliation Tool".into(),
                    "Orbital 380mm HE Barrage".into(),
                    "B-100 Portable Hellbomb".into(),
                    "B/FLAM-80 Cremator".into(),
                ],
                vec![
                    "Experimental Infusion".into(),
                    String::new(),
                    String::new(),
                    String::new(),
                ],
            ],
        };
        let labels = rows_to_label_file(&rows).expect("转换");
        assert_eq!(labels.target.as_deref(), Some("loadout"));
        assert_eq!(labels.cells.len(), 8);
        assert_eq!(labels.cells[0].id.as_deref(), Some("defoliation_tool"));
        assert_eq!(labels.cells[3].id.as_deref(), Some("cremator"));
        assert_eq!(labels.cells[4].id.as_deref(), Some("experimental_infusion"));
        assert_eq!(labels.cells[5].empty, Some(true));
        labels.validate().expect("转换结果必须自洽");
    }

    #[test]
    fn rows_conversion_rejects_bad_input() {
        let ragged = RowLabelFile {
            screen: None,
            target: Some("list".into()),
            rows: vec![vec!["Orbital Laser".into()], vec![]],
        };
        assert!(rows_to_label_file(&ragged).is_err(), "列数不一致必须报错");

        let unknown = RowLabelFile {
            screen: None,
            target: None,
            rows: vec![vec!["Not A Stratagem".into()]],
        };
        assert!(rows_to_label_file(&unknown).is_err(), "未知名称必须报错");
    }

    #[test]
    fn label_validation_rejects_duplicates_and_missing_expectation() {
        let duplicate = LabelFile {
            target: Some("list".into()),
            cells: vec![
                LabelCell {
                    slot: 1,
                    id: Some("orbital_laser".into()),
                    empty: None,
                },
                LabelCell {
                    slot: 1,
                    id: Some("orbital_laser".into()),
                    empty: None,
                },
            ],
        };
        assert!(duplicate.validate().is_err());

        let empty_cell = LabelFile {
            target: None,
            cells: vec![LabelCell {
                slot: 0,
                id: None,
                empty: None,
            }],
        };
        assert!(empty_cell.validate().is_err());

        let bad_id = LabelFile {
            target: None,
            cells: vec![LabelCell {
                slot: 0,
                id: Some("totally_made_up".into()),
                empty: None,
            }],
        };
        assert!(bad_id.validate().is_err());

        let good = LabelFile {
            target: Some("loadout".into()),
            cells: vec![
                LabelCell {
                    slot: 0,
                    id: Some("tesla_tower".into()),
                    empty: None,
                },
                LabelCell {
                    slot: 1,
                    id: None,
                    empty: Some(true),
                },
            ],
        };
        assert!(good.validate().is_ok());
    }
}
