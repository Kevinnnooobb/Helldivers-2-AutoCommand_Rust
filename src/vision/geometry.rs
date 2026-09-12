// 几何检测 —— 参考实现 hd2-preset-helper 的「固定锚点 + 边线响应验证」口径。
//
// 与旧自适应搜索的区别（这是本次几何重写的核心）：
//   * 候选位置**固定**：列由标定给出（home 11/124/237/350、列表 77/190/304/417），
//     行只在标定的 y 区间内逐像素打分 —— 不做盲搜、不做逐格吸附；
//   * 每个候选位置都要**验证**：用积分图上的「三带亮度窗口」在上下左右四条边测响应，
//     再乘上边框一致性系数（36 段亮度的相对离散度），分数不达标直接判为无效候选；
//   * 列表行用 DP 在「最小行距」硬约束下整体最优（不是逐行贪心，也不会被分类标题顶歪）。
//
// 坐标系：全部计算都在 **canonical ROI**（默认 576×832）上进行，
// 检测结果直接就是 ROI 参考坐标，再由 `FrameGeometry::roi_rect` 映射到帧像素。
use image::{GrayImage, RgbaImage};

use crate::loadout_sync::types::ImageRect;

use super::config::{GeometryConfig, SegmentationConfig};
use super::segment::chroma_likeness;

/// 检测出来的槽位类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectedKind {
    Stratagem,
    Booster,
}

impl DetectedKind {
    /// 仅测试与诊断使用的别名（等价于 `SlotKind::is_booster`）。
    #[cfg(test)]
    pub fn is_booster(self) -> bool {
        matches!(self, Self::Booster)
    }
}

/// 一个通过验证的槽位（坐标为 ROI 参考像素）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DetectedSlot {
    pub row: u32,
    pub col: u32,
    pub kind: DetectedKind,
    pub rect: ImageRect,
    /// 边线响应 × 边框一致性（0~1）
    pub score: f32,
    /// 内容检查结果（home 用：中心区域相对标准差是否达到「有内容」门槛）。
    /// 只作为交叉验证信息，最终空/非空由分割层决定。
    pub content: bool,
}

/// 一次几何检测的结果。
#[derive(Debug, Clone, PartialEq)]
pub struct GeometryDetection {
    /// 是否检测到 home 版面
    pub home: bool,
    /// 通过验证的行数（0 = 未通过验证）
    pub rows: usize,
    pub cols: u32,
    pub slots: Vec<DetectedSlot>,
    /// 最强行分数（日志与阈值诊断用）
    pub best_row_score: f32,
}

impl GeometryDetection {
    pub fn verified(&self) -> bool {
        self.rows > 0 && !self.slots.is_empty()
    }

    /// Booster 槽（仅测试与诊断使用）。
    #[cfg(test)]
    pub fn booster(&self) -> Option<&DetectedSlot> {
        self.slots.iter().find(|s| s.kind.is_booster())
    }

    pub fn empty(cols: u32) -> Self {
        Self {
            home: false,
            rows: 0,
            cols,
            slots: Vec::new(),
            best_row_score: 0.0,
        }
    }
}

/// home 版面：固定 4 列 + Booster（参考实现 `detect_home`）。
pub fn detect_home(
    roi: &RgbaImage,
    cfg: &GeometryConfig,
    seg: &SegmentationConfig,
) -> GeometryDetection {
    let canonical = canonical_rgba(roi, cfg);
    let luma = luma_of(&canonical);
    let integral = IntegralImage::from_luma(&luma);
    let profile = Profile {
        cols: cfg.home_cols.to_vec(),
        y_min: cfg.home_y_min,
        y_max: cfg.home_y_max,
        max_rows: cfg.home_max_rows,
        min_slots: cfg.home_min_slots,
    };
    let lookup = ProfileLookup::new(&integral, &profile, cfg);
    let rows = scan_profile(&lookup, &integral, &profile, cfg);
    let Some(row) = rows.first() else {
        return GeometryDetection::empty(cfg.home_cols.len() as u32);
    };
    if row.slots.len() < cfg.home_min_slots {
        return GeometryDetection::empty(cfg.home_cols.len() as u32);
    }

    let mut slots: Vec<DetectedSlot> = row
        .slots
        .iter()
        .map(|candidate| DetectedSlot {
            row: 0,
            col: candidate.col,
            kind: DetectedKind::Stratagem,
            rect: slot_rect(cfg.home_cols[candidate.col as usize], row.y, cfg),
            score: candidate.score,
            content: slot_has_content(&integral, candidate.x, row.y, cfg),
        })
        .collect();

    // Booster：位置固定（参考实现 HOME_BOOSTER_X），类别由黄色占比决定
    let booster_rect = slot_rect(cfg.home_booster_x, row.y, cfg);
    let booster_score = score_slot(&lookup, &integral, cfg.home_booster_x, row.y, cfg)
        .map(|s| s.score)
        .unwrap_or(0.0);
    slots.push(DetectedSlot {
        row: 0,
        col: slots.len() as u32,
        kind: DetectedKind::Booster,
        rect: booster_rect,
        score: booster_score,
        content: booster_has_content(&canonical, booster_rect, cfg, seg),
    });

    GeometryDetection {
        home: true,
        rows: 1,
        cols: cfg.home_cols.len() as u32,
        slots,
        best_row_score: row.score,
    }
}

/// 列表版面：固定 4 列 + DP 选出的可见行（参考实现 `detect_list`）。
pub fn detect_list(roi: &RgbaImage, cfg: &GeometryConfig) -> GeometryDetection {
    let canonical = canonical_rgba(roi, cfg);
    let luma = luma_of(&canonical);
    let integral = IntegralImage::from_luma(&luma);
    let profile = Profile {
        cols: cfg.list_cols.to_vec(),
        y_min: cfg.list_y_min,
        y_max: cfg.list_y_max,
        max_rows: cfg.list_max_rows,
        min_slots: cfg.list_min_slots,
    };
    let lookup = ProfileLookup::new(&integral, &profile, cfg);
    let rows = scan_profile(&lookup, &integral, &profile, cfg);
    if rows.is_empty() {
        return GeometryDetection::empty(cfg.list_cols.len() as u32);
    }

    let mut slots = Vec::new();
    for (row_index, row) in rows.iter().enumerate() {
        for candidate in &row.slots {
            slots.push(DetectedSlot {
                row: row_index as u32,
                col: candidate.col,
                kind: DetectedKind::Stratagem,
                rect: slot_rect(cfg.list_cols[candidate.col as usize], row.y, cfg),
                score: candidate.score,
                content: true,
            });
        }
    }
    GeometryDetection {
        home: false,
        rows: rows.len(),
        cols: cfg.list_cols.len() as u32,
        best_row_score: rows.iter().map(|r| r.score).fold(0.0, f32::max),
        slots,
    }
}

/// Booster 自身是否已装备（黄色占比，参考实现口径）。
pub fn booster_has_content(
    canonical: &RgbaImage,
    rect: ImageRect,
    cfg: &GeometryConfig,
    seg: &SegmentationConfig,
) -> bool {
    let mut yellow = 0u32;
    let mut mask_pixels = 0u32;
    for local_y in 0..rect.h {
        for local_x in booster_hex_row_span(local_y, rect.w, rect.h, cfg) {
            let x = rect.x + local_x;
            let y = rect.y + local_y;
            if x < 0 || y < 0 || x >= canonical.width() as i32 || y >= canonical.height() as i32 {
                continue;
            }
            mask_pixels += 1;
            let p = canonical.get_pixel(x as u32, y as u32).0;
            if chroma_likeness(p[0], p[1], p[2], &seg.booster_yellow)
                >= seg.booster_yellow.min_likeness
            {
                yellow += 1;
            }
        }
    }
    mask_pixels > 0 && yellow as f32 >= mask_pixels as f32 * cfg.booster_min_yellow_ratio
}

/// 六边形内容区在给定行上的水平跨度（参考实现 `booster_hex_row_span`）。
fn booster_hex_row_span(
    local_y: i32,
    width: i32,
    height: i32,
    cfg: &GeometryConfig,
) -> std::ops::Range<i32> {
    const SQRT_3: f32 = 1.732_050_8;
    let y = (local_y as f32 + 0.5) / height.max(1) as f32;
    let dy = (y - cfg.booster_hex_center_y).abs();
    if dy > 0.5 * SQRT_3 * cfg.booster_content_side_len {
        return 0..0;
    }
    let half_width = cfg.booster_content_side_len - dy / SQRT_3;
    let width_f = width as f32;
    let left = ((cfg.booster_hex_center_x - half_width) * width_f)
        .floor()
        .clamp(0.0, width_f) as i32;
    let right = ((cfg.booster_hex_center_x + half_width) * width_f)
        .ceil()
        .clamp(0.0, width_f) as i32;
    left..right
}

/// 参考坐标槽位矩形（canonical ROI 坐标）。
fn slot_rect(x: i32, y: i32, cfg: &GeometryConfig) -> ImageRect {
    ImageRect::new(x, y, cfg.slot_size, cfg.slot_size)
}

// ─── canonical 化 ───

/// 把 ROI 图像缩放到 canonical 尺寸（等价于参考实现的「canonical luma」）。
///
/// 缩小用面积平均（避免混叠），放大用双线性；参考实现用 Lanczos3，
/// 这里不引入 fast_image_resize：面积/双线性在这两个尺度上的差异远小于阈值间隔。
pub fn canonical_rgba(roi: &RgbaImage, cfg: &GeometryConfig) -> RgbaImage {
    let (dw, dh) = (cfg.canonical_w.max(1), cfg.canonical_h.max(1));
    let (sw, sh) = (roi.width(), roi.height());
    if sw == dw && sh == dh {
        return roi.clone();
    }
    if sw == 0 || sh == 0 {
        return RgbaImage::new(dw, dh);
    }
    let scale_x = sw as f32 / dw as f32;
    let scale_y = sh as f32 / dh as f32;
    let mut out = RgbaImage::new(dw, dh);
    if scale_x >= 1.0 && scale_y >= 1.0 {
        for y in 0..dh {
            for x in 0..dw {
                let x0 = x as f32 * scale_x;
                let x1 = (x + 1) as f32 * scale_x;
                let y0 = y as f32 * scale_y;
                let y1 = (y + 1) as f32 * scale_y;
                let ix0 = x0.floor().max(0.0) as u32;
                let ix1 = (x1.ceil() as u32).min(sw);
                let iy0 = y0.floor().max(0.0) as u32;
                let iy1 = (y1.ceil() as u32).min(sh);
                let mut acc = [0.0f32; 4];
                let mut weight_sum = 0.0f32;
                for sy in iy0..iy1.max(iy0 + 1).min(sh) {
                    let wy = ((sy as f32 + 1.0).min(y1) - (sy as f32).max(y0)).max(0.0);
                    for sx in ix0..ix1.max(ix0 + 1).min(sw) {
                        let wx = ((sx as f32 + 1.0).min(x1) - (sx as f32).max(x0)).max(0.0);
                        let w = wx * wy;
                        if w <= 0.0 {
                            continue;
                        }
                        let p = roi.get_pixel(sx, sy).0;
                        for c in 0..4 {
                            acc[c] += p[c] as f32 * w;
                        }
                        weight_sum += w;
                    }
                }
                let weight_sum = weight_sum.max(1e-6);
                out.put_pixel(
                    x,
                    y,
                    image::Rgba([
                        (acc[0] / weight_sum).round().clamp(0.0, 255.0) as u8,
                        (acc[1] / weight_sum).round().clamp(0.0, 255.0) as u8,
                        (acc[2] / weight_sum).round().clamp(0.0, 255.0) as u8,
                        (acc[3] / weight_sum).round().clamp(0.0, 255.0) as u8,
                    ]),
                );
            }
        }
        return out;
    }

    // 放大：双线性
    for y in 0..dh {
        for x in 0..dw {
            let sx = ((x as f32 + 0.5) * scale_x - 0.5).clamp(0.0, sw as f32 - 1.0);
            let sy = ((y as f32 + 0.5) * scale_y - 0.5).clamp(0.0, sh as f32 - 1.0);
            let x0 = sx.floor() as u32;
            let y0 = sy.floor() as u32;
            let x1 = (x0 + 1).min(sw - 1);
            let y1 = (y0 + 1).min(sh - 1);
            let fx = sx - x0 as f32;
            let fy = sy - y0 as f32;
            let p00 = roi.get_pixel(x0, y0).0;
            let p10 = roi.get_pixel(x1, y0).0;
            let p01 = roi.get_pixel(x0, y1).0;
            let p11 = roi.get_pixel(x1, y1).0;
            let mut px = [0u8; 4];
            for c in 0..4 {
                let top = p00[c] as f32 * (1.0 - fx) + p10[c] as f32 * fx;
                let bottom = p01[c] as f32 * (1.0 - fx) + p11[c] as f32 * fx;
                px[c] = (top * (1.0 - fy) + bottom * fy).round().clamp(0.0, 255.0) as u8;
            }
            out.put_pixel(x, y, image::Rgba(px));
        }
    }
    out
}

/// 灰度图（luma601，0~255）。
pub fn luma_of(rgba: &RgbaImage) -> GrayImage {
    let mut out = GrayImage::new(rgba.width(), rgba.height());
    for y in 0..rgba.height() {
        for x in 0..rgba.width() {
            let p = rgba.get_pixel(x, y).0;
            let luma =
                ((77u32 * p[0] as u32 + 150 * p[1] as u32 + 29 * p[2] as u32 + 128) >> 8) as u8;
            out.put_pixel(x, y, image::Luma([luma]));
        }
    }
    out
}

// ─── 积分图 ───

/// 归一化亮度（0~1）积分图 + 平方积分图（用于局部标准差）。
struct IntegralImage {
    width: usize,
    height: usize,
    sum: Vec<f32>,
    sum_sq: Vec<f32>,
}

impl IntegralImage {
    fn from_luma(luma: &GrayImage) -> Self {
        let width = luma.width() as usize;
        let height = luma.height() as usize;
        let values = luma.as_raw();
        let stride = width + 1;
        let scale = 1.0 / 255.0;
        let mut sum = vec![0.0f32; (width + 1) * (height + 1)];
        let mut sum_sq = vec![0.0f32; (width + 1) * (height + 1)];
        for (y, row) in values.chunks_exact(width).enumerate() {
            let mut row_sum = 0.0f32;
            let mut row_sum_sq = 0.0f32;
            for (x, &value) in row.iter().enumerate() {
                let value = value as f32 * scale;
                row_sum += value;
                row_sum_sq += value * value;
                let index = (y + 1) * stride + x + 1;
                sum[index] = sum[y * stride + x + 1] + row_sum;
                sum_sq[index] = sum_sq[y * stride + x + 1] + row_sum_sq;
            }
        }
        Self {
            width,
            height,
            sum,
            sum_sq,
        }
    }

    fn rect_sum(&self, table: &[f32], x0: i32, y0: i32, x1: i32, y1: i32) -> (f32, u32) {
        let x0 = x0.clamp(0, self.width as i32) as usize;
        let y0 = y0.clamp(0, self.height as i32) as usize;
        let x1 = x1.clamp(0, self.width as i32) as usize;
        let y1 = y1.clamp(0, self.height as i32) as usize;
        if x1 <= x0 || y1 <= y0 {
            return (0.0, 0);
        }
        let stride = self.width + 1;
        let sum = table[y1 * stride + x1] - table[y0 * stride + x1] - table[y1 * stride + x0]
            + table[y0 * stride + x0];
        (sum, ((x1 - x0) * (y1 - y0)) as u32)
    }

    fn rect_sum_centered(
        &self,
        table: &[f32],
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> (f32, u32) {
        let x0 = (x - width / 2).clamp(0, self.width as i32);
        let y0 = (y - height / 2).clamp(0, self.height as i32);
        self.rect_sum(table, x0, y0, x0 + width, y0 + height)
    }

    fn mean_centered(&self, x: i32, y: i32, width: i32, height: i32) -> f32 {
        let (sum, count) = self.rect_sum_centered(&self.sum, x, y, width, height);
        sum / count.max(1) as f32
    }

    fn mean_std_centered(&self, x: i32, y: i32, width: i32, height: i32) -> (f32, f32) {
        let (sum, count) = self.rect_sum_centered(&self.sum, x, y, width, height);
        let (sum_sq, _) = self.rect_sum_centered(&self.sum_sq, x, y, width, height);
        let count = count.max(1) as f32;
        let mean = sum / count;
        let variance = (sum_sq / count - mean * mean).max(0.0);
        (mean, variance.sqrt())
    }

    fn mean_rect(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> f32 {
        let (sum, count) = self.rect_sum(&self.sum, x0, y0, x1, y1);
        sum / count.max(1) as f32
    }
}

// ─── 三带边线响应 ───

fn horizontal_response(integral: &IntegralImage, x: i32, y: i32, cfg: &GeometryConfig) -> f32 {
    let side_offset = (cfg.h_line_h + cfg.h_side_h) / 2;
    let core = integral.mean_centered(x, y, cfg.h_window, cfg.h_line_h);
    let above = integral.mean_centered(x, y - side_offset, cfg.h_window, cfg.h_side_h);
    let below = integral.mean_centered(x, y + side_offset, cfg.h_window, cfg.h_side_h);
    let (_, std) = integral.mean_std_centered(x, y, cfg.h_window, cfg.h_line_h + 2 * cfg.h_side_h);
    ((core - above).abs() + (core - below).abs()) / (2.0 * (std + cfg.z_eps))
}

fn vertical_response(integral: &IntegralImage, x: i32, y: i32, cfg: &GeometryConfig) -> f32 {
    let side_offset = (cfg.v_line_w + cfg.v_side_w) / 2;
    let core = integral.mean_centered(x, y, cfg.v_line_w, cfg.v_window);
    let left = integral.mean_centered(x - side_offset, y, cfg.v_side_w, cfg.v_window);
    let right = integral.mean_centered(x + side_offset, y, cfg.v_side_w, cfg.v_window);
    let (_, std) = integral.mean_std_centered(x, y, cfg.v_line_w + 2 * cfg.v_side_w, cfg.v_window);
    ((core - left).abs() + (core - right).abs()) / (2.0 * (std + cfg.z_eps))
}

fn response_to_score(response: f32, threshold: f32, high: f32) -> f32 {
    ((response - threshold) / (high - threshold).max(1e-6)).clamp(0.0, 1.0)
}

// ─── 分段打分 ───

fn segment_bounds(length: i32, bin: usize, bins: usize) -> (i32, i32) {
    let scale = length as f32 / bins.max(1) as f32;
    (
        (bin as f32 * scale).round() as i32,
        ((bin + 1) as f32 * scale).round() as i32,
    )
}

fn segmented_score_masked<F>(
    length: i32,
    skip_center_bins: usize,
    bins: usize,
    cfg: &GeometryConfig,
    mut bin_mean: F,
) -> f32
where
    F: FnMut(i32, i32) -> f32,
{
    if length <= 0 || bins == 0 {
        return 0.0;
    }
    let skip = skip_center_bins.min(bins.saturating_sub(1));
    let skip_start = (bins - skip) / 2;
    let skip_end = skip_start + skip;
    let mut mean_score = 0.0f32;
    let mut active = 0usize;
    let mut used = 0usize;
    for bin in 0..bins {
        if skip > 0 && (skip_start..skip_end).contains(&bin) {
            continue;
        }
        let (a, b) = segment_bounds(length, bin, bins);
        if b <= a {
            continue;
        }
        let mean = bin_mean(a, b);
        mean_score += mean;
        used += 1;
        if mean >= cfg.segment_min_bin_score {
            active += 1;
        }
    }
    let used = used.max(1);
    let mean_score = mean_score / used as f32;
    let active_ratio = active as f32 / used as f32;
    (cfg.segment_mean_weight * mean_score + cfg.segment_active_weight * active_ratio)
        .clamp(0.0, 1.0)
}

fn segmented_score_by_sampling_step<F>(
    length: i32,
    step: i32,
    cfg: &GeometryConfig,
    mut sample: F,
) -> f32
where
    F: FnMut(i32) -> f32,
{
    let step = step.max(1);
    let bins = cfg.segment_bins;
    segmented_score_masked(length, 0, bins, cfg, |start, end| {
        let mut sum = 0.0f32;
        let mut count = 0usize;
        let mut offset = start;
        while offset < end {
            sum += sample(offset);
            count += 1;
            offset += step;
        }
        if count == 0 {
            0.0
        } else {
            sum / count as f32
        }
    })
}

fn segmented_score_from_prefix_masked(
    prefix: &[f32],
    base: i32,
    start: i32,
    length: i32,
    skip_center_bins: usize,
    cfg: &GeometryConfig,
) -> f32 {
    segmented_score_masked(
        length,
        skip_center_bins,
        cfg.segment_bins,
        cfg,
        |bin_start, bin_end| {
            let y0 = (start + bin_start - base).max(0) as usize;
            let y1 =
                (start + bin_end - base).clamp(0, prefix.len().saturating_sub(1) as i32) as usize;
            if y1 <= y0 {
                return 0.0;
            }
            (prefix[y1] - prefix[y0]) / (y1 - y0) as f32
        },
    )
}

// ─── 预计算边线分数 ───

struct LineScores {
    pos: i32,
    start: i32,
    scores: Vec<f32>,
}

impl LineScores {
    fn score_at(&self, position: i32) -> f32 {
        let index = position - self.start;
        if index < 0 {
            return 0.0;
        }
        self.scores.get(index as usize).copied().unwrap_or(0.0)
    }
}

struct ProfileLookup {
    h_lines: Vec<LineScores>,
    h_x_to_index: Vec<Option<usize>>,
    v_lines: Vec<LineScores>,
    v_x_to_index: Vec<Option<usize>>,
}

impl ProfileLookup {
    fn new(integral: &IntegralImage, profile: &Profile, cfg: &GeometryConfig) -> Self {
        let width = integral.width;
        let h_y_min = profile.y_min - cfg.edge_band;
        let h_y_max = profile.y_max + cfg.slot_size + cfg.edge_band;
        let h_lines: Vec<LineScores> = profile
            .cols
            .iter()
            .map(|x| build_horizontal_line_scores(integral, *x, h_y_min, h_y_max, cfg))
            .collect();
        let mut h_x_to_index = vec![None; width];
        for (index, line) in h_lines.iter().enumerate() {
            if line.pos >= 0 && (line.pos as usize) < width {
                h_x_to_index[line.pos as usize] = Some(index);
            }
        }

        let mut v_positions: Vec<i32> = Vec::new();
        for x in &profile.cols {
            for edge_x in [*x, *x + cfg.slot_size] {
                v_positions.extend(edge_x - cfg.edge_band..=edge_x + cfg.edge_band);
            }
        }
        v_positions.sort_unstable();
        v_positions.dedup();
        let v_lines: Vec<LineScores> = v_positions
            .iter()
            .map(|x| build_vertical_line_scores(integral, *x, profile, cfg))
            .collect();
        let mut v_x_to_index = vec![None; width];
        for (index, line) in v_lines.iter().enumerate() {
            if line.pos >= 0 && (line.pos as usize) < width {
                v_x_to_index[line.pos as usize] = Some(index);
            }
        }

        Self {
            h_lines,
            h_x_to_index,
            v_lines,
            v_x_to_index,
        }
    }

    fn horizontal_edge_score(&self, x: i32, y: i32, cfg: &GeometryConfig) -> f32 {
        let Some(index) = self.h_x_to_index.get(x.max(0) as usize).copied().flatten() else {
            return 0.0;
        };
        let line = &self.h_lines[index];
        let center = line.score_at(y);
        let neighbor = line
            .score_at(y - cfg.edge_band)
            .max(line.score_at(y + cfg.edge_band));
        (cfg.h_edge_center_weight * center + neighbor) / (cfg.h_edge_center_weight + 1.0)
    }

    fn vertical_edge_score(&self, x: i32, y: i32, cfg: &GeometryConfig) -> f32 {
        let mut best = f32::NEG_INFINITY;
        let mut second = f32::NEG_INFINITY;
        let mut count = 0usize;
        for xx in x - cfg.edge_band..=x + cfg.edge_band {
            if xx < 0 {
                continue;
            }
            let Some(index) = self.v_x_to_index.get(xx as usize).copied().flatten() else {
                continue;
            };
            let value = self.v_lines[index].score_at(y);
            push_top2(value, &mut best, &mut second);
            count += 1;
        }
        if count == 0 {
            0.0
        } else if cfg.edge_band_topk <= 1 || count == 1 {
            best.max(0.0)
        } else {
            (((best + second) * 0.5).max(0.0)).min(1.0)
        }
    }
}

fn push_top2(value: f32, best: &mut f32, second: &mut f32) {
    if value > *best {
        *second = *best;
        *best = value;
    } else if value > *second {
        *second = value;
    }
}

fn build_horizontal_line_scores(
    integral: &IntegralImage,
    x: i32,
    y_min: i32,
    y_max: i32,
    cfg: &GeometryConfig,
) -> LineScores {
    let mut scores = Vec::with_capacity((y_max - y_min + 1).max(0) as usize);
    for y in y_min..=y_max {
        scores.push(segmented_score_by_sampling_step(
            cfg.slot_size,
            1,
            cfg,
            |offset| {
                response_to_score(
                    horizontal_response(integral, x + offset, y, cfg),
                    cfg.h_response_thr,
                    cfg.h_response_hi,
                )
            },
        ));
    }
    LineScores {
        pos: x,
        start: y_min,
        scores,
    }
}

fn build_vertical_line_scores(
    integral: &IntegralImage,
    x: i32,
    profile: &Profile,
    cfg: &GeometryConfig,
) -> LineScores {
    let y_min = profile.y_min;
    let y_max = profile.y_max;
    let response_y_max = y_max + cfg.slot_size;
    let mut response_scores = Vec::with_capacity((response_y_max - y_min + 1).max(0) as usize);
    for y in y_min..=response_y_max {
        response_scores.push(response_to_score(
            vertical_response(integral, x, y, cfg),
            cfg.v_response_thr,
            cfg.v_response_hi,
        ));
    }
    let mut prefix = Vec::with_capacity(response_scores.len() + 1);
    prefix.push(0.0);
    for score in response_scores {
        prefix.push(prefix.last().copied().unwrap_or(0.0) + score);
    }
    let mut scores = Vec::with_capacity((y_max - y_min + 1).max(0) as usize);
    for y in y_min..=y_max {
        scores.push(segmented_score_from_prefix_masked(
            &prefix,
            y_min,
            y,
            cfg.slot_size,
            cfg.v_segment_skip_center_bins,
            cfg,
        ));
    }
    LineScores {
        pos: x,
        start: y_min,
        scores,
    }
}

// ─── 槽位与行打分 ───

struct Profile {
    cols: Vec<i32>,
    y_min: i32,
    y_max: i32,
    max_rows: usize,
    min_slots: usize,
}

#[derive(Clone)]
struct SlotCandidate {
    x: i32,
    col: u32,
    score: f32,
}

#[derive(Clone)]
struct RowCandidate {
    y: i32,
    score: f32,
    slots: Vec<SlotCandidate>,
}

/// 单个候选槽位的验证分数（参考实现 `score_slot_at`）。
fn score_slot(
    lookup: &ProfileLookup,
    integral: &IntegralImage,
    x: i32,
    y: i32,
    cfg: &GeometryConfig,
) -> Option<SlotCandidate> {
    let slot_size = cfg.slot_size;
    let top = lookup.horizontal_edge_score(x, y, cfg);
    let bottom = lookup.horizontal_edge_score(x, y + slot_size, cfg);
    let left = lookup.vertical_edge_score(x, y, cfg);
    let right = lookup.vertical_edge_score(x + slot_size, y, cfg);

    let tb_min = top.min(bottom);
    let tb_mean = 0.5 * (top + bottom);
    let side_mean = 0.5 * (left + right);
    let side_best = left.max(right);
    let base_score = cfg.tb_min_weight * tb_min
        + cfg.tb_mean_weight * tb_mean
        + cfg.side_mean_weight * side_mean
        + cfg.side_best_weight * side_best;

    if top < cfg.min_horizontal_edge
        || bottom < cfg.min_horizontal_edge
        || side_best < cfg.min_side
        || base_score < cfg.min_slot_score
    {
        return None;
    }
    let score = base_score * border_uniformity_factor(integral, x, y, cfg);
    if score < cfg.min_slot_score {
        return None;
    }
    Some(SlotCandidate { x, col: 0, score })
}

/// 边框一致性：36 段亮度的相对离散度 → 0.4~1.0 的惩罚系数
/// （参考实现 `border_uniformity_factor`）。
fn border_uniformity_factor(integral: &IntegralImage, x: i32, y: i32, cfg: &GeometryConfig) -> f32 {
    let width = cfg.slot_size;
    let height = cfg.slot_size;
    let segments = cfg.border_segments();
    let mut values = vec![0.0f32; segments];
    let mut index = 0usize;
    let top = y - cfg.h_line_h / 2;
    let bottom = y + height - cfg.h_line_h / 2;
    let left = x - cfg.v_line_w / 2;
    let right = x + width - cfg.v_line_w / 2;
    for bin in 0..cfg.segment_bins {
        let (start, end) = segment_bounds(width, bin, cfg.segment_bins);
        if index + 1 >= values.len() {
            break;
        }
        values[index] = integral.mean_rect(x + start, top, x + end, top + cfg.h_line_h);
        values[index + 1] = integral.mean_rect(x + start, bottom, x + end, bottom + cfg.h_line_h);
        index += 2;
    }
    let skip = cfg
        .v_segment_skip_center_bins
        .min(cfg.segment_bins.saturating_sub(1));
    let skip_start = (cfg.segment_bins - skip) / 2;
    let skip_end = skip_start + skip;
    for bin in 0..cfg.segment_bins {
        if skip > 0 && (skip_start..skip_end).contains(&bin) {
            continue;
        }
        let (start, end) = segment_bounds(height, bin, cfg.segment_bins);
        if index + 1 >= values.len() {
            break;
        }
        values[index] = integral.mean_rect(left, y + start, left + cfg.v_line_w, y + end);
        values[index + 1] = integral.mean_rect(right, y + start, right + cfg.v_line_w, y + end);
        index += 2;
    }
    // 未填充的段按 0 处理（越界槽位）：直接给最低一致性
    if index < segments {
        return 0.4;
    }

    values.sort_by(f32::total_cmp);
    let median = median_sorted(&values);
    let mut first = 0usize;
    let mut last = values.len();
    for _ in 0..cfg.border_uniformity_trim {
        if last <= first + 1 {
            break;
        }
        if median - values[first] > values[last - 1] - median {
            first += 1;
        } else {
            last -= 1;
        }
    }
    let retained = &values[first..last];
    if retained.is_empty() {
        return 0.4;
    }
    let center = median_sorted(retained).max(cfg.border_uniformity_luma_floor);
    let count = retained.len() as f32;
    let mean = retained.iter().sum::<f32>() / count;
    let variance = (retained.iter().map(|v| v * v).sum::<f32>() / count - mean * mean).max(0.0);
    let relative_std = variance.sqrt() / center;
    let t = ((relative_std - cfg.border_uniformity_good)
        / (cfg.border_uniformity_bad - cfg.border_uniformity_good).max(1e-6))
    .clamp(0.0, 1.0);
    let penalty = t * t * (3.0 - 2.0 * t);
    (1.0 - cfg.border_uniformity_max_penalty * penalty).clamp(0.0, 1.0)
}

fn median_sorted(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mid = values.len() / 2;
    if values.len() % 2 == 0 {
        0.5 * (values[mid - 1] + values[mid])
    } else {
        values[mid]
    }
}

/// 单个 y 上的行候选（参考实现 `score_row_at`）。
fn score_row_at(
    lookup: &ProfileLookup,
    integral: &IntegralImage,
    profile: &Profile,
    y: i32,
    cfg: &GeometryConfig,
) -> Option<RowCandidate> {
    let mut slots: Vec<SlotCandidate> = Vec::new();
    let mut score = 0.0f32;
    for (col, x) in profile.cols.iter().enumerate() {
        if let Some(mut candidate) = score_slot(lookup, integral, *x, y, cfg) {
            candidate.col = col as u32;
            score = score.max(candidate.score);
            slots.push(candidate);
        }
    }
    if slots.len() < profile.min_slots || score < cfg.row_threshold {
        return None;
    }
    Some(RowCandidate {
        y,
        score: score.clamp(0.0, 1.0),
        slots,
    })
}

/// 逐像素扫描行区间，再用 DP 选行（参考实现 `scan_profile` + `select_rows_dp_hard`）。
fn scan_profile(
    lookup: &ProfileLookup,
    integral: &IntegralImage,
    profile: &Profile,
    cfg: &GeometryConfig,
) -> Vec<RowCandidate> {
    let mut candidates = Vec::new();
    for y in profile.y_min..=profile.y_max {
        if let Some(row) = score_row_at(lookup, integral, profile, y, cfg) {
            candidates.push(row);
        }
    }
    select_rows_dp_hard(&candidates, profile.max_rows, cfg.row_min_dist)
}

/// DP：在「行距 >= min_gap」的硬约束下挑出总分最高的至多 max_rows 行。
fn select_rows_dp_hard(
    candidates: &[RowCandidate],
    max_rows: usize,
    min_gap: i32,
) -> Vec<RowCandidate> {
    if candidates.is_empty() || max_rows == 0 {
        return Vec::new();
    }
    let n = candidates.len();
    let k_max = max_rows.min(n);
    let mut prev = vec![-1isize; n];
    let mut j: isize = -1;
    for i in 0..n {
        while (j + 1) < i as isize && candidates[i].y - candidates[(j + 1) as usize].y >= min_gap {
            j += 1;
        }
        prev[i] = j;
    }

    let neg = -1.0e30f32;
    let stride = k_max + 1;
    let mut dp = vec![neg; (n + 1) * stride];
    let mut take = vec![false; (n + 1) * stride];
    for i in 0..=n {
        dp[i * stride] = 0.0;
    }
    for i in 1..=n {
        let row = &candidates[i - 1];
        let p = (prev[i - 1] + 1) as usize;
        for k in 1..=k_max {
            let index = i * stride + k;
            let skip = dp[(i - 1) * stride + k];
            let use_score = dp[p * stride + k - 1] + row.score;
            if use_score > skip + 1e-9 {
                dp[index] = use_score;
                take[index] = true;
            } else {
                dp[index] = skip;
            }
        }
    }
    let mut best_k = 0usize;
    let mut best_score = dp[n * stride];
    for k in 1..=k_max {
        let score = dp[n * stride + k];
        if score > best_score + 1e-9 {
            best_score = score;
            best_k = k;
        }
    }
    let mut selected = Vec::new();
    let mut i = n;
    let mut k = best_k;
    while i > 0 && k > 0 {
        if take[i * stride + k] {
            selected.push(candidates[i - 1].clone());
            i = (prev[i - 1] + 1) as usize;
            k -= 1;
        } else {
            i -= 1;
        }
    }
    selected.reverse();
    selected
}

/// home 槽位内容检查：中心区域的相对标准差是否达到「有内容」门槛
/// （参考实现 `slot_has_content`）。
fn slot_has_content(integral: &IntegralImage, x: i32, y: i32, cfg: &GeometryConfig) -> bool {
    let content_size = (cfg.slot_size - 2 * cfg.home_content_inset).max(4);
    let center_offset = cfg.slot_size / 2;
    let (mean, std) = integral.mean_std_centered(
        x + center_offset,
        y + center_offset,
        content_size,
        content_size,
    );
    std / mean.max(cfg.home_content_mean_floor) >= cfg.home_content_min_relative_std.max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn cfg() -> GeometryConfig {
        GeometryConfig::default()
    }

    fn seg() -> SegmentationConfig {
        SegmentationConfig::default()
    }

    /// 造一张 canonical 尺寸的 ROI：暗底 + 指定位置的亮边框方格。
    fn canonical_roi(cells: &[ImageRect], glyph: Option<ImageRect>) -> RgbaImage {
        let c = cfg();
        let mut img = RgbaImage::new(c.canonical_w, c.canonical_h);
        for (_, _, p) in img.enumerate_pixels_mut() {
            *p = Rgba([30, 33, 38, 255]);
        }
        for cell in cells {
            for y in cell.y..cell.bottom() {
                for x in cell.x..cell.right() {
                    if x < 0 || y < 0 || x >= c.canonical_w as i32 || y >= c.canonical_h as i32 {
                        continue;
                    }
                    let edge = x == cell.x
                        || y == cell.y
                        || x == cell.right() - 1
                        || y == cell.bottom() - 1;
                    let color = if edge {
                        [200, 200, 196, 255]
                    } else {
                        [70, 74, 80, 255]
                    };
                    img.put_pixel(x as u32, y as u32, Rgba(color));
                }
            }
        }
        if let Some(rect) = glyph {
            for y in rect.y..rect.bottom() {
                for x in rect.x..rect.right() {
                    if x < 0 || y < 0 || x >= c.canonical_w as i32 || y >= c.canonical_h as i32 {
                        continue;
                    }
                    img.put_pixel(x as u32, y as u32, Rgba([232, 160, 90, 255]));
                }
            }
        }
        img
    }

    fn home_cells(y: i32) -> Vec<ImageRect> {
        let c = cfg();
        let mut cells: Vec<ImageRect> = c
            .home_cols
            .iter()
            .map(|x| ImageRect::new(*x, y, c.slot_size, c.slot_size))
            .collect();
        cells.push(ImageRect::new(
            c.home_booster_x,
            y,
            c.slot_size,
            c.slot_size,
        ));
        cells
    }

    #[test]
    fn canonical_resize_preserves_size_and_is_identity_at_same_size() {
        let c = cfg();
        let roi = canonical_roi(&home_cells(636), None);
        let same = canonical_rgba(&roi, &c);
        assert_eq!(same.dimensions(), (c.canonical_w, c.canonical_h));
        assert_eq!(same.get_pixel(10, 10).0, roi.get_pixel(10, 10).0);

        // 缩小一半：仍返回 canonical 尺寸
        let small = image::imageops::resize(
            &roi,
            c.canonical_w / 2,
            c.canonical_h / 2,
            image::imageops::FilterType::Triangle,
        );
        let back = canonical_rgba(&small, &c);
        assert_eq!(back.dimensions(), (c.canonical_w, c.canonical_h));
    }

    #[test]
    fn detect_home_finds_fixed_columns_and_row() {
        let c = cfg();
        let roi = canonical_roi(&home_cells(636), None);
        let detection = detect_home(&roi, &c, &seg());
        assert!(detection.verified(), "固定锚点 + 边线验证应通过");
        assert!(detection.home);
        assert_eq!(detection.rows, 1);
        assert_eq!(detection.cols, 4);
        assert_eq!(detection.slots.len(), 5);
        for (index, slot) in detection.slots.iter().take(4).enumerate() {
            assert_eq!(slot.col, index as u32);
            assert_eq!(slot.rect.x, c.home_cols[index]);
            assert!((slot.rect.y - 636).abs() <= 3, "行位置应在标定附近");
            assert!(slot.score > c.min_slot_score);
            assert!(!slot.content, "空格子的内容检查应为 false");
        }
        assert!(detection.booster().is_some());
        assert!(detection.best_row_score > c.row_threshold);
    }

    #[test]
    fn detect_home_reports_content_when_glyph_is_present() {
        let c = cfg();
        let glyph = ImageRect::new(40, 660, 40, 40);
        let roi = canonical_roi(&home_cells(636), Some(glyph));
        let detection = detect_home(&roi, &c, &seg());
        assert!(detection.verified());
        let first = detection.slots[0];
        assert!(first.content, "有内容时内容检查应为 true");
    }

    #[test]
    fn detect_home_fails_on_flat_frame() {
        let c = cfg();
        let mut flat = RgbaImage::new(c.canonical_w, c.canonical_h);
        for (_, _, p) in flat.enumerate_pixels_mut() {
            *p = Rgba([40, 42, 46, 255]);
        }
        let detection = detect_home(&flat, &c, &seg());
        assert!(!detection.verified(), "无边框画面不得通过验证");
        assert!(detection.slots.is_empty());
    }

    #[test]
    fn detect_list_uses_dp_rows_with_min_gap() {
        let c = cfg();
        // 三行：120 / 233 / 346（行距 113，符合硬约束）
        let mut cells = Vec::new();
        for y in [120, 233, 346] {
            for x in c.list_cols {
                cells.push(ImageRect::new(x, y, c.slot_size, c.slot_size));
            }
        }
        let roi = canonical_roi(&cells, None);
        let detection = detect_list(&roi, &c);
        assert!(detection.verified());
        assert!(!detection.home);
        assert_eq!(detection.rows, 3, "应选出 3 行");
        assert_eq!(detection.slots.len(), 12);
        let rows: Vec<i32> = detection
            .slots
            .iter()
            .map(|s| s.rect.y)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        assert_eq!(rows.len(), 3);
        for (row, y) in rows.iter().enumerate() {
            assert!(
                (y - [120, 233, 346][row]).abs() <= 3,
                "第 {row} 行位置偏差过大：{y}"
            );
        }
    }

    #[test]
    fn detect_list_rejects_rows_closer_than_min_gap() {
        let c = cfg();
        // 两行间距只有 40px（< row_min_dist）：DP 的硬约束只能选一行
        let mut cells = Vec::new();
        for y in [120, 160] {
            for x in c.list_cols {
                cells.push(ImageRect::new(x, y, c.slot_size, c.slot_size));
            }
        }
        let roi = canonical_roi(&cells, None);
        let detection = detect_list(&roi, &c);
        assert_eq!(detection.rows, 1, "行距不足时必须只保留一行");
    }

    #[test]
    fn detect_list_fails_on_flat_frame() {
        let c = cfg();
        let mut flat = RgbaImage::new(c.canonical_w, c.canonical_h);
        for (_, _, p) in flat.enumerate_pixels_mut() {
            *p = Rgba([35, 35, 38, 255]);
        }
        let detection = detect_list(&flat, &c);
        assert!(!detection.verified());
    }

    #[test]
    fn border_uniformity_penalizes_uneven_borders() {
        let c = cfg();
        // 同一条边上亮度差异极大 → 一致性惩罚
        let mut img = RgbaImage::new(c.canonical_w, c.canonical_h);
        for (_, _, p) in img.enumerate_pixels_mut() {
            *p = Rgba([30, 33, 38, 255]);
        }
        let cell = ImageRect::new(77, 120, c.slot_size, c.slot_size);
        for y in cell.y..cell.bottom() {
            for x in cell.x..cell.right() {
                let edge =
                    x == cell.x || y == cell.y || x == cell.right() - 1 || y == cell.bottom() - 1;
                let color = if edge {
                    // 上半亮、下半暗：边框亮度极不均匀
                    if y < cell.y + cell.h / 2 {
                        [220, 220, 216, 255]
                    } else {
                        [60, 60, 58, 255]
                    }
                } else {
                    [70, 74, 80, 255]
                };
                img.put_pixel(x as u32, y as u32, Rgba(color));
            }
        }
        let luma = luma_of(&img);
        let integral = IntegralImage::from_luma(&luma);
        let factor = border_uniformity_factor(&integral, cell.x, cell.y, &c);
        assert!(factor < 0.95, "不均匀边框必须被惩罚，实际系数 {factor}");
        assert!(factor >= 0.4 - 1e-6);
    }

    #[test]
    fn dp_selection_prefers_higher_total() {
        let row = |y: i32, score: f32| RowCandidate {
            y,
            score,
            slots: vec![SlotCandidate {
                x: 0,
                col: 0,
                score,
            }],
        };
        // 候选：120(0.9) / 233(0.8) / 260(0.85)，最小行距 113。
        // 可行组合：{120,233}=1.70、{120,260}=1.75、{233,260} 被硬约束排除 → 必须选后者。
        let candidates = vec![row(120, 0.9), row(233, 0.8), row(260, 0.85)];
        let selected = select_rows_dp_hard(&candidates, 3, 113);
        assert_eq!(selected.len(), 2);
        let ys: Vec<i32> = selected.iter().map(|r| r.y).collect();
        assert!(
            ys.contains(&120) && ys.contains(&260),
            "DP 应取总分最高的 120 + 260，实际 {ys:?}"
        );
        assert!(!ys.contains(&233));
    }

    #[test]
    fn booster_content_requires_yellow() {
        let c = cfg();
        let rect = ImageRect::new(c.home_booster_x, 636, c.slot_size, c.slot_size);
        // 纯灰：不算装备
        let grey = {
            let mut img = RgbaImage::new(c.canonical_w, c.canonical_h);
            for (_, _, p) in img.enumerate_pixels_mut() {
                *p = Rgba([70, 74, 80, 255]);
            }
            img
        };
        assert!(!booster_has_content(&grey, rect, &c, &seg()));
        // 六边形内容区涂成 Booster 黄：算装备
        let mut yellow = grey.clone();
        for local_y in 0..rect.h {
            for local_x in booster_hex_row_span(local_y, rect.w, rect.h, &c) {
                yellow.put_pixel(
                    (rect.x + local_x) as u32,
                    (rect.y + local_y) as u32,
                    Rgba([255, 222, 38, 255]),
                );
            }
        }
        assert!(booster_has_content(&yellow, rect, &c, &seg()));
    }

    #[test]
    fn empty_detection_is_reported_not_panicking() {
        let detection = GeometryDetection::empty(4);
        assert!(!detection.verified());
        assert!(detection.booster().is_none());
    }
}
