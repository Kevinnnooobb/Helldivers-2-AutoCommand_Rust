// Viewport 建模 —— 把游戏选择列表看成「可滚动的可见项目集合」，而不是固定 Page。
//
// plan6 §5.3 之后，生产路径的滚动位移判定在 `direct_select::page_relation`；
// 本模块只剩底层适配契约的回归测试调用者（`tests.rs` 的真实截图位移断言）。
#![allow(dead_code)]
//
// 每个可见行提取一个视觉签名（landmark）。滚动前后用共同 landmark 推断位移，
// 从而知道「滚轮到底有没有生效、位移了几行、方向对不对」。
// 这是与「比较 old_image != new_image」最本质的区别：位移是语义量，不是像素差。
use image::GrayImage;

use crate::loadout_sync::types::{ImageRect, ListCell, ListGrid};

/// 每个单元格签名分辨率（8x8 = 64 维）。
pub const SIG_GRID: usize = 8;
/// 判定「同一 landmark」的签名距离上限。
pub const LANDMARK_DISTANCE: f32 = 0.25;

#[derive(Debug, Clone, PartialEq)]
pub struct RowSignature {
    pub values: Vec<f32>,
}

impl RowSignature {
    /// 由一行可见单元格构建签名：逐格做「去均值 + 归一化」，对亮度/HDR 变化免疫。
    pub fn from_row(gray: &GrayImage, cells: &[ListCell]) -> Option<Self> {
        let mut ordered: Vec<&ListCell> = cells.iter().collect();
        ordered.sort_by_key(|c| c.col);
        let mut values = Vec::with_capacity(ordered.len() * SIG_GRID * SIG_GRID);
        for cell in ordered {
            // 取样窗口按「亮像素重心」对齐：识别出的格子位置可能有 ±3px 误差，
            // 直接按矩形裁剪会让同一图标在不同帧给出不同签名（实测距离 0.29 > 阈值）。
            let win_size = ((cell.rect.w.min(cell.rect.h) as f32) * 0.62)
                .round()
                .max(4.0) as i32;
            let (cx0, cy0) = bright_centroid(gray, cell.rect).unwrap_or(cell.rect.center());
            let half = win_size / 2;
            let cx = cx0.clamp(
                cell.rect.x + half,
                (cell.rect.right() - half).max(cell.rect.x + half),
            );
            let cy = cy0.clamp(
                cell.rect.y + half,
                (cell.rect.bottom() - half).max(cell.rect.y + half),
            );
            let inner = ImageRect::new(cx - half, cy - half, win_size, win_size);
            let inner = inner.clamp_to_frame(gray.width() as i32, gray.height() as i32)?;
            let crop = image::imageops::crop_imm(
                gray,
                inner.x as u32,
                inner.y as u32,
                inner.w as u32,
                inner.h as u32,
            )
            .to_image();
            let small = image::imageops::resize(
                &crop,
                SIG_GRID as u32,
                SIG_GRID as u32,
                image::imageops::FilterType::Triangle,
            );
            let raw: Vec<f32> = small.pixels().map(|p| p[0] as f32 / 255.0).collect();
            let n = raw.len() as f32;
            let mean = raw.iter().sum::<f32>() / n;
            let var = raw.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / n;
            let std = var.sqrt();
            if std < 1e-3 {
                // 空槽 / 均匀区块：不携带 landmark 信息
                values.extend(std::iter::repeat_n(0.0, raw.len()));
            } else {
                values.extend(raw.iter().map(|v| (v - mean) / std));
            }
        }
        if values.is_empty() {
            return None;
        }
        // 整体 L2 归一化
        let norm = values.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 1e-3 {
            for v in values.iter_mut() {
                *v /= norm;
            }
        }
        Some(Self { values })
    }

    pub fn is_degenerate(&self) -> bool {
        self.values.iter().all(|v| v.abs() < 1e-4)
    }

    /// 余弦距离：0 = 完全一致，2 = 完全相反。两个退化签名（均为空槽）视为相同。
    pub fn distance(&self, other: &Self) -> f32 {
        if self.values.len() != other.values.len() {
            return 2.0;
        }
        if self.is_degenerate() && other.is_degenerate() {
            return 0.0;
        }
        if self.is_degenerate() || other.is_degenerate() {
            return 2.0;
        }
        let dot: f32 = self
            .values
            .iter()
            .zip(other.values.iter())
            .map(|(a, b)| a * b)
            .sum();
        (1.0 - dot).clamp(0.0, 2.0)
    }

    pub fn is_same_landmark(&self, other: &Self) -> bool {
        self.distance(other) <= LANDMARK_DISTANCE
    }

    /// 量化哈希：用于「连续两帧识别结果是否一致」的 UI 稳定判定。
    pub fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for v in &self.values {
            let q = ((v * 8.0).round() as i32).clamp(-64, 64) as i64;
            h ^= (q as u64).wrapping_add(0x9E37_79B9_7F4A_7C15);
            h = h.wrapping_mul(0x1000_0000_01B3);
        }
        h
    }
}

/// 格子内亮像素重心（用于对齐取样窗口）；几乎均匀的格子返回 None。
fn bright_centroid(gray: &GrayImage, rect: ImageRect) -> Option<(i32, i32)> {
    let inner = rect.inset((rect.w.min(rect.h) as f32 * 0.12).round() as i32);
    let inner = inner.clamp_to_frame(gray.width() as i32, gray.height() as i32)?;
    let step = (inner.w as f32 / 16.0).round().max(1.0) as i32;
    let mut sum = 0.0f32;
    let mut n = 0usize;
    for y in (inner.y..inner.bottom()).step_by(step.max(1) as usize) {
        for x in (inner.x..inner.right()).step_by(step.max(1) as usize) {
            sum += gray.get_pixel(x as u32, y as u32)[0] as f32 / 255.0;
            n += 1;
        }
    }
    if n == 0 {
        return None;
    }
    let mean = sum / n as f32;
    let mut wsum = 0.0f32;
    let (mut wx, mut wy) = (0.0f32, 0.0f32);
    for y in (inner.y..inner.bottom()).step_by(step.max(1) as usize) {
        for x in (inner.x..inner.right()).step_by(step.max(1) as usize) {
            let v = gray.get_pixel(x as u32, y as u32)[0] as f32 / 255.0;
            let w = (v - mean).max(0.0);
            wsum += w;
            wx += w * x as f32;
            wy += w * y as f32;
        }
    }
    if wsum < 0.05 {
        return None;
    }
    Some(((wx / wsum).round() as i32, (wy / wsum).round() as i32))
}

#[derive(Debug, Clone, PartialEq)]
pub struct ViewportRow {
    pub row: u32,
    pub top_y: i32,
    pub cells: Vec<ListCell>,
    pub signature: RowSignature,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ViewportState {
    pub rows: Vec<ViewportRow>,
    /// 实测行距（用于判断滚动位移是否合理）
    pub row_pitch: f32,
    /// 整屏签名哈希
    pub signature_hash: u64,
}

impl ViewportState {
    /// 由识别出的网格构建当前 Viewport；空网格返回 None。
    pub fn build(gray: &GrayImage, grid: &ListGrid) -> Option<Self> {
        if grid.is_empty() {
            return None;
        }
        let mut rows: Vec<ViewportRow> = Vec::new();
        for r in 0..grid.rows {
            let cells: Vec<ListCell> = grid.row_cells(r).into_iter().copied().collect();
            if cells.is_empty() {
                continue;
            }
            let signature = RowSignature::from_row(gray, &cells)?;
            let top_y = cells.iter().map(|c| c.rect.y).min().unwrap_or(0);
            rows.push(ViewportRow {
                row: r,
                top_y,
                cells,
                signature,
            });
        }
        if rows.is_empty() {
            return None;
        }
        let row_pitch = if rows.len() >= 2 {
            let d: Vec<f32> = rows
                .windows(2)
                .map(|w| (w[1].top_y - w[0].top_y) as f32)
                .collect();
            d.iter().sum::<f32>() / d.len() as f32
        } else {
            0.0
        };
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for row in &rows {
            hash ^= row.signature.hash();
            hash = hash.wrapping_mul(0x1000_0000_01B3);
        }
        Some(Self {
            rows,
            row_pitch,
            signature_hash: hash,
        })
    }

    pub fn cells(&self) -> Vec<ListCell> {
        self.rows
            .iter()
            .flat_map(|r| r.cells.iter().copied())
            .collect()
    }

    /// 两帧识别内容是否一致（UI 是否已稳定）。
    pub fn is_same_content(&self, other: &Self) -> bool {
        self.signature_hash == other.signature_hash
    }
}
