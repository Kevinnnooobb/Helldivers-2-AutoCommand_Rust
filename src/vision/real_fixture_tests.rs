// 真实截图回归测试（计划 §十一 / Phase 7）。
//
// 真实截图是仓库里体积较大的手工数据，CI 上不一定存在，因此：
//   * 文件不存在时**显式跳过并打印原因**，绝不静默失败；
//   * 断言只用「结构上一定成立」的性质（几何、槽位数、分割不为空、弃权可解释），
//     不把某一天的准确率写成硬断言——准确率由 `vision-eval` 逐次测量。
//
// 打开更严格的断言（top-3 必须命中）需要显式设置环境变量：
//   $env:H2AC_REAL_VISION_FIXTURES = "1"
use std::path::{Path, PathBuf};

use super::config::VisionConfig;
use super::grid::GeometrySource;
use super::recognize::{DetectionOutcome, RecognizeTarget, VisionEngine};
use super::roi::load_frame;

fn screenshot(name: &str) -> Option<PathBuf> {
    // 相对仓库根目录；测试进程的 cwd 即 crate 根目录。
    let path = Path::new("screenshots").join(name);
    if path.is_file() {
        Some(path)
    } else {
        eprintln!("跳过：真实截图缺失 {}", path.display());
        None
    }
}

fn strict() -> bool {
    std::env::var("H2AC_REAL_VISION_FIXTURES")
        .map(|v| v == "1")
        .unwrap_or(false)
}

fn run(name: &str, target: RecognizeTarget) -> Option<super::recognize::VisionOutput> {
    let path = screenshot(name)?;
    let frame = load_frame(&path).unwrap_or_else(|e| panic!("读取 {} 失败: {e}", path.display()));
    let engine = VisionEngine::new(VisionConfig::default()).expect("引擎构建");
    let output = engine
        .run(&frame, target)
        .unwrap_or_else(|e| panic!("识别 {} 失败: {e}", path.display()));
    Some(output)
}

/// 几何：Home 必须是 5 个槽位（4 战备 + Booster），且几何校验通过。
#[test]
fn vision_real_home_geometry_stays_verified_with_five_slots() {
    for name in ["HOME1.png", "HOME2.png"] {
        let Some(output) = run(name, RecognizeTarget::Loadout) else {
            continue;
        };
        assert_eq!(
            output.plan.slots.len(),
            5,
            "{name}: Home 应为 4 战备 + 1 Booster"
        );
        assert_eq!(
            output.recognition.geometry_source,
            GeometrySource::Verified,
            "{name}: 几何必须来自实测校验"
        );
        assert_eq!(
            output.plan.booster().map(|s| s.index),
            Some(4),
            "{name}: Booster 槽位编号必须是 4"
        );
        assert_eq!(
            output.plan.stratagems().count(),
            4,
            "{name}: 战备槽位应为 4"
        );
    }
}

/// 几何：list 必须是 5 行 × 4 列 = 20 个槽位。
#[test]
fn vision_real_list_geometry_stays_verified_with_twenty_slots() {
    let Some(output) = run("list.png", RecognizeTarget::List) else {
        return;
    };
    assert_eq!(output.plan.slots.len(), 20, "list 应为 20 个槽位");
    assert_eq!(
        (output.plan.rows, output.plan.cols),
        (5, 4),
        "list 应为 5×4"
    );
    assert_eq!(
        output.recognition.geometry_source,
        GeometrySource::Verified,
        "list 几何必须来自实测校验"
    );
    // 槽位编号必须是 0..19 且按行优先递增
    for (i, slot) in output.plan.slots.iter().enumerate() {
        assert_eq!(slot.index, i, "槽位编号必须连续");
        assert_eq!(slot.row as usize, i / 4, "槽位行号必须按行优先");
        assert_eq!(slot.col as usize, i % 4, "槽位列号必须按行优先");
    }
}

/// 分割：真实图标不得整列被判成空槽（重写前的回归点）。
#[test]
fn vision_real_list_icons_are_not_all_empty() {
    let Some(output) = run("list.png", RecognizeTarget::List) else {
        return;
    };
    let empty = output
        .recognition
        .detections
        .iter()
        .filter(|d| d.outcome.is_empty())
        .count();
    assert!(empty <= 1, "20 个真实图标最多只允许 1 个判空，实际 {empty}");
    for trace in &output.traces {
        assert!(
            trace.analysis.bbox.is_some(),
            "槽位 {} 必须能找到主体外接框",
            trace.slot
        );
        let bbox = trace.analysis.bbox.expect("已断言存在");
        assert!(
            bbox.w >= 16 && bbox.h >= 16,
            "槽位 {} 的主体外接框过小（{}×{}）——分割把图标裁碎了",
            trace.slot,
            bbox.w,
            bbox.h
        );
        assert!(
            trace.masks.foreground_px() > 120,
            "槽位 {} 的前景像素过少（{}）",
            trace.slot,
            trace.masks.foreground_px()
        );
    }
}

/// 识别：弃权必须带明确原因；已识别的结果必须有 top-k 与 margin。
#[test]
fn vision_real_list_abstentions_are_explainable() {
    let Some(output) = run("list.png", RecognizeTarget::List) else {
        return;
    };
    let mut recognized = 0;
    for detection in &output.recognition.detections {
        match &detection.outcome {
            DetectionOutcome::Recognized {
                margin, methods, ..
            } => {
                recognized += 1;
                assert!(
                    *margin > 0.0,
                    "槽位 {} 已识别却 margin 为 0",
                    detection.slot
                );
                assert!(
                    methods.mask > 0.0 || methods.edge > 0.0,
                    "槽位 {} 的度量全为 0，证据不足不得识别",
                    detection.slot
                );
            }
            DetectionOutcome::Unknown { reason, .. } => {
                assert!(
                    !reason.label().is_empty(),
                    "槽位 {} 的弃权必须有可读原因",
                    detection.slot
                );
            }
            DetectionOutcome::Empty => {}
        }
    }
    // 真实数据上必须至少有一个可识别结果，否则说明证据链整体失效
    assert!(recognized >= 1, "list 上应当至少识别出 1 个战备");
}

/// 识别（严格模式）：正确模板必须进入 top-3。
///
/// 默认跳过，因为这条断言依赖当前的模板库覆盖度；用
/// `H2AC_REAL_VISION_FIXTURES=1` 打开后它就是发布门槛。
#[test]
fn vision_real_list_correct_templates_reach_top_three() {
    if !strict() {
        eprintln!("跳过：未设置 H2AC_REAL_VISION_FIXTURES=1");
        return;
    }
    let labels_path = Path::new("screenshots/list.labels.json");
    if !labels_path.is_file() {
        eprintln!("跳过：缺失 {}", labels_path.display());
        return;
    }
    let Some(output) = run("list.png", RecognizeTarget::List) else {
        return;
    };
    let raw = std::fs::read_to_string(labels_path).expect("读取标签");
    let labels: super::cli::LabelFile = serde_json::from_str(&raw).expect("解析标签");
    let mut in_top3 = 0;
    for cell in &labels.cells {
        let Some(expected) = cell.id.as_deref() else {
            continue;
        };
        let Some(detection) = output
            .recognition
            .detections
            .iter()
            .find(|d| d.slot == cell.slot)
        else {
            continue;
        };
        let hit = match &detection.outcome {
            DetectionOutcome::Recognized {
                id, alternatives, ..
            } => id.as_str() == expected || alternatives.iter().any(|a| a.id.as_str() == expected),
            DetectionOutcome::Unknown { best, .. } => best
                .as_ref()
                .map(|b| b.as_str() == expected)
                .unwrap_or(false),
            DetectionOutcome::Empty => false,
        };
        if hit {
            in_top3 += 1;
        }
    }
    assert!(
        in_top3 >= 17,
        "正确模板进入 top-3 的数量不足（{in_top3}/20）"
    );
}
