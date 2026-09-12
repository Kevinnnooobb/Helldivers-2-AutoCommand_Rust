//! **临时验证模块（spike）** —— 参考实现 `vision/classifier.rs` 的核心打分（ZNCC）
//! 在本项目**真实标注帧**上的实测，以及实测暴露出的几何问题。
//!
//! ## 为什么要留着它
//!
//! plan6 §P1.3 的「迁移 TemplateClassifier」在动手前需要一个可判定的前提：
//! 参考算法在本项目的真实帧上到底有没有判别力。本模块用 ~300 行复刻了参考的
//! 核心（去均值单位化点积 over union-alpha mask + 有界偏移搜索），
//! 在真实标注帧上量出了数字。**结论见 `docs/plan6-findings.md` 第九节。**
//!
//! ## 实测结论（简）
//!
//! 1. 参考打分本身是干净的：把模板滑到图标位置时 ZNCC 峰值高（railgun 0.954）；
//! 2. ❌ ~~`detect_list` 的格子几何在真实帧上是错的~~ —— **已被叠加图推翻**。
//!    绿框（`detect_list` 输出）贴在真实卡片边框上，`score=0.80~0.95`；
//!    偏掉的是本模块早期那份参考网格（`MEASURED_GRID`），
//!    它的搜索带被列表上方的 UI 标记带偏了；
//! 3. 既有 `probe_labeled_accuracy` 的 `top1 0/13` 仍然**不能**当打分器成绩，
//!    但原因尚未定位（早期归因于"错位裁剪"是错的）；
//! 4. 「图标在卡片内的偏移」目前**没测准**（结果顶在搜索边界），
//!    在测准之前不改 `detect_list`。
//!
//! 结论与更正过程的完整记录见 `docs/plan6-findings.md` 第九、十节。
//! 本模块保留为这些结论的可复跑测量。
#![allow(dead_code)]

use image::{GrayImage, RgbaImage};

use crate::loadout_sync::capture::CapturedFrame;
use crate::loadout_sync::types::ImageRect;
use crate::vision::reference_catalog::{IconCatalog, ItemKind as CatalogItemKind};

const ALPHA_MASK_THRESHOLD: u8 = 64;
const BACKGROUND: f32 = 30.0;

/// ❌ **作废的参考网格** —— 保留只为让叠加图能画出「错误参考系」作对照。
///
/// 它由「逐图标在 y 380~760 的窄带里滑模板取峰值」得到，其中 y≈404 那一行
/// 命中的是列表上方的**小 UI 标记**（缩放/收藏类图标），不是战果格。
/// 第十节的叠加图已证明：`detect_list()` 的卡片几何才是对的。
///
/// 不要用本结构做任何判定；它现在只服务于 `spike_dump_detected_rects_overlay`
/// 里的红框（用于展示"错误参考系长什么样"）。
pub const MEASURED_GRID: MeasuredGrid = MeasuredGrid {
    col0_x: 89,
    col_pitch: 92,
    row0_y: 404,
    row_pitch: 123,
    icon_side: 52,
};

pub struct MeasuredGrid {
    pub col0_x: i32,
    pub col_pitch: i32,
    pub row0_y: i32,
    pub row_pitch: i32,
    pub icon_side: u32,
}

impl MeasuredGrid {
    pub fn top_left(&self, row: u32, col: u32) -> (i32, i32) {
        (
            self.col0_x + self.col_pitch * col as i32,
            self.row0_y + self.row_pitch * row as i32,
        )
    }
}

/// 归一化模板（mask + 去均值单位化取值）。
pub struct NormalizedTemplate {
    pub item_id: String,
    pub mask: Vec<(u16, u16)>,
    pub values: Vec<f32>,
}

impl NormalizedTemplate {
    pub fn build(item_id: &str, image: &RgbaImage, side: u32) -> Option<Self> {
        let scaled = crate::loadout_sync::matcher::resize_rgba(image, side, side);
        let mut mask = Vec::new();
        let mut raw = Vec::new();
        for y in 0..side {
            for x in 0..side {
                let p = scaled.get_pixel(x, y).0;
                if p[3] > ALPHA_MASK_THRESHOLD {
                    mask.push((x as u16, y as u16));
                    raw.push(luma601_px(p[0], p[1], p[2]) as f32);
                }
            }
        }
        if mask.len() < 64 {
            return None;
        }
        let values = unit_normalize(&raw)?;
        Some(Self {
            item_id: item_id.to_string(),
            mask,
            values,
        })
    }
}

/// 整库模板（同一边长），用于整库判别。
pub fn build_library(catalog: &IconCatalog, side: u32) -> Vec<NormalizedTemplate> {
    let root = crate::vision::reference_catalog::default_root();
    catalog
        .by_kind(CatalogItemKind::Stratagem)
        .filter_map(|entry| {
            let image = image::open(root.join(&entry.path)).ok()?.to_rgba8();
            NormalizedTemplate::build(&entry.item_id, &image, side)
        })
        .collect()
}

/// 在 `(ox, oy)` 处对灰度图做 ZNCC；越界或方差为 0 → None。
pub fn zncc_at(
    gray: &[f32],
    width: usize,
    height: usize,
    template: &NormalizedTemplate,
    ox: i32,
    oy: i32,
) -> Option<f32> {
    let mut raw = Vec::with_capacity(template.mask.len());
    for (mx, my) in &template.mask {
        let x = ox + *mx as i32;
        let y = oy + *my as i32;
        if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
            return None;
        }
        raw.push(gray[y as usize * width + x as usize]);
    }
    let normalized = unit_normalize(&raw)?;
    Some(
        normalized
            .iter()
            .zip(&template.values)
            .map(|(a, b)| a * b)
            .sum(),
    )
}

/// 在 `±radius` 内取最大 ZNCC（参考实现的 translation 层的最简形式）。
pub fn zncc_best(
    gray: &[f32],
    width: usize,
    height: usize,
    template: &NormalizedTemplate,
    center: (i32, i32),
    radius: i32,
) -> f32 {
    let mut best = f32::NEG_INFINITY;
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if let Some(score) =
                zncc_at(gray, width, height, template, center.0 + dx, center.1 + dy)
            {
                if score > best {
                    best = score;
                }
            }
        }
    }
    best
}

/// 整库排名（同一边长、同一搜索半径）。
pub fn rank_library(
    gray: &[f32],
    width: usize,
    height: usize,
    library: &[NormalizedTemplate],
    center: (i32, i32),
    radius: i32,
) -> Vec<(String, f32)> {
    let mut ranked: Vec<(String, f32)> = library
        .iter()
        .map(|t| {
            (
                t.item_id.clone(),
                zncc_best(gray, width, height, t, center, radius),
            )
        })
        .filter(|(_, s)| s.is_finite())
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked
}

/// 整幅帧的 luma601 灰度（与游戏抓帧同一权重）。
pub fn frame_luma(frame: &CapturedFrame) -> Vec<f32> {
    luma601(&frame.rgba)
}

pub fn luma601(src: &RgbaImage) -> Vec<f32> {
    src.pixels()
        .map(|p| luma601_px(p.0[0], p.0[1], p.0[2]) as f32)
        .collect()
}

pub fn luma601_px(r: u8, g: u8, b: u8) -> u8 {
    ((r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000) as u8
}

fn unit_normalize(values: &[f32]) -> Option<Vec<f32>> {
    let n = values.len() as f32;
    if n < 64.0 {
        return None;
    }
    let mean = values.iter().sum::<f32>() / n;
    let var = values.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>();
    if !var.is_finite() || var <= 1e-8 {
        return None;
    }
    let inv = var.sqrt().recip();
    Some(values.iter().map(|v| (v - mean) * inv).collect())
}

/// 真实标注帧 + 其标注表（与 `loadout_sync::tests` 的 `LABELED_LIST` 同一来源）。
#[cfg(test)]
pub(crate) fn labeled_fixture() -> Option<(CapturedFrame, Vec<u8>)> {
    use std::path::Path;
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/fixtures/loadout_sync");
    let img = image::open(dir.join("stratagem_list_labeled_1914x1080.png"))
        .ok()?
        .to_rgba8();
    let mut gray = GrayImage::new(img.width(), img.height());
    for (x, y, p) in img.enumerate_pixels() {
        gray.put_pixel(x, y, image::Luma([luma601_px(p[0], p[1], p[2])]));
    }
    Some((
        crate::loadout_sync::fixture_support::make_frame(gray, Some(img)),
        Vec::new(),
    ))
}

/// 与 `loadout_sync::tests` 的标注表一致（同一帧、同一人工标签）。
#[cfg(test)]
pub(crate) const LABELED: [((u32, u32), &str); 13] = [
    ((0, 0), "defoliation_tool"),
    ((0, 1), "cqc_20"),
    ((0, 3), "grenade_launcher"),
    ((1, 0), "stalwart"),
    ((1, 1), "heavy_machine_gun"),
    ((1, 2), "railgun"),
    ((1, 3), "speargun"),
    ((2, 0), "anti_materiel_rifle"),
    ((2, 2), "grenade_launcher"),
    ((3, 0), "gl_52_de_escalator"),
    ((3, 1), "sterilizer"),
    ((3, 2), "flamethrower"),
    ((3, 3), "laser_cannon"),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> IconCatalog {
        IconCatalog::load(&crate::vision::reference_catalog::default_root())
            .expect("参考目录加载失败")
    }

    /// 测量 1：真实图标几何 —— 对每个标注图标在窄带里滑模板，读取峰值与位置。
    ///
    /// 这一条产生了 `MEASURED_GRID`（列距 92 / 行距 123 / 边长 52）。
    #[test]
    #[ignore = "spike 测量：真实图标网格"]
    fn spike_measure_real_icon_grid() {
        let Some((frame, _)) = labeled_fixture() else {
            eprintln!("标注夹具缺失，跳过");
            return;
        };
        let gray = frame_luma(&frame);
        let (w, h) = (frame.width() as usize, frame.height() as usize);
        let catalog = catalog();
        let root = crate::vision::reference_catalog::default_root();

        // 近似网格：起点取自测绘，带宽 ±25（把「行距 123」的趋势测出来）
        let approx_cols = [89, 191, 280, 366];
        let approx_rows = [404, 527, 649, 781];
        for ((row, col), key) in LABELED {
            let Some(expected_id) = catalog.resolve_item_id(key) else {
                continue;
            };
            let Ok(icon) = image::open(
                root.join(
                    catalog
                        .by_kind(CatalogItemKind::Stratagem)
                        .find(|e| e.item_id == expected_id)
                        .map(|e| e.path.clone())
                        .unwrap_or_default(),
                ),
            ) else {
                eprintln!("r{row}c{col} {key}: 资源缺失");
                continue;
            };
            let Some(template) =
                NormalizedTemplate::build(&expected_id, &icon.to_rgba8(), MEASURED_GRID.icon_side)
            else {
                continue;
            };
            let cx = approx_cols[col as usize];
            let cy = approx_rows[row as usize];
            let best = zncc_best(&gray, w, h, &template, (cx, cy), 25);
            eprintln!("r{row}c{col} {key:<22} 峰值 {best:.3} (近似位置 x={cx} y={cy} ±25)");
        }
    }

    /// 测量 2：**错位裁剪**（既有 probe 用的几何）下的整库判别 —— 复现 0/13。
    #[test]
    #[ignore = "spike 测量：错位裁剪下的整库判别"]
    fn spike_misaligned_baseline() {
        let Some((frame, _)) = labeled_fixture() else {
            return;
        };
        let gray = frame_luma(&frame);
        let (w, h) = (frame.width() as usize, frame.height() as usize);
        let catalog = catalog();
        let library = build_library(&catalog, MEASURED_GRID.icon_side);

        // 既有几何：Calibration 先验给出的格子（真实帧上错位的那个）
        let cal = crate::loadout_sync::types::Calibration::default();
        let (_, mapping) = cal
            .resolve(frame.width(), frame.height())
            .expect("标定解析");
        let mut hit = 0usize;
        let mut n = 0usize;
        for ((row, col), key) in LABELED {
            let Some(expected_id) = catalog.resolve_item_id(key) else {
                continue;
            };
            let rect = cal.roi_to_frame(
                &mapping,
                ImageRect::new(
                    cal.list_cols[col as usize],
                    cal.list_top + row as i32 * cal.row_pitch,
                    cal.slot_size,
                    cal.slot_size,
                ),
            );
            let ranked = rank_library(&gray, w, h, &library, (rect.x, rect.y), 2);
            let Some(top) = ranked.first() else { continue };
            n += 1;
            let ok = top.0 == expected_id;
            if ok {
                hit += 1;
            }
            eprintln!(
                "r{row}c{col} 期望 {key:<22} → top1 {:<22} {:.3} {}",
                top.0,
                top.1,
                if ok { "OK" } else { "MISS" }
            );
        }
        eprintln!("== 错位裁剪整库判别 top1 {hit}/{n}（既有 probe 复现）==");
    }

    /// 测量 3：**测绘网格**（±3px 对齐）下的整库判别。
    ///
    /// 结论：即使几何对了，只有 ZNCC + 整库搜索也只能拿到 0~1/13 ——
    /// 参考实现的一致性还依赖被本 spike 省略的 category evidence。
    #[test]
    #[ignore = "spike 测量：对齐后的整库判别"]
    fn spike_aligned_discrimination() {
        let Some((frame, _)) = labeled_fixture() else {
            return;
        };
        let gray = frame_luma(&frame);
        let (w, h) = (frame.width() as usize, frame.height() as usize);
        let catalog = catalog();
        let library = build_library(&catalog, MEASURED_GRID.icon_side);

        let mut hit = 0usize;
        let mut n = 0usize;
        for ((row, col), key) in LABELED {
            let Some(expected_id) = catalog.resolve_item_id(key) else {
                continue;
            };
            let center = MEASURED_GRID.top_left(row, col);
            let ranked = rank_library(&gray, w, h, &library, center, 3);
            let Some(top) = ranked.first() else { continue };
            n += 1;
            let ok = top.0 == expected_id;
            if ok {
                hit += 1;
            }
            let expected_score = ranked
                .iter()
                .find(|(id, _)| *id == expected_id)
                .map(|(_, s)| *s)
                .unwrap_or(f32::NAN);
            eprintln!(
                "r{row}c{col} 期望 {key:<22} → top1 {:<22} {:.3} | 期望项 {expected_score:.3} {}",
                top.0,
                top.1,
                if ok { "OK" } else { "MISS" }
            );
        }
        eprintln!("== 对齐网格整库判别 top1 {hit}/{n} ==");
    }

    /// 测量 5：把 `detect_list()` 在真实帧上输出的格子画到原图上，肉眼定位几何偏移。
    ///
    /// 这是 Phase A 的证据来源：不做猜测，直接看检测框落在哪里。
    /// 绿框 = `detect_list()` 输出；红框 = 测绘出的真实图标网格（52px）。
    #[test]
    #[ignore = "spike 测量：检测框叠加图"]
    fn spike_dump_detected_rects_overlay() {
        let Some((frame, _)) = labeled_fixture() else {
            return;
        };
        let out = std::env::var("H2AC_ZNCC_OUT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("screenshots/zncc")
            });
        std::fs::create_dir_all(&out).unwrap();

        let rec = crate::loadout_sync::recognizer::GameUIRecognizer::new(Default::default());
        let geom = rec.resolve_geometry(&frame).expect("几何解析");
        eprintln!("scale={:.4} roi={:?}", geom.mapping.scale, geom.roi);
        let grid = rec.detect_list(&frame).expect("列表识别");
        eprintln!("rows={} cols={}", grid.rows, grid.cols);
        for cell in &grid.cells {
            eprintln!(
                "  r{}c{} rect=({}, {}, {}, {}) score={:.2}",
                cell.row, cell.col, cell.rect.x, cell.rect.y, cell.rect.w, cell.rect.h, cell.score
            );
        }

        let mut img = frame.rgba.clone();
        for cell in &grid.cells {
            draw_rect(&mut img, cell.rect, [0, 255, 0, 255]);
        }
        for row in 0..5u32 {
            for col in 0..4u32 {
                let (x, y) = MEASURED_GRID.top_left(row, col);
                if y + 60 > img.height() as i32 {
                    continue;
                }
                draw_rect(
                    &mut img,
                    ImageRect::new(
                        x,
                        y,
                        MEASURED_GRID.icon_side as i32,
                        MEASURED_GRID.icon_side as i32,
                    ),
                    [255, 0, 0, 255],
                );
            }
        }
        let roi = ImageRect::new(60, 340, 420, 560);
        let crop =
            image::imageops::crop_imm(&img, roi.x as u32, roi.y as u32, roi.w as u32, roi.h as u32)
                .to_image();
        let big = image::imageops::resize(
            &crop,
            crop.width() * 2,
            crop.height() * 2,
            image::imageops::FilterType::Nearest,
        );
        let path = out.join("detected_overlay.png");
        big.save(&path).unwrap();
        eprintln!("叠加图: {}", path.display());
    }

    fn draw_rect(img: &mut RgbaImage, rect: ImageRect, color: [u8; 4]) {
        let (x0, y0) = (rect.x.max(0), rect.y.max(0));
        let (x1, y1) = (
            (rect.x + rect.w - 1).min(img.width() as i32 - 1),
            (rect.y + rect.h - 1).min(img.height() as i32 - 1),
        );
        for x in x0..=x1 {
            for y in [y0, y1] {
                if y >= 0 && y < img.height() as i32 {
                    img.put_pixel(x as u32, y as u32, image::Rgba(color));
                }
            }
        }
        for y in y0..=y1 {
            for x in [x0, x1] {
                if x >= 0 && x < img.width() as i32 {
                    img.put_pixel(x as u32, y as u32, image::Rgba(color));
                }
            }
        }
    }

    /// 测量 6（Phase A1）：**按参考项目的分层**测图标 ROI，而不是全幅找峰值。
    ///
    /// 与之前那版的区别（之前那版结果顶在搜索边界，不可信）：
    /// 1. **唯一父坐标 = `detect_list()` 的卡片矩形**，不再全幅扫描；
    /// 2. 以参考 `LIST_ICON_SCALE = 68/104` 为**名义边长**，只在其邻域试几个尺寸；
    /// 3. 搜索带**限制在卡片内部**，并排除卡片边框（内缩）与下方名称条
    ///    （偏移上限只到卡片高度的 ~40%）；
    /// 4. 记录「是否触碰搜索边界」—— 触边界说明假设错，不当作结论。
    ///
    /// 验收（用户指定）：不触边界 + 偏移分布集中 + 尺寸比例集中。
    #[test]
    #[ignore = "spike 测量 A1：按参考比例的卡片内图标 ROI"]
    fn spike_measure_icon_roi_reference_scale() {
        // 参考实现：列表卡片 104 → 图标画布 68
        const REF_LIST_SCALE: f32 = 68.0 / 104.0;

        let Some((frame, _)) = labeled_fixture() else {
            return;
        };
        let gray = frame_luma(&frame);
        let (w, h) = (frame.width() as usize, frame.height() as usize);
        let rec = crate::loadout_sync::recognizer::GameUIRecognizer::new(Default::default());
        let grid = rec.detect_list(&frame).expect("列表识别");
        let catalog = catalog();
        let root = crate::vision::reference_catalog::default_root();

        let mut ratios: Vec<f32> = Vec::new();
        let mut dxs: Vec<i32> = Vec::new();
        let mut dys: Vec<i32> = Vec::new();
        let mut boundary_hits = 0usize;
        let mut samples = 0usize;

        for row in 0..grid.rows {
            for col in 0..grid.cols {
                let Some(((_, _), key)) = LABELED.iter().find(|((r, c), _)| *r == row && *c == col)
                else {
                    continue;
                };
                let Some(expected_id) = catalog.resolve_item_id(key) else {
                    continue;
                };
                let Some(entry) = catalog
                    .by_kind(CatalogItemKind::Stratagem)
                    .find(|e| e.item_id == expected_id)
                else {
                    continue;
                };
                let Ok(icon) = image::open(root.join(&entry.path)) else {
                    continue;
                };
                let Some(cell) = grid
                    .cells
                    .iter()
                    .find(|c| c.row == row && c.col == col)
                else {
                    continue;
                };
                let card = cell.rect;
                let side_min = card.w.min(card.h);

                // 搜索带：卡片内部，排除边框与下方名称条
                let inset = ((side_min as f32 * 0.04).round() as i32).max(1);
                let nominal = ((side_min as f32 * REF_LIST_SCALE).round() as i32).max(6);
                let dx_lo = -((card.w as f32 * 0.15).round() as i32);
                let dx_hi = ((card.w as f32 * 0.15).round() as i32);
                // dy 放到「图标整体仍在卡片内」的全区间：若最优压在上限，
                // 只能说明它想出去（名称条/卡片定义有问题），不能当测量值。
                let dy_lo = -((card.h as f32 * 0.15).round() as i32);
                let dy_hi = (card.h - 2 * inset - nominal).max(dy_lo);

                let mut best = (f32::NEG_INFINITY, 0i32, 0i32, 0i32);
                let sides = [
                    nominal - 6,
                    nominal - 3,
                    nominal,
                    nominal + 3,
                    nominal + 6,
                    nominal + 9,
                ];
                for side in sides {
                    if side < 6 || side > side_min {
                        continue;
                    }
                    let Some(t) = NormalizedTemplate::build(key, &icon.to_rgba8(), side as u32)
                    else {
                        continue;
                    };
                    for dy in dy_lo..=dy_hi {
                        for dx in dx_lo..=dx_hi {
                            let ox = card.x + inset + dx;
                            let oy = card.y + inset + dy;
                            if let Some(s) = zncc_at(&gray, w, h, &t, ox, oy) {
                                if s > best.0 {
                                    best = (s, dx, dy, side);
                                }
                            }
                        }
                    }
                }
                let (score, dx, dy, side) = best;
                if !score.is_finite() {
                    eprintln!("r{row}c{col} {key:<22} 无有效峰值");
                    continue;
                }
                let at_boundary =
                    dx == dx_lo || dx == dx_hi || dy == dy_lo || dy == dy_hi || side == sides[0] || side == sides[4];
                if at_boundary {
                    boundary_hits += 1;
                }
                samples += 1;
                let ratio = side as f32 / side_min as f32;
                ratios.push(ratio);
                dxs.push(dx);
                dys.push(dy);
                eprintln!(
                    "r{row}c{col} 卡片{card:?} 名义{nominal} → side={side} ratio={ratio:.3} \
                     dx={dx:+} dy={dy:+} score={score:.3}{}",
                    if at_boundary { " ⚠触边界" } else { "" }
                );
            }
        }

        let stats = |mut v: Vec<f32>| -> (f32, f32, f32) {
            if v.is_empty() {
                return (f32::NAN, f32::NAN, f32::NAN);
            }
            v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            (v[0], v[v.len() / 2], *v.last().unwrap())
        };
        let (rmin, rmed, rmax) = stats(ratios.clone());
        let (dxmin, dxmed, dxmax) = stats(dxs.iter().map(|v| *v as f32).collect());
        let (dymin, dymed, dymax) = stats(dys.iter().map(|v| *v as f32).collect());
        eprintln!(
            "== A1: 样本 {samples} | 触边界 {boundary_hits} | ratio min/中位/max = \
             {rmin:.3}/{rmed:.3}/{rmax:.3}（参考 {REF_LIST_SCALE:.3}）"
        );
        eprintln!("== dx min/中位/max = {dxmin:.0}/{dxmed:.0}/{dxmax:.0} | dy = {dymin:.0}/{dymed:.0}/{dymax:.0} ==");
        eprintln!(
            "== 验收: {} ==",
            if boundary_hits == 0 && samples >= 10 {
                "不触边界 ✅（可作为 ROI 依据）"
            } else {
                "仍触边界 ❌（不可作为 ROI 依据，需修正名义边长或搜索带）"
            }
        );
    }

    /// 测量 4：用**同一格**对照「错位」与「对齐」的峰值 —— 证明几何是可判定的差异。
    #[test]
    #[ignore = "spike 测量：几何错位 vs 对齐"]
    fn spike_alignment_is_decidable() {
        let Some((frame, _)) = labeled_fixture() else {
            return;
        };
        let gray = frame_luma(&frame);
        let (w, h) = (frame.width() as usize, frame.height() as usize);
        let catalog = catalog();
        let root = crate::vision::reference_catalog::default_root();
        let entry = catalog
            .by_kind(CatalogItemKind::Stratagem)
            .find(|e| e.item_id == "railgun")
            .expect("目录缺少 railgun");
        let icon = image::open(root.join(&entry.path)).unwrap().to_rgba8();
        let template =
            NormalizedTemplate::build("railgun", &icon, MEASURED_GRID.icon_side).unwrap();

        let cal = crate::loadout_sync::types::Calibration::default();
        let (_, mapping) = cal.resolve(frame.width(), frame.height()).unwrap();
        let wrong = cal.roi_to_frame(
            &mapping,
            ImageRect::new(
                cal.list_cols[2],
                cal.list_top + cal.row_pitch,
                cal.slot_size,
                cal.slot_size,
            ),
        );
        let right = MEASURED_GRID.top_left(1, 2);
        let a = zncc_at(&gray, w, h, &template, wrong.x, wrong.y).unwrap_or(f32::NAN);
        let b = zncc_at(&gray, w, h, &template, right.0, right.1).unwrap_or(f32::NAN);
        eprintln!(
            "railgun @错位几何({}, {}) = {a:.3} | @测绘网格{:?} = {b:.3}",
            wrong.x, wrong.y, right
        );
        assert!(
            b > a + 0.3,
            "对齐后的峰值必须显著高于错位裁剪：{b:.3} vs {a:.3}"
        );
    }
}
