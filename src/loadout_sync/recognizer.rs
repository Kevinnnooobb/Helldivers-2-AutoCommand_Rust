// 游戏 UI 识别 —— 只识别本功能需要的三种界面：Loadout Home / Stratagem List / Booster List。
//
// 设计原则（对应实施计划 §14~§17）：
//   * 标定先验只用来「限定搜索范围」，每一帧都必须实测边框线响应并通过阈值；
//     搜索不到就失败，绝不按先验坐标盲点。
//   * 多特征评分：4 个战备槽 + Booster + 布局一致性（等距）。
//   * 列表按「可滚动 Viewport」建模：行位置逐帧实测，不存在固定 Page 1/2/3。
use image::GrayImage;

use crate::loadout_sync::capture::CapturedFrame;
use crate::loadout_sync::error::LoadoutSyncError;
use crate::loadout_sync::matcher::{CellFeatures, EMPTY_INSET_RATIO};
use crate::loadout_sync::types::{
    Calibration, GameLoadoutSlots, ImageRect, ListCell, ListGrid, RoiMapping, SlotRegion, UiState,
};

/// 单个槽位可信度下限（实测真实截图 0.65~0.95，背景控制组 0.28）。
pub const MIN_SLOT_FRAME_SCORE: f32 = 0.50;
/// Booster 六边形轮廓命中率下限（实测真实截图 0.89~0.94）。
pub const MIN_BOOSTER_SCORE: f32 = 0.70;
/// Home 总分下限。
pub const MIN_HOME_SCORE: f32 = 0.50;
/// 列表行/列边线命中率下限。
pub const MIN_GRID_LINE_SCORE: f32 = 0.50;
/// 峰追踪的单行最低边线响应：比网格拟合松，容忍高亮块盖住边线的行
pub const PEAK_MIN_SIGNAL: f32 = 0.30;
/// 强边线阈值：真实列表里未被遮挡的行边线响应约为 0.9~1.0
pub const STRONG_LINE_SIGNAL: f32 = 0.75;
/// 同一条边线的最大连续像素跨度（超过则认为不是同一条线）
pub const STRONG_LINE_MAX_RUN: i32 = 40;
/// 位置搜索半径（Home）。
pub const SEARCH_RADIUS_HOME: i32 = 10;
/// 位置搜索半径（List，逐格微调）。
pub const SEARCH_RADIUS_LIST: i32 = 6;
/// 列表至少要有 2 行才算「确实打开了列表」（1 行可能是 Home 上方的任务战备行）。
pub const MIN_LIST_ROWS: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameGeom {
    pub roi: ImageRect,
    pub mapping: RoiMapping,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Recognition {
    pub state: UiState,
    pub geom: Option<FrameGeom>,
    pub slots: Option<GameLoadoutSlots>,
    pub grid: Option<ListGrid>,
    pub home_score: f32,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect {
    Any,
    Home,
    List,
}

pub struct GameUIRecognizer {
    pub calibration: Calibration,
}

impl Default for GameUIRecognizer {
    fn default() -> Self {
        Self::new(Calibration::default())
    }
}

impl GameUIRecognizer {
    pub fn new(calibration: Calibration) -> Self {
        Self { calibration }
    }

    /// 解析当前帧的 ROI 与坐标映射（非 16:9 / UI 裁剪会在这里安全失败）。
    pub fn resolve_geometry(&self, frame: &CapturedFrame) -> Result<FrameGeom, LoadoutSyncError> {
        let (roi, mapping) = self.calibration.resolve(frame.width(), frame.height())?;
        Ok(FrameGeom { roi, mapping })
    }

    // ─── Home ───

    /// 识别 Loadout Home 并给出 4 个战备槽 + Booster 槽的实测位置。
    pub fn detect_home(&self, frame: &CapturedFrame) -> Result<GameLoadoutSlots, LoadoutSyncError> {
        let geom = self.resolve_geometry(frame)?;
        let g = &frame.gray;
        let scale = geom.mapping.scale;
        let thickness = ((3.0 * scale).round() as i32).max(2);
        let (priors, booster_prior) = self.calibration.home_slot_rects();
        let size_frame = ((self.calibration.slot_size as f32) * scale).round() as i32;

        let mut regions: Vec<SlotRegion> = Vec::with_capacity(4);
        for (i, prior) in priors.iter().enumerate() {
            let target = self.calibration.roi_to_frame(&geom.mapping, *prior);
            // 调整长宽到当前缩放下应有的尺寸（先验以参考像素定义）
            let target = ImageRect::new(
                target.x,
                target.y,
                ((self.calibration.slot_size as f32) * scale).round() as i32,
                ((self.calibration.slot_size as f32) * scale).round() as i32,
            );
            match refine_frame(g, target, SEARCH_RADIUS_HOME, thickness) {
                Some((rect, score)) if score >= MIN_SLOT_FRAME_SCORE => {
                    regions.push(SlotRegion { rect, score })
                }
                // 边框可信度不足：位置无法实测确认，直接失败（绝不按先验坐标盲点）
                Some(_) | None => return Err(LoadoutSyncError::SlotNotDetected { slot: i + 1 }),
            }
        }

        // Booster：位置由「已实测的战备槽 4 + 标定偏移」推导（布局驱动，而不是固定坐标），
        // 再用六边形轮廓评分在小范围内确认。
        let hex_prior = {
            let s = size_frame;
            let slot4 = regions[3].rect;
            let dx = ((self.calibration.booster_x - self.calibration.home_cols[3]) as f32 * scale)
                .round() as i32;
            ImageRect::new(slot4.x + dx, slot4.y, s, s)
        };
        let _ = booster_prior;
        let mut booster = SlotRegion {
            rect: self.calibration.booster_hex_rect(hex_prior),
            score: 0.0,
        };
        let mut best_hex = 0.0f32;
        for dy in -(SEARCH_RADIUS_HOME / 2)..=(SEARCH_RADIUS_HOME / 2) {
            for dx in -(SEARCH_RADIUS_HOME / 2)..=(SEARCH_RADIUS_HOME / 2) {
                let probe_slot =
                    ImageRect::new(hex_prior.x + dx, hex_prior.y + dy, hex_prior.w, hex_prior.h);
                let hex = self.calibration.booster_hex_rect(probe_slot);
                if hex
                    .clamp_to_frame(g.width() as i32, g.height() as i32)
                    .is_none()
                {
                    continue;
                }
                let s = hexagon_score(g, hex, thickness);
                if s > best_hex {
                    best_hex = s;
                    booster = SlotRegion {
                        rect: hex,
                        score: s,
                    };
                }
            }
        }
        if best_hex < MIN_BOOSTER_SCORE {
            return Err(LoadoutSyncError::SlotNotDetected { slot: 5 });
        }

        // 布局一致性：4 列等距（同分辨率下游戏 UI 由同一模板绘制）
        let mut layout_ok = true;
        for pair in regions.windows(2) {
            let expected =
                (self.calibration.home_cols[1] - self.calibration.home_cols[0]) as f32 * scale;
            let actual = (pair[1].rect.x - pair[0].rect.x).abs() as f32;
            if (actual - expected).abs() > 8.0 {
                layout_ok = false;
            }
        }

        let mean_slot: f32 = regions.iter().map(|r| r.score).sum::<f32>() / regions.len() as f32;
        let layout_score = if layout_ok { 1.0 } else { 0.0 };
        let home_score = 0.6 * mean_slot + 0.2 * booster.score + 0.2 * layout_score;
        if home_score < MIN_HOME_SCORE {
            return Err(LoadoutSyncError::LoadoutHomeNotDetected { score: home_score });
        }

        Ok(GameLoadoutSlots {
            stratagems: [regions[0], regions[1], regions[2], regions[3]],
            booster,
            home_score,
        })
    }

    /// 统计 Home 上已有内容的战备槽数量（用于「已有 Loadout」策略判断）。
    ///
    /// plan6 §5.3 之后，装配前的策略判断由 `direct_select` 的 Home 观察承担；
    /// 本方法只剩底层适配回归测试的调用者。
    #[allow(dead_code)]
    pub fn count_filled_stratagems(
        &self,
        frame: &CapturedFrame,
        slots: &GameLoadoutSlots,
    ) -> usize {
        slots
            .stratagems
            .iter()
            .filter(|s| !self.cell_is_empty(frame, s.rect))
            .count()
    }

    /// 槽位是否为空。
    ///
    /// 采样区四边按 `EMPTY_INSET_RATIO`（22%）内缩：识别出的矩形可能比真实槽位大 1~3px，
    /// 内缩不足会把明亮边框切进采样区（实测空槽的相对标准差会从 0.000 跳到 0.185），
    /// 从而把空槽误判为「已有战备」并错误拒绝整次装配。
    pub fn cell_is_empty(&self, frame: &CapturedFrame, rect: ImageRect) -> bool {
        match CellFeatures::from_frame_inset(frame, rect, EMPTY_INSET_RATIO) {
            Some(f) => f.is_empty_like(),
            None => true,
        }
    }

    // ─── 列表（可滚动 Viewport） ───

    /// 识别战备/Booster 选择列表，返回当前可见的单元格网格（行位置逐帧实测）。
    pub fn detect_list(&self, frame: &CapturedFrame) -> Result<ListGrid, LoadoutSyncError> {
        let geom = self.resolve_geometry(frame)?;
        let g = &frame.gray;
        let scale = geom.mapping.scale;
        let size = ((self.calibration.slot_size as f32) * scale).round() as i32;
        let thickness = ((3.0 * scale).round() as i32).max(2);

        // 列的先验范围（用于行扫描时的横向采样带）
        let col_priors: Vec<ImageRect> = self
            .calibration
            .list_cols
            .iter()
            .map(|x| {
                self.calibration.roi_to_frame(
                    &geom.mapping,
                    ImageRect::new(*x, self.calibration.list_top, size, size),
                )
            })
            .collect();

        let rows = self.detect_rows(g, &geom, &col_priors, size)?;
        if (rows.len() as u32) < MIN_LIST_ROWS {
            return Err(LoadoutSyncError::ListNotDetected { slot: 0 });
        }

        // 列：以各行所在 y 带做竖直边线搜索
        let bands: Vec<ImageRect> = rows
            .iter()
            .map(|top| ImageRect::new(col_priors[0].x, *top, size, size))
            .collect();
        let cols = self.detect_cols(g, &geom, &bands, size);

        let mut cells: Vec<ListCell> = Vec::new();
        match cols {
            Some(cols) if cols.len() == 4 => {
                for (r, top) in rows.iter().enumerate() {
                    for (c, left) in cols.iter().enumerate() {
                        // 网格拟合只给出平均位置：每个单元格都要吸附到真实边线，
                        // 否则行列误差会累积（实测第 4 列偏差 5px，直接毁掉图标匹配与行签名）
                        let rect = snap_rect(g, ImageRect::new(*left, *top, size, size), thickness);
                        let score = frame_score(g, rect, thickness);
                        cells.push(ListCell {
                            rect,
                            row: r as u32,
                            col: c as u32,
                            score,
                        });
                    }
                }
            }
            _ => {
                // 列检测不可靠时退回先验 + 逐格实测（仍然要求边框得分达标）
                for (r, top) in rows.iter().enumerate() {
                    for (c, prior) in col_priors.iter().enumerate() {
                        let target = ImageRect::new(prior.x, *top, size, size);
                        let Some((rect, score)) =
                            refine_frame(g, target, SEARCH_RADIUS_LIST, thickness)
                        else {
                            continue;
                        };
                        if score < MIN_SLOT_FRAME_SCORE {
                            continue;
                        }
                        cells.push(ListCell {
                            rect,
                            row: r as u32,
                            col: c as u32,
                            score,
                        });
                    }
                }
                if cells.len() < 4 * MIN_LIST_ROWS as usize {
                    return Err(LoadoutSyncError::ListNotDetected { slot: 0 });
                }
            }
        }

        Ok(ListGrid::from_cells(cells))
    }

    /// 逐帧实测列表行位置。
    ///
    /// 两条路径取行数更多者：
    /// * **等距网格拟合**：合成夹具 / 行距均匀的列表最稳；
    /// * **边线峰追踪**：真实游戏列表里夹着分类标题（"补给"、"支援"…），
    ///   行距会被标题顶开，固定行距的网格只会在两个标题之间匹配到一小段
    ///   （实机实测只找到 2 行，导致滚动验证永远判定「没有位移」）。
    ///   峰追踪允许一次性跨越标题的较大间隔，因此能看到全部可见行。
    fn detect_rows(
        &self,
        g: &GrayImage,
        geom: &FrameGeom,
        col_priors: &[ImageRect],
        size: i32,
    ) -> Result<Vec<i32>, LoadoutSyncError> {
        let scale = geom.mapping.scale;
        let (_, y0) =
            self.calibration
                .roi_point(&geom.mapping, 0.0, self.calibration.list_top as f32 - 12.0);
        let (_, y1) = self.calibration.roi_point(
            &geom.mapping,
            0.0,
            self.calibration.list_bottom as f32 + 12.0,
        );
        let from = (y0.round() as i32).max(0);
        let to = (y1.round() as i32).min(g.height() as i32 - 2);
        if to - from < size {
            return Err(LoadoutSyncError::ListNotDetected { slot: 0 });
        }
        let thickness = ((3.0 * scale).round() as i32).max(2);
        // 预计算每一行的上边线命中率（跨 4 列的横向采样）
        let profile: Vec<f32> = (from..=to)
            .map(|y| row_line_score(g, y, col_priors, size, thickness))
            .collect();
        let signal = |y: i32| -> f32 {
            if y < from || y + size > to {
                0.0
            } else {
                profile[(y - from) as usize].min(profile[(y + size - from) as usize])
            }
        };

        let expected_pitch = self.calibration.row_pitch as f32 * scale;
        // 首选：明确的强边线聚类。真实列表里行边线的响应是双峰的（≈1.0 与 ≈0.5），
        // 强边线聚类不会把高亮块内部的弱边缘误当成一行，因此比峰追踪更准。
        let strong_rows = cluster_strong_lines(&signal, from, to, size, expected_pitch);
        let mut rows = if strong_rows.len() >= MIN_LIST_ROWS as usize {
            strong_rows
        } else {
            // 兜底：峰追踪（容忍弱边线）→ 等距网格（行距均匀的合成夹具）
            let peak_rows = track_row_peaks(&signal, from, to, size, expected_pitch);
            let grid_rows = fit_row_grid(&signal, from, to, size, expected_pitch);
            if peak_rows.len() >= grid_rows.len() {
                peak_rows
            } else {
                grid_rows
            }
        };
        rows.sort_unstable();
        rows.dedup();
        if rows.is_empty() {
            return Err(LoadoutSyncError::ListNotDetected { slot: 0 });
        }
        Ok(rows)
    }

    /// 逐帧实测列位置（期望等距），返回 None 表示检测不可靠。
    fn detect_cols(
        &self,
        g: &GrayImage,
        geom: &FrameGeom,
        row_bands: &[ImageRect],
        size: i32,
    ) -> Option<Vec<i32>> {
        if row_bands.is_empty() {
            return None;
        }
        let scale = geom.mapping.scale;
        let x_from = (row_bands[0].x - (16.0 * scale) as i32).max(0);
        let x_to = (row_bands[0].x
            + size
            + (self.calibration.row_pitch as f32 * scale) as i32 * 3
            + (16.0 * scale) as i32)
            .min(g.width() as i32 - 2);
        if x_to - x_from < size {
            return None;
        }
        let thickness = ((3.0 * scale).round() as i32).max(2);
        let profile: Vec<f32> = (x_from..=x_to)
            .map(|x| col_line_score(g, x, row_bands, size, thickness))
            .collect();
        let at = |x: i32| -> f32 {
            if x < x_from || x > x_to {
                0.0
            } else {
                profile[(x - x_from) as usize]
            }
        };
        let expected_pitch =
            (self.calibration.list_cols[1] - self.calibration.list_cols[0]) as f32 * scale;
        let pitch_lo = (expected_pitch * 0.85).round() as i32;
        let pitch_hi = (expected_pitch * 1.20).round() as i32;
        let mut best: Option<(i32, i32, f32)> = None;
        for pitch in pitch_lo.max(size).max(8)..=pitch_hi.max(size + 1) {
            for left in x_from..=(x_to - size).max(x_from) {
                let mut sum = 0.0f32;
                let mut count = 0;
                for k in 0..4 {
                    let x = left + k * pitch;
                    let l = at(x);
                    let r = at(x + size);
                    if l < MIN_GRID_LINE_SCORE || r < MIN_GRID_LINE_SCORE {
                        break;
                    }
                    sum += l.min(r);
                    count += 1;
                }
                if count < 4 {
                    continue;
                }
                let score = sum / count as f32;
                if best.map(|(_, _, s)| score > s).unwrap_or(true) {
                    best = Some((left, pitch, score));
                }
            }
        }
        let (left, pitch, _) = best?;
        Some((0..4).map(|k| left + k * pitch).collect())
    }

    /// 自上而下识别当前是哪一种界面。
    pub fn recognize(&self, frame: &CapturedFrame, expect: Expect) -> Recognition {
        let mut notes = Vec::new();
        let geom = match self.resolve_geometry(frame) {
            Ok(g) => Some(g),
            Err(e) => {
                notes.push(e.message());
                return Recognition {
                    state: UiState::Unknown,
                    geom: None,
                    slots: None,
                    grid: None,
                    home_score: 0.0,
                    notes,
                };
            }
        };
        let home = self.detect_home(frame).ok();
        let home_score = home.as_ref().map(|h| h.home_score).unwrap_or(0.0);
        if let Some(h) = home {
            if expect != Expect::List {
                return Recognition {
                    state: UiState::LoadoutHome,
                    geom,
                    slots: Some(h),
                    grid: None,
                    home_score,
                    notes,
                };
            }
        }
        if expect != Expect::Home {
            if let Ok(grid) = self.detect_list(frame) {
                let state = if home_score >= MIN_HOME_SCORE {
                    // Home 仍然成立时，把「列表」判定降级：宁可重试点击，也不能误点。
                    notes.push("Home 与 List 特征同时成立，按 Home 处理".into());
                    UiState::LoadoutHome
                } else {
                    UiState::StratagemList
                };
                return Recognition {
                    state,
                    geom,
                    slots: None,
                    grid: Some(grid),
                    home_score,
                    notes,
                };
            }
        }
        notes.push("未识别到本功能需要的界面".into());
        Recognition {
            state: UiState::Unknown,
            geom,
            slots: None,
            grid: None,
            home_score,
            notes,
        }
    }
}

// ─── 像素级边线响应 ───
//
// 采用「梯度强度」而不是「比两侧更亮」：游戏槽位边框是浅色细线且外侧可能是
// 明亮的背景艺术（危险条纹/舱内高光），单纯比较亮度的线响应在 720p 下会被
// 抗锯齿宽度直接压到阈值以下（实测左/右边命中率为 0）。
// 梯度强度只看「这里有没有一条边」，对亮度极性与边框粗细都不敏感。

/// 边线梯度阈值（归一化亮度差）。
pub const EDGE_THRESHOLD: f32 = 0.12;

fn luma_at(g: &GrayImage, x: i32, y: i32) -> f32 {
    if x < 0 || y < 0 || x >= g.width() as i32 || y >= g.height() as i32 {
        return 0.0;
    }
    g.get_pixel(x as u32, y as u32)[0] as f32 / 255.0
}

/// 竖直方向梯度（用于水平边线）：在 ±probe 窗口内取最强的上下亮度差。
fn h_edge_strength(g: &GrayImage, x: i32, y: i32, probe: i32) -> f32 {
    let mut best = 0.0f32;
    for t in -probe..=probe {
        let a = luma_at(g, x, y + t - 1);
        let b = luma_at(g, x, y + t + 1);
        best = best.max((b - a).abs());
    }
    best
}

/// 水平方向梯度（用于竖直边线）。
fn v_edge_strength(g: &GrayImage, x: i32, y: i32, probe: i32) -> f32 {
    let mut best = 0.0f32;
    for t in -probe..=probe {
        let a = luma_at(g, x + t - 1, y);
        let b = luma_at(g, x + t + 1, y);
        best = best.max((b - a).abs());
    }
    best
}

/// 一个矩形槽位的边框可信度：上/下/左/右四条边的边线命中率均值。
///
/// * 上/下边：中间 80% 宽度；
/// * 左/右边：上下各 35% 高度（跳过游戏 UI 在边线中段留的缺口）。
pub fn frame_score(g: &GrayImage, r: ImageRect, probe: i32) -> f32 {
    if !r.is_valid() {
        return 0.0;
    }
    let probe = probe.max(1);
    let inset_x = (r.w as f32 * 0.10).round() as i32;
    let inset_y = (r.h as f32 * 0.15).round() as i32;
    let step = (r.w as f32 / 24.0).round().max(1.0) as i32;

    let top = ratio(
        |i| {
            let x = r.x + inset_x + i * step;
            h_edge_strength(g, x, r.y, probe)
        },
        (r.w - 2 * inset_x) / step.max(1),
    );

    let bottom = ratio(
        |i| {
            let x = r.x + inset_x + i * step;
            h_edge_strength(g, x, r.bottom() - 1, probe)
        },
        (r.w - 2 * inset_x) / step.max(1),
    );

    let vstep = (r.h as f32 / 16.0).round().max(1.0) as i32;
    let band_h = ((r.h as f32) * 0.35).round() as i32;
    let m = (band_h / vstep.max(1)).max(1);
    let bands = [r.y + inset_y, r.bottom() - inset_y - band_h];

    let left = ratio(
        |i| {
            let y = bands[(i as usize) / (m as usize).max(1)] + (i % m) * vstep;
            v_edge_strength(g, r.x, y, probe)
        },
        2 * m,
    );

    let right = ratio(
        |i| {
            let y = bands[(i as usize) / (m as usize).max(1)] + (i % m) * vstep;
            v_edge_strength(g, r.right() - 1, y, probe)
        },
        2 * m,
    );

    (top + bottom + left + right) / 4.0
}

/// 命中率：n 个采样点中边线强度超过阈值的比例。
fn ratio<F: Fn(i32) -> f32>(sample: F, n: i32) -> f32 {
    let n = n.max(1);
    let mut ok = 0usize;
    for i in 0..n {
        if sample(i) > EDGE_THRESHOLD {
            ok += 1;
        }
    }
    ok as f32 / n as f32
}

/// Booster 六边形轮廓评分：沿六边形 6 条边采样，统计存在边线的比例。
///
/// 与矩形槽位不同，六边形左右两端是「顶点」，用矩形边线采样必然失败，
/// 因此这里直接按几何轮廓取点（对应实施计划 §16 的槽位形状差异处理）。
pub fn hexagon_score(g: &GrayImage, hex: ImageRect, probe: i32) -> f32 {
    if !hex.is_valid() {
        return 0.0;
    }
    let probe = probe.max(1);
    let (cx, cy) = hex.center();
    let rx = hex.w as f32 / 2.0;
    let ry = hex.h as f32 / 2.0;
    let mut verts: [(f32, f32); 6] = [(0.0, 0.0); 6];
    for (k, v) in verts.iter_mut().enumerate() {
        let a = std::f32::consts::PI / 3.0 * k as f32;
        *v = (cx as f32 + rx * a.cos(), cy as f32 + ry * a.sin());
    }
    const PER_EDGE: i32 = 12;
    let mut total = 0usize;
    let mut hit = 0usize;
    for k in 0..6usize {
        let (x0, y0) = verts[k];
        let (x1, y1) = verts[(k + 1) % 6];
        for i in 0..PER_EDGE {
            let t = (i as f32 + 0.5) / PER_EDGE as f32;
            let px = (x0 + (x1 - x0) * t).round() as i32;
            let py = (y0 + (y1 - y0) * t).round() as i32;
            total += 1;
            let s = v_edge_strength(g, px, py, probe).max(h_edge_strength(g, px, py, probe));
            if s > EDGE_THRESHOLD {
                hit += 1;
            }
        }
    }
    if total == 0 {
        0.0
    } else {
        hit as f32 / total as f32
    }
}

/// 边缘吸附：把粗定位得到的矩形四条边分别吸附到真实边线（±3px 内精修）。
///
/// 为什么必须做：粗搜索用 ±probe 的梯度窗口换取鲁棒性，代价是位置可能整体偏移 3~5px。
/// 实测该偏移会把同一图标的匹配分从 0.895 压到 0.566，并让相邻帧的行签名 landmark 对不上
/// （滚动位移判定的基础）。吸附后每格位置误差在 1px 内。
pub fn snap_rect(g: &GrayImage, r: ImageRect, _probe: i32) -> ImageRect {
    if !r.is_valid() {
        return r;
    }
    // 精修用固定的小探针：大探针（粗搜索）会让 2~3px 的边框平台全部命中，失去精度
    let probe = 1;
    let inset_x = (r.w as f32 * 0.10).round() as i32;
    let step = (r.w as f32 / 24.0).round().max(1.0) as i32;
    let n_x = ((r.w - 2 * inset_x) / step.max(1)).max(1);
    let mean_h = |y: i32| -> f32 {
        let mut s = 0.0f32;
        for i in 0..n_x {
            s += h_edge_strength(g, r.x + inset_x + i * step, y, probe);
        }
        s / n_x as f32
    };
    let inset_y = (r.h as f32 * 0.15).round() as i32;
    let vstep = (r.h as f32 / 16.0).round().max(1.0) as i32;
    let band_h = ((r.h as f32) * 0.35).round() as i32;
    let m = (band_h / vstep.max(1)).max(1);
    let mean_v = |x: i32| -> f32 {
        let mut s = 0.0f32;
        let mut c = 0;
        for band_top in [r.y + inset_y, r.bottom() - inset_y - band_h] {
            for i in 0..m {
                s += v_edge_strength(g, x, band_top + i * vstep, probe);
                c += 1;
            }
        }
        if c == 0 {
            0.0
        } else {
            s / c as f32
        }
    };
    // 单条边无法定位：相邻槽位之间的间隙只有 2~4px，梯度在「本格右边界 + 间隙 + 下一格左边界」
    // 上是连续平台（实测整段强度相同，argmax/重心都会落在中间）。
    // 因此对 (左,右) 与 (上,下) 成对搜索，用边长把两条边绑定在一起。
    let snap_pair = |base_pos: i32, base_size: i32, f: &dyn Fn(i32) -> f32| -> (i32, i32) {
        let mut best = (base_pos, base_size.max(4));
        let mut best_score = f32::MIN;
        for ds in -2..=2 {
            let size = (base_size + ds).max(4);
            for d in -5..=5 {
                let pos = base_pos + d;
                let s = f(pos) + f(pos + size - 1);
                if s > best_score {
                    best_score = s;
                    best = (pos, size);
                }
            }
        }
        best
    };
    let (left, w) = snap_pair(r.x, r.w, &mean_v);
    let (top, h) = snap_pair(r.y, r.h, &mean_h);
    let snapped = ImageRect::new(left, top, w, h);
    // 合理性检查：吸附幅度不能过大（避免吸到相邻槽位的边线）
    if (snapped.w - r.w).abs() > 4 || (snapped.h - r.h).abs() > 4 {
        return r;
    }
    snapped
}

/// 在先验位置附近搜索边框响应最高的矩形（先验只作为搜索中心）。
///
/// 按「离先验的距离」由近及远推进：距离相同时优先靠近先验的候选，
/// 避免在背景艺术的高梯度区域里爬到错误的局部极值。
pub fn refine_frame(
    g: &GrayImage,
    prior: ImageRect,
    search: i32,
    probe: i32,
) -> Option<(ImageRect, f32)> {
    if !prior.is_valid() {
        return None;
    }
    let mut offsets: Vec<(i32, i32)> = Vec::new();
    for dy in -search..=search {
        for dx in -search..=search {
            offsets.push((dx, dy));
        }
    }
    offsets.sort_by_key(|(dx, dy)| dx * dx + dy * dy);
    let mut best: Option<(ImageRect, f32)> = None;
    for (dx, dy) in offsets {
        let cand = ImageRect::new(prior.x + dx, prior.y + dy, prior.w, prior.h);
        if cand
            .clamp_to_frame(g.width() as i32, g.height() as i32)
            .is_none()
        {
            continue;
        }
        // 粗定位（容忍偏移）→ 边缘吸附（精确位置）→ 用吸附后的位置评分
        let snapped = snap_rect(g, cand, probe);
        if snapped
            .clamp_to_frame(g.width() as i32, g.height() as i32)
            .is_none()
        {
            continue;
        }
        let mut s = frame_score(g, snapped, probe);
        // 距离惩罚：同样分数时优先相信标定先验
        let dist = ((dx * dx + dy * dy) as f32).sqrt();
        s -= 0.002 * dist;
        if best.map(|(_, bs)| s > bs).unwrap_or(true) {
            best = Some((snapped, s));
        }
        if s >= 0.90 {
            break;
        }
    }
    best
}

/// 一行上边线的命中率（跨 4 列的横向采样）。
/// 等距网格拟合：在期望行距附近做二维搜索，最大化各行上下边线响应之和。
pub(crate) fn fit_row_grid(
    signal: &dyn Fn(i32) -> f32,
    from: i32,
    to: i32,
    size: i32,
    expected_pitch: f32,
) -> Vec<i32> {
    let pitch_lo = (expected_pitch * 0.85).round() as i32;
    let pitch_hi = (expected_pitch * 1.20).round() as i32;
    let max_rows = 12usize;
    let mut best: Option<(i32, i32, f32, usize)> = None;

    for pitch in pitch_lo.max(size).max(8)..=pitch_hi.max(size + 1) {
        for top in from..=(to - size).max(from) {
            let mut sum = 0.0f32;
            let mut count = 0usize;
            for k in 0..max_rows {
                let y = top + (k as i32) * pitch;
                if y + size > to {
                    break;
                }
                let s = signal(y);
                if s < MIN_GRID_LINE_SCORE {
                    break;
                }
                sum += s;
                count += 1;
            }
            if count == 0 {
                continue;
            }
            let score = sum / count as f32 + 0.05 * count as f32;
            if best.map(|(_, _, s, _)| score > s).unwrap_or(true) {
                best = Some((top, pitch, score, count));
            }
        }
    }

    let Some((top, pitch, _, count)) = best else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    for k in 0..count {
        let base = top + (k as i32) * pitch;
        if let Some(y) = snap_to_line(signal, base, size) {
            rows.push(y);
        }
    }
    rows
}

/// 强边线聚类：把响应接近饱和（`STRONG_LINE_SIGNAL`）的相邻像素合成一条行边线，
/// 再按行距合并重复边线。
///
/// 真实游戏列表的行边线响应呈「强边线 + 被高亮盖住的弱边线」双峰分布，
/// 直接用弱阈值会把高亮块内部的边缘也算成行，因此先用强边线定骨架。
pub(crate) fn cluster_strong_lines(
    signal: &dyn Fn(i32) -> f32,
    from: i32,
    to: i32,
    size: i32,
    expected_pitch: f32,
) -> Vec<i32> {
    let last = to - size;
    if last < from {
        return Vec::new();
    }
    let mut clusters: Vec<(i32, f32)> = Vec::new();
    let mut run: Option<(i32, i32, f32)> = None; // (start, best_y, best_s)
    for y in from..=last {
        let s = signal(y);
        if s >= STRONG_LINE_SIGNAL {
            run = match run {
                Some((start, _best_y, best_s)) if s > best_s => Some((start, y, s)),
                Some(other) => Some(other),
                None => Some((y, y, s)),
            };
        } else if let Some((start, best_y, best_s)) = run.take() {
            if y - start <= STRONG_LINE_MAX_RUN {
                clusters.push((best_y, best_s));
            }
        }
    }
    if let Some((start, best_y, best_s)) = run {
        if last + 1 - start <= STRONG_LINE_MAX_RUN {
            clusters.push((best_y, best_s));
        }
    }

    // 行距太近的边线属于同一行（例如上下边框都被判成边线）：保留更强者
    let min_gap = (expected_pitch * 0.70).round().max(8.0) as i32;
    let mut rows: Vec<(i32, f32)> = Vec::new();
    for (y, s) in clusters {
        match rows.last_mut() {
            Some((last_y, last_s)) if (y - *last_y) < min_gap => {
                if s > *last_s {
                    *last_y = y;
                    *last_s = s;
                }
            }
            _ => rows.push((y, s)),
        }
    }
    rows.into_iter().map(|(y, _)| y).collect()
}

/// 边线峰追踪：以最强峰为锚点向两侧延伸。
///
/// 与等距网格不同，这里允许两种间隔：
/// * 正常行距窗口（±25%）—— 取窗口内响应最强的候选；
/// * 跨标题窗口（1.35~2.6 倍行距）—— 分类标题会把行距顶开，但标题边界本身响应较强。
///
/// 实机列表里被选中/高亮的整块行、以及被高亮盖住的边线，响应会明显弱于普通行，
/// 因此候选门槛取得比网格拟合更松（见 `PEAK_MIN_SIGNAL`），否则会整行漏掉。
pub(crate) fn track_row_peaks(
    signal: &dyn Fn(i32) -> f32,
    from: i32,
    to: i32,
    size: i32,
    expected_pitch: f32,
) -> Vec<i32> {
    let pitch = expected_pitch.max(8.0);
    let last = to - size;

    // 锚点 = 全范围最强的一条行边线
    let mut strongest = 0.0f32;
    let mut anchor = from;
    let mut y = from;
    while y <= last {
        let s = signal(y);
        if s > strongest {
            strongest = s;
            anchor = y;
        }
        y += 1;
    }
    if strongest < PEAK_MIN_SIGNAL {
        return Vec::new();
    }

    // 候选窗口按优先级排列：正常行距的强边线 → 正常行距的弱边线（被高亮盖住的行）
    // → 跨分类标题的大间隔（标题会把行距顶开）
    let windows: [(f32, f32, f32); 3] = [
        (0.75, 1.30, MIN_GRID_LINE_SCORE),
        (0.75, 1.30, PEAK_MIN_SIGNAL),
        (1.30, 2.60, MIN_GRID_LINE_SCORE),
    ];

    let mut rows = vec![anchor];
    for dir in [1i32, -1i32] {
        let mut current = anchor;
        loop {
            let mut advanced = false;
            'window: for (lo, hi, min_signal) in windows {
                let a = current + dir * (pitch * lo).round() as i32;
                let b = current + dir * (pitch * hi).round() as i32;
                let (start, end) = if dir > 0 { (a, b) } else { (b, a) };
                // 窗口内候选按信号从强到弱
                let mut candidates: Vec<(i32, f32)> = (start.max(from)..=end.min(last))
                    .map(|y| (y, signal(y)))
                    .filter(|(_, s)| *s >= min_signal)
                    .collect();
                candidates
                    .sort_by(|x, z| z.1.partial_cmp(&x.1).unwrap_or(std::cmp::Ordering::Equal));
                for (y, s) in candidates {
                    let Some(snapped) = snap_to_line_relaxed(signal, y, s.max(min_signal)) else {
                        continue;
                    };
                    if rows.contains(&snapped)
                        || rows
                            .iter()
                            .any(|r| ((snapped - *r).abs() as f32) < pitch * 0.75)
                    {
                        continue;
                    }
                    rows.push(snapped);
                    current = snapped;
                    advanced = true;
                    break 'window;
                }
            }
            if !advanced {
                break;
            }
        }
    }
    rows.sort_unstable();
    rows
}

/// 把候选行吸附到 ±4px 内边线响应最强处；最强响应不得低于 `min_signal`。
pub(crate) fn snap_to_line_relaxed(
    signal: &dyn Fn(i32) -> f32,
    base: i32,
    min_signal: f32,
) -> Option<i32> {
    let mut best_y = base;
    let mut best_s = 0.0f32;
    for dy in -4..=4 {
        let s = signal(base + dy);
        if s > best_s {
            best_s = s;
            best_y = base + dy;
        }
    }
    (best_s >= min_signal).then_some(best_y)
}

/// 把候选行吸附到 ±4px 内边线响应最强的位置。
pub(crate) fn snap_to_line(signal: &dyn Fn(i32) -> f32, base: i32, _size: i32) -> Option<i32> {
    let mut best_y = base;
    let mut best_s = 0.0f32;
    for dy in -4..=4 {
        let y = base + dy;
        let s = signal(y);
        if s > best_s {
            best_s = s;
            best_y = y;
        }
    }
    (best_s >= MIN_GRID_LINE_SCORE).then_some(best_y)
}

pub(crate) fn row_line_score(
    g: &GrayImage,
    y: i32,
    col_priors: &[ImageRect],
    size: i32,
    probe: i32,
) -> f32 {
    let step = (size as f32 / 16.0).round().max(1.0) as i32;
    let inset = (size as f32 * 0.10).round() as i32;
    let n = ((size - 2 * inset) / step).max(1);
    let mut total = 0usize;
    let mut ok = 0usize;
    for band in col_priors {
        for i in 0..n {
            let x = band.x + inset + i * step;
            total += 1;
            if h_edge_strength(g, x, y, probe) > EDGE_THRESHOLD {
                ok += 1;
            }
        }
    }
    if total == 0 {
        0.0
    } else {
        ok as f32 / total as f32
    }
}

/// 一列左边线的命中率（跨若干行、跳过中段缺口的竖直采样）。
fn col_line_score(g: &GrayImage, x: i32, row_bands: &[ImageRect], size: i32, probe: i32) -> f32 {
    let step = (size as f32 / 12.0).round().max(1.0) as i32;
    let band_h = ((size as f32) * 0.35).round() as i32;
    let inset_y = (size as f32 * 0.15).round() as i32;
    let n = (band_h / step).max(1);
    let mut total = 0usize;
    let mut ok = 0usize;
    for band in row_bands {
        for band_top in [band.y + inset_y, band.bottom() - inset_y - band_h] {
            for i in 0..n {
                let y = band_top + i * step;
                total += 1;
                if v_edge_strength(g, x, y, probe) > EDGE_THRESHOLD {
                    ok += 1;
                }
            }
        }
    }
    if total == 0 {
        0.0
    } else {
        ok as f32 / total as f32
    }
}
