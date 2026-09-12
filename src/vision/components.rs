// 连通域分析与分量打分（需求 §12 / §13）。
//
// 为什么不能「面积 > 阈值」一刀切（实测教训）：
//   * 不同战备图标的有效面积差异极大（背包类 vs 细长武器类）；
//   * 一个图标经常由多个分量组成（白字 + 彩色剪影 + 内部镂空），
//     只取最大分量会丢掉判别信息。
// 因此这里给每个分量算可解释的四项分数（面积 / 中心 / 形状 / 颜色强度），
// 再对「空间上确实属于同一个图标」的分量做合并（有距离上限）。
use crate::loadout_sync::types::ImageRect;

use super::config::ComponentConfig;
use super::segment::ForegroundMasks;

/// 分量被拒绝的原因（必须可解释，禁止只给一个布尔）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentReject {
    /// 面积低于下限（噪点）
    TooSmall,
    /// 面积占比过高（把边框/整格底色吞进来了）
    TooLarge,
    /// 细长直线伪影（UI 分隔线 / 槽位边线残影）
    ThinLine,
    /// 填充率过低（散点噪声）
    LowFill,
    /// 完全落在外围、与格子中央区域无交集（贴边 UI）
    OffCenter,
    /// 综合分数低于下限
    LowScore,
}

/// 单个连通域的统计（全部坐标为**内裁剪图像**的局部坐标）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComponentStats {
    pub id: usize,
    pub area: usize,
    /// 外接框（局部坐标）
    pub bbox: ImageRect,
    pub width: i32,
    pub height: i32,
    pub fill_ratio: f32,
    pub aspect_ratio: f32,
    pub centroid: (f32, f32),
    /// 到图像中心的归一化距离（0 = 正中心，1 ≈ 角落）
    pub distance_to_center: f32,
    /// 分量的橙色像素占比
    pub orange_ratio: f32,
    /// 分量的白色像素占比
    pub white_ratio: f32,
    /// 面积占整幅图像的比例
    pub area_ratio: f32,
    /// 分量内前景分数的平均值（颜色/强度证据）
    pub mean_foreground_score: f32,
    /// 四项加权后的综合分数（0~1）
    pub score: f32,
    pub kept: bool,
    pub reject: Option<ComponentReject>,
}

/// 连通域分析结果。
#[derive(Debug, Clone, PartialEq)]
pub struct ComponentAnalysis {
    pub width: usize,
    pub height: usize,
    pub components: Vec<ComponentStats>,
    /// 全部保留分量（含合并）的并集外接框；None = 没有任何有效前景
    pub bbox: Option<ImageRect>,
    /// 参与最终外接框的分量 id（合并后的分组）
    pub merged: Vec<usize>,
    /// 最大保留分量面积（空槽判定用）
    pub largest_kept_area: usize,
}

impl ComponentAnalysis {
    pub fn kept(&self) -> impl Iterator<Item = &ComponentStats> {
        self.components.iter().filter(|c| c.kept)
    }

    /// 最大保留分量面积占整幅图像的比例（空槽判定用）。
    pub fn largest_kept_ratio(&self) -> f32 {
        let total = (self.width * self.height).max(1);
        self.largest_kept_area as f32 / total as f32
    }
}

/// 对前景掩码做连通域分析 + 打分 + 合并。
pub fn analyze(masks: &ForegroundMasks, cfg: &ComponentConfig) -> ComponentAnalysis {
    let width = masks.width;
    let height = masks.height;
    let total = (width * height).max(1);
    let labels = label_components(&masks.combined, width, height);

    let mut stats: Vec<ComponentStats> = Vec::new();
    for id in 0..labels.count {
        let pixels = &labels.members[id];
        if pixels.is_empty() {
            continue;
        }
        let area = pixels.len();
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, -1i32, -1i32);
        let (mut sx, mut sy) = (0.0f64, 0.0f64);
        let (mut orange, mut white) = (0usize, 0usize);
        let mut score_sum = 0.0f64;
        for index in pixels {
            let x = (*index % width) as i32;
            let y = (*index / width) as i32;
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
            sx += x as f64;
            sy += y as f64;
            if masks.orange[*index] {
                orange += 1;
            }
            if masks.white[*index] {
                white += 1;
            }
            score_sum += masks.scores.combined[*index] as f64;
        }
        let bbox = ImageRect::new(x0, y0, x1 - x0 + 1, y1 - y0 + 1);
        let fill_ratio = area as f32 / (bbox.w * bbox.h).max(1) as f32;
        let aspect_ratio = bbox.w.max(bbox.h) as f32 / bbox.w.min(bbox.h).max(1) as f32;
        let centroid = ((sx / area as f64) as f32, (sy / area as f64) as f32);
        let (cx, cy) = (width as f32 / 2.0, height as f32 / 2.0);
        let half_diag = (cx * cx + cy * cy).sqrt().max(1.0);
        let distance_to_center = (((centroid.0 - cx).powi(2) + (centroid.1 - cy).powi(2)).sqrt()
            / half_diag)
            .clamp(0.0, 1.0);
        let area_ratio = area as f32 / total as f32;
        let mean_foreground_score = (score_sum / area as f64) as f32;

        let (score, reject) = score_component(
            cfg,
            area,
            area_ratio,
            fill_ratio,
            aspect_ratio,
            distance_to_center,
            mean_foreground_score,
            bbox,
            width,
            height,
        );

        stats.push(ComponentStats {
            id,
            area,
            bbox,
            width: bbox.w,
            height: bbox.h,
            fill_ratio,
            aspect_ratio,
            centroid,
            distance_to_center,
            orange_ratio: orange as f32 / area as f32,
            white_ratio: white as f32 / area as f32,
            area_ratio,
            mean_foreground_score,
            score,
            kept: reject.is_none(),
            reject,
        });
    }

    // 合并：把空间上邻近的保留分量归为一组（一个图标 = 白字 + 剪影 + 镂空部件）
    let merge_gap = (cfg.merge_distance_ratio.clamp(0.0, 0.5) * width.min(height) as f32)
        .round()
        .max(1.0) as i32;
    let groups = group_nearby(&stats, merge_gap);
    let mut merged: Vec<usize> = Vec::new();
    let mut bbox: Option<ImageRect> = None;
    // 选组：优先总面积，其次取「更靠近格心」的组。
    //
    // 为什么不能只看最大分量（实测教训）：真实图标的白字部件是被渲染成
    // 逐行断开的短横线（每段 2~4px，中间隔 1 行），单个分量面积很小；
    // 只取最大分量会把图标主体裁成一条底座（bbox 38×14），
    // 归一化后与模板的 IoU 只有 0.5 左右。
    let mut ranked_groups: Vec<(usize, usize, f32)> = groups
        .iter()
        .enumerate()
        .map(|(index, g)| {
            let area: usize = g.iter().map(|id| stats[*id].area).sum();
            let dist = g
                .iter()
                .map(|id| stats[*id].distance_to_center)
                .fold(f32::INFINITY, f32::min);
            (index, area, dist)
        })
        .collect();
    ranked_groups.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal))
    });
    for (index, _, _) in ranked_groups {
        let group = &groups[index];
        let mut chosen: Vec<usize> = group.iter().copied().filter(|id| stats[*id].kept).collect();
        if chosen.is_empty() {
            continue;
        }
        chosen.sort_by(|a, b| {
            stats[*b].area.cmp(&stats[*a].area).then_with(|| {
                stats[*b]
                    .score
                    .partial_cmp(&stats[*a].score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
        });
        chosen.truncate(cfg.max_merged_components.max(1));
        let mut union = stats[chosen[0]].bbox;
        for id in chosen.iter().skip(1) {
            union = union_rect(union, stats[*id].bbox);
        }
        bbox = Some(union);
        merged = chosen;
        break;
    }

    let largest_kept_area = stats
        .iter()
        .filter(|c| c.kept)
        .map(|c| c.area)
        .max()
        .unwrap_or(0);

    ComponentAnalysis {
        width,
        height,
        components: stats,
        bbox,
        merged,
        largest_kept_area,
    }
}

#[allow(clippy::too_many_arguments)]
fn score_component(
    cfg: &ComponentConfig,
    area: usize,
    area_ratio: f32,
    fill_ratio: f32,
    aspect_ratio: f32,
    distance_to_center: f32,
    mean_foreground_score: f32,
    bbox: ImageRect,
    width: usize,
    height: usize,
) -> (f32, Option<ComponentReject>) {
    // 硬性拒绝（几何上不可能是图标的一部分）
    if area < cfg.min_area_px || area_ratio < cfg.min_area_ratio {
        return (0.0, Some(ComponentReject::TooSmall));
    }
    if area_ratio > cfg.max_area_ratio {
        return (0.0, Some(ComponentReject::TooLarge));
    }
    let thin = bbox.w.min(bbox.h) <= cfg.line_thin_max;
    let long = bbox.w.max(bbox.h) as f32 >= cfg.line_length_ratio * width.max(height) as f32;
    if thin && long {
        return (0.0, Some(ComponentReject::ThinLine));
    }
    if fill_ratio < cfg.min_fill_ratio {
        return (0.0, Some(ComponentReject::LowFill));
    }
    // 与中央区域完全无交集 → 贴边 UI
    let central = ImageRect::new(
        (width as f32 * 0.25) as i32,
        (height as f32 * 0.25) as i32,
        (width as f32 * 0.5).max(1.0) as i32,
        (height as f32 * 0.5).max(1.0) as i32,
    );
    if intersect(bbox, central).is_none() {
        return (0.0, Some(ComponentReject::OffCenter));
    }

    let area_score = if cfg.good_area_ratio <= cfg.min_area_ratio {
        1.0
    } else {
        smoothstep(cfg.min_area_ratio, cfg.good_area_ratio, area_ratio)
    };
    let center_score = 1.0 - distance_to_center;
    let aspect_penalty = smoothstep(1.2, cfg.max_aspect_ratio.max(1.5), aspect_ratio);
    let shape_score = fill_ratio.clamp(0.0, 1.0) * (1.0 - aspect_penalty);
    let color_score = mean_foreground_score.clamp(0.0, 1.0);

    let weight_sum =
        (cfg.weight_area + cfg.weight_center + cfg.weight_shape + cfg.weight_color).max(1e-6);
    let score = (cfg.weight_area * area_score
        + cfg.weight_center * center_score
        + cfg.weight_shape * shape_score
        + cfg.weight_color * color_score)
        / weight_sum;
    if score < cfg.min_component_score {
        (score, Some(ComponentReject::LowScore))
    } else {
        (score, None)
    }
}

/// 保留分量之间按空间邻近关系分组（单链聚类：距离 < gap 即同组）。
///
/// 只对 `kept` 分量分组；被拒绝的分量不参与合并（否则会把 UI 线条拉进图标框）。
fn group_nearby(stats: &[ComponentStats], gap: i32) -> Vec<Vec<usize>> {
    let n = stats.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut Vec<usize>, mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for i in 0..n {
        if !stats[i].kept {
            continue;
        }
        for j in (i + 1)..n {
            if !stats[j].kept {
                continue;
            }
            if rect_gap(stats[i].bbox, stats[j].bbox) <= gap {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri != rj {
                    parent[ri] = rj;
                }
            }
        }
    }
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut index_of: Vec<Option<usize>> = vec![None; n];
    for i in 0..n {
        if !stats[i].kept {
            continue;
        }
        let root = find(&mut parent, i);
        match index_of[root] {
            Some(g) => groups[g].push(i),
            None => {
                index_of[root] = Some(groups.len());
                groups.push(vec![i]);
            }
        }
    }
    groups
}

/// 两个外接框之间的最小间距（相交时为 0）。
fn rect_gap(a: ImageRect, b: ImageRect) -> i32 {
    let dx = (b.x - a.right()).max(a.x - b.right()).max(0);
    let dy = (b.y - a.bottom()).max(a.y - b.bottom()).max(0);
    dx.max(dy)
}

fn intersect(a: ImageRect, b: ImageRect) -> Option<ImageRect> {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = a.right().min(b.right());
    let y1 = a.bottom().min(b.bottom());
    (x1 > x0 && y1 > y0).then(|| ImageRect::new(x0, y0, x1 - x0, y1 - y0))
}

fn union_rect(a: ImageRect, b: ImageRect) -> ImageRect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = a.right().max(b.right());
    let y1 = a.bottom().max(b.bottom());
    ImageRect::new(x0, y0, x1 - x0, y1 - y0)
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if (edge1 - edge0).abs() <= f32::EPSILON {
        return if x >= edge1 { 1.0 } else { 0.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 连通域标记结果（4 连通，显式栈，避免递归爆栈）。
struct Labels {
    count: usize,
    members: Vec<Vec<usize>>,
}

fn label_components(mask: &[bool], width: usize, height: usize) -> Labels {
    let mut label = vec![usize::MAX; mask.len()];
    let mut members: Vec<Vec<usize>> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for start in 0..mask.len() {
        if !mask[start] || label[start] != usize::MAX {
            continue;
        }
        let id = members.len();
        let mut group = Vec::new();
        label[start] = id;
        stack.push(start);
        while let Some(index) = stack.pop() {
            group.push(index);
            let x = (index % width) as i32;
            let y = (index / width) as i32;
            let push = |nx: i32, ny: i32, stack: &mut Vec<usize>, label: &mut Vec<usize>| {
                if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                    return;
                }
                let n = ny as usize * width + nx as usize;
                if mask[n] && label[n] == usize::MAX {
                    label[n] = id;
                    stack.push(n);
                }
            };
            push(x - 1, y, &mut stack, &mut label);
            push(x + 1, y, &mut stack, &mut label);
            push(x, y - 1, &mut stack, &mut label);
            push(x, y + 1, &mut stack, &mut label);
        }
        members.push(group);
    }
    Labels {
        count: members.len(),
        members,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vision::config::SegmentationConfig;
    use crate::vision::segment::segment_cell;
    use image::{Rgba, RgbaImage};

    fn masks_with(f: impl Fn(u32, u32) -> [u8; 4], w: u32, h: u32) -> ForegroundMasks {
        let mut img = RgbaImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                img.put_pixel(x, y, Rgba(f(x, y)));
            }
        }
        segment_cell(&img, &SegmentationConfig::default())
    }

    fn cfg() -> ComponentConfig {
        ComponentConfig::default()
    }

    #[test]
    fn single_glyph_yields_one_kept_component() {
        let masks = masks_with(
            |x, y| {
                if (30..50).contains(&x) && (30..50).contains(&y) {
                    [232, 160, 90, 255]
                } else {
                    [80, 80, 78, 255]
                }
            },
            80,
            80,
        );
        let analysis = analyze(&masks, &cfg());
        assert_eq!(analysis.kept().count(), 1);
        let c = analysis.kept().next().expect("有保留分量");
        assert!(c.area >= 300);
        assert!(c.fill_ratio > 0.9);
        assert!(c.orange_ratio > 0.9);
        assert!(c.score > 0.5, "分数应达标，实际 {}", c.score);
        let bbox = analysis.bbox.expect("外接框");
        assert!(bbox.w >= 18 && bbox.w <= 24);
    }

    #[test]
    fn empty_cell_yields_no_components() {
        let masks = masks_with(|_, _| [80, 80, 78, 255], 80, 80);
        let analysis = analyze(&masks, &cfg());
        assert_eq!(analysis.components.len(), 0);
        assert!(analysis.bbox.is_none());
        assert_eq!(analysis.largest_kept_area, 0);
    }

    #[test]
    fn thin_long_line_is_rejected_as_ui_artifact() {
        // 一条横穿整格的 2px 亮线（模拟槽位分隔线）
        let masks = masks_with(
            |_, y| {
                if (38..40).contains(&y) {
                    [232, 160, 90, 255]
                } else {
                    [80, 80, 78, 255]
                }
            },
            80,
            80,
        );
        let analysis = analyze(&masks, &cfg());
        assert_eq!(analysis.kept().count(), 0, "细长直线不得进入图标框");
        assert!(analysis
            .components
            .iter()
            .any(|c| c.reject == Some(ComponentReject::ThinLine)));
        assert!(analysis.bbox.is_none());
    }

    #[test]
    fn two_nearby_parts_merge_into_one_icon_bbox() {
        // 白字在上、橙色剪影在下：相距 8px（< 合并间距）→ 必须合并成一个图标
        let masks = masks_with(
            |x, y| {
                let upper = (36..44).contains(&x) && (30..36).contains(&y);
                let lower = (32..48).contains(&x) && (44..56).contains(&y);
                if upper {
                    [248, 248, 243, 255]
                } else if lower {
                    [232, 160, 90, 255]
                } else {
                    [80, 80, 78, 255]
                }
            },
            80,
            80,
        );
        let analysis = analyze(&masks, &cfg());
        assert_eq!(analysis.merged.len(), 2, "两块应合并到同一图标分组");
        let bbox = analysis.bbox.expect("外接框");
        assert!(bbox.y <= 31 && bbox.bottom() >= 55, "并集应覆盖上下两块");
        assert!(bbox.h >= 24);
    }

    #[test]
    fn far_apart_parts_do_not_merge() {
        // 左上角小块与右下角大块：距离超过合并上限 → 只取更可信的一组
        let masks = masks_with(
            |x, y| {
                let corner = (8..14).contains(&x) && (8..14).contains(&y);
                let glyph = (44..64).contains(&x) && (44..64).contains(&y);
                if corner {
                    [232, 160, 90, 255]
                } else if glyph {
                    [232, 160, 90, 255]
                } else {
                    [80, 80, 78, 255]
                }
            },
            80,
            80,
        );
        let analysis = analyze(&masks, &cfg());
        let bbox = analysis.bbox.expect("外接框");
        assert!(bbox.w < 40, "不应把角落噪点并入图标框，实际 {bbox:?}");
    }

    #[test]
    fn off_center_component_is_rejected() {
        // 贴边（左上）的亮块：位于中央区之外
        let masks = masks_with(
            |x, y| {
                if (4..16).contains(&x) && (4..16).contains(&y) {
                    [232, 160, 90, 255]
                } else {
                    [80, 80, 78, 255]
                }
            },
            80,
            80,
        );
        let analysis = analyze(&masks, &cfg());
        assert!(analysis
            .components
            .iter()
            .all(|c| c.reject == Some(ComponentReject::OffCenter) || !c.kept));
        assert!(analysis.bbox.is_none());
    }
}
