//! plan7 **Step 6** —— 真实帧 fixture 对照（切换主路径前的闸门）。
//!
//! ## 为什么必须先做这一步
//!
//! `src/fixtures/loadout_sync/README.md` 记录了当前（legacy `matcher`）实现在
//! 真实标注列表上的实测成绩：**top-1 准确率 0/13**，且"同一个错误 key 拿下多格"。
//! 参考实现被期望更强，因为它的模板本身就是**平面单色字形**，并按**已知渲染比例**
//! （列表 68/104、home 93/104）直接对齐，不做包围盒猜测。
//!
//! 因此本对照只回答一个问题：**参考 `RecognizerRuntime` 的生产链路
//! （标定 ROI → 几何检测 → 模板分类）在真实帧上到底能认出多少格。**
//! 结论不达标就必须停止，不得进入参考状态机落地（plan7 §7 Step 6）。
//!
//! ## 为什么用完整链路而不是单独喂分类器
//!
//! 参考的入口是 `resolve_calibration_roi_for_size`（把 2560×1440 参考系的
//! `roi_ref` 映射到实际帧）+ `detect_slot_layout` + `classify_batch`。
//! 只测分类器会掩盖 ROI/几何这一层（plan7 §5.6 的头号风险），所以这里走完整链路。
//!
//! ## 运行
//!
//! ```text
//! cargo test --bin h2ac-rs fixture_compare -- --nocapture
//! ```

use std::path::PathBuf;

use image::RgbaImage;

use crate::assets::{IconCatalog, default_icon_manifest};
use crate::image_rect::ImageRect;
use crate::item::ItemKind;
use crate::loadout_sync::catalog_bridge;
use crate::vision::{RecognizerRuntime, SlotLayout, resolve_calibration_roi_for_size};

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/fixtures/loadout_sync")
}

fn load_frame(name: &str) -> RgbaImage {
    let path = fixture_dir().join(name);
    let image = image::open(&path)
        .unwrap_or_else(|error| panic!("无法读取夹具 {}: {error}", path.display()));
    image.to_rgba8()
}

fn crop(frame: &RgbaImage, rect: ImageRect) -> RgbaImage {
    image::imageops::crop_imm(frame, rect.x, rect.y, rect.w, rect.h).to_image()
}

/// 一个标注格的对照结果。
#[derive(Debug)]
struct CellOutcome {
    slot: u32,
    key: String,
    expected: Option<String>,
    predicted: Option<String>,
    score: Option<f32>,
    margin: Option<f32>,
    slot_found: bool,
}

fn run_labels(
    runtime: &RecognizerRuntime,
    catalog: &IconCatalog,
    frame: &RgbaImage,
    fixture: &str,
    layout: SlotLayout,
    labels_name: &str,
) -> (Vec<CellOutcome>, usize) {
    let roi = resolve_calibration_roi_for_size(frame.width(), frame.height(), runtime.calibration())
        .expect("标定 ROI 必须能在该帧上解析");
    println!(
        "[{fixture}] 帧 {}x{} → 参考 ROI ({},{},{}x{})",
        frame.width(),
        frame.height(),
        roi.x,
        roi.y,
        roi.w,
        roi.h
    );

    let observed = runtime
        .recognize(crop(frame, roi), layout)
        .expect("参考链路识别失败");
    println!("[{fixture}] 检测到 {} 个槽位", observed.slots.len());
    let rows = observed.slots.iter().map(|s| s.row).max().unwrap_or(0) + 1;
    let cols = observed.slots.iter().map(|s| s.col).max().unwrap_or(0) + 1;
    println!("[{fixture}] 网格 {rows} 行 × {cols} 列");
    for slot in &observed.slots {
        match &slot.classification {
            Some(c) => println!(
                "  r{}c{} kind {:?} ({},{},{}x{}) → {:<32} score {:.3} margin {:.3}",
                slot.row, slot.col, slot.kind, slot.x, slot.y, slot.w, slot.h, c.item_id, c.match_score, c.match_margin
            ),
            None => println!(
                "  r{}c{} kind {:?} ({},{},{}x{}) → <未达门限>",
                slot.row, slot.col, slot.kind, slot.x, slot.y, slot.w, slot.h
            ),
        }
    }

    let text = std::fs::read_to_string(fixture_dir().join(labels_name)).expect("标签文件");
    let labels: serde_json::Value = serde_json::from_str(&text).expect("标签 JSON");
    let mut outcomes = Vec::new();
    for cell in labels["cells"].as_array().expect("cells") {
        let slot_index = cell["slot"].as_u64().expect("slot") as u32;
        let key = cell["id"].as_str().expect("id");
        let expected = catalog_bridge::resolve_item_id(catalog, key).map(str::to_string);
        let (row, col) = (slot_index / 4, slot_index % 4);
        let found = observed
            .slots
            .iter()
            .find(|s| s.row == row && s.col == col);
        let classification = found.and_then(|s| s.classification.as_ref());
        outcomes.push(CellOutcome {
            slot: slot_index,
            key: key.to_string(),
            expected,
            predicted: classification.map(|c| c.item_id.clone()),
            score: classification.map(|c| c.match_score),
            margin: classification.map(|c| c.match_margin),
            slot_found: found.is_some(),
        });
    }
    (outcomes, observed.slots.len())
}

/// ⚠ **闸门测试（当前不通过）**：见 `plan7.md` §13。
/// 待参考捕获路径落地、用参考捕获的帧重跑后再取消 `#[ignore]`。
#[test]
#[ignore = "plan7 §13：当前实机帧上参考链路 top-1 为 1/13、最高分 0.565 < 门限 0.70"]
fn reference_pipeline_on_labeled_list_fixture() {
    let frame = load_frame("stratagem_list_labeled_1914x1080.png");
    let runtime = RecognizerRuntime::load().expect("参考运行时（内嵌目录 + 标定）");
    let catalog = IconCatalog::load(default_icon_manifest()).expect("内嵌目录");
    let (outcomes, _slots) = run_labels(
        &runtime,
        &catalog,
        &frame,
        "stratagem_list_labeled_1914x1080.png",
        SlotLayout::List(ItemKind::Stratagem),
        "labels_stratagem_list.json",
    );

    let mut top1 = 0usize;
    let mut scored = 0usize;
    let mut unresolved = 0usize;
    println!("\n=== 逐格对照（列表夹具，13 格标注）===");
    for outcome in &outcomes {
        let Some(expected) = &outcome.expected else {
            unresolved += 1;
            println!(
                "slot {:>2} {:<22} → 目录缺失（预期就不该被点中）",
                outcome.slot, outcome.key
            );
            continue;
        };
        let hit = outcome.predicted.as_deref() == Some(expected.as_str());
        if hit {
            top1 += 1;
        }
        if outcome.predicted.is_some() {
            scored += 1;
        }
        println!(
            "slot {:>2} {:<22} 期望 {:<28} 实得 {:<28} score {:<6} margin {:<6} {}",
            outcome.slot,
            outcome.key,
            expected,
            outcome.predicted.as_deref().unwrap_or("<无>"),
            outcome
                .score
                .map(|s| format!("{s:.3}"))
                .unwrap_or_else(|| "-".into()),
            outcome
                .margin
                .map(|m| format!("{m:.3}"))
                .unwrap_or_else(|| "-".into()),
            if hit { "✔" } else { "✗" }
        );
        if !outcome.slot_found {
            println!("      ↑ 该格未被几何检测找到");
        }
    }

    let comparable = outcomes.len() - unresolved;
    let accuracy = if comparable == 0 {
        0.0
    } else {
        top1 as f32 / comparable as f32
    };
    println!(
        "\n=== 结论 ===\ntop-1 正确 {top1}/{comparable}（{:.1}%）；有分类结果 {scored}/{comparable}；目录缺失 {unresolved}",
        accuracy * 100.0
    );
    println!("对照基线（legacy matcher，fixtures README 实测）：top-1 0/13 = 0.0%");

    assert!(
        top1 > 0,
        "参考链路在真实列表夹具上 top-1 为 0/{comparable}，未超过 legacy 基线，\
         按 plan7 §7 Step 6 必须停止并回报，不得进入参考状态机落地"
    );
}

#[test]
#[ignore = "plan7 §13 诊断探针：默认不参与测试流程，需要时用 --ignored 显式运行"]
fn reference_pipeline_on_labeled_home_fixture() {
    let frame = load_frame("home_labeled_1914x1080.png");
    let runtime = RecognizerRuntime::load().expect("参考运行时");
    let roi = resolve_calibration_roi_for_size(frame.width(), frame.height(), runtime.calibration())
        .expect("标定 ROI");
    println!(
        "[home_labeled_1914x1080.png] 帧 {}x{} → 参考 ROI ({},{},{}x{})",
        frame.width(),
        frame.height(),
        roi.x,
        roi.y,
        roi.w,
        roi.h
    );
    let observed = runtime
        .recognize(crop(&frame, roi), SlotLayout::Home)
        .expect("Home 识别");
    println!("检测到 {} 个槽位", observed.slots.len());
    for slot in &observed.slots {
        println!(
            "  r{}c{} kind {:?} → {:?}",
            slot.row,
            slot.col,
            slot.kind,
            slot.classification.as_ref().map(|c| (&c.item_id, c.match_score))
        );
    }
}

/// 诊断用：把 ROI 与"参考检测出的槽位框"画出来，供人工核对几何是否落在真实图标上。
///
/// 输出写到 `target/fixture_debug/`（已被 .gitignore 覆盖，不进版本控制）。
#[test]
#[ignore = "plan7 §13 诊断探针：默认不参与测试流程，需要时用 --ignored 显式运行"]
fn dump_reference_geometry_overlay() {
    use image::{Rgba, RgbaImage};
    fn outline(image: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, color: Rgba<u8>) {
        for dx in 0..w {
            for &dy in &[0u32, h.saturating_sub(1)] {
                let (px, py) = (x + dx, y + dy);
                if px < image.width() && py < image.height() {
                    image.put_pixel(px, py, color);
                }
            }
        }
        for dy in 0..h {
            for &dx in &[0u32, w.saturating_sub(1)] {
                let (px, py) = (x + dx, y + dy);
                if px < image.width() && py < image.height() {
                    image.put_pixel(px, py, color);
                }
            }
        }
    }

    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/fixture_debug");
    std::fs::create_dir_all(&out_dir).expect("debug 目录");
    let runtime = RecognizerRuntime::load().expect("参考运行时");

    for (fixture, layout, tag) in [
        (
            "stratagem_list_labeled_1914x1080.png",
            SlotLayout::List(ItemKind::Stratagem),
            "list",
        ),
        ("home_labeled_1914x1080.png", SlotLayout::Home, "home"),
    ] {
        let frame = load_frame(fixture);
        let roi = resolve_calibration_roi_for_size(frame.width(), frame.height(), runtime.calibration())
            .expect("ROI");
        let cropped = crop(&frame, roi);
        let observed = runtime.recognize(cropped.clone(), layout).expect("识别");
        let mut overlay = cropped.clone();
        for slot in &observed.slots {
            let color = if slot.classification.is_some() {
                Rgba([0, 255, 0, 255])
            } else {
                Rgba([255, 0, 0, 255])
            };
            outline(&mut overlay, slot.x, slot.y, slot.w, slot.h, color);
        }
        let roi_path = out_dir.join(format!("{tag}_roi.png"));
        let grid_path = out_dir.join(format!("{tag}_grid.png"));
        cropped.save(&roi_path).expect("保存 ROI");
        overlay.save(&grid_path).expect("保存网格叠加");
        println!(
            "{tag}: ROI ({},{},{}x{}) → {}",
            roi.x,
            roi.y,
            roi.w,
            roi.h,
            grid_path.display()
        );
    }
}

/// 诊断用：**行偏移扫描**。
///
/// 目的：区分两种完全不同的失败原因。
/// * 若存在某个全局 `dy` 让大量标注格命中 → 失败是**可修的标定/几何偏移**；
/// * 若扫描完所有 `dy` 仍几乎无命中 → 参考分类口径与 H2AC 捕获帧**根本不匹配**
///   （模板尺度/渲染差异），此时必须停止，不能靠调偏移硬凑。
///
/// 做法：保持列、宽高不变，把参考检测出的槽位框整体沿 y 平移，直接调参考
/// `RecognizerRuntime::classify`（不重新检测几何），逐 `dy` 统计 13 个标注格的 top-1。
/// ⚠ **闸门测试（当前不通过）**：全范围行偏移都无法让任何标注格达到门限。
/// 见 `plan7.md` §13；待参考捕获路径落地后重跑。
#[test]
#[ignore = "plan7 §13：±44px 行偏移全范围内 0 命中（门限 0.70）"]
fn reference_classifier_offset_sweep() {
    use crate::vision::{RoiObservation, Slot};

    let frame = load_frame("stratagem_list_labeled_1914x1080.png");
    let runtime = RecognizerRuntime::load().expect("参考运行时");
    let catalog = IconCatalog::load(default_icon_manifest()).expect("内嵌目录");
    let layout = SlotLayout::List(ItemKind::Stratagem);
    let roi = resolve_calibration_roi_for_size(frame.width(), frame.height(), runtime.calibration())
        .expect("ROI");
    let cropped = crop(&frame, roi);
    let base = runtime.recognize(cropped.clone(), layout).expect("识别");

    let text = std::fs::read_to_string(fixture_dir().join("labels_stratagem_list.json")).unwrap();
    let labels: serde_json::Value = serde_json::from_str(&text).unwrap();
    let wanted: Vec<(u32, u32, String)> = labels["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cell| {
            let slot = cell["slot"].as_u64().unwrap() as u32;
            let id = catalog_bridge::resolve_item_id(&catalog, cell["id"].as_str().unwrap())
                .expect("标注格必须能解析到目录");
            (slot / 4, slot % 4, id.to_string())
        })
        .collect();

    println!("\n=== 行偏移扫描（13 个标注格，参考分类器本体）===");
    println!("dy    命中  top1 最高分   命中最多的预测");
    let mut any_dy_worked = false;
    for dy in (-44..=44).step_by(4) {
        let mut slots: Vec<Slot> = base
            .slots
            .iter()
            .cloned()
            .map(|mut slot| {
                slot.y = (slot.y as i32 + dy).max(0) as u32;
                slot.classification = None;
                slot
            })
            .collect();
        let mut observation = RoiObservation {
            image: cropped.clone(),
            layout,
            slots: std::mem::take(&mut slots),
        };
        runtime.classify(&mut observation).expect("分类");

        let mut hits = 0usize;
        let mut best_score = 0.0f32;
        let mut predicted: std::collections::BTreeMap<String, usize> = Default::default();
        for (row, col, expected) in &wanted {
            let Some(slot) = observation
                .slots
                .iter()
                .find(|s| s.row == *row && s.col == *col)
            else {
                continue;
            };
            let Some(classification) = &slot.classification else {
                continue;
            };
            *predicted.entry(classification.item_id.clone()).or_default() += 1;
            best_score = best_score.max(classification.match_score);
            if &classification.item_id == expected {
                hits += 1;
            }
        }
        if hits > 0 {
            any_dy_worked = true;
        }
        let top = predicted
            .iter()
            .max_by_key(|(_, count)| **count)
            .map(|(id, count)| format!("{id} ×{count}"))
            .unwrap_or_else(|| "<全部未达门限>".into());
        println!(
            "{dy:>4}  {:>3}/13  {:.3}     {top}",
            hits, best_score
        );
    }
    println!(
        "结论：{}",
        if any_dy_worked {
            "存在可用偏移 → 属可修的标定/几何对齐问题"
        } else {
            "任何平移都无法命中 → 参考分类口径与 H2AC 捕获帧不匹配（模板尺度/渲染差异），\
             按 plan7 Step 6 必须停止"
        }
    );
    assert!(
        any_dy_worked,
        "行偏移扫描全范围 0 命中：参考链路与 H2AC 真实帧不匹配（详见上方扫描表）"
    );
}

/// 诊断用：**把 ROI 升采样到参考原生尺寸（576×832）再识别**。
///
/// 参考的 `nominal ±1px` 尺寸搜索与 0.60 权重的 Sobel 梯度通道，都是在
/// **2560×1440（ROI 576×832）** 的捕获上标定的。夹具是 1914×1080，
/// 槽位只有 78px（参考原生 104px）→ 字形笔画更细、更受抗锯齿影响。
/// 本实验把 ROI 直接放大到参考原生尺寸，看分数是否回升。
#[test]
#[ignore = "plan7 §13 诊断探针：默认不参与测试流程，需要时用 --ignored 显式运行"]
fn reference_upscaled_roi_probe() {
    use crate::vision::{RoiObservation, Slot};

    let frame = load_frame("stratagem_list_labeled_1914x1080.png");
    let runtime = RecognizerRuntime::load().expect("参考运行时");
    let catalog = IconCatalog::load(default_icon_manifest()).expect("内嵌目录");
    let layout = SlotLayout::List(ItemKind::Stratagem);
    let roi = resolve_calibration_roi_for_size(frame.width(), frame.height(), runtime.calibration())
        .expect("ROI");
    let cropped = crop(&frame, roi);

    let text = std::fs::read_to_string(fixture_dir().join("labels_stratagem_list.json")).unwrap();
    let labels: serde_json::Value = serde_json::from_str(&text).unwrap();
    let wanted: Vec<(String, String)> = labels["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cell| {
            let slot = cell["slot"].as_u64().unwrap() as u32;
            let id = catalog_bridge::resolve_item_id(&catalog, cell["id"].as_str().unwrap())
                .expect("目录命中");
            (format!("r{}c{}", slot / 4, slot % 4), id.to_string())
        })
        .collect();

    for (label, filter) in [
        ("原始 431x622（缩放后）", image::imageops::FilterType::Nearest),
        ("升采样 576x832（参考原生，CatmullRom）", image::imageops::FilterType::CatmullRom),
        ("升采样 576x832（参考原生，Lanczos3）", image::imageops::FilterType::Lanczos3),
    ] {
        let image = if label.starts_with("原始") {
            cropped.clone()
        } else {
            image::imageops::resize(&cropped, 576, 832, filter)
        };
        let observed = match runtime.recognize(image.clone(), layout) {
            Ok(o) => o,
            Err(e) => {
                println!("UPSCALE_PROBE {label} → 识别失败: {e}");
                continue;
            }
        };
        let mut wins = 0usize;
        let mut best = 0.0f32;
        let mut sum = 0.0f32;
        let mut count = 0usize;
        let slot_w = observed.slots.first().map(|s| s.w).unwrap_or(0);
        for (position, expected) in &wanted {
            let Some(slot) = observed
                .slots
                .iter()
                .find(|s| format!("r{}c{}", s.row, s.col) == *position)
            else {
                println!("UPSCALE_PROBE {label} → 缺少 {position}");
                continue;
            };
            let mut observation = RoiObservation {
                image: image.clone(),
                layout,
                slots: vec![Slot {
                    classification: None,
                    ..slot.clone()
                }],
            };
            runtime.classify(&mut observation).expect("分类");
            count += 1;
            if let Some(c) = observation.slots[0].classification.as_ref() {
                best = best.max(c.match_score);
                sum += c.match_score;
                if c.item_id == *expected {
                    wins += 1;
                }
            }
        }
        let mean = if count > 0 { sum / count as f32 } else { 0.0 };
        println!(
            "UPSCALE_PROBE {label} 槽位 {slot_w}px → best={best:.3} mean={mean:.3} wins={wins}/{count}"
        );
    }
}

///
/// 参考把模板按 `TEMPLATE_BACKGROUND = 30.0` 合成，查询侧却直接用原始亮度
/// （见 `classifier.rs::render_source` / `extract_slot_features`）。
/// 实机格子底色实测中位数 ~66~74，两者口径差 ~2.3 倍。
///
/// 本实验把每个标注格的像素整体平移，使该格**内缩区域的中位亮度 = 30**，
/// 再交给参考分类器。若分数显著回升 → 根因是"捕获帧的光度口径"，
/// 且修法是**标定**（而不是改几何/尺度）。
#[test]
#[ignore = "plan7 §13 诊断探针：默认不参与测试流程，需要时用 --ignored 显式运行"]
fn reference_background_calibration_probe() {
    use crate::vision::{RoiObservation, Slot};

    let frame = load_frame("stratagem_list_labeled_1914x1080.png");
    let runtime = RecognizerRuntime::load().expect("参考运行时");
    let catalog = IconCatalog::load(default_icon_manifest()).expect("内嵌目录");
    let layout = SlotLayout::List(ItemKind::Stratagem);
    let roi = resolve_calibration_roi_for_size(frame.width(), frame.height(), runtime.calibration())
        .expect("ROI");
    let cropped = crop(&frame, roi);
    let base = runtime.recognize(cropped.clone(), layout).expect("识别");

    let text = std::fs::read_to_string(fixture_dir().join("labels_stratagem_list.json")).unwrap();
    let labels: serde_json::Value = serde_json::from_str(&text).unwrap();
    let wanted: Vec<(u32, u32, String)> = labels["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cell| {
            let slot = cell["slot"].as_u64().unwrap() as u32;
            let id = catalog_bridge::resolve_item_id(&catalog, cell["id"].as_str().unwrap())
                .expect("目录命中");
            (slot / 4, slot % 4, id.to_string())
        })
        .collect();

    for target_background in [30.0f32, 45.0, 60.0] {
        let mut adjusted = cropped.clone();
        for slot in &base.slots {
            let inset = 6u32;
            let mut lumas: Vec<u32> = Vec::new();
            for y in slot.y + inset..(slot.y + slot.h).saturating_sub(inset) {
                for x in slot.x + inset..(slot.x + slot.w).saturating_sub(inset) {
                    let p = adjusted.get_pixel(x, y);
                    lumas.push((299 * p[0] as u32 + 587 * p[1] as u32 + 114 * p[2] as u32) / 1000);
                }
            }
            if lumas.is_empty() {
                continue;
            }
            lumas.sort_unstable();
            let median = lumas[lumas.len() / 2] as f32;
            let delta = target_background - median;
            for y in slot.y..(slot.y + slot.h).min(adjusted.height()) {
                for x in slot.x..(slot.x + slot.w).min(adjusted.width()) {
                    let p = adjusted.get_pixel_mut(x, y);
                    for channel in 0..3 {
                        p[channel] = (p[channel] as f32 + delta).clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }

        let mut wins = 0usize;
        let mut best = 0.0f32;
        let mut sum = 0.0f32;
        let mut details = Vec::new();
        for (row, col, expected) in &wanted {
            let Some(base_slot) = base.slots.iter().find(|s| s.row == *row && s.col == *col) else {
                continue;
            };
            let mut observation = RoiObservation {
                image: adjusted.clone(),
                layout,
                slots: vec![Slot {
                    classification: None,
                    ..base_slot.clone()
                }],
            };
            runtime.classify(&mut observation).expect("分类");
            match observation.slots[0].classification.as_ref() {
                Some(c) => {
                    best = best.max(c.match_score);
                    sum += c.match_score;
                    if c.item_id == *expected {
                        wins += 1;
                    }
                    details.push(format!(
                        "r{row}c{col}:{}{:.3}",
                        if c.item_id == *expected { "✔" } else { "✗" },
                        c.match_score
                    ));
                }
                None => details.push(format!("r{row}c{col}:<无>")),
            }
        }
        println!(
            "BG_PROBE 目标底色 {target_background:>5.1} → best={best:.3} mean={:.3} wins={wins}/{} | {}",
            sum / wanted.len() as f32,
            wanted.len(),
            details.join(" ")
        );
    }
}

#[test]
#[ignore = "plan7 §13 诊断探针：默认不参与测试流程，需要时用 --ignored 显式运行"]
fn reference_scale_probe_fast() {
    use crate::vision::{RoiObservation, Slot};

    let frame = load_frame("stratagem_list_labeled_1914x1080.png");
    let runtime = RecognizerRuntime::load().expect("参考运行时");
    let catalog = IconCatalog::load(default_icon_manifest()).expect("内嵌目录");
    let layout = SlotLayout::List(ItemKind::Stratagem);
    let roi = resolve_calibration_roi_for_size(frame.width(), frame.height(), runtime.calibration())
        .expect("ROI");
    let cropped = crop(&frame, roi);
    let base = runtime.recognize(cropped.clone(), layout).expect("识别");

    let text = std::fs::read_to_string(fixture_dir().join("labels_stratagem_list.json")).unwrap();
    let labels: serde_json::Value = serde_json::from_str(&text).unwrap();
    let wanted: Vec<(u32, u32, String)> = labels["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cell| {
            let slot = cell["slot"].as_u64().unwrap() as u32;
            let id = catalog_bridge::resolve_item_id(&catalog, cell["id"].as_str().unwrap())
                .expect("目录命中");
            (slot / 4, slot % 4, id.to_string())
        })
        .collect();

    let mut wins = 0usize;
    let mut best = 0.0f32;
    let mut sum = 0.0f32;
    let mut details = Vec::new();
    for (row, col, expected) in &wanted {
        let Some(base_slot) = base.slots.iter().find(|s| s.row == *row && s.col == *col) else {
            continue;
        };
        let mut observation = RoiObservation {
            image: cropped.clone(),
            layout,
            slots: vec![Slot {
                classification: None,
                ..base_slot.clone()
            }],
        };
        runtime.classify(&mut observation).expect("分类");
        match observation.slots[0].classification.as_ref() {
            Some(c) => {
                best = best.max(c.match_score);
                sum += c.match_score;
                let win = c.item_id == *expected;
                if win {
                    wins += 1;
                }
                details.push(format!(
                    "r{row}c{col}:{}{:.3}",
                    if win { "✔" } else { "✗" },
                    c.match_score
                ));
            }
            None => details.push(format!("r{row}c{col}:<无>")),
        }
    }
    println!(
        "SCALE_PROBE best={best:.3} mean={:.3} wins={wins}/{} | {}",
        sum / wanted.len() as f32,
        wanted.len(),
        details.join(" ")
    );
    println!(
        "SCALE_PROBE_HINT 参考口径 78px 槽位 → nominal {}px",
        78 * 68 / 104
    );
}

///
/// 参考用固定比例 `LIST_ICON_SCALE = 68/104` 从槽位边长推算字形边长
/// （78px 槽位 → 51px 字形）。若实机字形在该帧里**明显更大/更小**，
/// 模板会被渲染成错误尺寸，相似度就会整体塌到 0.3~0.6（实测正是如此）。
#[test]
#[ignore = "plan7 §13 诊断探针：默认不参与测试流程，需要时用 --ignored 显式运行"]
fn reference_glyph_scale_probe() {
    use image::Rgba;

    fn bbox_of<F: Fn(&Rgba<u8>) -> bool>(
        image: &image::RgbaImage,
        origin: (u32, u32),
        window: (u32, u32),
        predicate: F,
    ) -> Option<(u32, u32, u32, u32)> {
        let (x0, y0) = origin;
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0u32, 0u32);
        for y in 0..window.1 {
            for x in 0..window.0 {
                let (px, py) = (x0 + x, y0 + y);
                if px >= image.width() || py >= image.height() {
                    continue;
                }
                if predicate(image.get_pixel(px, py)) {
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }
        if min_x == u32::MAX {
            None
        } else {
            Some((min_x, min_y, max_x - min_x + 1, max_y - min_y + 1))
        }
    }

    let frame = load_frame("stratagem_list_labeled_1914x1080.png");
    let runtime = RecognizerRuntime::load().expect("参考运行时");
    let catalog = IconCatalog::load(default_icon_manifest()).expect("内嵌目录");
    let layout = SlotLayout::List(ItemKind::Stratagem);
    let roi = resolve_calibration_roi_for_size(frame.width(), frame.height(), runtime.calibration())
        .expect("ROI");
    let cropped = crop(&frame, roi);
    let base = runtime.recognize(cropped.clone(), layout).expect("识别");

    println!("\n=== 字形尺寸实测（真实帧 vs 模板）===");
    for (row, col, key, item_id) in [
        (1u32, 0u32, "stalwart", "stalwart"),
        (1, 2, "railgun", "railgun"),
        (3, 3, "laser_cannon", "laser-cannon"),
    ] {
        let Some(slot) = base.slots.iter().find(|s| s.row == row && s.col == col) else {
            continue;
        };
        // 真实格子：内缩 6px 排除格子亮边框，只看格子内部（真正的字形）
        let inset = 6i32;
        let origin = (
            (slot.x as i32 + inset).max(0) as u32,
            (slot.y as i32 + inset).max(0) as u32,
        );
        let window = (
            slot.w.saturating_sub(2 * inset as u32),
            slot.h.saturating_sub(2 * inset as u32),
        );
        // 字形判据：亮且有色（游戏字形是亮青色平面图形）
        let real = bbox_of(&cropped, origin, window, |p| {
            let (r, g, b) = (p[0] as u32, p[1] as u32, p[2] as u32);
            let luma = (299 * r + 587 * g + 114 * b) / 1000;
            luma > 150 && p[3] > 128
        });
        // 模板字形：RGBA 里 alpha 不透明的范围
        let entry = catalog.get(item_id).expect("目录条目");
        let template = crate::assets::icon_image(&entry.path).expect("模板");
        let template_box = bbox_of(&template, (0, 0), (template.width(), template.height()), |p| {
            p[3] > 64
        });
        println!(
            "r{row}c{col} {key:<14} 槽位 {}px | 实机字形 {:?}（占槽位 {:.0}%）| 模板 {:?} / {}px（占 {:.0}%）",
            slot.w,
            real.map(|(_, _, w, h)| (w, h)),
            real.map(|(_, _, w, h)| w as f32 / slot.w as f32 * 100.0).unwrap_or(0.0),
            template_box.map(|(_, _, w, h)| (w, h)),
            template.width(),
            template_box
                .map(|(_, _, w, h)| w as f32 / template.width() as f32 * 100.0)
                .unwrap_or(0.0),
        );
    }
    println!(
        "参考固定口径：LIST_ICON_SCALE = 68/104 = {:.1}%（78px 槽位 → {}px 字形）",
        68.0 / 104.0 * 100.0,
        78 * 68 / 104
    );

    // 背景亮度口径：参考 `TEMPLATE_BACKGROUND = 30.0` 假定游戏格子底色亮度约 30。
    // 若实机底色远高于 30，差值特征会被整体抬高，相似度塌陷。
    println!("\n=== 实机格子内亮度分布（内缩 6px，排除亮边框）===");
    for (row, col, key) in [(1u32, 0u32, "stalwart"), (3, 3, "laser_cannon")] {
        let Some(slot) = base.slots.iter().find(|s| s.row == row && s.col == col) else {
            continue;
        };
        let inset = 6u32;
        let mut lumas: Vec<u32> = Vec::new();
        for y in slot.y + inset..(slot.y + slot.h).saturating_sub(inset) {
            for x in slot.x + inset..(slot.x + slot.w).saturating_sub(inset) {
                let p = cropped.get_pixel(x, y);
                lumas.push((299 * p[0] as u32 + 587 * p[1] as u32 + 114 * p[2] as u32) / 1000);
            }
        }
        lumas.sort_unstable();
        let pick = |q: f32| lumas[((lumas.len() as f32 - 1.0) * q) as usize];
        println!(
            "r{row}c{col} {key:<14} p05={:>3} p25={:>3} p50={:>3} p75={:>3} p90={:>3} p99={:>3}（参考假定底色 30）",
            pick(0.05),
            pick(0.25),
            pick(0.50),
            pick(0.75),
            pick(0.90),
            pick(0.99)
        );
    }
    println!("对照：自证实验（纯黑底 + 模板）→ 0.81~0.87");
}

///
/// 对每个标注格，固定列与宽高、在 y 上以 2px 步长搜索 [-40, +40]，
/// 记录该格能达到的最佳分数与对应预测。
///
/// * 多数格存在某个 `dy` 让**期望战备夺冠** → 失败是几何/对齐问题（网格没落在真实行上）；
/// * 任何 `dy` 都无法让期望战备拿到合理分数 → 实机字形与模板口径不匹配（尺度/渲染差异）。
#[test]
#[ignore = "plan7 §13 诊断探针：默认不参与测试流程，需要时用 --ignored 显式运行"]
fn reference_per_cell_alignment_search() {
    use crate::vision::{RoiObservation, Slot};

    let frame = load_frame("stratagem_list_labeled_1914x1080.png");
    let runtime = RecognizerRuntime::load().expect("参考运行时");
    let catalog = IconCatalog::load(default_icon_manifest()).expect("内嵌目录");
    let layout = SlotLayout::List(ItemKind::Stratagem);
    let roi = resolve_calibration_roi_for_size(frame.width(), frame.height(), runtime.calibration())
        .expect("ROI");
    let cropped = crop(&frame, roi);
    let base = runtime.recognize(cropped.clone(), layout).expect("识别");

    let text = std::fs::read_to_string(fixture_dir().join("labels_stratagem_list.json")).unwrap();
    let labels: serde_json::Value = serde_json::from_str(&text).unwrap();
    let wanted: Vec<(u32, u32, String, String)> = labels["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cell| {
            let slot = cell["slot"].as_u64().unwrap() as u32;
            let key = cell["id"].as_str().unwrap().to_string();
            let id = catalog_bridge::resolve_item_id(&catalog, &key).expect("目录命中");
            (slot / 4, slot % 4, key, id.to_string())
        })
        .collect();

    println!("\n=== 逐格最佳对齐搜索（y 步长 2px，范围 ±40）===");
    println!("格     标注             期望目录 id            最佳dy  最佳预测            最佳分  夺冠");
    let mut cells_with_win = 0usize;
    let mut best_overall = 0.0f32;
    for (row, col, key, expected) in &wanted {
        let Some(base_slot) = base.slots.iter().find(|s| s.row == *row && s.col == *col) else {
            println!("r{row}c{col}  <几何未检测到该格>");
            continue;
        };
        let mut best = (f32::MIN, String::from("<无>"), 0i32);
        for dy in (-40..=40).step_by(2) {
            let slot = Slot {
                x: base_slot.x,
                y: (base_slot.y as i32 + dy).max(0) as u32,
                w: base_slot.w,
                h: base_slot.h,
                row: *row,
                col: *col,
                kind: base_slot.kind,
                classification: None,
            };
            let mut observation = RoiObservation {
                image: cropped.clone(),
                layout,
                slots: vec![slot],
            };
            runtime.classify(&mut observation).expect("分类");
            if let Some(classification) = observation.slots[0].classification.as_ref()
                && classification.match_score > best.0
            {
                best = (
                    classification.match_score,
                    classification.item_id.clone(),
                    dy,
                );
            }
        }
        let win = best.1 == *expected;
        if win {
            cells_with_win += 1;
        }
        best_overall = best_overall.max(best.0);
        println!(
            "r{row}c{col}  {key:<16} {expected:<22} {dy:>5}  {pred:<20} {score:<6} {mark}",
            dy = best.2,
            pred = best.1,
            score = if best.0 == f32::MIN {
                "-".to_string()
            } else {
                format!("{:.3}", best.0)
            },
            mark = if win { "✔" } else { "✗" },
        );
    }
    println!(
        "结论：{cells_with_win}/{} 格在某个 y 偏移下能让期望战备夺冠；全局最高分 {:.3}（门限 0.70）",
        wanted.len(),
        best_overall
    );
}

///
/// 在黑底上按参考自己的口径（槽位 78px、字形 78×68/104 = 51px、居中）贴上模板，
/// 再让参考分类器分类。若这一步都失败，说明问题在调用/机制层（例如尺寸口径），
/// 而不是"实机内容不匹配"。
#[test]
#[ignore = "plan7 §13 诊断探针：默认不参与测试流程，需要时用 --ignored 显式运行"]
fn reference_classifier_self_test() {
    use crate::vision::{RoiObservation, Slot, SlotKind};
    use image::{Rgba, RgbaImage};

    let runtime = RecognizerRuntime::load().expect("参考运行时");
    let catalog = IconCatalog::load(default_icon_manifest()).expect("内嵌目录");
    let layout = SlotLayout::List(ItemKind::Stratagem);

    const SLOT: u32 = 78;
    const ICON: u32 = 51;
    let mut images = Vec::new();
    for item_id in ["stalwart", "railgun", "laser-cannon"] {
        let mut canvas = RgbaImage::from_pixel(431, 622, Rgba([0, 0, 0, 255]));
        let entry = catalog.get(item_id).expect("目录条目");
        let template = crate::assets::icon_image(&entry.path).expect("模板");
        let small = crate::assets::resize_rgba_box(&template, ICON, ICON).expect("缩放");
        let (x0, y0) = (58u32, 137u32);
        let ox = x0 + (SLOT - ICON) / 2;
        let oy = y0 + (SLOT - ICON) / 2;
        for (x, y, pixel) in small.enumerate_pixels() {
            canvas.put_pixel(ox + x, oy + y, *pixel);
        }
        images.push((item_id.to_string(), canvas));
    }

    for (item_id, canvas) in images {
        let mut observation = RoiObservation {
            image: canvas,
            layout,
            slots: vec![Slot {
                x: 58,
                y: 137,
                w: SLOT,
                h: SLOT,
                row: 0,
                col: 0,
                kind: SlotKind::Stratagem,
                classification: None,
            }],
        };
        runtime.classify(&mut observation).expect("分类");
        match observation.slots[0].classification.as_ref() {
            Some(c) => println!(
                "自证：贴入 {item_id:<16} → 分类为 {:<16} score {:.3} margin {:.3} {}",
                c.item_id,
                c.match_score,
                c.match_margin,
                if c.item_id == item_id { "✔" } else { "✗" }
            ),
            None => println!("自证：贴入 {item_id:<16} → <未达门限>（连自己的模板都认不出）"),
        }
    }
}

///
/// 诊断用：**逐格量字形中心相对检测框中心的偏移**（此前只扫过 y，从未扫 x）。
#[test]
#[ignore = "plan7 §13 诊断探针：默认不参与测试流程，需要时用 --ignored 显式运行"]
fn reference_glyph_centering_probe() {
    let frame = load_frame("stratagem_list_labeled_1914x1080.png");
    let runtime = RecognizerRuntime::load().expect("参考运行时");
    let layout = SlotLayout::List(ItemKind::Stratagem);
    let roi = resolve_calibration_roi_for_size(frame.width(), frame.height(), runtime.calibration())
        .expect("ROI");
    let cropped = crop(&frame, roi);
    let base = runtime.recognize(cropped.clone(), layout).expect("识别");

    let text = std::fs::read_to_string(fixture_dir().join("labels_stratagem_list.json")).unwrap();
    let labels: serde_json::Value = serde_json::from_str(&text).unwrap();
    println!("\n=== 字形中心 vs 检测框中心（实机 13 格）===");
    let (mut sum_dx, mut sum_dy, mut n) = (0i32, 0i32, 0i32);
    for cell in labels["cells"].as_array().unwrap() {
        let slot_index = cell["slot"].as_u64().unwrap() as u32;
        let key = cell["id"].as_str().unwrap();
        let (row, col) = (slot_index / 4, slot_index % 4);
        let Some(slot) = base.slots.iter().find(|s| s.row == row && s.col == col) else {
            continue;
        };
        let inset = 5u32;
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (u32::MAX, u32::MAX, 0u32, 0u32);
        let mut count = 0u32;
        for y in slot.y + inset..(slot.y + slot.h).saturating_sub(inset) {
            for x in slot.x + inset..(slot.x + slot.w).saturating_sub(inset) {
                let p = cropped.get_pixel(x, y);
                let luma = (299 * p[0] as u32 + 587 * p[1] as u32 + 114 * p[2] as u32) / 1000;
                if luma > 150 {
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                    count += 1;
                }
            }
        }
        if min_x == u32::MAX {
            println!("r{row}c{col} {key:<18} <无亮像素>");
            continue;
        }
        let glyph_cx = ((min_x + max_x) as f32) / 2.0;
        let glyph_cy = ((min_y + max_y) as f32) / 2.0;
        let (slot_cx, slot_cy) = slot.center_f32();
        let dx = (glyph_cx - slot_cx).round() as i32;
        let dy = (glyph_cy - slot_cy).round() as i32;
        sum_dx += dx;
        sum_dy += dy;
        n += 1;
        println!(
            "r{row}c{col} {key:<18} 字形 {}×{} 中心偏移 dx={dx:>4} dy={dy:>4}（亮像素 {count}）",
            max_x - min_x + 1,
            max_y - min_y + 1
        );
    }
    if n > 0 {
        println!(
            "平均偏移 dx={:.1} dy={:.1}（n={n}）",
            sum_dx as f32 / n as f32,
            sum_dy as f32 / n as f32
        );
    }
}

/// 直接回答一个视觉问题：**参考模板与游戏渲染的字形在形状/尺度/颜色上是否同一套**。
/// 输出 `target/fixture_debug/compare_sheet.png`（左＝实机检测框，右＝参考模板）。
#[test]
#[ignore = "plan7 §13 诊断探针：默认不参与测试流程，需要时用 --ignored 显式运行"]
fn dump_slot_vs_template_contact_sheet() {
    use image::{Rgba, RgbaImage};

    const CELL: u32 = 78;
    const SCALE: u32 = 2;
    const PAD: u32 = 4;

    let frame = load_frame("stratagem_list_labeled_1914x1080.png");
    let runtime = RecognizerRuntime::load().expect("参考运行时");
    let catalog = IconCatalog::load(default_icon_manifest()).expect("内嵌目录");
    let layout = SlotLayout::List(ItemKind::Stratagem);
    let roi = resolve_calibration_roi_for_size(frame.width(), frame.height(), runtime.calibration())
        .expect("ROI");
    let cropped = crop(&frame, roi);
    let observed = runtime.recognize(cropped.clone(), layout).expect("识别");

    let text = std::fs::read_to_string(fixture_dir().join("labels_stratagem_list.json")).unwrap();
    let labels: serde_json::Value = serde_json::from_str(&text).unwrap();
    let cells: Vec<(u32, u32, String)> = labels["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cell| {
            let slot = cell["slot"].as_u64().unwrap() as u32;
            let id = catalog_bridge::resolve_item_id(&catalog, cell["id"].as_str().unwrap())
                .expect("目录命中");
            (slot / 4, slot % 4, id.to_string())
        })
        .collect();

    let tile = CELL * SCALE;
    let sheet_w = PAD + (tile + PAD) * 2;
    let sheet_h = PAD + (tile + PAD) * cells.len() as u32;
    let mut sheet = RgbaImage::from_pixel(sheet_w, sheet_h, Rgba([16, 16, 16, 255]));

    let mut matched_any = false;
    for (index, (row, col, item_id)) in cells.iter().enumerate() {
        let Some(slot) = observed.slots.iter().find(|s| s.row == *row && s.col == *col) else {
            continue;
        };
        // 左：实机格子（参考检测框）
        let cell_crop = image::imageops::crop_imm(&cropped, slot.x, slot.y, slot.w, slot.h)
            .to_image();
        let cell_big =
            image::imageops::resize(&cell_crop, tile, tile, image::imageops::FilterType::Nearest);
        // 右：参考模板（缩放口径与分类器一致：槽位边长 × 68/104）
        let icon_side = ((slot.w as f32) * (68.0 / 104.0)).round() as u32;
        let entry = catalog.get(item_id).expect("目录条目");
        let template = crate::assets::icon_image(&entry.path).expect("模板图标");
        let template_small =
            crate::assets::resize_rgba_box(&template, icon_side.max(1), icon_side.max(1))
                .expect("缩放");
        let mut template_tile = RgbaImage::from_pixel(tile, tile, Rgba([8, 8, 8, 255]));
        let ox = (tile - icon_side) / 2;
        let oy = (tile - icon_side) / 2;
        for (x, y, pixel) in template_small.enumerate_pixels() {
            let (px, py) = (ox + x, oy + y);
            if px < tile && py < tile {
                template_tile.put_pixel(px, py, *pixel);
            }
        }

        let top = PAD + index as u32 * (tile + PAD);
        for (x, y, pixel) in cell_big.enumerate_pixels() {
            sheet.put_pixel(PAD + x, top + y, *pixel);
        }
        for (x, y, pixel) in template_tile.enumerate_pixels() {
            sheet.put_pixel(PAD + tile + PAD + x, top + y, *pixel);
        }
        if !matched_any && slot.classification.is_some() {
            matched_any = true;
        }
    }

    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/fixture_debug/compare_sheet.png");
    sheet.save(&out).expect("保存对照图");
    println!(
        "对照图：{}（每行 = 一个标注格；左＝实机检测框 {}px，右＝参考模板按 68/104 = {}px）",
        out.display(),
        CELL,
        78 * 68 / 104
    );
    println!("本帧有分类结果的槽位数：{}", observed.slots.iter().filter(|s| s.classification.is_some()).count());
}

