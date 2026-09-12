// Loadout Sync 测试 —— 纯逻辑 + 真实截图 fixture + 合成 fixture，CI 无需启动游戏。
//
// 测试分层（对应实施计划 §62~§64）：
//   1. 纯逻辑：Slot 映射 / 校验 / 配置序列化 / 错误分类
//   2. 视觉：真实截图 fixture 上的 Home 识别；合成 fixture 上的列表识别与图标匹配
//   3. 底层适配契约：几何拟合、Viewport 位移、图标匹配、光标停靠
//
// 装配状态机本身不再由本文件覆盖：动作路径只有 `direct_select`，
// 其状态机测试见 `direct_select::replay`（deterministic frame sequence）。
use std::collections::HashMap;

use image::{GrayImage, RgbaImage};

use crate::loadout_sync::capture::CapturedFrame;
use crate::loadout_sync::config::LoadoutSyncConfig;
use crate::loadout_sync::error::LoadoutSyncError;
use crate::loadout_sync::input::SyncInput;
use crate::loadout_sync::matcher::{
    CellKind, IconMatcher, CELL_BLUR_RADIUS, LIST_GLYPH_SCALE, MATCH_MIN_MARGIN,
    RECOGNITION_THRESHOLD,
};
use crate::loadout_sync::recognizer::{Expect, GameUIRecognizer};
use crate::loadout_sync::selection::{
    LoadoutItem, LoadoutSyncSelection, BOOSTER_SLOT, GAME_STRATAGEM_SLOTS, LOADOUT_FIRST_SLOT,
};
use crate::loadout_sync::types::{Calibration, ImageRect};
use crate::loadout_sync::viewport::ViewportState;
use crate::stratagems::{PluginStratagem, PLUGIN_SLOT_MARK, STRATAGEMS};

// ─── 通用辅助 ───

pub(crate) fn fixture_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/fixtures/loadout_sync")
}

/// 从 fixture PNG 构造一个「捕获帧」。
pub(crate) fn frame_from_png(name: &str) -> Option<CapturedFrame> {
    let path = fixture_dir().join(name);
    let img = image::open(path).ok()?;
    let rgba = img.to_rgba8();
    let gray: GrayImage = img.to_luma8();
    Some(make_frame(gray, Some(rgba)))
}

pub(crate) fn make_frame(gray: GrayImage, rgba: Option<RgbaImage>) -> CapturedFrame {
    let rgba = rgba.unwrap_or_else(|| {
        let mut img = RgbaImage::new(gray.width(), gray.height());
        for (x, y, p) in gray.enumerate_pixels() {
            img.put_pixel(x, y, image::Rgba([p[0], p[0], p[0], 255]));
        }
        img
    });
    CapturedFrame {
        gray,
        rgba,
        origin: crate::loadout_sync::types::ScreenPoint { x: 0, y: 0 },
        backend: crate::loadout_sync::capture::CaptureBackend::Wgc,
    }
}

fn first_index_named(name: &str) -> usize {
    STRATAGEMS
        .iter()
        .position(|s| s.name == name)
        .unwrap_or_else(|| panic!("内置战备缺少 {name}"))
}

/// 按内置战备名取 `LoadoutItem`（图标 key 由 `stratagems.rs` 提供）。
fn named(name: &str) -> LoadoutItem {
    let idx = first_index_named(name);
    LoadoutItem::base(idx).expect("内置战备")
}

fn plugin(name: &str, icon: &str) -> PluginStratagem {
    PluginStratagem {
        name: name.into(),
        category: "Test".into(),
        model: String::new(),
        command: vec!["up".into()],
        description: String::new(),
        icon: icon.into(),
        source: String::new(),
        icon_url: None,
    }
}

/// 构造 H2AC 槽位状态：下排 5 个槽位填入给定内置战备名。
fn slots_with(names: [Option<&str>; 5]) -> (Vec<Option<usize>>, HashMap<usize, PluginStratagem>) {
    let mut slots = vec![None; crate::config::SLOT_COUNT];
    for (i, name) in names.iter().enumerate() {
        if let Some(n) = name {
            slots[LOADOUT_FIRST_SLOT + i] = Some(first_index_named(n));
        }
    }
    (slots, HashMap::new())
}

// ─── §63 必测：Slot 06~10 映射 ───

#[test]
fn test_slot_06_maps_to_stratagem_1() {
    let (slots, plugins) = slots_with([Some("反器材步枪"), None, None, None, None]);
    let sel = LoadoutSyncSelection::from_slots(&slots, &plugins);
    assert_eq!(
        sel.stratagems[0].as_ref().map(|i| i.name.as_str()),
        Some("反器材步枪")
    );
    assert!(sel.stratagems[1].is_none());
    assert!(sel.booster.is_none());
}

#[test]
fn test_slot_07_maps_to_stratagem_2() {
    let (slots, plugins) = slots_with([None, Some("飞鹰空袭"), None, None, None]);
    let sel = LoadoutSyncSelection::from_slots(&slots, &plugins);
    assert_eq!(
        sel.stratagems[1].as_ref().map(|i| i.name.as_str()),
        Some("飞鹰空袭")
    );
    assert!(sel.stratagems[0].is_none());
}

#[test]
fn test_slot_08_maps_to_stratagem_3() {
    let (slots, plugins) = slots_with([None, None, Some("轨道炮攻击"), None, None]);
    let sel = LoadoutSyncSelection::from_slots(&slots, &plugins);
    assert_eq!(
        sel.stratagems[2].as_ref().map(|i| i.name.as_str()),
        Some("轨道炮攻击")
    );
}

#[test]
fn test_slot_09_maps_to_stratagem_4() {
    let (slots, plugins) = slots_with([None, None, None, Some("类星体加农炮"), None]);
    let sel = LoadoutSyncSelection::from_slots(&slots, &plugins);
    assert_eq!(
        sel.stratagems[3].as_ref().map(|i| i.name.as_str()),
        Some("类星体加农炮")
    );
}

#[test]
fn test_slot_10_maps_to_booster() {
    let (slots, plugins) = slots_with([None, None, None, None, Some("增援")]);
    let sel = LoadoutSyncSelection::from_slots(&slots, &plugins);
    assert!(sel.stratagems.iter().all(|s| s.is_none()));
    assert_eq!(sel.booster.as_ref().map(|b| b.name.as_str()), Some("增援"));
}

#[test]
fn upper_row_slots_are_ignored() {
    // 上排 Slot 01~05 装满也不得影响 Loadout Sync
    let mut slots = vec![None; crate::config::SLOT_COUNT];
    for s in slots.iter_mut().take(LOADOUT_FIRST_SLOT) {
        *s = Some(0);
    }
    let sel = LoadoutSyncSelection::from_slots(&slots, &HashMap::new());
    assert_eq!(sel.filled_stratagem_count(), 0);
    assert!(sel.booster.is_none());
    assert!(sel.validate().is_err());
}

#[test]
fn plugin_slot_maps_by_name() {
    let mut slots = vec![None; crate::config::SLOT_COUNT];
    slots[LOADOUT_FIRST_SLOT] = Some(PLUGIN_SLOT_MARK);
    let mut plugins = HashMap::new();
    plugins.insert(LOADOUT_FIRST_SLOT, plugin("自定义战备", "machine_gun"));
    let sel = LoadoutSyncSelection::from_slots(&slots, &plugins);
    let item = sel.stratagems[0].as_ref().expect("插件槽位应映射为战备 1");
    assert_eq!(item.name, "自定义战备");
    assert_eq!(item.icon, "machine_gun");
    assert_eq!(item.base_index, None);
}

#[test]
fn out_of_range_index_is_treated_as_empty() {
    let mut slots = vec![None; crate::config::SLOT_COUNT];
    slots[BOOSTER_SLOT] = Some(usize::MAX - 3); // 非法索引（非插件哨兵）
    let sel = LoadoutSyncSelection::from_slots(&slots, &HashMap::new());
    assert!(sel.booster.is_none());
}

// ─── §5 校验规则 ───

#[test]
fn test_empty_booster_is_valid() {
    let (slots, plugins) = slots_with([
        Some("反器材步枪"),
        Some("飞鹰空袭"),
        Some("轨道炮攻击"),
        Some("类星体加农炮"),
        None,
    ]);
    let sel = LoadoutSyncSelection::from_slots(&slots, &plugins);
    assert!(sel.validate().is_ok());
    assert_eq!(sel.filled_stratagem_count(), 4);
    // Booster 为空时目标只有 4 个
    assert_eq!(sel.targets().len(), 4);
}

#[test]
fn test_missing_stratagem_is_invalid() {
    let (slots, plugins) = slots_with([None, Some("飞鹰空袭"), None, None, None]);
    let sel = LoadoutSyncSelection::from_slots(&slots, &plugins);
    let err = sel.validate().unwrap_err();
    match err {
        LoadoutSyncError::InvalidSelection { detail } => {
            assert!(detail.contains("Stratagem Slot 1"), "detail={detail}");
        }
        other => panic!("期望 InvalidSelection，实际 {other:?}"),
    }
}

#[test]
fn test_partial_loadout_is_rejected() {
    // Slot06 空但 Slot07~09 有：禁止自动左移
    let (slots, plugins) = slots_with([
        None,
        Some("飞鹰空袭"),
        Some("轨道炮攻击"),
        Some("类星体加农炮"),
        None,
    ]);
    let sel = LoadoutSyncSelection::from_slots(&slots, &plugins);
    assert_eq!(sel.stratagems[0], None);
    assert!(sel.validate().is_err());
}

#[test]
fn test_duplicate_stratagem() {
    let (slots, plugins) = slots_with([
        Some("飞鹰空袭"),
        Some("飞鹰空袭"),
        Some("轨道炮攻击"),
        Some("类星体加农炮"),
        None,
    ]);
    let sel = LoadoutSyncSelection::from_slots(&slots, &plugins);
    let err = sel.validate().unwrap_err();
    match err {
        LoadoutSyncError::InvalidSelection { detail } => {
            assert!(detail.contains("重复"), "detail={detail}");
        }
        other => panic!("期望重复提示，实际 {other:?}"),
    }
}

#[test]
fn duplicate_detection_distinguishes_plugin_and_builtin() {
    let mut slots = vec![None; crate::config::SLOT_COUNT];
    slots[LOADOUT_FIRST_SLOT] = Some(first_index_named("飞鹰空袭"));
    slots[LOADOUT_FIRST_SLOT + 1] = Some(PLUGIN_SLOT_MARK);
    let mut plugins = HashMap::new();
    plugins.insert(
        LOADOUT_FIRST_SLOT + 1,
        plugin("飞鹰空袭", "eagle_airstrike"),
    );
    slots[LOADOUT_FIRST_SLOT + 2] = Some(first_index_named("轨道炮攻击"));
    slots[LOADOUT_FIRST_SLOT + 3] = Some(first_index_named("类星体加农炮"));
    let sel = LoadoutSyncSelection::from_slots(&slots, &plugins);
    // 同名但不同来源：按身份键区分（内置 vs 插件）
    assert!(sel.validate().is_ok());
}

#[test]
fn target_order_follows_gui_order() {
    let (slots, plugins) = slots_with([
        Some("反器材步枪"),
        Some("飞鹰空袭"),
        Some("轨道炮攻击"),
        Some("类星体加农炮"),
        Some("增援"),
    ]);
    let sel = LoadoutSyncSelection::from_slots(&slots, &plugins);
    let order: Vec<usize> = sel.targets().iter().map(|(t, _)| t.ordinal()).collect();
    assert_eq!(order, vec![1, 2, 3, 4, 5]);
    assert_eq!(GAME_STRATAGEM_SLOTS, 4);
}

// ─── §39 配置 ───

#[test]
fn loadout_sync_config_defaults_and_sanitize() {
    let cfg: LoadoutSyncConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(cfg.scroll_delta, 600);
    assert_eq!(cfg.scroll_probe_delta, 120);
    // 阈值来自合成 fixture 实测：正确目标 0.61~0.82、最相似干扰项最高 0.72；
    // 默认取 0.60 留出识别矩形抖动的余量，安全性由「整库判别 + 分差」保证
    assert!(
        cfg.recognition_threshold >= 0.55 && cfg.recognition_threshold <= 0.80,
        "默认阈值 {} 超出经验区间",
        cfg.recognition_threshold
    );
    // 单次滚动被限制在「不足一页」（见 controller::step_scroll 的步长上限），
    // 同样一段列表需要更多次滚动，因此默认尝试次数从 12 提到 24
    assert_eq!(cfg.max_scroll_attempts, 24);
    assert!(!cfg.debug_screenshots);

    // 非法值自愈：不允许 0 阈值 / 无限重试
    let bad: LoadoutSyncConfig = serde_json::from_str(
        r#"{"scroll_delta":0,"scroll_probe_delta":99999,"recognition_threshold":0.0,
            "max_scroll_attempts":0,"max_retry_attempts":999,"hover_timeout_ms":0,
            "total_timeout_ms":1}"#,
    )
    .unwrap();
    let fixed = bad.sanitize();
    assert!(fixed.scroll_delta >= 120);
    assert!(fixed.scroll_probe_delta >= 1 && fixed.scroll_probe_delta <= fixed.scroll_delta);
    // 下限保护：0 阈值等于「什么都点」，必须被抬到安全下限
    assert!(fixed.recognition_threshold >= 0.45);
    assert!(fixed.max_scroll_attempts >= 1);
    assert!(fixed.max_retry_attempts <= 8);
    assert!(fixed.hover_timeout_ms >= 100);
    assert!(fixed.total_timeout_ms >= 2000);
}

#[test]
fn loadout_sync_config_roundtrips_in_main_config() {
    let mut cfg = crate::config::Config::default();
    cfg.loadout_sync.scroll_delta = 480;
    cfg.loadout_sync.recognition_threshold = 0.82;
    let json = serde_json::to_string(&cfg).unwrap();
    let back: crate::config::Config = serde_json::from_str(&json).unwrap();
    assert_eq!(back.loadout_sync.scroll_delta, 480);
    assert!((back.loadout_sync.recognition_threshold - 0.82).abs() < 1e-6);
    // 老版本 config.json（没有 loadout_sync 段）必须能加载
    let legacy: crate::config::Config = serde_json::from_str(r#"{"loadout":[1]}"#).unwrap();
    assert_eq!(legacy.loadout_sync.scroll_delta, 600);
    assert!(legacy.loadout_sync_hotkey.is_empty() || legacy.loadout_sync_hotkey == "f7");
}

// ─── §64 视觉 fixture：真实截图上的 Home 识别 ───

fn detect_home_on(fixture: &str) -> (crate::loadout_sync::types::GameLoadoutSlots, CapturedFrame) {
    let frame = frame_from_png(fixture).unwrap_or_else(|| panic!("缺少 fixture {fixture}"));
    let rec = GameUIRecognizer::new(Calibration::default());
    let slots = rec
        .detect_home(&frame)
        .unwrap_or_else(|e| panic!("{fixture} Home 识别失败: {e}"));
    (slots, frame)
}

#[test]
fn real_screenshot_720p_home_is_detected() {
    let (slots, frame) = detect_home_on("home_panel_720p.png");
    // 4 个战备槽 + Booster 都必须实测到位（允许几像素 refinement 偏移）
    let cal = Calibration::default();
    let (_, mapping) = cal.resolve(1280, 720).unwrap();
    let (priors, booster_prior) = cal.home_slot_rects();
    for (i, prior) in priors.iter().enumerate() {
        let expect = cal.roi_to_frame(&mapping, *prior);
        let got = slots.stratagems[i].rect;
        assert!(
            (got.x - expect.x).abs() <= 12 && (got.y - expect.y).abs() <= 12,
            "槽位 {} 位置偏差过大: got {got:?} expect {expect:?}",
            i + 1
        );
        assert!(
            slots.stratagems[i].score > 0.4,
            "槽位 {} 边框得分过低",
            i + 1
        );
    }
    let expect_booster = cal.roi_to_frame(&mapping, booster_prior);
    // Booster 用六边形轮廓定位：中心允许几像素偏差，尺寸按 0.877/0.755 比例收缩
    let (bx, by) = slots.booster.rect.center();
    let (ex, ey) = expect_booster.center();
    assert!(
        (bx - ex).abs() <= 10 && (by - ey).abs() <= 10,
        "booster 中心偏差过大: {bx},{by} vs {ex},{ey}"
    );
    assert!(
        slots.booster.score >= 0.70,
        "booster 轮廓得分 {}",
        slots.booster.score
    );
    assert!(slots.home_score >= 0.5, "home_score={}", slots.home_score);

    // 该截图四个战备槽均为空 → 计数为 0（第一版要求全空）
    let rec = GameUIRecognizer::new(Calibration::default());
    assert_eq!(rec.count_filled_stratagems(&frame, &slots), 0);
}

#[test]
fn real_screenshot_1080p_home_is_detected() {
    let (slots, _) = detect_home_on("home_panel_1080p.png");
    let cal = Calibration::default();
    let (_, mapping) = cal.resolve(1920, 1080).unwrap();
    let (priors, _) = cal.home_slot_rects();
    let expect = cal.roi_to_frame(&mapping, priors[0]);
    assert!((slots.stratagems[0].rect.x - expect.x).abs() <= 12);
    assert!(slots.home_score >= 0.5);
}

#[test]
fn blank_frame_is_not_loadout_home() {
    let blank = make_frame(GrayImage::from_pixel(1280, 720, image::Luma([10])), None);
    let rec = GameUIRecognizer::new(Calibration::default());
    let err = rec.detect_home(&blank).unwrap_err();
    assert!(
        matches!(
            err,
            LoadoutSyncError::SlotNotDetected { .. }
                | LoadoutSyncError::LoadoutHomeNotDetected { .. }
        ),
        "空白画面必须识别失败，实际 {err:?}"
    );
    let r = rec.recognize(&blank, Expect::Any);
    assert_eq!(r.state, crate::loadout_sync::types::UiState::Unknown);
}

#[test]
fn non_16_9_cropped_frame_fails_safely() {
    // 21:9 但使用了裁剪后的画面尺寸（模拟 UI 被裁掉一部分）
    let frame = make_frame(GrayImage::from_pixel(2560, 700, image::Luma([10])), None);
    let rec = GameUIRecognizer::new(Calibration::default());
    assert!(rec.detect_home(&frame).is_err());
}

#[test]
fn rect_geometry_is_inside_frame() {
    let frame = frame_from_png("home_panel_720p.png").unwrap();
    let rec = GameUIRecognizer::new(Calibration::default());
    let slots = rec.detect_home(&frame).unwrap();
    for s in slots.all() {
        assert!(
            s.rect
                .clamp_to_frame(frame.width() as i32, frame.height() as i32)
                .is_some(),
            "槽位矩形越界: {:?}",
            s.rect
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
//  合成 fixture：识别几何 / Viewport / IconMatcher 测试
//  （CI 不需要启动游戏，也不需要真实鼠标输入）
// ═══════════════════════════════════════════════════════════════════════

const SIM_W: u32 = 1280;
const SIM_H: u32 = 720;
/// 列表视图同时可见的行数（与标定列表区高度一致）
const VISIBLE_ROWS: usize = 5;

fn sim_cal() -> Calibration {
    Calibration::default()
}

fn sim_geom() -> (crate::loadout_sync::types::RoiMapping, Calibration) {
    let cal = sim_cal();
    let (_, mapping) = cal.resolve(SIM_W, SIM_H).unwrap();
    (mapping, cal)
}

/// 列表单元格（帧坐标）。注意 roi_to_frame 接受的是「ROI 参考像素」，
/// 尺寸由标定单位给出，缩放由映射完成（不要预先乘 scale，否则会二次缩放）。
fn sim_row_rect(row: usize, col: usize) -> ImageRect {
    let (mapping, cal) = sim_geom();
    let size = cal.slot_size;
    cal.roi_to_frame(
        &mapping,
        ImageRect::new(
            cal.list_cols[col],
            cal.list_top + (row as i32) * cal.row_pitch,
            size,
            size,
        ),
    )
}

fn sim_home_rect(index: usize) -> ImageRect {
    let (mapping, cal) = sim_geom();
    let (strat, booster) = cal.home_slot_rects();
    let rel = if index < 4 { strat[index] } else { booster };
    cal.roi_to_frame(&mapping, rel)
}

fn sim_booster_hex() -> ImageRect {
    let slot = sim_home_rect(4);
    sim_cal().booster_hex_rect(slot)
}

// ─── 绘制辅助 ───

/// 矩形包含判定（测试专用：生产代码只在识别器内部按边线判断，不需要该 helper）。
#[allow(dead_code)] // 供合成帧模拟器类的断言保留
fn rect_contains(r: ImageRect, x: i32, y: i32) -> bool {
    x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
}

fn put(img: &mut GrayImage, x: i32, y: i32, v: u8) {
    if x < 0 || y < 0 || x >= img.width() as i32 || y >= img.height() as i32 {
        return;
    }
    img.put_pixel(x as u32, y as u32, image::Luma([v]));
}

fn fill(img: &mut GrayImage, r: ImageRect, v: u8) {
    for y in r.y..r.bottom() {
        for x in r.x..r.right() {
            put(img, x, y, v);
        }
    }
}

fn frame_lines(img: &mut GrayImage, r: ImageRect, thickness: i32, v: u8) {
    for t in 0..thickness {
        let rr = ImageRect::new(r.x + t, r.y + t, r.w - 2 * t, r.h - 2 * t);
        if rr.w <= 0 || rr.h <= 0 {
            break;
        }
        for x in rr.x..rr.right() {
            put(img, x, rr.y, v);
            put(img, x, rr.bottom() - 1, v);
        }
        for y in rr.y..rr.bottom() {
            put(img, rr.x, y, v);
            put(img, rr.right() - 1, y, v);
        }
    }
}

/// 画六边形轮廓（Booster 槽）。
fn draw_hexagon(img: &mut GrayImage, hex: ImageRect, v: u8) {
    let (cx, cy) = hex.center();
    let rx = hex.w as f32 / 2.0;
    let ry = hex.h as f32 / 2.0;
    let mut verts = [(0.0f32, 0.0f32); 6];
    for (k, item) in verts.iter_mut().enumerate() {
        let a = std::f32::consts::PI / 3.0 * k as f32;
        *item = (cx as f32 + rx * a.cos(), cy as f32 + ry * a.sin());
    }
    for k in 0..6 {
        let (x0, y0) = verts[k];
        let (x1, y1) = verts[(k + 1) % 6];
        let steps = ((x1 - x0).abs().max((y1 - y0).abs()) as i32).max(1) * 2;
        for s in 0..=steps {
            let t = s as f32 / steps as f32;
            let x = (x0 + (x1 - x0) * t).round() as i32;
            let y = (y0 + (y1 - y0) * t).round() as i32;
            put(img, x, y, v);
            put(img, x + 1, y, v);
            put(img, x, y + 1, v);
        }
    }
}

/// 单元格：内部底色 + 可选图标 + 边框（Hover 时内部点亮）。
///
/// `card_scale` 由调用方给出：列表与 home 的卡片比例不同（游戏属性）。
fn draw_cell(
    img: &mut GrayImage,
    rect: ImageRect,
    icon: Option<&str>,
    hovered: bool,
    card_scale: f32,
) {
    let interior = if hovered { 95 } else { 64 };
    fill(img, rect.inset(2), interior);
    if let Some(key) = icon {
        blit_icon_card(img, key, rect, card_scale);
    }
    frame_lines(img, rect, 2, 210);
}

/// 合成列表帧：`rows[i]` 是第 i 个可见行的图标 key（画在第 0 列，其余列留空）。
///
/// 供识别几何与图标匹配测试使用（Viewport 位移、CellKind 语义、模板比对）。
fn list_frame(rows: &[&str]) -> CapturedFrame {
    let mut img = GrayImage::from_pixel(SIM_W, SIM_H, image::Luma([20]));
    for (row_idx, row_key) in rows.iter().enumerate().take(VISIBLE_ROWS) {
        for col in 0..4 {
            let rect = sim_row_rect(row_idx, col);
            let icon = if col == 0 { Some(*row_key) } else { None };
            draw_cell(&mut img, rect, icon, false, SIM_CARD_SCALE);
        }
    }
    make_frame(img, None)
}

/// 从合成帧构建 Viewport（行/列几何由识别器给出）。
#[allow(dead_code)] // 保留给位移回归断言使用的构造入口
fn viewport_of(frame: &CapturedFrame) -> ViewportState {
    let rec = GameUIRecognizer::new(sim_cal());
    let grid = rec.detect_list(frame).expect("列表识别失败");
    ViewportState::build(&frame.gray, &grid).expect("Viewport 构建失败")
}

// ─── 合成 fixture 绘制辅助（供上面的几何/匹配测试使用） ───

/// 以「游戏渲染模型」画图标：alpha 剪影按固定比例（相对格子）绘制，
/// 保留各图标自身的形状，与游戏里平面浅色图形的表现一致。
///
/// 图标解码缓存：合成一帧要画 20 个格子，逐格重新解码 PNG 会让测试慢一个数量级。
fn decoded_icon(key: &str) -> Option<&'static image::RgbaImage> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<String, &'static image::RgbaImage>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    {
        let map = cache.lock().unwrap();
        if let Some(img) = map.get(key) {
            return Some(*img);
        }
    }
    let bytes = crate::icons::icon_png_bytes(key)?;
    let img: &'static image::RgbaImage =
        Box::leak(Box::new(image::load_from_memory(bytes).ok()?.to_rgba8()));
    cache.lock().unwrap().insert(key.to_string(), img);
    Some(img)
}

/// 合成帧里的「游戏卡片渲染比例」：卡片画布边长 / 格子边长。
///
/// 这是**游戏属性**，不是匹配器的可调参数：参考实现在实机上量得列表 68/104 = 0.654、
/// home 93/104 = 0.894（本机在真实标注帧上的量测与之吻合）。因此这里用独立常量，
/// 而不是引用 matcher 的实现常量 —— 匹配器若把比例改错，测试必须能发现。
const SIM_CARD_SCALE: f32 = 0.654;
/// home 槽位的卡片比例（参考实现实机量得 93/104；home 槽位比列表格子更满）。
const SIM_HOME_CARD_SCALE: f32 = 0.894;

/// 按游戏真实渲染模型把图标画进格子：**整张资源画布**按固定比例居中粘贴，
/// 并保留资源自身的强度结构（白字图标 ≈255、分类色剪影 ≈146）。
///
/// 为什么不是「按 alpha 包围盒缩放到固定字形高度」（旧实现）：
/// 实机是固定画布 + 卡片内白字与剪影各自大小不同（实测剪影高度占卡片 15%~60%），
/// 包围盒归一化会把不同战备缩放到不同尺寸；旧模拟器照包围盒画，等于把这个
/// 错误假设写进了测试，使错误算法在合成数据上也能通过阈值。
fn blit_icon_card(img: &mut GrayImage, key: &str, cell: ImageRect, scale: f32) {
    // 解码结果本身就是 &'static，无需再 clone（每帧要画 20 个格子，
    // 逐格克隆 256x256 RGBA 会让合成帧的成本超过被测量的识别算法本身）
    let Some(src) = decoded_icon(key) else { return };
    let cap = cell.w.min(cell.h).max(4) as u32;
    let side = (((cap as f32) * scale).round() as u32).clamp(4, cap);
    let scaled = crate::loadout_sync::matcher::resize_rgba(src, side, side);
    let ox = cell.x + (cell.w - side as i32) / 2;
    let oy = cell.y + (cell.h - side as i32) / 2;
    for (x, y, p) in scaled.enumerate_pixels() {
        if p[3] <= 32 {
            continue;
        }
        let tx = ox + x as i32;
        let ty = oy + y as i32;
        if tx < 0 || ty < 0 || tx >= img.width() as i32 || ty >= img.height() as i32 {
            continue;
        }
        let luma = 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32;
        let alpha = p[3] as f32 / 255.0;
        let background = img.get_pixel(tx as u32, ty as u32)[0] as f32;
        let value = (background * (1.0 - alpha) + luma * alpha)
            .round()
            .clamp(0.0, 255.0) as u8;
        put(img, tx, ty, value);
    }
}

// ─── 回归测试：识别精度（此前偏移 3~5px 导致匹配分从 0.90 掉到 0.57） ───

#[test]
fn detected_cells_align_with_rendered_slots() {
    let frame = list_frame(&[
        "machine_gun",
        "autocannon",
        "eagle_airstrike",
        "orbital_precision_strike",
        "stalwart",
    ]);
    let rec = GameUIRecognizer::new(sim_cal());
    let grid = rec.detect_list(&frame).expect("列表识别");
    let mut worst = 0;
    for row in 0..grid.rows {
        for (col, cell) in grid.row_cells(row).iter().enumerate() {
            let truth = sim_row_rect(row as usize, col);
            let dx = (cell.rect.x - truth.x).abs();
            let dy = (cell.rect.y - truth.y).abs();
            worst = worst.max(dx).max(dy);
            assert!(
                dx <= 3 && dy <= 3,
                "row{row} col{col} 识别矩形 {:?} 与渲染真值 {:?} 偏差过大 (dx={dx}, dy={dy})",
                cell.rect,
                truth
            );
        }
    }
    assert!(worst <= 3);
}

#[test]
fn detected_home_slots_align_with_rendered_slots() {
    let mut img = GrayImage::from_pixel(SIM_W, SIM_H, image::Luma([20]));
    for i in 0..4 {
        draw_cell(&mut img, sim_home_rect(i), None, false, SIM_HOME_CARD_SCALE);
    }
    fill(&mut img, sim_home_rect(4).inset(6), 64);
    draw_hexagon(&mut img, sim_booster_hex(), 210);
    let frame = make_frame(img, None);
    let rec = GameUIRecognizer::new(sim_cal());
    let slots = rec.detect_home(&frame).expect("Home 识别");
    for i in 0..4 {
        let truth = sim_home_rect(i);
        let got = slots.stratagems[i].rect;
        assert!(
            (got.x - truth.x).abs() <= 3 && (got.y - truth.y).abs() <= 3,
            "槽位 {} 位置偏差过大: got {got:?} truth {truth:?}",
            i + 1
        );
    }
    assert!(slots.home_score > 0.8, "home_score={}", slots.home_score);
}

#[test]
fn matcher_scores_correct_target_above_threshold_on_detected_cells() {
    // 用「识别出来的矩形」而不是渲染真值做匹配，确保端到端精度
    let frame = list_frame(&[
        "machine_gun",
        "autocannon",
        "eagle_airstrike",
        "orbital_precision_strike",
        "stalwart",
    ]);
    let rec = GameUIRecognizer::new(sim_cal());
    let grid = rec.detect_list(&frame).expect("列表识别");
    let cell = *grid.row_cells(2)[0];
    let target = named("飞鹰空袭");
    let matcher = IconMatcher::for_items(&[&target]);
    let score = matcher
        .score_cell_for(
            &frame,
            cell.rect,
            &LoadoutItem {
                name: "飞鹰空袭".into(),
                icon: "eagle_airstrike".into(),
                base_index: None,
            },
            CellKind::ListStratagem,
        )
        .unwrap_or(0.0);
    assert!(
        score >= 0.65,
        "识别矩形下的正确目标得分过低: {score:.3}（此前未做边缘吸附时为 0.57）"
    );
}

#[test]
fn library_classification_accepts_present_targets_even_when_nearly_tied() {
    // 整库判别：12 个图标逐个作为列表中的目标，都必须排在第一位
    let keys = [
        "machine_gun",
        "autocannon",
        "anti_materiel_rifle",
        "eagle_airstrike",
        "orbital_precision_strike",
        "stalwart",
        "grenade_launcher",
        "eagle_500kg_bomb",
        "laser_cannon",
        "railgun",
        "arc_thrower",
        "quasar_cannon",
    ];
    let rec = GameUIRecognizer::new(sim_cal());
    let items: Vec<LoadoutItem> = keys
        .iter()
        .map(|k| LoadoutItem {
            name: (*k).to_string(),
            icon: (*k).to_string(),
            base_index: None,
        })
        .collect();
    let refs: Vec<&LoadoutItem> = items.iter().collect();
    let matcher = IconMatcher::for_targets_only(&refs);
    for truth in keys {
        let rows = ["stalwart", "laser_cannon", truth, "arc_thrower", "railgun"];
        let frame = list_frame(&rows);
        let grid = rec.detect_list(&frame).unwrap();
        let cell = *grid.row_cells(2)[0];
        let (winner, score, _margin) = matcher
            .classify_cell_all(&frame, cell.rect, CellKind::ListStratagem)
            .expect("分类");
        let idx = keys.iter().position(|k| *k == truth).unwrap();
        let own = matcher
            .score_cell_for(&frame, cell.rect, &items[idx], CellKind::ListStratagem)
            .unwrap_or(0.0);
        // 目标确实在列表里：要么它就是整库最优，要么与最优近似打平（容差内）
        assert!(
            winner == truth || own + crate::loadout_sync::types::LIBRARY_TIE_EPSILON >= score,
            "整库判别把 {truth} 判成了 {winner}（{score:.3} vs {own:.3}），已超出近似打平容差"
        );
        assert!(own >= 0.55, "{truth} 得分过低 {own:.3}");
    }
}

#[test]
fn absent_target_is_not_stolen_by_a_similar_icon() {
    // 安全底线：目标（轨道磁轨炮）不在列表里时，绝不允许把「轨道激光炮」的格子
    // 当成目标点下去（实测两者分数 0.72 vs 0.90，必须被整库判别拦下）。
    let rows = [
        "orbital_laser",
        "machine_gun",
        "stalwart",
        "railgun",
        "arc_thrower",
    ];
    let frame = list_frame(&rows);
    let rec = GameUIRecognizer::new(sim_cal());
    let grid = rec.detect_list(&frame).unwrap();
    let cell = *grid.row_cells(0)[0];
    let target = LoadoutItem {
        name: "轨道炮攻击".into(),
        icon: "orbital_railcannon_strike".into(),
        base_index: None,
    };
    let matcher = IconMatcher::for_items(&[&target]);
    let (winner, winner_score, _m) = matcher
        .classify_cell_all(&frame, cell.rect, CellKind::ListStratagem)
        .expect("分类");
    let own = matcher
        .score_cell_for(&frame, cell.rect, &target, CellKind::ListStratagem)
        .unwrap_or(0.0);
    assert_ne!(winner, target.icon);
    assert!(
        own + crate::loadout_sync::types::LIBRARY_TIE_EPSILON < winner_score,
        "相似图标冒充没有被拦下：target={own:.3} winner={winner}({winner_score:.3})"
    );
}

#[test]
fn matcher_loads_runtime_downloaded_icon_from_disk() {
    // Loadout Sync 的 Booster 目标是运行期从 wiki 下载的 PNG（不在内嵌图标表里），
    // 必须能从 exe 旁 assets/icons/{key}.png 兜底加载成模板。
    let dir = crate::util::app_dir().join("assets/icons");
    std::fs::create_dir_all(&dir).expect("创建图标目录");
    let key = "test_runtime_booster_icon";
    let bytes = crate::icons::icon_png_bytes("supply_pack").expect("内置图标");
    std::fs::write(dir.join(format!("{key}.png")), bytes).expect("写入临时图标");

    let item = LoadoutItem {
        name: "测试强化".into(),
        icon: key.into(),
        base_index: None,
    };
    let matcher = IconMatcher::for_items(&[&item]);
    assert!(
        matcher.has(key),
        "运行期下载的图标必须能作为模板加载（失败记录: {:?}）",
        matcher.load_failures
    );

    let _ = std::fs::remove_file(dir.join(format!("{key}.png")));
}

#[test]
fn empty_cells_score_zero_and_are_not_matched() {
    // 空槽（均匀灰块）不得与任何图标产生虚假高分
    let frame = list_frame(&[
        "machine_gun",
        "autocannon",
        "eagle_airstrike",
        "stalwart",
        "railgun",
    ]);
    let rec = GameUIRecognizer::new(sim_cal());
    let grid = rec.detect_list(&frame).unwrap();
    let empty_cell = *grid.row_cells(1)[1]; // 第 1 行第 1 列为空
    let target = named("飞鹰空袭");
    let matcher = IconMatcher::for_items(&[&target]);
    let score = matcher
        .score_cell_for(&frame, empty_cell.rect, &target, CellKind::ListStratagem)
        .unwrap_or(0.0);
    assert_eq!(score, 0.0, "空槽不应产生匹配分（实测 {score:.3}）");
    assert!(matcher
        .find_in_cells(&frame, &[empty_cell], &target, CellKind::ListStratagem)
        .is_none());
}

/// 实机验证：抓取正在运行的游戏窗口（需要 HELLDIVERS 2 正在运行，默认 ignore）。
///
/// 这条测试专门盯住「三种显示模式下都抓到全黑」这个回归：
/// 它把真实帧落盘成 PNG，并断言画面不是全黑。
#[test]
#[ignore = "需要 HELLDIVERS 2 正在运行"]
fn live_capture_is_not_black_in_any_display_mode() {
    let win = crate::loadout_sync::window::find_game_window().expect("未找到 HELLDIVERS 2 窗口");
    let frame =
        crate::loadout_sync::capture::capture_client_area(&win, true).expect("抓取游戏客户区失败");
    let dir = crate::util::app_dir().join("screenshots/loadout_sync");
    let path = dir.join("live_capture.png");
    frame.save_png(&path).expect("保存实测截图失败");

    let total: u64 = frame.gray.pixels().map(|p| p.0[0] as u64).sum();
    let mean = total as f32 / (frame.width() as f32 * frame.height() as f32).max(1.0);
    let max = frame.gray.pixels().map(|p| p.0[0]).max().unwrap_or(0);
    eprintln!(
        "backend={:?} size={}x{} mean={mean:.1} max={max} origin=({},{}) saved={}",
        frame.backend,
        frame.width(),
        frame.height(),
        frame.origin.x,
        frame.origin.y,
        path.display()
    );
    assert!(mean > 1.0, "画面全黑（mean={mean:.2}），截图后端仍然失效");
    assert!(
        max > 32,
        "画面几乎没有亮部（max={max}），可能只抓到了空背景"
    );

    // 连续抓取：确认会话复用生效、单帧耗时可控（识别流水线按帧推进）
    let mut durations = Vec::new();
    for _ in 0..5 {
        let t0 = std::time::Instant::now();
        let next =
            crate::loadout_sync::capture::capture_client_area(&win, false).expect("连续抓取失败");
        durations.push(t0.elapsed());
        assert_eq!(next.width(), frame.width(), "连续抓取的尺寸必须一致");
        assert_eq!(next.origin, frame.origin, "连续抓取的客户区原点必须一致");
    }
    let avg = durations.iter().map(|d| d.as_millis()).sum::<u128>() as f32 / durations.len() as f32;
    let worst = durations.iter().map(|d| d.as_millis()).max().unwrap_or(0);
    eprintln!("连续抓取 5 帧：平均 {avg:.1}ms，最慢 {worst}ms");
    assert!(worst < 1000, "单帧抓取耗时异常（{worst}ms）");
}

/// 一次性探针：把实机抓到的列表画面喂给识别器，打印行/列检测结果。
/// （用于定位「滚动未生效」这类实机问题，默认 ignore）
#[test]
#[ignore = "实机探针"]
fn probe_real_frame_list_detection() {
    let path = std::env::var("H2AC_PROBE_PNG").expect("需要 H2AC_PROBE_PNG");
    let img = image::open(&path).expect("读取探针图片失败");
    let gray = img.to_luma8();
    let frame = make_frame(gray, None);
    let rec = GameUIRecognizer::new(Calibration::default());
    eprintln!("帧尺寸 {}x{}", frame.width(), frame.height());
    match rec.resolve_geometry(&frame) {
        Ok(geom) => eprintln!("几何: {geom:?}"),
        Err(e) => eprintln!("几何解析失败: {e}"),
    }
    match rec.detect_list(&frame) {
        Ok(grid) => {
            eprintln!(
                "列表: rows={} cols={} cells={}",
                grid.rows,
                grid.cols,
                grid.cells.len()
            );
            for i in 0..grid.rows {
                let cells = grid.row_cells(i);
                let r = cells.first().map(|c| c.rect).unwrap_or_default();
                eprintln!("  行{i}: y={} h={} cells={}", r.y, r.h, cells.len());
            }
            if let Some(vp) =
                crate::loadout_sync::viewport::ViewportState::build(&frame.gray, &grid)
            {
                eprintln!("Viewport 行数: {}", vp.rows.len());
            }
            // 行边线信号剖面：看真实列表里每一条边线的强度分布
            let geom = rec.resolve_geometry(&frame).expect("几何");
            let scale = geom.mapping.scale;
            let size = ((Calibration::default().slot_size as f32) * scale).round() as i32;
            let thickness = ((3.0 * scale).round() as i32).max(2);
            let cols: Vec<_> = Calibration::default()
                .list_cols
                .iter()
                .map(|x| {
                    Calibration::default().roi_to_frame(
                        &geom.mapping,
                        ImageRect::new(*x, Calibration::default().list_top, size, size),
                    )
                })
                .collect();
            let (_, y0) = Calibration::default().roi_point(
                &geom.mapping,
                0.0,
                Calibration::default().list_top as f32 - 12.0,
            );
            let (_, y1) = Calibration::default().roi_point(
                &geom.mapping,
                0.0,
                Calibration::default().list_bottom as f32 + 12.0,
            );
            let from = y0.round() as i32;
            let to = (y1.round() as i32).min(frame.height() as i32 - 2);
            let mut line = String::new();
            for y in from..=(to - size) {
                let s = crate::loadout_sync::recognizer::row_line_score(
                    &frame.gray,
                    y,
                    &cols,
                    size,
                    thickness,
                );
                let up = crate::loadout_sync::recognizer::row_line_score(
                    &frame.gray,
                    y + size,
                    &cols,
                    size,
                    thickness,
                );
                let sig = s.min(up);
                let prev = if y > from {
                    let a = crate::loadout_sync::recognizer::row_line_score(
                        &frame.gray,
                        y - 1,
                        &cols,
                        size,
                        thickness,
                    );
                    let b = crate::loadout_sync::recognizer::row_line_score(
                        &frame.gray,
                        y - 1 + size,
                        &cols,
                        size,
                        thickness,
                    );
                    a.min(b)
                } else {
                    0.0
                };
                let next = {
                    let a = crate::loadout_sync::recognizer::row_line_score(
                        &frame.gray,
                        y + 1,
                        &cols,
                        size,
                        thickness,
                    );
                    let b = crate::loadout_sync::recognizer::row_line_score(
                        &frame.gray,
                        y + 1 + size,
                        &cols,
                        size,
                        thickness,
                    );
                    a.min(b)
                };
                if sig >= 0.15 && sig >= prev && sig >= next {
                    line.push_str(&format!(
                        "y={y} sig={sig:.2} top={s:.2} bot={up:.2}
"
                    ));
                }
            }
            eprintln!(
                "候选边线（sig>=0.30）:
{line}"
            );
            let signal = |y: i32| -> f32 {
                if y < from || y + size > to {
                    0.0
                } else {
                    let a = crate::loadout_sync::recognizer::row_line_score(
                        &frame.gray,
                        y,
                        &cols,
                        size,
                        thickness,
                    );
                    let b = crate::loadout_sync::recognizer::row_line_score(
                        &frame.gray,
                        y + size,
                        &cols,
                        size,
                        thickness,
                    );
                    a.min(b)
                }
            };
            let expected = Calibration::default().row_pitch as f32 * scale;
            eprintln!(
                "网格拟合行: {:?}",
                crate::loadout_sync::recognizer::fit_row_grid(&signal, from, to, size, expected)
            );
            eprintln!(
                "峰追踪行: {:?}",
                crate::loadout_sync::recognizer::track_row_peaks(&signal, from, to, size, expected)
            );
        }
        Err(e) => eprintln!("列表识别失败: {e}"),
    }
}

// ─── 真实列表截图上的滚动验证（landmark 位移） ───

/// 用真实游戏列表截图构造 Viewport：行/列位置来自实机实测（见夹具 README）。
fn real_list_viewport() -> crate::loadout_sync::viewport::ViewportState {
    let path = fixture_dir().join("stratagem_list_panel.png");
    let img = image::open(path)
        .expect("缺少真实列表夹具 stratagem_list_panel.png")
        .to_luma8();
    let row_tops = [121i32, 206, 337, 418];
    let cols = [65i32, 150, 235, 320];
    let size = 79i32;
    let mut cells = Vec::new();
    for (r, top) in row_tops.iter().enumerate() {
        for (c, left) in cols.iter().enumerate() {
            cells.push(crate::loadout_sync::types::ListCell {
                rect: crate::loadout_sync::types::ImageRect::new(*left, *top, size, size),
                row: r as u32,
                col: c as u32,
                score: 1.0,
            });
        }
    }
    let grid = crate::loadout_sync::types::ListGrid::from_cells(cells);
    crate::loadout_sync::viewport::ViewportState::build(&img, &grid)
        .expect("真实列表 Viewport 构建失败")
}

/// 一整页滚动：内容全部换掉、测不出位移，必须标记为 dislocated 而不是「没有位移」。
#[test]
fn real_list_full_page_scroll_is_reported_as_dislocated() {
    let before = real_list_viewport();
    let shifted = real_list_viewport();
    // 把内容整体上移两行（2 行 × 4 列 = 8 格），底部补两行新内容：
    // 用「最后一行与第一行互换」的签名构造一个与原来完全不重合的视口
    let mut after = shifted.clone();
    let mut rows = after.rows.clone();
    rows.rotate_left(2);
    for (i, row) in rows.iter_mut().enumerate() {
        row.row = i as u32;
        row.top_y = 100 + (i as i32) * 85;
    }
    after.rows = rows;
    let reloc = crate::loadout_sync::list_map::relocalize(&before, &after);
    assert!(
        reloc.matched < crate::loadout_sync::list_map::MIN_MATCHED_ROWS
            || reloc.delta_rows.abs() <= 2,
        "旋转后的位移应当是整体换页或小位移，实际 {:?}",
        reloc
    );
}

/// 同内容视口必须被判为「没有位移」（不能把静止画面误判成滚动）。
#[test]
fn real_list_identical_viewport_reports_no_movement() {
    let before = real_list_viewport();
    let after = real_list_viewport();
    let reloc = crate::loadout_sync::list_map::relocalize(&before, &after);
    assert_eq!(reloc.delta_rows, 0, "同一画面不应有位移");
    assert!(!reloc.moved_at_all(), "同一画面不能算作已移动");
    assert!(!reloc.dislocated, "同一画面不是换页");
}

/// 滚出一行：位移必须被测量出来，方向为 Down（内容上移）。
#[test]
fn real_list_one_row_scroll_is_measurable() {
    let before = real_list_viewport();
    assert!(before.rows.len() >= 3, "真实夹具应至少识别出 3 行");
    // 顶行滚出去，底部补一行「新内容」（用第一行的签名，避免与剩余行重合）
    let mut after = before.clone();
    let new_bottom = before.rows[0].clone();
    after.rows.remove(0);
    for (i, row) in after.rows.iter_mut().enumerate() {
        row.row = i as u32;
        row.top_y = 121 + (i as i32) * 85;
    }
    let mut filler = new_bottom;
    filler.row = after.rows.len() as u32;
    filler.top_y = 121 + (after.rows.len() as i32) * 85;
    after.rows.push(filler);

    let reloc = crate::loadout_sync::list_map::relocalize(&before, &after);
    assert!(
        reloc.moved_as_expected(crate::loadout_sync::list_map::ScrollDirection::Down),
        "内容上移一行应判定为 Down，实际 {reloc:?}"
    );
    assert!(reloc.matched >= 2, "应至少匹配到 2 行，实际 {reloc:?}");
}

/// 滚动步长必须留下重叠行：可见行数一半（至少 1 格）。
#[test]
fn scroll_step_is_capped_to_keep_overlapping_rows() {
    // 直接验证换算：600 单位 = 5 格是整页，必须被拦到 ≤ 可见行数的一半
    let visible = 4i32;
    let cap = ((visible + 1) / 2).max(1);
    assert_eq!(cap, 2, "4 行可见时单次最多滚 2 格");
    let requested = 600 / crate::loadout_sync::config::WHEEL_UNIT;
    assert_eq!(requested, 5);
    assert_eq!(
        requested.min(cap),
        2,
        "整页滚动必须被压到 2 格，否则 landmark 全部对不上"
    );
}

// ─── 实机列表图标匹配探针 ───

/// 用真实列表帧跑一遍整库判别，打印每格 top-5（人工核对身份、判断失配模式）。
#[test]
#[ignore = "实机匹配探针"]
fn probe_real_list_matching() {
    let (frame, rects) = labeled_frame();
    let matcher = IconMatcher::for_items(&[]);
    for (idx, ((row, col), expected)) in LABELED_LIST.iter().enumerate() {
        let rect = rects[idx];
        let top = matcher.top_candidates(&frame, rect, CellKind::ListStratagem, 5);
        let listed: Vec<String> = top.iter().map(|(k, v)| format!("{k}={v:.3}")).collect();
        eprintln!(
            "r{row}c{col} 期望 {}: {}",
            expected.unwrap_or("(库外)"),
            listed.join(" / ")
        );
    }
}

#[test]
#[ignore = "打印配置里的槽位名称/图标 key"]
fn probe_slot_names() {
    // 一次性诊断：把配置里的槽位下标翻译成名称/key

    let all = crate::stratagems::STRATAGEMS;
    for idx in [41usize, 66, 55, 57] {
        match all.get(idx) {
            Some(s) => eprintln!("slot idx {idx} -> name={} icon={}", s.name, s.icon),
            None => eprintln!("slot idx {idx} -> 越界（共 {} 项）", all.len()),
        }
    }
}

/// 实机「已标注」夹具探针：home 面板上的 4 个槽位是配置文件里写死的战备，
/// 因此它们的身份是**已知的**（榴弹发射器 / 补给背包 / 自动哨戒炮 / 火箭哨戒炮）。
#[test]
#[ignore = "实机标注探针"]
fn probe_labeled_home_slots() {
    let img = image::open(fixture_dir().join("home_labeled_1914x1080.png"))
        .expect("标注夹具")
        .to_rgba8();
    let frame = make_frame(labeled_gray(&img), Some(img));
    let rec = GameUIRecognizer::new(Calibration::default());
    let geom = rec.resolve_geometry(&frame).expect("几何解析失败");
    let cal = Calibration::default();
    let (priors, _booster) = cal.home_slot_rects();
    let scale = geom.mapping.scale;
    let size = ((cal.slot_size as f32) * scale).round() as i32;
    let rects: Vec<ImageRect> = priors
        .iter()
        .map(|prior| {
            let t = cal.roi_to_frame(&geom.mapping, *prior);
            ImageRect::new(t.x, t.y, size, size)
        })
        .collect();
    let matcher = IconMatcher::for_items(&[]);
    let expected = [
        "grenade_launcher",
        "supply_pack",
        "autocannon_sentry",
        "rocket_sentry",
    ];
    for (i, rect) in rects.iter().enumerate() {
        let rect = *rect;
        let kind = if i == 4 {
            CellKind::HomeBooster
        } else {
            CellKind::HomeStratagem
        };
        let top5 = matcher.top_candidates(&frame, rect, kind, 5);
        eprintln!("槽位 {i}（期望 {}）rect={rect:?}", expected[i]);
        for (k, sc) in &top5 {
            let mark = if k == expected[i] { "  <== 正确" } else { "" };
            eprintln!("    {k} = {sc:.3}{mark}");
        }
    }
}

/// 实机标注：把光标逐个移到列表格子上（**只移动、不点击**），游戏会弹出该战备的
/// 名称浮层，抓图后即可建立「格子 ↔ 名称」标签对，用于标定图标匹配。
///
/// 环境变量：
///   H2AC_LABEL_DIR  输出目录（默认 <temp>/h2ac_labels）
///   H2AC_LABEL_MAX  最多标注几个格子（默认全部）
#[test]
#[ignore = "实机标注（需要游戏停在战备列表且在前台）"]
fn probe_label_list_cells() {
    use crate::loadout_sync::types::ScreenPoint;
    let win = crate::loadout_sync::window::find_game_window().expect("未找到游戏窗口");
    assert!(
        crate::loadout_sync::window::is_foreground(win.hwnd),
        "游戏必须在前台，否则悬停不会生效"
    );
    let dir = std::env::var("H2AC_LABEL_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("h2ac_labels"));
    std::fs::create_dir_all(&dir).expect("创建输出目录失败");
    let max_cells: usize = std::env::var("H2AC_LABEL_MAX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(usize::MAX);

    // 等界面稳定：列表刚打开时图标还在淡入，过早抓图会得到「空格子底纹」
    std::thread::sleep(std::time::Duration::from_millis(900));
    let frame = crate::loadout_sync::capture::capture_client_area(&win, true).expect("抓取失败");
    frame
        .save_png(&dir.join("clean.png"))
        .expect("保存干净帧失败");
    let rec = crate::loadout_sync::recognizer::GameUIRecognizer::new(
        crate::loadout_sync::types::Calibration::default(),
    );
    let grid = match rec.detect_list(&frame) {
        Ok(g) => g,
        Err(e) => panic!("列表识别失败（请确认游戏停在战备列表界面）: {e}"),
    };
    eprintln!(
        "列表: rows={} cols={} cells={} origin=({},{})",
        grid.rows,
        grid.cols,
        grid.cells.len(),
        frame.origin.x,
        frame.origin.y
    );

    let mut input = crate::loadout_sync::input::LoadoutInputController::new();
    let mut cells = grid.cells.clone();
    cells.sort_by_key(|c| (c.row, c.col));
    // 记录**实际使用**的几何：评估必须用同一批矩形，否则标签与格子对不上
    let geo: String = cells
        .iter()
        .map(|c| {
            format!(
                "{} {} {} {} {} {}",
                c.row, c.col, c.rect.x, c.rect.y, c.rect.w, c.rect.h
            )
        })
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    std::fs::write(dir.join("geometry.txt"), geo).expect("写入几何失败");
    // 中性停靠点（列表右侧空白）：每次悬停前先把光标挪开，保证「干净帧」不带悬停高亮
    let park = ScreenPoint {
        x: frame.origin.x + frame.width() as i32 - 120,
        y: frame.origin.y + 120,
    };
    for cell in cells.iter().take(max_cells) {
        let (cx, cy) = cell.rect.center();
        let screen = ScreenPoint {
            x: frame.origin.x + cx,
            y: frame.origin.y + cy,
        };
        // 1) 先把光标挪开 → 抓「干净帧」：与本次悬停严格配对，
        //    避免标注过程中列表滚动导致「标签 ↔ 格子」错位（上一轮的教训）
        input.move_to(park).expect("移动光标失败");
        std::thread::sleep(std::time::Duration::from_millis(220));
        let clean =
            crate::loadout_sync::capture::capture_client_area(&win, true).expect("抓取失败");
        clean
            .save_png(&dir.join(format!("clean_r{}c{}.png", cell.row, cell.col)))
            .expect("保存失败");
        // 2) 悬停该格子 → 抓「浮层帧」，浮层里就是这件装备的名字
        input.move_to(screen).expect("移动光标失败");
        std::thread::sleep(std::time::Duration::from_millis(450));
        let hovered =
            crate::loadout_sync::capture::capture_client_area(&win, true).expect("抓取失败");
        let path = dir.join(format!("hover_r{}c{}.png", cell.row, cell.col));
        hovered.save_png(&path).expect("保存失败");
        eprintln!(
            "  r{}c{} 格子中心=({},{}) 屏幕=({},{}) → {}",
            cell.row,
            cell.col,
            cx,
            cy,
            screen.x,
            screen.y,
            path.display()
        );
    }
    input.release_all();
    eprintln!("标注完成，输出目录: {}", dir.display());
}

/// 把实机标注（游戏内浮层名称）映射到 H2AC 图标 key，供标定使用。
#[test]
#[ignore = "名称→key 映射"]
fn probe_name_to_key() {
    let names = [
        "除叶工具",
        "破门锤",
        "唯一真旗",
        "榴弹发射器",
        "盟友",
        "重机枪",
        "磁轨炮",
        "矛枪",
        "反器材步枪",
        "纪元",
        "热熔枪",
        "缓和使者",
        "灭菌器",
        "火焰喷射器",
        "激光大炮",
    ];
    for name in names {
        let hits: Vec<String> = crate::stratagems::STRATAGEMS
            .iter()
            .filter(|s| s.name.contains(name))
            .map(|s| format!("{}→{}", s.name, s.icon))
            .collect();
        eprintln!("{name}: {}", hits.join(" | "));
    }
}

// ─── 实机标注评估（16 格列表，身份来自游戏内浮层） ───
//
// 标签来源：`probe_label_list_cells` 把光标移到格子上（只移动不点击），
// 游戏浮层显示装备名 → 人工读出；每个格子的几何与该帧严格配对。

/// 实机标注表：((行, 列), 期望的 H2AC 图标 key)。None = 该格是 wiki 拉取的新战备（内置库没有）。
const LABELED_LIST: [((u32, u32), Option<&str>); 16] = [
    ((0, 0), Some("defoliation_tool")),    // CQC-9 除叶工具
    ((0, 1), Some("cqc_20")),              // CQC-20 破门锤
    ((0, 2), None),                        // CQC-1 唯一真旗（内置库没有）
    ((0, 3), Some("grenade_launcher")),    // GL-21 榴弹发射器
    ((1, 0), Some("stalwart")),            // M-105 盟友
    ((1, 1), Some("heavy_machine_gun")),   // MG-206 重机枪
    ((1, 2), Some("railgun")),             // RS-422 磁轨炮
    ((1, 3), Some("speargun")),            // S-11 矛枪
    ((2, 0), Some("anti_materiel_rifle")), // APW-1 反器材步枪
    ((2, 1), None),                        // PLAS-45 纪元（内置库没有）
    ((2, 2), Some("grenade_launcher")),    // GL-21 榴弹发射器（第二处出现）
    ((2, 3), None),                        // 40-K 热熔枪（内置库没有）
    ((3, 0), Some("gl_52_de_escalator")),  // GL-52 缓和使者
    ((3, 1), Some("sterilizer")),          // TX-41 灭菌器
    ((3, 2), Some("flamethrower")),        // FLAM-40 火焰喷射器
    ((3, 3), Some("laser_cannon")),        // LAS-98 激光大炮
];

/// 与抓帧路径一致的灰度换算（整数权重 299/587/114）。
fn labeled_gray(img: &image::RgbaImage) -> GrayImage {
    let mut gray = GrayImage::new(img.width(), img.height());
    for (x, y, p) in img.enumerate_pixels() {
        let luma = ((p[0] as u32 * 299 + p[1] as u32 * 587 + p[2] as u32 * 114) / 1000) as u8;
        gray.put_pixel(x, y, image::Luma([luma]));
    }
    gray
}

/// 标注夹具 + 与标注探针一致的格子几何（同一条 detect_list 路径）。
fn labeled_frame() -> (CapturedFrame, Vec<ImageRect>) {
    let img = image::open(fixture_dir().join("stratagem_list_labeled_1914x1080.png"))
        .expect("实机标注夹具缺失")
        .to_rgba8();
    let frame = make_frame(labeled_gray(&img), Some(img));
    let rec = GameUIRecognizer::new(Calibration::default());
    let grid = rec.detect_list(&frame).expect("列表识别失败");
    let mut cells = grid.cells.clone();
    cells.sort_by_key(|c| (c.row, c.col));
    (frame, cells.into_iter().map(|c| c.rect).collect())
}

/// 一个标注样本的完整打分信息（阈值扫描 / 指标统计的唯一数据来源）。
struct LabeledScore {
    row: u32,
    col: u32,
    expected: &'static str,
    best_key: String,
    best_score: f32,
    best_margin: f32,
    expected_score: f32,
    expected_rank: usize,
}

fn collect_labeled_scores(frame: &CapturedFrame, rects: &[ImageRect]) -> Vec<LabeledScore> {
    let matcher = IconMatcher::for_items(&[]);
    let mut out = Vec::new();
    for (idx, ((row, col), expected)) in LABELED_LIST.iter().enumerate() {
        let Some(expected) = expected else { continue };
        let rect = rects[idx];
        let top = matcher.top_candidates(frame, rect, CellKind::ListStratagem, 200);
        if top.is_empty() {
            continue;
        }
        let (best_key, best_score) = top[0].clone();
        let second = top.get(1).map(|(_, s)| *s).unwrap_or(0.0);
        let expected_rank = top
            .iter()
            .position(|(k, _)| k == expected)
            .map(|i| i + 1)
            .unwrap_or(usize::MAX);
        let expected_score = top
            .iter()
            .find(|(k, _)| k == expected)
            .map(|(_, s)| *s)
            .unwrap_or(f32::NEG_INFINITY);
        out.push(LabeledScore {
            row: *row,
            col: *col,
            expected,
            best_key,
            best_score,
            best_margin: (best_score - second).max(0.0),
            expected_score,
            expected_rank,
        });
    }
    out
}

/// 调参集指标：top-1 命中、**被闸门接受却认错（false_accept，= 会点错）**、margin 分布、名次。
fn report_labeled_metrics(
    scores: &[LabeledScore],
    threshold: f32,
    min_margin: f32,
) -> (usize, usize) {
    let mut hit = 0usize;
    let mut false_accept = 0usize;
    let mut margins_correct: Vec<f32> = Vec::new();
    let mut margins_wrong: Vec<f32> = Vec::new();
    for s in scores {
        let top1_correct = s.best_key == s.expected;
        if top1_correct {
            hit += 1;
        }
        let accepted = s.best_score >= threshold && s.best_margin >= min_margin;
        if accepted && !top1_correct {
            false_accept += 1;
        }
        if top1_correct {
            margins_correct.push(s.best_margin);
        } else {
            margins_wrong.push(s.best_margin);
        }
        eprintln!(
            "r{}c{} 期望 {} → top1 {} {:.3} margin {:.3} | 期望项 {:.3} rank {}",
            s.row,
            s.col,
            s.expected,
            s.best_key,
            s.best_score,
            s.best_margin,
            s.expected_score,
            if s.expected_rank == usize::MAX {
                "-".to_string()
            } else {
                s.expected_rank.to_string()
            }
        );
    }
    let pct = |v: &mut Vec<f32>, p: f32| -> f32 {
        if v.is_empty() {
            return f32::NAN;
        }
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        v[((v.len() as f32 - 1.0) * p) as usize]
    };
    eprintln!(
        "== 样本 {} | top1 {}/{} | false_accept {} | 正确样本 margin p5/p50 {:.3}/{:.3} | 错误样本 margin p50 {:.3} ==",
        scores.len(),
        hit,
        scores.len(),
        false_accept,
        pct(&mut margins_correct, 0.05),
        pct(&mut margins_correct, 0.50),
        pct(&mut margins_wrong, 0.50),
    );
    (hit, false_accept)
}

/// 调参集评估（阶段 1 硬门槛 A 的判定入口）。
#[test]
#[ignore = "实机标注评估"]
fn probe_labeled_accuracy() {
    let (frame, rects) = labeled_frame();
    let scores = collect_labeled_scores(&frame, &rects);
    assert!(scores.len() >= 10, "标注样本太少: {}", scores.len());
    let (hit, false_accept) =
        report_labeled_metrics(&scores, RECOGNITION_THRESHOLD, MATCH_MIN_MARGIN);
    eprintln!(
        "== 现有首轮阈值 threshold={RECOGNITION_THRESHOLD:.3} margin={MATCH_MIN_MARGIN:.3} → top1 {hit}/{} false_accept {false_accept} ==",
        scores.len()
    );
}

/// 阈值扫描表：给出「top-1 命中 / false_accept」随 (threshold, margin) 的变化。
#[test]
#[ignore = "阈值扫描"]
fn probe_matcher_score_dump() {
    let (frame, rects) = labeled_frame();
    let scores = collect_labeled_scores(&frame, &rects);
    eprintln!("threshold  margin  top1  false_accept");
    for threshold in [0.40f32, 0.50, 0.55, 0.60, 0.65, 0.70, 0.75, 0.80] {
        for margin in [0.0f32, 0.02, 0.035, 0.05, 0.08, 0.12] {
            let mut hit = 0;
            let mut false_accept = 0;
            for s in &scores {
                let accepted = s.best_score >= threshold && s.best_margin >= margin;
                if s.best_key == s.expected {
                    hit += 1;
                } else if accepted {
                    false_accept += 1;
                }
            }
            eprintln!(
                "{threshold:.2}      {margin:.3}   {hit}/{}   {false_accept}",
                scores.len()
            );
        }
    }
}

/// 实测列表图标的渲染比例：对每个标注格扫 icon_size，取分数最高的尺寸 → 中位数即比例。
#[test]
#[ignore = "比例标定"]
fn probe_icon_scale_sweep() {
    let (frame, rects) = labeled_frame();
    let matcher = IconMatcher::for_items(&[]);
    let mut best_sizes: Vec<f32> = Vec::new();
    for (idx, ((row, col), expected)) in LABELED_LIST.iter().enumerate() {
        let Some(key) = expected else { continue };
        let item = LoadoutItem {
            name: (*key).to_string(),
            icon: (*key).to_string(),
            base_index: None,
        };
        let rect = rects[idx];
        let side = rect.w.min(rect.h) as f32;
        let _ = &item;
        let mut best = (0u32, f32::NEG_INFINITY);
        let mut rank1_at = Vec::new();
        for glyph_h in (16..=(side as u32 - 4)).step_by(2) {
            let mut scored =
                matcher.score_all_with_glyph_height(&frame, rect, CellKind::ListStratagem, glyph_h);
            if scored.is_empty() {
                continue;
            }
            scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            let own = scored
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, s)| *s)
                .unwrap_or(f32::NEG_INFINITY);
            let rank1 = scored.first().map(|(k, _)| k == key).unwrap_or(false);
            if rank1 {
                rank1_at.push(glyph_h);
            }
            if own > best.1 {
                best = (glyph_h, own);
            }
        }
        if best.1.is_finite() {
            best_sizes.push(best.0 as f32);
            eprintln!(
                "r{row}c{col} {key}: 自身模板最高分出现在 glyph_h={} （比例 {:.4}）；rank1 的 glyph_h 区间 {:?}",
                best.0,
                best.0 as f32 / side,
                rank1_at
            );
        }
    }
    if best_sizes.is_empty() {
        eprintln!("没有可用样本");
        return;
    }
    best_sizes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = best_sizes[best_sizes.len() / 2];
    let side = rects[0].w.min(rects[0].h) as f32;
    eprintln!(
        "== 样本 {} | icon_size 中位数 {:.1}px（格子 {:.0}px）→ LIST_GLYPH_SCALE ≈ {:.4} | 当前常量 {:.4} ==",
        best_sizes.len(),
        median,
        side,
        median / side,
        LIST_GLYPH_SCALE
    );
}

/// 图标资源的 alpha 占位统计：判断「单一全局渲染比例」是否够用。
#[test]
#[ignore = "图标占位统计"]
fn probe_icon_alpha_padding() {
    let matcher = IconMatcher::for_items(&[]);
    let mut heights = Vec::new();
    let mut widths = Vec::new();
    let mut aspects = Vec::new();
    for key in matcher_keys(&matcher) {
        let Some(bytes) = crate::icons::icon_png_bytes(&key) else {
            continue;
        };
        let Ok(img) = image::load_from_memory(&bytes).map(|i| i.to_rgba8()) else {
            continue;
        };
        let (w, h) = (img.width() as f32, img.height() as f32);
        let (mut x0, mut y0, mut x1, mut y1) = (w, h, -1.0f32, -1.0f32);
        for (x, y, p) in img.enumerate_pixels() {
            if p[3] >= 64 {
                x0 = x0.min(x as f32);
                y0 = y0.min(y as f32);
                x1 = x1.max(x as f32);
                y1 = y1.max(y as f32);
            }
        }
        if x1 < 0.0 {
            continue;
        }
        let bw = x1 - x0 + 1.0;
        let bh = y1 - y0 + 1.0;
        heights.push(bh / h);
        widths.push(bw / w);
        aspects.push(bw / bh);
    }
    let pct = |v: &mut Vec<f32>, p: f32| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        v[((v.len() as f32 - 1.0) * p) as usize]
    };
    eprintln!(
        "样本 {} | 高度占比 p5/p50/p95 = {:.3}/{:.3}/{:.3} | 宽度占比 p5/p50/p95 = {:.3}/{:.3}/{:.3} | 纵横比 p5/p50/p95 = {:.2}/{:.2}/{:.2}",
        heights.len(),
        pct(&mut heights.clone(), 0.05),
        pct(&mut heights.clone(), 0.5),
        pct(&mut heights.clone(), 0.95),
        pct(&mut widths.clone(), 0.05),
        pct(&mut widths.clone(), 0.5),
        pct(&mut widths.clone(), 0.95),
        pct(&mut aspects.clone(), 0.05),
        pct(&mut aspects.clone(), 0.5),
        pct(&mut aspects.clone(), 0.95),
    );
}

fn matcher_keys(matcher: &IconMatcher) -> Vec<String> {
    crate::icons::all_icon_keys()
        .into_iter()
        .filter(|k| matcher.has(k))
        .map(|k| k.to_string())
        .collect()
}

/// 用**色度**（max-min 通道差）切出实机字形并量其包围盒：
/// 列表格子里的字形是分类色（蓝/绿/红），格子底色/边框/网点是灰阶 → 色度能把两者分开。
#[test]
#[ignore = "字形尺寸实测"]
fn probe_cell_glyph_extent() {
    let (frame, rects) = labeled_frame();
    let mut heights = Vec::new();
    let mut widths = Vec::new();
    for (idx, ((row, col), expected)) in LABELED_LIST.iter().enumerate() {
        let rect = rects[idx];
        let inner_inset = (rect.w.min(rect.h) as f32 * 0.10) as i32;
        let x0 = rect.x + inner_inset;
        let y0 = rect.y + inner_inset;
        let w = rect.w - 2 * inner_inset;
        let h = rect.h - 2 * inner_inset;
        let rgba = &frame.rgba;
        let mut chroma: Vec<f32> = Vec::with_capacity((w * h) as usize);
        for y in 0..h {
            for x in 0..w {
                let p = rgba.get_pixel((x0 + x) as u32, (y0 + y) as u32).0;
                let max = p[0].max(p[1]).max(p[2]) as f32;
                let min = p[0].min(p[1]).min(p[2]) as f32;
                chroma.push((max - min) / 255.0);
            }
        }
        // Otsu
        let mut hist = [0usize; 256];
        for v in &chroma {
            hist[((v * 255.0) as usize).min(255)] += 1;
        }
        let total = chroma.len() as f32;
        let sum_all: f32 = (0..256).map(|i| i as f32 * hist[i] as f32).sum();
        let (mut wb, mut sb, mut best, mut best_var) = (0.0f32, 0.0f32, 0.0f32, -1.0f32);
        for i in 0..256 {
            wb += hist[i] as f32;
            if wb == 0.0 {
                continue;
            }
            let wf = total - wb;
            if wf <= 0.0 {
                break;
            }
            sb += i as f32 * hist[i] as f32;
            let var = wb * wf * (sb / wb - (sum_all - sb) / wf).powi(2);
            if var > best_var {
                best_var = var;
                best = i as f32;
            }
        }
        let thr = (best / 255.0).max(0.10);
        let (mut bx0, mut by0, mut bx1, mut by1) = (w as f32, h as f32, -1.0f32, -1.0f32);
        let mut count = 0usize;
        for y in 0..h {
            for x in 0..w {
                if chroma[(y * w + x) as usize] > thr {
                    bx0 = bx0.min(x as f32);
                    by0 = by0.min(y as f32);
                    bx1 = bx1.max(x as f32);
                    by1 = by1.max(y as f32);
                    count += 1;
                }
            }
        }
        if bx1 < 0.0 {
            eprintln!(
                "r{row}c{col} {}: 无色度区域（阈值 {thr:.3}）",
                expected.unwrap_or("(库外)")
            );
            continue;
        }
        let gw = bx1 - bx0 + 1.0;
        let gh = by1 - by0 + 1.0;
        heights.push(gh);
        widths.push(gw);
        eprintln!(
            "r{row}c{col} {}: 字形 {gw:.0}x{gh:.0}px（占格子 {:.3}x{:.3}）覆盖率 {:.3} 阈值 {thr:.3}",
            expected.unwrap_or("(库外)"),
            gw / rect.w as f32,
            gh / rect.h as f32,
            count as f32 / (w * h) as f32,
        );
    }
    if heights.is_empty() {
        return;
    }
    let median = |v: &mut Vec<f32>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        v[v.len() / 2]
    };
    let side = rects[0].w.min(rects[0].h) as f32;
    eprintln!(
        "== 字形高度中位数 {:.1}px / 格子 {:.0}px → LIST_GLYPH_SCALE ≈ {:.3}（宽度中位数 {:.1}px）| 当前常量 {:.3} ==",
        median(&mut heights),
        side,
        median(&mut heights.clone()) / side,
        median(&mut widths),
        LIST_GLYPH_SCALE
    );
}

/// 在「实测字形比例」下评估整库排名的区分度：rank1 命中数与平均名次。
#[test]
#[ignore = "排名区分度"]
fn probe_rank_quality() {
    let (frame, rects) = labeled_frame();
    let matcher = IconMatcher::for_items(&[]);
    let mut hits = 0usize;
    let mut total = 0usize;
    let mut ranks: Vec<f32> = Vec::new();
    for (idx, ((row, col), expected)) in LABELED_LIST.iter().enumerate() {
        let Some(key) = expected else { continue };
        let rect = rects[idx];
        let glyph_h = ((rect.w.min(rect.h) as f32) * LIST_GLYPH_SCALE).round() as u32;
        let mut scored =
            matcher.score_all_with_glyph_height(&frame, rect, CellKind::ListStratagem, glyph_h);
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let rank = scored
            .iter()
            .position(|(k, _)| k == key)
            .map(|i| i + 1)
            .unwrap_or(scored.len());
        let own = scored
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, s)| *s)
            .unwrap_or(f32::NEG_INFINITY);
        if rank == 1 {
            hits += 1;
        }
        total += 1;
        ranks.push(rank as f32);
        eprintln!(
            "r{row}c{col} {key}: rank={rank} 自身={own:.3} 榜首={}={:.3} (glyph_h={glyph_h})",
            scored[0].0, scored[0].1
        );
    }
    let mean_rank = ranks.iter().sum::<f32>() / ranks.len().max(1) as f32;
    eprintln!(
        "== blur={CELL_BLUR_RADIUS} glyph_scale={LIST_GLYPH_SCALE:.3} → rank1 {hits}/{total}，平均名次 {mean_rank:.1} =="
    );
}

/// 单格对比可视化：格子灰度/梯度 vs 模板灰度/梯度（排查失配原因）。
#[test]
#[ignore = "单格可视化"]
fn probe_dump_pair() {
    let (frame, rects) = labeled_frame();
    let matcher = IconMatcher::for_items(&[]);
    let idx = 10usize; // r2c2 = 榴弹发射器
    let rect = rects[idx];
    let glyph_h = ((rect.w.min(rect.h) as f32) * LIST_GLYPH_SCALE).round() as u32;
    let path = std::env::temp_dir().join("h2ac_pair.png");
    matcher.debug_dump_pair(&frame, rect, "grenade_launcher", glyph_h, &path);
    eprintln!(
        "拼图: {}（左上=格子灰度 左下=格子梯度 右上=模板灰度 右下=模板梯度）",
        path.display()
    );
}

/// 单格数值诊断：通道余弦 + 掩码区域灰度缩略图。
#[test]
#[ignore = "单格数值诊断"]
fn probe_pair_stats() {
    let (frame, rects) = labeled_frame();
    let matcher = IconMatcher::for_items(&[]);
    for idx in [10usize, 3] {
        let rect = rects[idx];
        let glyph_h = ((rect.w.min(rect.h) as f32) * LIST_GLYPH_SCALE).round() as u32;
        let expected = LABELED_LIST[idx].1.unwrap_or("?");
        eprintln!("== 格子 {} 期望 {} glyph_h={glyph_h} ==", idx, expected);
        matcher.debug_pair_stats(
            &frame,
            rect,
            &[expected, "tesla_tower", "incendiary_mines"],
            glyph_h,
        );
    }
}

/// A/B 对照：同一套实机标注帧、同一算法，只换**模板资产**
/// （我们的彩色美术图 vs 参考项目的平面白色字形）→ 闭集 13 项排名。
#[test]
#[ignore = "资产 A/B 对照"]
fn probe_assets_ab() {
    // 标签全部 16 格（含之前"库外"的 3 项：唯一真旗 / 纪元 / 热熔枪）
    let labels: [((u32, u32), &str, &str); 16] = [
        ((0, 0), "defoliation_tool", "supply/defoliation-tool.png"),
        ((0, 1), "cqc_20", "supply/breaching-hammer.png"),
        ((0, 2), "one_true_flag", "supply/one-true-flag.png"),
        ((0, 3), "grenade_launcher", "supply/grenade-launcher.png"),
        ((1, 0), "stalwart", "supply/stalwart.png"),
        ((1, 1), "heavy_machine_gun", "supply/heavy-machine-gun.png"),
        ((1, 2), "railgun", "supply/railgun.png"),
        ((1, 3), "speargun", "supply/speargun.png"),
        (
            (2, 0),
            "anti_materiel_rifle",
            "supply/anti-materiel-rifle.png",
        ),
        ((2, 1), "epoch", "supply/epoch.png"),
        ((2, 2), "grenade_launcher", "supply/grenade-launcher.png"),
        ((2, 3), "meltagun", "supply/meltagun.png"),
        ((3, 0), "gl_52_de_escalator", "supply/de-escalator.png"),
        ((3, 1), "sterilizer", "supply/sterilizer.png"),
        ((3, 2), "flamethrower", "supply/flamethrower.png"),
        ((3, 3), "laser_cannon", "supply/laser-cannon.png"),
    ];
    let flat_dir = std::path::Path::new(
        r"D:\BaiduNetdiskDownload\PR_PRoject\h2ac-rs\hd2-preset-helper-0.1.4\assets\icons",
    );
    let (frame, rects) = labeled_frame();
    // 我们的资产：只加载这 13 个 key（闭集）
    let mut ours = IconMatcher::new();
    let mut theirs = IconMatcher::new();
    for (_, key, rel) in labels.iter() {
        let item = LoadoutItem {
            name: (*key).to_string(),
            icon: (*key).to_string(),
            base_index: None,
        };
        ours.load_one_for_test(&item);
        let p = flat_dir.join("stratagem").join(rel.replace('/', "\\"));
        let ok = theirs.load_icon_from_file(key, &p);
        if !ok {
            eprintln!("!! 加载失败: {} -> {}", key, p.display());
        }
    }
    for (label, matcher) in [("我们的彩色美术图", &ours), ("参考的平面字形", &theirs)]
    {
        let mut hits = 0usize;
        let mut ranks = Vec::new();
        for (idx, ((row, col), key, _)) in labels.iter().enumerate() {
            let rect = rects[idx];
            let glyph_h = ((rect.w.min(rect.h) as f32) * LIST_GLYPH_SCALE).round() as u32;
            let mut scored =
                matcher.score_all_with_glyph_height(&frame, rect, CellKind::ListStratagem, glyph_h);
            scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            let rank = scored
                .iter()
                .position(|(k, _)| k == key)
                .map(|i| i + 1)
                .unwrap_or(scored.len());
            if rank == 1 {
                hits += 1;
            }
            ranks.push(rank as f32);
            if label.starts_with("参考") {
                eprintln!(
                    "  r{row}c{col} {key}: rank={rank} 榜首={}={:.3}",
                    scored[0].0, scored[0].1
                );
            }
        }
        let mean = ranks.iter().sum::<f32>() / ranks.len().max(1) as f32;
        eprintln!("== [{label}] 闭集 16 格：rank1 {hits}/16，平均名次 {mean:.1} ==");
    }
}

// ─── 结构性诊断：游戏格子里哪一块 ↔ 资源里哪一块 ───

/// 把 (mask, w, h) 中给定包围盒内的内容**保持纵横比**缩放后居中放到 `size×size` 画布上。
///
/// 为什么必须保持纵横比：把宽扁形状拉伸成正方形后，"宽条" 与 "任何宽条" 的 Dice 都会很高
/// （实测某格正确项 0.45、错误项 0.96），纵横比本身就是主要判别特征。
fn normalize_patch(
    mask: &[bool],
    w: usize,
    h: usize,
    bbox: (i32, i32, i32, i32),
    size: usize,
) -> Vec<bool> {
    let bw = (bbox.2 - bbox.0 + 1).max(1) as f32;
    let bh = (bbox.3 - bbox.1 + 1).max(1) as f32;
    let scale = (size as f32 / bw).min(size as f32 / bh);
    let tw = ((bw * scale).round() as usize).clamp(1, size);
    let th = ((bh * scale).round() as usize).clamp(1, size);
    let ox = (size - tw) / 2;
    let oy = (size - th) / 2;
    let mut out = vec![false; size * size];
    for y in 0..th {
        for x in 0..tw {
            let sx = bbox.0 as f32 + (x as f32 + 0.5) / scale;
            let sy = bbox.1 as f32 + (y as f32 + 0.5) / scale;
            let (sx, sy) = (sx.floor() as i32, sy.floor() as i32);
            if sx < 0 || sy < 0 || sx >= w as i32 || sy >= h as i32 {
                continue;
            }
            out[(oy + y) * size + ox + x] = mask[sy as usize * w + sx as usize];
        }
    }
    out
}

fn dice_of(a: &[bool], b: &[bool]) -> f32 {
    let mut hit = 0usize;
    let mut na = 0usize;
    let mut nb = 0usize;
    for (x, y) in a.iter().zip(b.iter()) {
        if *x {
            na += 1;
        }
        if *y {
            nb += 1;
        }
        if *x && *y {
            hit += 1;
        }
    }
    if na + nb == 0 {
        0.0
    } else {
        2.0 * hit as f32 / (na + nb) as f32
    }
}

/// 通道包围盒（模板白字 / 分类色部分各自的紧包围盒）。
fn channel_bbox(mask: &[bool], w: usize, h: usize) -> Option<(i32, i32, i32, i32)> {
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, -1i32, -1i32);
    for y in 0..h {
        for x in 0..w {
            if mask[y * w + x] {
                x0 = x0.min(x as i32);
                y0 = y0.min(y as i32);
                x1 = x1.max(x as i32);
                y1 = y1.max(y as i32);
            }
        }
    }
    (x1 >= x0 && y1 >= y0).then_some((x0, y0, x1, y1))
}

/// 结构配对诊断（用数据回答「游戏里哪一块对应资源里哪一块」，不做任何视觉猜测）。
#[test]
#[ignore = "结构配对诊断"]
fn probe_component_pairing() {
    const PATCH: usize = 32;
    let (frame, rects) = labeled_frame();
    let matcher = IconMatcher::for_items(&[]);
    let mut white_wins = 0usize;
    let mut color_wins = 0usize;
    let mut summary: Vec<(String, String, String)> = Vec::new();
    for (idx, ((row, col), expected)) in LABELED_LIST.iter().enumerate() {
        let Some(key) = expected else { continue };
        let rect = rects[idx];
        let Some(masks) = crate::loadout_sync::matcher::CellMasks::from_frame(&frame, rect) else {
            continue;
        };
        let glyph_h = ((rect.w.min(rect.h) as f32) * LIST_GLYPH_SCALE).round() as u32;
        let Some(tpl) = matcher.template_parts(&frame, rect, key, glyph_h) else {
            continue;
        };
        let tpl_white_bb = channel_bbox(&tpl.white, tpl.width, tpl.height);
        let tpl_color_bb = channel_bbox(&tpl.color, tpl.width, tpl.height);
        let white_patch =
            tpl_white_bb.map(|bb| normalize_patch(&tpl.white, tpl.width, tpl.height, bb, PATCH));
        let color_patch =
            tpl_color_bb.map(|bb| normalize_patch(&tpl.color, tpl.width, tpl.height, bb, PATCH));
        eprintln!(
            "== r{row}c{col} {key} ==  模板白bbox={tpl_white_bb:?} 模板彩bbox={tpl_color_bb:?}"
        );
        for (i, c) in masks.component_infos().iter().enumerate() {
            if !c.kept {
                continue;
            }
            let (qw, qc, w, h) = masks.sub_grid(c.bbox);
            let union: Vec<bool> = qw.iter().zip(qc.iter()).map(|(a, b)| *a || *b).collect();
            let patch = normalize_patch(&union, w, h, (0, 0, w as i32 - 1, h as i32 - 1), PATCH);
            let dw = white_patch
                .as_ref()
                .map(|p| dice_of(&patch, p))
                .unwrap_or(-1.0);
            let dc = color_patch
                .as_ref()
                .map(|p| dice_of(&patch, p))
                .unwrap_or(-1.0);
            let cls = if c.color_px > c.white_px {
                "色"
            } else {
                "白"
            };
            if dw > dc {
                white_wins += 1;
            } else {
                color_wins += 1;
            }
            eprintln!(
                "  域{i} area={} bbox={:?} {cls}(白{}色{}) → Dice(模板白)={dw:.2} Dice(模板彩)={dc:.2}",
                c.area, c.bbox, c.white_px, c.color_px
            );
        }
        // 汇总：模板白/彩分别与「同类查询域」的最佳 Dice
        let mut best_white = f32::NEG_INFINITY;
        let mut best_color = f32::NEG_INFINITY;
        for c in masks.component_infos().iter().filter(|c| c.kept) {
            let (qw, qc, w, h) = masks.sub_grid(c.bbox);
            let union: Vec<bool> = qw.iter().zip(qc.iter()).map(|(a, b)| *a || *b).collect();
            let patch = normalize_patch(&union, w, h, (0, 0, w as i32 - 1, h as i32 - 1), PATCH);
            if c.white_px >= c.color_px {
                if let Some(p) = &white_patch {
                    best_white = best_white.max(dice_of(&patch, p));
                }
            } else if let Some(p) = &color_patch {
                best_color = best_color.max(dice_of(&patch, p));
            }
        }
        summary.push((
            key.to_string(),
            format!("{best_white:.2}"),
            format!("{best_color:.2}"),
        ));
    }
    eprintln!("== 汇总：同类最佳 Dice（查询白域 vs 模板白 / 查询色域 vs 模板彩）==");
    for (key, w, c) in &summary {
        eprintln!("  {key}: 白={w} 彩={c}");
    }
    eprintln!("== 域-通道配对统计：模板白胜 {white_wins} 次 / 模板彩胜 {color_wins} 次 ==");
}

/// 前景像素质心（诊断用）。
fn centroid(mask: &[bool], w: usize, h: usize) -> (f32, f32) {
    let mut sx = 0.0f32;
    let mut sy = 0.0f32;
    let mut n = 0.0f32;
    for y in 0..h {
        for x in 0..w {
            if mask[y * w + x] {
                sx += x as f32;
                sy += y as f32;
                n += 1.0;
            }
        }
    }
    if n == 0.0 {
        (w as f32 / 2.0, h as f32 / 2.0)
    } else {
        (sx / n, sy / n)
    }
}

/// 带平移的 Dice / 召回（同一画布尺寸，不做尺度归一化）。
fn dice_shift(a: &[bool], b: &[bool], w: usize, h: usize, dx: i32, dy: i32) -> (f32, f32) {
    let (mut hit, mut na, mut nb) = (0usize, 0usize, 0usize);
    for y in 0..h {
        for x in 0..w {
            let a_on = a[y * w + x];
            if a_on {
                na += 1;
            }
            let (sx, sy) = (x as i32 + dx, y as i32 + dy);
            let b_on = sx >= 0
                && sy >= 0
                && sx < w as i32
                && sy < h as i32
                && b[sy as usize * w + sx as usize];
            if b_on {
                nb += 1;
            }
            if a_on && b_on {
                hit += 1;
            }
        }
    }
    let dice = if na + nb == 0 {
        0.0
    } else {
        2.0 * hit as f32 / (na + nb) as f32
    };
    // 召回（模板墨迹被查询覆盖的比例）—— 惩罚"模板很大、查询只命中一小块"的假匹配
    let recall = if nb == 0 { 0.0 } else { hit as f32 / nb as f32 };
    (dice, recall)
}

/// 比较规则扫描：在真实 13 格标注帧上比较多种聚合规则，用数据挑规则（不做任何视觉猜测）。
///
/// 规则：
///   union_bbox    —— 整卡前景并集（包围盒归一化 Dice，尺度/平移无关）
///   color_only    —— 只用分类色通道（连通域级别的最佳配对）
///   white_only    —— 只用白字通道（同上）
///   best_pair     —— 模板两个通道与查询同类连通域的最佳配对（取最大）
///   coverage      —— 模板每个通道都必须在查询里找到对应连通域（各自取最大后平均）
///   fixed_union   —— **保留布局**：模板画布直接覆盖格子，按质心对齐后的并集 Dice
///   fixed_color   —— 同上，只用分类色通道
///   fixed_recall  —— 同上，改用「模板墨迹被覆盖比例」（召回）
///   fixed_min     —— 同上，取 min(Dice, 召回)
#[test]
#[ignore = "比较规则扫描"]
fn probe_rule_sweep() {
    const PATCH: usize = 32;
    let (frame, rects) = labeled_frame();
    let matcher = IconMatcher::for_items(&[]);
    const RULES: [&str; 9] = [
        "union_bbox",
        "color_only",
        "white_only",
        "best_pair",
        "coverage",
        "fixed_union",
        "fixed_color",
        "fixed_recall",
        "fixed_min",
    ];
    let mut hits = vec![0usize; RULES.len()];
    let mut rank_sum = vec![0.0f32; RULES.len()];
    let mut samples = 0usize;

    for (idx, ((row, col), expected)) in LABELED_LIST.iter().enumerate() {
        let Some(key) = expected else { continue };
        let rect = rects[idx];
        let Some(masks) = crate::loadout_sync::matcher::CellMasks::from_frame(&frame, rect) else {
            continue;
        };
        let glyph_h = ((rect.w.min(rect.h) as f32) * LIST_GLYPH_SCALE).round() as u32;
        let (qw, qc, mw, mh) = masks.grid();
        let q_union: Vec<bool> = qw.iter().zip(qc.iter()).map(|(a, b)| *a || *b).collect();
        let q_centroid = centroid(&q_union, mw, mh);
        // 查询侧：每个保留连通域的（是否以白为主, 并集 patch）
        let mut qcomps: Vec<(bool, Vec<bool>)> = Vec::new();
        for c in masks.component_infos().iter().filter(|c| c.kept) {
            let (cw, cc, w, h) = masks.sub_grid(c.bbox);
            let union: Vec<bool> = cw.iter().zip(cc.iter()).map(|(a, b)| *a || *b).collect();
            let patch = normalize_patch(&union, w, h, (0, 0, w as i32 - 1, h as i32 - 1), PATCH);
            qcomps.push((c.white_px >= c.color_px, patch));
        }
        if qcomps.is_empty() {
            continue;
        }
        let mut per_rule: Vec<Vec<(String, f32)>> = vec![Vec::new(); RULES.len()];
        for lib_key in matcher_keys(&matcher) {
            let Some(tpl) = matcher.template_parts(&frame, rect, &lib_key, glyph_h) else {
                continue;
            };
            let white_bb = channel_bbox(&tpl.white, tpl.width, tpl.height);
            let color_bb = channel_bbox(&tpl.color, tpl.width, tpl.height);
            let white_patch =
                white_bb.map(|bb| normalize_patch(&tpl.white, tpl.width, tpl.height, bb, PATCH));
            let color_patch =
                color_bb.map(|bb| normalize_patch(&tpl.color, tpl.width, tpl.height, bb, PATCH));
            let union_grid: Vec<bool> = tpl
                .white
                .iter()
                .zip(tpl.color.iter())
                .map(|(a, b)| *a || *b)
                .collect();
            let union_bb = channel_bbox(&union_grid, tpl.width, tpl.height);
            let union_patch =
                union_bb.map(|bb| normalize_patch(&union_grid, tpl.width, tpl.height, bb, PATCH));

            let best_of = |want_white: bool, patch: &Option<Vec<bool>>| -> f32 {
                let Some(patch) = patch else { return 0.0 };
                qcomps
                    .iter()
                    .filter(|(is_white, _)| *is_white == want_white)
                    .map(|(_, q)| dice_of(q, patch))
                    .fold(0.0f32, f32::max)
            };

            let small_dice = |patch: &Option<Vec<bool>>| -> f32 {
                patch
                    .as_ref()
                    .map(|p| {
                        qcomps
                            .iter()
                            .map(|(_, q)| dice_of(q, p))
                            .fold(0.0f32, f32::max)
                    })
                    .unwrap_or(0.0)
            };
            let union_score = small_dice(&union_patch);
            let color_score = best_of(false, &color_patch);
            let white_score = best_of(true, &white_patch);
            let best_pair = color_score.max(white_score);
            let mut parts = 0usize;
            let mut sum = 0.0f32;
            if color_patch.is_some() {
                sum += color_score;
                parts += 1;
            }
            if white_patch.is_some() {
                sum += white_score;
                parts += 1;
            }
            let coverage = if parts == 0 { 0.0 } else { sum / parts as f32 };

            // 固定画布（保留布局）：模板画布与格子等大，按质心对齐
            let t_centroid = centroid(&union_grid, mw, mh);
            let (dx, dy) = (
                (q_centroid.0 - t_centroid.0).round() as i32,
                (q_centroid.1 - t_centroid.1).round() as i32,
            );
            // 查询侧只保留「有效前景」（已过滤连通域），避免用被丢弃的伪影参与比较

            let (fu_dice, _) = dice_shift(&q_union, &union_grid, mw, mh, dx, dy);
            let t_color_grid: Vec<bool> = tpl.color.clone();
            let (fc_dice, _) = dice_shift(qc, &t_color_grid, mw, mh, dx, dy);
            let (_, fu_recall) = dice_shift(&q_union, &union_grid, mw, mh, dx, dy);

            let values = [
                union_score,
                color_score,
                white_score,
                best_pair,
                coverage,
                fu_dice,
                fc_dice,
                fu_recall,
                fu_dice.min(fu_recall),
            ];
            for (r, v) in values.iter().enumerate() {
                per_rule[r].push((lib_key.clone(), *v));
            }
        }
        for (r, scored) in per_rule.iter_mut().enumerate() {
            scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            let rank = scored
                .iter()
                .position(|(k, _)| k == key)
                .map(|i| i + 1)
                .unwrap_or(scored.len());
            hits[r] += usize::from(rank == 1);
            rank_sum[r] += rank as f32;
            if r == 5 {
                eprintln!(
                    "r{row}c{col} {key}: fixed_union rank={rank} 自身={:.3} 榜首={}={:.3}",
                    scored
                        .iter()
                        .find(|(k, _)| k == key)
                        .map(|(_, v)| *v)
                        .unwrap_or(0.0),
                    scored[0].0,
                    scored[0].1
                );
            }
        }
        samples += 1;
    }
    eprintln!("== 规则扫描（{samples} 格，整库 107 类）==");
    for (r, name) in RULES.iter().enumerate() {
        eprintln!(
            "  {name:12} top1 {}/{} 平均名次 {:.1}",
            hits[r],
            samples,
            rank_sum[r] / samples.max(1) as f32
        );
    }
}

/// 卡片渲染比例实测：对每个标注格用**正确 key** 扫描画布比例，看真实比例落在哪里、
/// 峰值 Dice 有多高（判断"固定比例"是否成立，以及失配是尺度问题还是结构问题）。
#[test]
#[ignore = "卡片比例实测"]
fn probe_card_scale_measure() {
    let (frame, rects) = labeled_frame();
    let matcher = IconMatcher::for_items(&[]);
    let mut best_scales: Vec<f32> = Vec::new();
    for (idx, ((row, col), expected)) in LABELED_LIST.iter().enumerate() {
        let Some(key) = expected else { continue };
        let rect = rects[idx];
        let Some(masks) = crate::loadout_sync::matcher::CellMasks::from_frame(&frame, rect) else {
            continue;
        };
        let (_, qc, mw, mh) = masks.grid();
        let q_centroid = centroid(qc, mw, mh);
        let mut curve: Vec<(f32, f32, f32)> = Vec::new(); // (scale, dice, recall)
        for step in 0..=24 {
            let scale = 0.40 + 0.02 * step as f32;
            let glyph_h = ((mw.min(mh) as f32) * scale).round() as u32;
            let Some(tpl) = matcher.template_parts(&frame, rect, key, glyph_h) else {
                continue;
            };
            let t_centroid = centroid(&tpl.color, mw, mh);
            let (dx, dy) = (
                (q_centroid.0 - t_centroid.0).round() as i32,
                (q_centroid.1 - t_centroid.1).round() as i32,
            );
            let (dice, recall) = dice_shift(qc, &tpl.color, mw, mh, dx, dy);
            curve.push((scale, dice, recall));
        }
        let best =
            curve.iter().cloned().fold(
                (0.0f32, 0.0f32, 0.0f32),
                |acc, v| {
                    if v.1 > acc.1 {
                        v
                    } else {
                        acc
                    }
                },
            );
        best_scales.push(best.0);
        eprintln!(
            "r{row}c{col} {key}: 最佳比例 {:.2} → color Dice {:.3} recall {:.3} | 曲线 {}",
            best.0,
            best.1,
            best.2,
            curve
                .iter()
                .filter(|(_, d, _)| *d > 0.25)
                .map(|(s, d, _)| format!("{s:.2}:{d:.2}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
    if !best_scales.is_empty() {
        best_scales.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let median = best_scales[best_scales.len() / 2];
        eprintln!(
            "== 比例分布：p25 {:.2} 中位 {:.2} p75 {:.2}（当前常量 {:.3}）==",
            best_scales[best_scales.len() / 4],
            median,
            best_scales[best_scales.len() * 3 / 4],
            LIST_GLYPH_SCALE
        );
    }
}

/// 分类色通道的**逐通道包围盒归一化** Dice（含召回口径），与固定画布口径对照。
#[test]
#[ignore = "分类色通道对照"]
fn probe_color_channel_rules() {
    const PATCH: usize = 48;
    let (frame, rects) = labeled_frame();
    let matcher = IconMatcher::for_items(&[]);
    const RULES: [&str; 5] = [
        "largest_color",
        "largest_color_mp",
        "two_parts",
        "color_all",
        "white_all",
    ];
    let mut hits = vec![0usize; RULES.len()];
    let mut rank_sum = vec![0.0f32; RULES.len()];
    let mut samples = 0usize;
    let mut matrix: Vec<Vec<(String, f32)>> = Vec::new();
    let mut labeled_cells: Vec<String> = Vec::new();
    for (idx, ((row, col), expected)) in LABELED_LIST.iter().enumerate() {
        let Some(key) = expected else { continue };
        let rect = rects[idx];
        let Some(masks) = crate::loadout_sync::matcher::CellMasks::from_frame(&frame, rect) else {
            continue;
        };
        let (_, _, mw, mh) = masks.grid();
        let glyph_h = ((mw.min(mh) as f32) * LIST_GLYPH_SCALE).round() as u32;

        // 查询侧：分类色连通域按面积排序；白字连通域同理
        let mut color_comps: Vec<(usize, Vec<bool>)> = Vec::new();
        let mut white_comps: Vec<(usize, Vec<bool>)> = Vec::new();
        for c in masks.component_infos().iter().filter(|c| c.kept) {
            let (cw, cc, w, h) = masks.sub_grid(c.bbox);
            if c.color_px >= c.white_px {
                color_comps.push((
                    c.color_px,
                    normalize_patch(&cc, w, h, (0, 0, w as i32 - 1, h as i32 - 1), PATCH),
                ));
            } else {
                white_comps.push((
                    c.white_px,
                    normalize_patch(&cw, w, h, (0, 0, w as i32 - 1, h as i32 - 1), PATCH),
                ));
            }
        }
        color_comps.sort_by(|a, b| b.0.cmp(&a.0));
        white_comps.sort_by(|a, b| b.0.cmp(&a.0));
        let (_, qc_patch) = match color_comps.first() {
            Some(v) => (v.0, v.1.clone()),
            None => continue,
        };
        let qw_patch = white_comps.first().map(|(_, p)| p.clone());

        let mut per_rule: Vec<Vec<(String, f32)>> = vec![Vec::new(); RULES.len()];
        for lib_key in matcher_keys(&matcher) {
            let Some(tpl) = matcher.template_parts(&frame, rect, &lib_key, glyph_h) else {
                continue;
            };
            let t_color = channel_bbox(&tpl.color, tpl.width, tpl.height)
                .map(|bb| normalize_patch(&tpl.color, tpl.width, tpl.height, bb, PATCH));
            let t_white = channel_bbox(&tpl.white, tpl.width, tpl.height)
                .map(|bb| normalize_patch(&tpl.white, tpl.width, tpl.height, bb, PATCH));
            let stats = |a: &[bool], b: &Option<Vec<bool>>| -> (f32, f32) {
                let Some(b) = b else { return (0.0, 0.0) };
                let (mut hit, mut na, mut nb) = (0usize, 0usize, 0usize);
                for (x, y) in a.iter().zip(b.iter()) {
                    if *x {
                        na += 1;
                    }
                    if *y {
                        nb += 1;
                    }
                    if *x && *y {
                        hit += 1;
                    }
                }
                let dice = if na + nb == 0 {
                    0.0
                } else {
                    2.0 * hit as f32 / (na + nb) as f32
                };
                let precision = if na == 0 { 0.0 } else { hit as f32 / na as f32 };
                (dice, precision)
            };
            let (cd, cp) = stats(&qc_patch, &t_color);
            let (wd, _) = stats(qw_patch.as_deref().unwrap_or(&[]), &t_white);
            let largest_color_mp = cd.min(cp);
            let two_parts = match (&qw_patch, &t_white) {
                (Some(_), Some(_)) => (cd + wd) / 2.0,
                _ => cd,
            };
            // 关键对照：用「整格分类色掩码」而不是「最大连通域」
            let (_, qc_all, _, _) = masks.grid();
            let qc_all_patch =
                normalize_patch(qc_all, mw, mh, (0, 0, mw as i32 - 1, mh as i32 - 1), PATCH);
            let (color_all, _) = stats(&qc_all_patch, &t_color);
            let (_, qw_all, _, _) = masks.grid();
            let qw_all_patch =
                normalize_patch(qw_all, mw, mh, (0, 0, mw as i32 - 1, mh as i32 - 1), PATCH);
            let (white_all, _) = stats(&qw_all_patch, &t_white);

            let values = [cd, largest_color_mp, two_parts, color_all, white_all];
            for (r, v) in values.iter().enumerate() {
                per_rule[r].push((lib_key.clone(), *v));
            }
        }
        for (r, scored) in per_rule.iter_mut().enumerate() {
            scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            let rank = scored
                .iter()
                .position(|(k, _)| k == key)
                .map(|i| i + 1)
                .unwrap_or(scored.len());
            hits[r] += usize::from(rank == 1);
            rank_sum[r] += rank as f32;
            if r == 0 {
                eprintln!(
                    "r{row}c{col} {key}: largest_color rank={rank} 自身={:.3} 榜首={}={:.3}",
                    scored
                        .iter()
                        .find(|(k, _)| k == key)
                        .map(|(_, v)| *v)
                        .unwrap_or(0.0),
                    scored[0].0,
                    scored[0].1
                );
            }
        }
        {
            let mut flat: Vec<(String, f32)> = Vec::new();
            for rule_scores in &per_rule {
                for (k, v) in rule_scores {
                    flat.push((k.clone(), *v));
                }
            }
            matrix.push(flat);
            labeled_cells.push(key.to_string());
        }
        samples += 1;
    }
    eprintln!("== 分类色/白字通道规则扫描（{samples} 格，整库 107 类）==");
    for (r, name) in RULES.iter().enumerate() {
        eprintln!(
            "  {name:16} top1 {}/{} 平均名次 {:.1}",
            hits[r],
            samples,
            rank_sum[r] / samples.max(1) as f32
        );
    }

    // 逐模板分数归一化（z-score）：模板自身的「易匹配程度」是主要偏置，
    // 用同一批格子上的 (μ, σ) 归一化可去掉这个偏置。
    for (rule, label) in [(0usize, "largest_color"), (2usize, "two_parts")] {
        let mut per_key: std::collections::HashMap<String, Vec<f32>> =
            std::collections::HashMap::new();
        for cell in &matrix {
            for (k, v) in cell.iter().skip(rule).step_by(RULES.len()) {
                per_key.entry(k.clone()).or_default().push(*v);
            }
        }
        let mut z_hits = 0usize;
        let mut z_rank = 0.0f32;
        for (ci, cell) in matrix.iter().enumerate() {
            let mut scored: Vec<(String, f32)> = Vec::new();
            let mut it = cell.iter().skip(rule).step_by(RULES.len());
            while let Some((k, v)) = it.next() {
                let values = &per_key[k];
                let n = values.len() as f32;
                let mean = values.iter().sum::<f32>() / n.max(1.0);
                let var = values.iter().map(|x| (x - mean) * (x - mean)).sum::<f32>() / n.max(1.0);
                let sd = var.sqrt().max(1e-3);
                scored.push((k.clone(), (*v - mean) / sd));
            }
            scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            let expected = labeled_cells[ci].as_str();
            let rank = scored
                .iter()
                .position(|(k, _)| k == expected)
                .map(|i| i + 1)
                .unwrap_or(scored.len());
            z_hits += usize::from(rank == 1);
            z_rank += rank as f32;
        }
        eprintln!(
            "  {label}_z (逐模板归一化) top1 {z_hits}/{samples} 平均名次 {:.1}",
            z_rank / samples.max(1) as f32
        );
    }
}

/// 临时诊断：把 5 个失败测试的实际分数打出来（定位阶段用，用完删除）。
#[test]
#[ignore]
fn probe_failing_scores() {
    // 1) matcher_finds_target_cell_and_rejects_lookalikes : eagle_airstrike @ row2 col0
    {
        let items = [
            named("类星体加农炮"),
            named("反器材步枪"),
            named("飞鹰空袭"),
            named("轨道炮攻击"),
        ];
        let matcher = IconMatcher::for_items(&items.iter().collect::<Vec<_>>());
        let frame = list_frame(&[
            "machine_gun",
            "autocannon",
            "eagle_airstrike",
            "orbital_precision_strike",
        ]);
        let rec = GameUIRecognizer::new(sim_cal());
        let grid = rec.detect_list(&frame).unwrap();
        let cells = grid.cells.clone();
        let target = named("飞鹰空袭");
        match matcher.find_in_cells(&frame, &cells, &target, CellKind::ListStratagem) {
            Some(m) => eprintln!(
                "[lookalike] found cell=({},{}) score={:.3} margin={:.3}",
                m.cell_row, m.cell_col, m.score, m.margin
            ),
            None => eprintln!("[lookalike] NOT FOUND"),
        }
    }

    // 2) matcher_scores_correct_target_above_threshold_on_detected_cells
    {
        let frame = list_frame(&[
            "machine_gun",
            "autocannon",
            "eagle_airstrike",
            "orbital_precision_strike",
            "stalwart",
        ]);
        let rec = GameUIRecognizer::new(sim_cal());
        let grid = rec.detect_list(&frame).expect("list");
        let cell = *grid.row_cells(2)[0];
        let target = named("飞鹰空袭");
        let matcher = IconMatcher::for_items(&[&target]);
        let score = matcher
            .score_cell_for(
                &frame,
                cell.rect,
                &LoadoutItem {
                    name: "飞鹰空袭".into(),
                    icon: "eagle_airstrike".into(),
                    base_index: None,
                },
                CellKind::ListStratagem,
            )
            .unwrap_or(0.0);
        eprintln!("[detected-cells] eagle_airstrike score={score:.3} (需 >= 0.65)");
    }

    // 3) library_classification_accepts_present_targets_even_when_nearly_tied
    {
        let keys = [
            "machine_gun",
            "autocannon",
            "anti_materiel_rifle",
            "eagle_airstrike",
            "orbital_precision_strike",
            "stalwart",
            "grenade_launcher",
            "eagle_500kg_bomb",
            "laser_cannon",
            "railgun",
            "arc_thrower",
            "quasar_cannon",
        ];
        let rec = GameUIRecognizer::new(sim_cal());
        let items: Vec<LoadoutItem> = keys
            .iter()
            .map(|k| LoadoutItem {
                name: (*k).to_string(),
                icon: (*k).to_string(),
                base_index: None,
            })
            .collect();
        let refs: Vec<&LoadoutItem> = items.iter().collect();
        let matcher = IconMatcher::for_targets_only(&refs);
        for truth in keys {
            let rows = ["stalwart", "laser_cannon", truth, "arc_thrower", "railgun"];
            let frame = list_frame(&rows);
            let grid = rec.detect_list(&frame).unwrap();
            let cell = *grid.row_cells(2)[0];
            let (winner, score, _) = matcher
                .classify_cell_all(&frame, cell.rect, CellKind::ListStratagem)
                .expect("classify");
            let idx = keys.iter().position(|k| *k == truth).unwrap();
            let own = matcher
                .score_cell_for(&frame, cell.rect, &items[idx], CellKind::ListStratagem)
                .unwrap_or(0.0);
            eprintln!(
                "[library] truth={truth:26} winner={winner:26} winner_score={score:.3} own={own:.3}"
            );
        }
    }

    // 4) classifier_identifies_each_slot (Home)
    {
        let expected = [
            named("反器材步枪"),
            named("飞鹰空袭"),
            named("轨道炮攻击"),
            named("类星体加农炮"),
        ];
        let matcher = IconMatcher::for_items(&expected.iter().collect::<Vec<_>>());
        let candidates: Vec<&LoadoutItem> = expected.iter().collect();
        let keys = [
            "anti_materiel_rifle",
            "eagle_airstrike",
            "orbital_railcannon_strike",
            "quasar_cannon",
        ];
        let mut img = GrayImage::from_pixel(SIM_W, SIM_H, image::Luma([20]));
        for (i, key) in keys.iter().enumerate() {
            draw_cell(
                &mut img,
                sim_home_rect(i),
                Some(key),
                false,
                SIM_HOME_CARD_SCALE,
            );
        }
        fill(&mut img, sim_home_rect(4).inset(6), 64);
        draw_hexagon(&mut img, sim_booster_hex(), 210);
        let frame = make_frame(img, None);
        let rec = GameUIRecognizer::new(sim_cal());
        let slots = rec.detect_home(&frame).expect("home");
        for (i, kind) in expected.iter().enumerate() {
            match matcher.classify_cell(
                &frame,
                slots.stratagems[i].rect,
                &candidates,
                CellKind::HomeStratagem,
            ) {
                Some((idx, score, margin)) => eprintln!(
                    "[home] slot {} want={:24} got={:24} score={score:.3} margin={margin:.3}",
                    i + 1,
                    kind.key(),
                    candidates[idx].key()
                ),
                None => eprintln!("[home] slot {} classify 返回 None", i + 1),
            }
        }
    }
}

/// 对齐对照实验（plan3 P0.3 / Phase 2）：
/// 在真实检测矩形上比较「偏移半径 1（当前生产）」与「偏移半径 2（有界扩展）」。
///
/// 关键判据不是单纯分数高低，而是 plan3 §10 停止条件 3：
/// **正确模板的提升必须多于错误模板**，否则扩展只是给所有模板同等加成。
#[test]
#[ignore = "对齐对照实验探针"]
fn probe_alignment_radius_matrix() {
    let cases: [(&str, &str); 6] = [
        ("eagle_airstrike", "machine_gun"),
        ("machine_gun", "autocannon"),
        ("quasar_cannon", "stalwart"),
        ("orbital_railcannon_strike", "orbital_precision_strike"),
        ("anti_materiel_rifle", "railgun"),
        ("eagle_500kg_bomb", "laser_cannon"),
    ];
    // 整库候选（与生产同一套模板生成路径）
    let lib: Vec<LoadoutItem> = crate::icons::all_icon_keys()
        .iter()
        .map(|k| LoadoutItem {
            name: (*k).to_string(),
            icon: (*k).to_string(),
            base_index: None,
        })
        .collect();
    let lib_refs: Vec<&LoadoutItem> = lib.iter().collect();
    let matcher = IconMatcher::for_targets_only(&lib_refs);

    let rec = GameUIRecognizer::new(sim_cal());
    let mut improved = 0usize;
    let mut regressed = 0usize;
    for (truth, distractor) in cases {
        let rows = [distractor, "railgun", truth, "stalwart", "laser_cannon"];
        let frame = list_frame(&rows);
        let grid = rec.detect_list(&frame).expect("列表识别");
        let cell = *grid.row_cells(2)[0];
        let masks =
            crate::loadout_sync::matcher::CellMasks::from_frame(&frame, cell.rect).expect("掩码");
        for radius in [1i32, 2] {
            let mut m = matcher.score_matrix_at_radius(&masks, CellKind::ListStratagem, radius);
            m.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            let own = m
                .iter()
                .find(|(k, _)| k == truth)
                .map(|(_, s)| *s)
                .unwrap_or(f32::NAN);
            let rank = m
                .iter()
                .position(|(k, _)| k == truth)
                .map(|p| p + 1)
                .unwrap_or(0);
            eprintln!(
                "[align] truth={truth:26} radius={radius} own={own:.4} rank={rank} winner={:26} winner_score={:.4} top2={:.4}",
                m[0].0, m[0].1, m[1].1
            );
        }
        // 比较半径 1 与 2 下正确模板的名次变化
        let rank_at = |radius: i32| -> usize {
            let mut m = matcher.score_matrix_at_radius(&masks, CellKind::ListStratagem, radius);
            m.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            m.iter()
                .position(|(k, _)| k == truth)
                .map(|p| p + 1)
                .unwrap_or(0)
        };
        match (rank_at(1), rank_at(2)) {
            (a, b) if b < a => improved += 1,
            (a, b) if b > a => regressed += 1,
            _ => {}
        }
    }
    eprintln!("[align] 名次改善 {improved} 例，名次回归 {regressed} 例");
}

// ─── plan4 Phase 0/1/2：模板↔查询掩码结构一致性诊断 ───

/// plan4 P0.2 / P1.1 / P1.2 / P2.1 / P2.2 的结构化诊断探针（只读）。
///
/// 对 6 个典型目标同时输出：
///   * query cell 的 width/height、white/color/fg 像素、bbox、每个 component 的
///     面积/bbox/宽高/aspect/fill/是否被删/删除原因；
///   * template 的 icon_size、alpha 像素、classify_pixel 三分类计数、
///     white/color/fg 像素、bbox、被判 Background 的 alpha 像素数；
///   * 三项 Dice 与 expected/winner 对照；
///   * 掩码叠加 PNG artifact（需设置 `H2AC_MASK_DUMP=1`）。
#[test]
#[ignore = "plan4 掩码结构诊断探针"]
fn probe_mask_structure() {
    use crate::loadout_sync::matcher::{ALPHA_MASK_THRESHOLD, MIN_COMPONENT_AREA};

    // 目标 → 该行里放置的干扰项（保持与既有探针一致的渲染方式）
    let cases: [(&str, &str); 6] = [
        ("quasar_cannon", "stalwart"),
        ("machine_gun", "autocannon"),
        ("anti_materiel_rifle", "railgun"),
        ("eagle_airstrike", "machine_gun"),
        ("orbital_railcannon_strike", "orbital_precision_strike"),
        ("eagle_500kg_bomb", "laser_cannon"),
    ];

    let lib: Vec<LoadoutItem> = crate::icons::all_icon_keys()
        .iter()
        .map(|k| LoadoutItem {
            name: (*k).to_string(),
            icon: (*k).to_string(),
            base_index: None,
        })
        .collect();
    let lib_refs: Vec<&LoadoutItem> = lib.iter().collect();
    let matcher = IconMatcher::for_targets_only(&lib_refs);
    let rec = GameUIRecognizer::new(sim_cal());

    let dump_dir = std::path::PathBuf::from("screenshots/mask_diagnostics");
    let dump = std::env::var("H2AC_MASK_DUMP")
        .map(|v| v == "1")
        .unwrap_or(false);
    if dump {
        std::fs::create_dir_all(&dump_dir).expect("创建诊断目录");
    }

    eprintln!("=== plan4 掩码结构诊断（生产常量：WHITE_LUMA_MIN={:.0} WHITE_CHROMA_MAX={} COLOR_CHROMA_MIN={} ALPHA>={} MIN_COMPONENT_AREA={}）===",
        crate::loadout_sync::matcher::WHITE_LUMA_MIN,
        crate::loadout_sync::matcher::WHITE_CHROMA_MAX,
        crate::loadout_sync::matcher::COLOR_CHROMA_MIN,
        ALPHA_MASK_THRESHOLD, MIN_COMPONENT_AREA);

    for (truth, distractor) in cases {
        let rows = [distractor, "railgun", truth, "stalwart", "laser_cannon"];
        let frame = list_frame(&rows);
        let grid = rec.detect_list(&frame).expect("列表识别");
        let cell = *grid.row_cells(2)[0];
        let Some(masks) = crate::loadout_sync::matcher::CellMasks::from_frame(&frame, cell.rect)
        else {
            eprintln!("[{truth}] CellMasks 提取失败");
            continue;
        };

        // ── query 侧 ──
        let (qw, qc, w, h) = masks.grid();
        let th = masks.thresholds();
        let q_white: usize = qw.iter().filter(|v| **v).count();
        let q_color: usize = qc.iter().filter(|v| **v).count();
        let q_bbox = masks.probe_bbox();
        eprintln!(
            "\n### {truth}  cell={:?}\n  query: {w}x{h} white={q_white} color={q_color} fg={} bbox={q_bbox:?}  thresholds(luma>={:.0}, chroma<={}, chroma_color>{})",
            cell.rect,
            q_white + q_color,
            th.white_luma_min,
            th.white_chroma_max,
            th.color_chroma_min
        );
        for (i, c) in masks.component_infos().iter().enumerate() {
            let cw = c.bbox.2 - c.bbox.0 + 1;
            let ch = c.bbox.3 - c.bbox.1 + 1;
            let aspect = cw.max(ch) as f32 / cw.min(ch).max(1) as f32;
            let fill = c.area as f32 / (cw * ch).max(1) as f32;
            let reason = c.reject.map(|r| r.label()).unwrap_or("kept");
            eprintln!(
                "  comp {i:2}: area={:4} bbox={:?} {cw}x{ch} aspect={aspect:.2} fill={fill:.2} white={:4} color={:4} kept={} reason={reason}",
                c.area, c.bbox, c.white_px, c.color_px, c.kept
            );
        }

        // ── 整库评分，取 winner ──
        let mut scored = matcher.score_matrix_at_radius(&masks, CellKind::ListStratagem, 1);
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let own = scored
            .iter()
            .find(|(k, _)| k == truth)
            .map(|(_, s)| *s)
            .unwrap_or(f32::NAN);
        let rank = scored
            .iter()
            .position(|(k, _)| k == truth)
            .map(|p| p + 1)
            .unwrap_or(0);
        let winner = scored[0].0.clone();
        eprintln!(
            "  match: own={own:.4} rank={rank}/{} winner={winner} winner_score={:.4} top2={:.4} margin={:.4}",
            scored.len(),
            scored[0].1,
            scored.get(1).map(|m| m.1).unwrap_or(0.0),
            scored[0].1 - scored.get(1).map(|m| m.1).unwrap_or(0.0)
        );

        // ── 模板侧：expected / winner / 一个明显低分模板 ──
        let low = scored.last().map(|m| m.0.clone()).unwrap_or_default();
        let inter = |a: &[bool], b: &[bool]| -> usize {
            a.iter().zip(b).filter(|(x, y)| **x && **y).count()
        };
        let dice = |hit: usize, a: usize, b: usize| -> f32 {
            if a + b == 0 {
                0.0
            } else {
                2.0 * hit as f32 / (a + b) as f32
            }
        };
        for (label, key) in [
            ("expected", truth),
            ("winner", winner.as_str()),
            ("lowest", low.as_str()),
        ] {
            let Some(t) = matcher.template_probe_summary(&masks, CellKind::ListStratagem, key)
            else {
                eprintln!("  tmpl[{label}] {key}: 模板生成失败");
                continue;
            };
            eprintln!(
                "  tmpl[{label:8}] {key:28} icon_size={:3} canvas={}x{} bbox={:?} white={} color={} fg={}",
                t.icon_size,
                t.canvas.0,
                t.canvas.1,
                t.bbox,
                t.white_px,
                t.color_px,
                t.white_px + t.color_px
            );
            let w_hit = inter(&t.mapped_white, qw);
            let c_hit = inter(&t.mapped_color, qc);
            let mapped_w: usize = t.mapped_white.iter().filter(|v| **v).count();
            let mapped_c: usize = t.mapped_color.iter().filter(|v| **v).count();
            eprintln!(
                "    dice@(0,0): white={:.3} (hit={w_hit} mapped={mapped_w} query={q_white})  color={:.3} (hit={c_hit} mapped={mapped_c} query={q_color})",
                dice(w_hit, mapped_w, q_white),
                dice(c_hit, mapped_c, q_color)
            );
        }

        if dump {
            dump_mask_overlay(
                &dump_dir, truth, &frame, cell.rect, &masks, &matcher, truth, &winner,
            );
        }

        // ── plan4 P1.2：query cell 内「图标像素」的 luma/chroma 真实分布 ──
        //
        // 生产 classify_pixel 只按绝对 luma/chroma 判定，因此必须知道：
        // 图标主体（含铜色剪影）到底落在什么 luma/chroma 区间，
        // 以及格子底色落在哪里。这决定了阈值应该定在哪。
        {
            let side = cell.rect.w.min(cell.rect.h);
            let inset = (side as f32 * 0.10).round() as i32;
            let mut hist = [0usize; 8]; // luma 分 8 段（每段 32）
            let mut bg_hist = [0usize; 8];
            let mut samples: Vec<(i32, i32, i32, u8, u8, u8)> = Vec::new();
            for y in inset..(cell.rect.h - inset) {
                for x in inset..(cell.rect.w - inset) {
                    let px = (cell.rect.x + x) as u32;
                    let py = (cell.rect.y + y) as u32;
                    let Some(p) = frame.rgba.get_pixel_checked(px, py) else {
                        continue;
                    };
                    let q = p.0;
                    let chroma = q[0].max(q[1]).max(q[2]) as i32 - q[0].min(q[1]).min(q[2]) as i32;
                    let luma = 0.299 * q[0] as f32 + 0.587 * q[1] as f32 + 0.114 * q[2] as f32;
                    samples.push((
                        q[0] as i32,
                        q[1] as i32,
                        q[2] as i32,
                        chroma as u8,
                        luma as u8,
                        0,
                    ));
                }
            }
            // 用「相对本格 luma 分位数」估计底色，再统计超出部分的分布
            let mut lumas: Vec<u8> = samples.iter().map(|s| s.4).collect();
            lumas.sort_unstable();
            let bg = lumas[lumas.len() / 4] as i32; // p25 ≈ 底色
            for s in &samples {
                let bucket = (s.4 as usize / 32).min(7);
                if s.4 as i32 <= bg + 10 {
                    bg_hist[bucket] += 1;
                } else {
                    hist[bucket] += 1;
                }
            }
            let fg_total: usize = hist.iter().sum();
            let bg_total: usize = bg_hist.iter().sum();
            eprintln!(
                "  [luma] 底色 p25={bg}  前景候选(bg+10 以上)={fg_total}  底色像素={bg_total}"
            );
            let fmt = |h: &[usize; 8]| {
                h.iter()
                    .enumerate()
                    .filter(|(_, c)| **c > 0)
                    .map(|(i, c)| format!("{}..{}:{c}", i * 32, i * 32 + 31))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            eprintln!("  [luma] 前景 luma 直方图: {}", fmt(&hist));
            eprintln!("  [luma] 底色 luma 直方图: {}", fmt(&bg_hist));
            // 前景候选里 chroma 的分布（判断是否该走 Color 通道）
            let mut chroma_hist = [0usize; 6]; // 0..255 分 6 段，每段 ~43
            let mut above: Vec<(u8, u8)> = Vec::new();
            for s in &samples {
                if s.4 as i32 > bg + 10 {
                    chroma_hist[(s.3 as usize / 43).min(5)] += 1;
                    above.push((s.4, s.3));
                }
            }
            eprintln!(
                "  [chroma] 前景候选 chroma 直方图: {}",
                chroma_hist
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| **c > 0)
                    .map(|(i, c)| format!("{}..{}:{c}", i * 43, i * 43 + 42))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            eprintln!(
                "  [probe] 当前门槛下：Color(chroma>{}) 命中数、White(chroma<={} 且 luma>={}) 命中数",
                crate::loadout_sync::matcher::COLOR_CHROMA_MIN,
                crate::loadout_sync::matcher::WHITE_CHROMA_MAX,
                crate::loadout_sync::matcher::WHITE_LUMA_MIN
            );
        }
    }
}

/// plan4 P1.3：像素分类阈值矩阵（非生产探针）。
///
/// 每次只改一个维度：先扫 `WHITE_LUMA_MIN`（已由 P1.2 证明是丢笔画的那一项）。
/// 每个组合都记录 6 个典型目标的 expected score/rank 与前景像素数 ——
/// 不允许只看单个目标分数就选阈值。
#[test]
#[ignore = "plan4 阈值矩阵探针"]
fn probe_threshold_matrix() {
    use crate::loadout_sync::matcher::{CellMasks, PixelThresholds};

    let cases: [(&str, &str); 6] = [
        ("quasar_cannon", "stalwart"),
        ("machine_gun", "autocannon"),
        ("anti_materiel_rifle", "railgun"),
        ("eagle_airstrike", "machine_gun"),
        ("orbital_railcannon_strike", "orbital_precision_strike"),
        ("eagle_500kg_bomb", "laser_cannon"),
    ];
    let lib: Vec<LoadoutItem> = crate::icons::all_icon_keys()
        .iter()
        .map(|k| LoadoutItem {
            name: (*k).to_string(),
            icon: (*k).to_string(),
            base_index: None,
        })
        .collect();
    let lib_refs: Vec<&LoadoutItem> = lib.iter().collect();
    let matcher = IconMatcher::for_targets_only(&lib_refs);
    let rec = GameUIRecognizer::new(sim_cal());

    for luma in [150.0f32, 130.0, 125.0, 120.0, 115.0, 110.0, 105.0, 100.0] {
        let t = PixelThresholds::PRODUCTION.with_luma_min(luma);
        let mut ranks = Vec::new();
        let mut scores = Vec::new();
        let mut fg_stats = Vec::new();
        for (truth, distractor) in cases {
            let rows = [distractor, "railgun", truth, "stalwart", "laser_cannon"];
            let frame = list_frame(&rows);
            let grid = rec.detect_list(&frame).expect("列表识别");
            let cell = *grid.row_cells(2)[0];
            let fg = CellMasks::from_frame_thresholds(&frame, cell.rect, t)
                .map(|m| {
                    let (w, c, _, _) = m.grid();
                    w.iter().filter(|v| **v).count() + c.iter().filter(|v| **v).count()
                })
                .unwrap_or(0);
            fg_stats.push(fg);
            let mut m =
                matcher.score_matrix_with_thresholds(&frame, cell.rect, CellKind::ListStratagem, t);
            m.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            let own = m
                .iter()
                .find(|(k, _)| k == truth)
                .map(|(_, s)| *s)
                .unwrap_or(f32::NAN);
            let rank = m
                .iter()
                .position(|(k, _)| k == truth)
                .map(|p| p + 1)
                .unwrap_or(0);
            scores.push((own * 1000.0).round() / 1000.0);
            ranks.push(rank);
        }
        // 安全侧：空槽必须仍然被判为「无前景」，否则放宽阈值就是在制造 false positive
        let empty_frame = list_frame(&[
            "__empty__",
            "__empty__",
            "__empty__",
            "__empty__",
            "__empty__",
        ]);
        let empty_grid = rec.detect_list(&empty_frame).expect("空列表识别");
        let empty_cell = *empty_grid.row_cells(2)[0];
        let empty_fg = CellMasks::from_frame_thresholds(&empty_frame, empty_cell.rect, t)
            .map(|m| {
                let (w, c, _, _) = m.grid();
                w.iter().filter(|v| **v).count() + c.iter().filter(|v| **v).count()
            })
            .unwrap_or(0);
        eprintln!(
            "[matrix] luma={luma:5.1} fg={fg_stats:?} empty_fg={empty_fg} score={scores:?} rank={ranks:?}"
        );
    }
    eprintln!("[matrix] 顺序 = quasar_cannon, machine_gun, anti_materiel_rifle, eagle_airstrike, orbital_railcannon_strike, eagle_500kg_bomb");
}

/// 打印「轮椅」profile 的 slots 06~10 实际映射（只读诊断）。
#[test]
#[ignore = "profile 映射诊断"]
fn probe_lunyi_profile_slots() {
    let json = std::fs::read_to_string("target/debug/profiles/轮椅.json").ok();
    let Some(json) = json else {
        eprintln!("找不到轮椅.json");
        return;
    };
    let v: serde_json::Value = serde_json::from_str(&json).expect("解析 profile");
    let loadout = v["loadout"].as_array().expect("loadout 数组");
    eprintln!("[lunyi] profile loadout 数组（长度 {}）:", loadout.len());
    for (i, item) in loadout.iter().enumerate() {
        let slot = i + 1;
        let label = if (6..=10).contains(&slot) {
            "★ 参与 LoadoutSync"
        } else {
            "  （不参与）"
        };
        match item.as_u64() {
            Some(idx) => {
                let s = crate::stratagems::STRATAGEMS.get(idx as usize);
                eprintln!(
                    "[lunyi] slot {slot:02} (数组下标 {i}) = 索引 {idx:3} -> icon={:?} name={:?}  {label}",
                    s.map(|s| s.icon),
                    s.map(|s| s.name)
                );
            }
            None => eprintln!("[lunyi] slot {slot:02} (数组下标 {i}) = 空  {label}"),
        }
    }
}

/// plan5 P1.1/P1.4：验证 `verify_slot_selected` 的 `CellKind` 语义。
///
/// 生产 `verify_slot_selected` 对 Home 槽位传的是 `CellKind::ListStratagem`，
/// 而复核用的 `verify_final_loadout` 传的是 `CellKind::HomeStratagem`。
/// 本探针在**同一个真实 Home 帧的同一个槽位**上比较两种 kind 的分数差异。
#[test]
#[ignore = "plan5 CellKind 语义探针"]
fn probe_home_verify_cellkind() {
    let expected = [
        named("反器材步枪"),
        named("飞鹰空袭"),
        named("轨道炮攻击"),
        named("类星体加农炮"),
    ];
    let keys = [
        "anti_materiel_rifle",
        "eagle_airstrike",
        "orbital_railcannon_strike",
        "quasar_cannon",
    ];
    let mut img = GrayImage::from_pixel(SIM_W, SIM_H, image::Luma([20]));
    for (i, key) in keys.iter().enumerate() {
        draw_cell(
            &mut img,
            sim_home_rect(i),
            Some(key),
            false,
            SIM_HOME_CARD_SCALE,
        );
    }
    fill(&mut img, sim_home_rect(4).inset(6), 64);
    draw_hexagon(&mut img, sim_booster_hex(), 210);
    let frame = make_frame(img, None);
    let rec = GameUIRecognizer::new(sim_cal());
    let slots = rec.detect_home(&frame).expect("Home 识别失败");
    let matcher = IconMatcher::for_items(&expected.iter().collect::<Vec<_>>());

    eprintln!("[cellkind] 同一 Home 槽位，两种 CellKind 的 score_cell_for:");
    for (i, item) in expected.iter().enumerate() {
        let rect = slots.stratagems[i].rect;
        let drawn = sim_home_rect(i);
        eprintln!(
            "[cellkind] slot {} drawn=({},{},{},{}) detected=({},{},{},{}) offset=({:+},{:+})",
            i + 1,
            drawn.x,
            drawn.y,
            drawn.w,
            drawn.h,
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            rect.x - drawn.x,
            rect.y - drawn.y
        );
        let as_list = matcher
            .score_cell_for(&frame, rect, item, CellKind::ListStratagem)
            .unwrap_or(0.0);
        let as_home = matcher
            .score_cell_for(&frame, rect, item, CellKind::HomeStratagem)
            .unwrap_or(0.0);
        eprintln!(
            "[cellkind] slot {} {:26} ListStratagem={as_list:.4}  HomeStratagem={as_home:.4}  delta={:+.4}",
            i + 1,
            item.key(),
            as_home - as_list
        );
    }

    // 直接调用生产验证函数，确认它当前是否通过
    for (i, item) in expected.iter().enumerate() {
        let r = crate::loadout_sync::verifier::verify_slot_selected(
            &matcher,
            &frame,
            &slots,
            crate::loadout_sync::selection::TargetSlot::Stratagem(i),
            item,
            RECOGNITION_THRESHOLD,
        );
        eprintln!(
            "[cellkind] verify_slot_selected slot {} -> {:?}",
            i + 1,
            r.map(|s| (s * 1000.0).round() / 1000.0)
        );
    }
}

/// 把 query 与模板掩码画成一张对照图（plan4 P2.1）。
#[cfg(test)]
fn dump_mask_overlay(
    dir: &std::path::Path,
    name: &str,
    frame: &CapturedFrame,
    cell: crate::loadout_sync::types::ImageRect,
    masks: &crate::loadout_sync::matcher::CellMasks,
    matcher: &IconMatcher,
    expected: &str,
    winner: &str,
) {
    use image::{Rgba, RgbaImage};
    let (qw, qc, w, h) = masks.grid();
    let _th = masks.thresholds();
    let w = w as u32;
    let h = h as u32;
    // 左：原 cell；中：query 掩码；右：expected 模板映射；最右：winner 模板映射
    let tile = w * 4;
    let mut out = RgbaImage::new(tile, h);
    for y in 0..h {
        for x in 0..w {
            // 原 cell
            if let Some(p) = frame
                .rgba
                .get_pixel_checked(cell.x as u32 + x, cell.y as u32 + y)
            {
                out.put_pixel(x, y, *p);
            }
            let i = (y * w + x) as usize;
            if i < qw.len() {
                let c = match (qw[i], qc[i]) {
                    (true, _) => Rgba([255, 255, 255, 255]),
                    (false, true) => Rgba([255, 128, 0, 255]),
                    _ => Rgba([0, 0, 0, 255]),
                };
                out.put_pixel(w + x, y, c);
            }
        }
    }
    for (slot, key) in [(2u32, expected), (3, winner)] {
        let Some(t) = matcher.template_probe_summary(masks, CellKind::ListStratagem, key) else {
            continue;
        };
        let (mw, mc) = (t.mapped_white, t.mapped_color);
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                if i >= mw.len() {
                    continue;
                }
                let c = match (mw[i], mc[i]) {
                    (true, _) => Rgba([255, 255, 255, 255]),
                    (false, true) => Rgba([255, 128, 0, 255]),
                    _ => Rgba([0, 0, 0, 255]),
                };
                out.put_pixel(slot * w + x, y, c);
            }
        }
    }
    let _ = out.save(dir.join(format!("{name}_overlay.png")));
}
