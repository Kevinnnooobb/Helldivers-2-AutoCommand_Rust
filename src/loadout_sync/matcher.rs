// 图标模板匹配 —— 前景掩码 Dice 版。
//
// plan6 §5.3 之后，本模块是 `direct_select` 的**底层观察适配**：
// 分类器（`direct_select::classify`）用它给每个槽位打分，除此之外不再有
// 生产调用者。它的诊断 API（掩码统计、阈值转储、连通域原因、字形尺寸扫描）
// 只被参考帧回归测试与手工探针调用 —— 这些代码保留是为了让底层适配契约
// 可验证，不是为了运行时装配。因此本模块整体豁免 dead_code 告警。
#![allow(dead_code)]
//
// 链路（每一步都在本项目实机标注帧上有量测依据，见 `src/fixtures/loadout_sync/README.md`）：
//
//   1. **格子前景分割**（Phase C）：格子内每个像素按「白字图标 / 分类色图形 / 背景」分类。
//      实机实测：格子底色是暗色（luma 75~100），字形是亮色 ——
//        白字图标 mean RGB ≈ (248,248,243)
//        分类色图形 mean RGB ≈ (170,250,251)  → luma ≈ 226
//      而 H2AC 资源里同一张卡片的彩色部分是 **luma ≈ 146**（#49ADC9 之类的实色填充）。
//      ⇒ 亮度通道在实机上**没有判别力**（248 vs 226 几乎持平），
//        旧版「灰度余弦 + 梯度余弦」因此在真实帧上退化成噪声（实测 13 格 top-1 = 0）。
//      因此判定改成**形状**：两侧都归约成掩码，再做 Dice。
//
//   2. **画布几何**：模板画布与格子等大，整张资源按固定比例居中绘制。
//      比例是**游戏属性**（列表 68/104、home 93/104，参考实现实机量得），
//      不是按 alpha 包围盒归一化 —— 卡片内白字与剪影各自大小不同，
//      包围盒归一化会把不同战备缩放到不同尺寸。
//
//   3. **对齐**：用两侧前景包围盒中心对齐（实测格子几何存在约 −6px 的系统横向偏差），
//      再在 ±1px 邻域精修；不做宽范围盲搜。
//
//   4. **细长线伪影过滤**：实机格子里残留的行/列分隔线会被判成前景，
//      用连通域 + 「细、直、长」规则剔除（对应实施计划中的 line suppression）。
//
//   5. **验收闸门**：分数阈值 + 与次优模板的间隔，由调用方（config）掌握；
//      本模块只负责给出可解释的 [0,1] 分数。
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use image::RgbaImage;

use crate::loadout_sync::capture::CapturedFrame;
use crate::loadout_sync::selection::LoadoutItem;
use crate::loadout_sync::types::{ImageRect, ListCell, Match};

// ─── 常量 ───

/// 模板 alpha 掩码阈值：低于该值视为背景。
pub const ALPHA_MASK_THRESHOLD: u8 = 64;

/// 列表格子里「**资源画布**边长 / 格子边长」。
///
/// 口径说明：这是整张资源画布（256×256，含资源自身留白）相对格子的比例，
/// 不是「字形包围盒」的比例。取值来自参考实现在实机上量得的 68/104；
/// 本项目资源留白略大，由 `probe_canvas_scale_sweep` 在真实标注帧上校准。
pub const LIST_GLYPH_SCALE: f32 = 0.654;
/// home 槽位里「资源画布边长 / 槽位边长」（参考实现实测 93/104）。
pub const HOME_GLYPH_SCALE: f32 = 0.894;

/// 验收分数阈值（掩码 Dice 尺度）。**生产阈值由
/// [`crate::loadout_sync::config::LoadoutSyncConfig::recognition_threshold`] 提供**
/// （可配置 + `sanitize()` 下限保护），这里只作为探针/断言的参考值。
#[cfg(test)]
pub const RECOGNITION_THRESHOLD: f32 = 0.60;

/// 验收间隔下限（与次优模板的最小分差）。
pub const MATCH_MIN_MARGIN: f32 = 0.03;

/// 空槽判定：格子内部相对标准差低于该值即视为空（与掩码前景数互为补充）。
pub const EMPTY_RELATIVE_STD: f32 = 0.15;
/// 空槽判定（掩码路径）：有效前景像素少于此数即视为空槽。
pub const MIN_FOREGROUND_PX: usize = 40;
/// 相对标准差路径使用的四边内缩比例。
pub const EMPTY_INSET_RATIO: f32 = 0.22;
/// 相对标准差路径的平滑半径。
pub const CELL_BLUR_RADIUS: i32 = 2;

/// 掩码采样区的四边内缩比例：剔除贴边的槽位边框与选中框。
///
/// 实机边框是亮白色细线（luma 210+，无彩色），会被判成「白字前景」，
/// 因此必须靠位置（内缩）剔除，而不是靠颜色阈值。
pub const BORDER_INSET_RATIO: f32 = 0.10;
/// 白字图标判定：无彩色且足够亮。
///
/// **取值来源（plan4 P1.2/P1.3 实测，不是估计值）**：
/// 游戏内列表格子里，图标主体的亮度分布是双峰——
/// 高光笔画约 192~255，而**占主体的铜色/描边部分集中在 128~159**；
/// 格子底色集中在 64~95（实测 p25 = 64，且底色像素全部 ≤95）。
///
/// 用 150 会把整个 128~159 的主体判成背景，实测后果：
/// * `quasar_cannon` 前景只剩 54px（应约 206px），整库最高分只有 0.322；
/// * `machine_gun` 前景 118px、名次 46/107；
/// * 六个典型目标里有四个的 score 远低于生产阈值 0.60。
///
/// 阈值矩阵实测（`probe_threshold_matrix`）：

/// | white_luma_min | quasar 名次 | machine_gun 名次 | anti_materiel 名次 | eagle_airstrike 名次 |
/// | ---: | ---: | ---: | ---: | ---: |
/// | 150 | 98 | 46 | 95 | 3 |
/// | 130 | 1 | 1 | 48 | 3 |
/// | **120** | **1** | **1** | **1** | 3 |
/// | 110 | 1 | 1 | 1 | 3 |
/// | 100 | 1 | 1 | 1 | 2 |
/// | 95 | 1 | 1 | 1 | 2 |
///
/// 取 120：六个目标全部进入 top-3，同时与底色上限 95 保持 25 的余量
/// （90/95 那一档虽然 `eagle_airstrike` 更好，但离底色太近，容易吞入格间缝隙）。
/// 该值只放宽**亮度**判定，`WHITE_CHROMA_MAX`、`COLOR_CHROMA_MIN`、
/// `MIN_COMPONENT_AREA`、包围盒 sanity 与 `RECOGNITION_THRESHOLD` 全部保持不变。
pub const WHITE_LUMA_MIN: f32 = 120.0;
pub const WHITE_CHROMA_MAX: i32 = 50;
/// 分类色图形判定：色度高（与白字互斥）。
pub const COLOR_CHROMA_MIN: i32 = 60;
/// 前景连通域最小面积（去除孤立噪点）。
pub const MIN_COMPONENT_AREA: usize = 12;
/// 「细长直」线伪影的判定：包围盒比例下限 + 细边上限 + 填充率下限。
pub const LINE_ASPECT_MIN: f32 = 5.0;
pub const LINE_THIN_MAX: i32 = 3;
pub const LINE_FILL_MIN: f32 = 0.60;
/// 用于判定线伪影的「长度」下限（占掩码宽度/高度的比例）。
pub const LINE_LENGTH_RATIO: f32 = 0.50;

/// 三个掩码 Dice 项的权重（彩色剪影 / 白字图标 / 整体前景）。
pub const MASK_WEIGHT_COLOR: f32 = 0.45;
pub const MASK_WEIGHT_WHITE: f32 = 0.25;
pub const MASK_WEIGHT_SHAPE: f32 = 0.30;

/// 对齐搜索：包围盒中心对齐后的精修半径（±1，共 9 个候选）。
///
/// 顺序是确定性的：(0,0) 优先，其余按「离原点距离、再按 y 与 x」排列。
/// 同分时先命中的偏移胜出，因此顺序不能随意变动。
pub const SEARCH_OFFSETS: [(i32, i32); 9] = [
    (0, 0),
    (-1, 0),
    (1, 0),
    (0, -1),
    (0, 1),
    (-1, -1),
    (1, -1),
    (-1, 1),
    (1, 1),
];
/// 前景包围盒占格子的最小/最大比例（坏裁剪与「把边框吞进来」的弃权门限）。
pub const MIN_CARD_RATIO: f32 = 0.22;
pub const MAX_CARD_RATIO: f32 = 0.95;
/// 模板画布边长的细化半径（±N 像素）。
pub const REFINE_SCALE_RADIUS: i32 = 1;

/// 对齐策略的最大偏移半径（manhattan 邻域，硬上限）。
///
/// 只被 `#[cfg(test)]` 的对照探针使用：生产路径固定用 [`SEARCH_OFFSETS`]（±1）。
/// 保留 ±2 的有界扩展能力，是为了让「扩大邻域是否让正确模板获益更多」这个
/// 问题有一个**不动生产常量**的实验入口。
#[cfg(test)]
pub const ALIGN_OFFSET_RADIUS: i32 = 2;

/// 有界偏移候选：`(0,0)` 优先，其余按「离原点距离、再按 y 与 x」稳定排序。
///
/// 稳定顺序很重要：同分时先命中的偏移胜出，顺序必须是确定性的，
/// 否则同一输入在不同运行间可能给出不同模板排序。
/// 单元测试 `search_offsets_match_alignment_offsets` 守住它与
/// [`SEARCH_OFFSETS`] 的一致性。
#[cfg(test)]
pub fn alignment_offsets(radius: i32) -> Vec<(i32, i32)> {
    let radius = radius.clamp(0, ALIGN_OFFSET_RADIUS);
    let mut out: Vec<(i32, i32)> = Vec::new();
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            out.push((dx, dy));
        }
    }
    out.sort_by_key(|(dx, dy)| (dx.abs() + dy.abs(), dy.abs(), dx.abs(), *dy, *dx));
    out
}

// ─── 格子类别 ───

/// 格子类别：决定资源画布在格子里的渲染比例。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    /// 列表里的战备格子
    ListStratagem,
    /// home 上的战备槽
    HomeStratagem,
    /// home 上的 Booster 六边形槽
    HomeBooster,
    /// Booster 列表里的格子
    ListBooster,
}

impl CellKind {
    /// 资源画布边长 / 格子边长。
    pub fn canvas_scale(self) -> f32 {
        match self {
            Self::ListStratagem | Self::ListBooster => LIST_GLYPH_SCALE,
            Self::HomeStratagem | Self::HomeBooster => HOME_GLYPH_SCALE,
        }
    }

    pub fn is_booster(self) -> bool {
        matches!(self, Self::HomeBooster | Self::ListBooster)
    }

    /// 该键是否属于 Booster 分类。
    pub fn key_is_booster(key: &str) -> bool {
        crate::stratagems::STRATAGEMS
            .iter()
            .find(|s| s.icon == key)
            .map(|s| {
                s.category
                    .eq_ignore_ascii_case(crate::stratagems::CAT_BOOSTERS)
            })
            .unwrap_or(false)
    }
}

// ─── 前景分类 ───

/// 单像素前景分类结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PixelClass {
    Background,
    /// 白字图标（无彩色 + 亮）
    White,
    /// 分类色图形（色度高）
    Color,
}

/// 单像素分类所用的阈值集合（plan4 P1.3 诊断矩阵用）。
///
/// 生产路径使用 [`PixelThresholds::PRODUCTION`]；探针可以传入候选组合做离线对照，
/// 而不改动任何生产常量。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelThresholds {
    pub color_chroma_min: i32,
    pub white_chroma_max: i32,
    pub white_luma_min: f32,
}

impl PixelThresholds {
    /// 与生产常量一致。
    pub const PRODUCTION: Self = Self {
        color_chroma_min: COLOR_CHROMA_MIN,
        white_chroma_max: WHITE_CHROMA_MAX,
        white_luma_min: WHITE_LUMA_MIN,
    };

    /// 只替换 `white_luma_min`（阈值矩阵探针用，生产不使用）。
    #[cfg(test)]
    pub const fn with_luma_min(self, luma: f32) -> Self {
        Self {
            white_luma_min: luma,
            ..self
        }
    }
}

/// 单像素前景分类。生产传 [`PixelThresholds::PRODUCTION`]，探针传候选组合。
fn classify_pixel(r: u8, g: u8, b: u8, t: PixelThresholds) -> PixelClass {
    let max = r.max(g).max(b) as i32;
    let min = r.min(g).min(b) as i32;
    let chroma = max - min;
    if chroma > t.color_chroma_min {
        return PixelClass::Color;
    }
    let luma = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
    if chroma <= t.white_chroma_max && luma >= t.white_luma_min {
        PixelClass::White
    } else {
        PixelClass::Background
    }
}

/// 一个前景连通域的统计（诊断用：看清格子里到底有什么）。
#[derive(Debug, Clone, Copy)]
pub struct ComponentInfo {
    pub area: usize,
    pub bbox: (i32, i32, i32, i32),
    pub white_px: usize,
    pub color_px: usize,
    pub kept: bool,
    /// 未保留的原因（plan4 P2.2 要求逐 component 记录删除原因）。
    pub reject: Option<ComponentReject>,
}

/// 连通域被丢弃的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentReject {
    /// 面积不足 [`MIN_COMPONENT_AREA`]
    TooSmall,
    /// 命中「细长直」线伪影规则
    LineArtifact,
}

impl ComponentReject {
    pub fn label(self) -> &'static str {
        match self {
            Self::TooSmall => "area<MIN_COMPONENT_AREA",
            Self::LineArtifact => "line_artifact",
        }
    }
}

/// 一格的前景掩码（坐标 = 格子矩形内部坐标）。
#[derive(Debug, Clone)]
pub struct CellMasks {
    width: usize,
    height: usize,
    white: Vec<bool>,
    color: Vec<bool>,
    white_px: usize,
    color_px: usize,
    /// 有效前景包围盒（含端点），None = 无前景
    bbox: Option<(i32, i32, i32, i32)>,
    /// 全部连通域（含被丢弃的），供诊断与失败归因使用
    components: Vec<ComponentInfo>,
    /// 生成本掩码所用的像素分类阈值（诊断矩阵用）。
    thresholds: PixelThresholds,
}

impl CellMasks {
    /// 从帧上的格子矩形提取前景掩码。
    ///
    /// 采样区按 [`BORDER_INSET_RATIO`] 内缩（贴边边框判为背景），
    /// 再按 [`MIN_COMPONENT_AREA`] 与「细长直线」规则过滤连通域。
    pub fn from_frame(frame: &CapturedFrame, cell: ImageRect) -> Option<Self> {
        Self::from_frame_thresholds(frame, cell, PixelThresholds::PRODUCTION)
    }

    /// 参数化提取：探针用它做像素分类阈值矩阵对照（plan4 P1.3），不改生产常量。
    pub fn from_frame_thresholds(
        frame: &CapturedFrame,
        cell: ImageRect,
        thresholds: PixelThresholds,
    ) -> Option<Self> {
        let rect = cell.clamp_to_frame(frame.rgba.width() as i32, frame.rgba.height() as i32)?;
        let width = rect.w.max(1) as usize;
        let height = rect.h.max(1) as usize;
        if width < 8 || height < 8 {
            return None;
        }
        let inner = inset_rect(ImageRect::new(0, 0, rect.w, rect.h), BORDER_INSET_RATIO);
        let mut white = vec![false; width * height];
        let mut color = vec![false; width * height];
        for y in 0..height {
            for x in 0..width {
                if x < inner.x as usize
                    || y < inner.y as usize
                    || x >= inner.right() as usize
                    || y >= inner.bottom() as usize
                {
                    continue;
                }
                let p = frame
                    .rgba
                    .get_pixel((rect.x + x as i32) as u32, (rect.y + y as i32) as u32)
                    .0;
                match classify_pixel(p[0], p[1], p[2], thresholds) {
                    PixelClass::White => white[y * width + x] = true,
                    PixelClass::Color => color[y * width + x] = true,
                    PixelClass::Background => {}
                }
            }
        }
        let mut masks = Self {
            width,
            height,
            white,
            color,
            white_px: 0,
            color_px: 0,
            bbox: None,
            components: Vec::new(),
            thresholds,
        };
        masks.filter_components();
        Some(masks)
    }

    /// 连通域过滤：去掉噪点与「细、直、长」的行/列分隔线残影。
    fn filter_components(&mut self) {
        let width = self.width;
        let height = self.height;
        let mut fg = vec![false; width * height];
        for i in 0..fg.len() {
            fg[i] = self.white[i] || self.color[i];
        }
        let mut label = vec![usize::MAX; fg.len()];
        let mut components: Vec<(usize, i32, i32, i32, i32)> = Vec::new(); // area,x0,y0,x1,y1
        let mut stack: Vec<usize> = Vec::new();
        for start in 0..fg.len() {
            if !fg[start] || label[start] != usize::MAX {
                continue;
            }
            let id = components.len();
            let mut area = 0usize;
            let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, -1i32, -1i32);
            stack.push(start);
            label[start] = id;
            while let Some(index) = stack.pop() {
                area += 1;
                let x = (index % width) as i32;
                let y = (index / width) as i32;
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
                let push = |nx: i32, ny: i32, stack: &mut Vec<usize>, label: &mut Vec<usize>| {
                    if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                        return;
                    }
                    let n = (ny * width as i32 + nx) as usize;
                    if fg[n] && label[n] == usize::MAX {
                        label[n] = id;
                        stack.push(n);
                    }
                };
                push(x - 1, y, &mut stack, &mut label);
                push(x + 1, y, &mut stack, &mut label);
                push(x, y - 1, &mut stack, &mut label);
                push(x, y + 1, &mut stack, &mut label);
            }
            components.push((area, x0, y0, x1, y1));
        }

        let mut keep = vec![false; components.len()];
        let mut reject: Vec<Option<ComponentReject>> = vec![None; components.len()];
        for (id, (area, x0, y0, x1, y1)) in components.iter().enumerate() {
            let w = x1 - x0 + 1;
            let h = y1 - y0 + 1;
            let thin = w.min(h) <= LINE_THIN_MAX;
            let long = w.max(h) as f32 >= LINE_LENGTH_RATIO * (width.max(height) as f32);
            let straight = (*area as f32) / ((w * h) as f32) >= LINE_FILL_MIN;
            let aspect = w.max(h) as f32 / (w.min(h).max(1) as f32);
            let line_artifact = thin && long && (straight || aspect >= LINE_ASPECT_MIN);
            // 顺序即优先级：先面积、再线伪影（诊断要能区分这两类原因）
            reject[id] = if *area < MIN_COMPONENT_AREA {
                Some(ComponentReject::TooSmall)
            } else if line_artifact {
                Some(ComponentReject::LineArtifact)
            } else {
                None
            };
            keep[id] = reject[id].is_none();
        }

        let (mut w_px, mut c_px) = (0usize, 0usize);
        let (mut bx0, mut by0, mut bx1, mut by1) = (i32::MAX, i32::MAX, -1i32, -1i32);
        let mut per_component = vec![(0usize, 0usize); components.len()]; // (white, color)
        for index in 0..fg.len() {
            let id = label[index];
            if id == usize::MAX {
                continue;
            }
            if self.white[index] {
                per_component[id].0 += 1;
            }
            if self.color[index] {
                per_component[id].1 += 1;
            }
            if !keep[id] {
                self.white[index] = false;
                self.color[index] = false;
                continue;
            }
            let x = (index % width) as i32;
            let y = (index / width) as i32;
            bx0 = bx0.min(x);
            by0 = by0.min(y);
            bx1 = bx1.max(x);
            by1 = by1.max(y);
            if self.white[index] {
                w_px += 1;
            }
            if self.color[index] {
                c_px += 1;
            }
        }
        self.white_px = w_px;
        self.color_px = c_px;
        self.bbox = (bx1 >= bx0 && by1 >= by0).then_some((bx0, by0, bx1, by1));
        self.components = components
            .iter()
            .enumerate()
            .map(|(id, (area, x0, y0, x1, y1))| ComponentInfo {
                area: *area,
                bbox: (*x0, *y0, *x1, *y1),
                white_px: per_component[id].0,
                color_px: per_component[id].1,
                kept: keep[id],
                reject: reject[id],
            })
            .collect();
    }

    /// 生成本掩码所用的像素分类阈值。
    ///
    /// 诊断输出必须带上它：不同阈值下的掩码不可直接比较，
    /// 报告里如果缺了阈值就无法复现（plan4 P1.1「所有输出必须标记分类常量版本」）。
    pub fn thresholds(&self) -> PixelThresholds {
        self.thresholds
    }

    /// 有效前景像素数（白字 + 分类色）。
    pub fn foreground_px(&self) -> usize {
        self.white_px + self.color_px
    }

    /// 空槽判定：前景太少即视为空。
    pub fn is_empty_like(&self) -> bool {
        self.foreground_px() < MIN_FOREGROUND_PX
    }

    /// 掩码画布宽度。
    pub fn width(&self) -> usize {
        self.width
    }

    /// 掩码画布高度。
    pub fn height(&self) -> usize {
        self.height
    }

    /// 有效前景包围盒（含端点）。
    pub fn bbox(&self) -> Option<(i32, i32, i32, i32)> {
        self.bbox
    }

    /// 诊断输出：把掩码画成 ASCII（文本模型无法读图，日志必须是文本）。
    #[cfg(test)]
    pub(crate) fn ascii(&self, step: usize) -> String {
        let mut out = String::new();
        let step = step.max(1);
        let mut y = 0;
        while y < self.height {
            let mut x = 0;
            while x < self.width {
                let i = y * self.width + x;
                out.push(if self.white[i] {
                    '#'
                } else if self.color[i] {
                    '*'
                } else {
                    '.'
                });
                x += step;
            }
            out.push('\n');
            y += step;
        }
        out
    }
}

// ─── 模板 ───

/// 模板缓存键：同一资源在不同格子尺寸/画布尺寸下是不同的模板。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TemplateKey {
    icon: String,
    canvas_w: u16,
    canvas_h: u16,
    icon_size: u16,
}

/// 单个模板：整张资源缩放到 `icon_size` 正方形后居中合成在与格子等大的画布上，
/// 再按与查询**完全相同**的分类规则归约为白字/分类色两个掩码。
#[derive(Debug, Clone)]
struct PreparedTemplate {
    key: String,
    canvas_w: u16,
    canvas_h: u16,
    icon_size: u16,
    white: Vec<bool>,
    color: Vec<bool>,
    white_px: usize,
    color_px: usize,
    /// 前景包围盒（画布坐标，含端点）
    bbox: (i32, i32, i32, i32),
}

impl PreparedTemplate {
    fn foreground_px(&self) -> usize {
        self.white_px + self.color_px
    }
}

/// 图标匹配器。
#[derive(Default)]
pub struct IconMatcher {
    /// 图标源（键 → 原始 RGBA）
    sources: HashMap<String, RgbaImage>,
    /// 已渲染模板缓存
    templates: Mutex<HashMap<TemplateKey, Arc<PreparedTemplate>>>,
    /// 打开图标资源时的失败记录（图标 Mod / 资源缺失）
    pub load_failures: Vec<String>,
}

impl Clone for IconMatcher {
    fn clone(&self) -> Self {
        Self {
            sources: self.sources.clone(),
            templates: Mutex::new(HashMap::new()),
            load_failures: self.load_failures.clone(),
        }
    }
}

impl std::fmt::Debug for IconMatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IconMatcher")
            .field("icons", &self.sources.len())
            .field("failures", &self.load_failures.len())
            .finish()
    }
}

impl IconMatcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// 为给定目标构建匹配器：优先内嵌图标资源，其次磁盘 `assets/icons`；
    /// 同时载入整库图标作为干扰项（否则相似战备会互相冒充）。
    pub fn for_items(items: &[&LoadoutItem]) -> Self {
        let mut m = Self::new();
        let library: Vec<LoadoutItem> = crate::icons::all_icon_keys()
            .into_iter()
            .map(|key| LoadoutItem {
                name: key.to_string(),
                icon: key.to_string(),
                base_index: None,
            })
            .collect();
        for extra in library.iter().chain(items.iter().copied()) {
            m.load_one(extra);
        }
        m
    }

    /// 仅加载指定目标（测试/诊断场景：不含整库干扰项）。
    pub fn for_targets_only(items: &[&LoadoutItem]) -> Self {
        let mut m = Self::new();
        for item in items {
            m.load_one(item);
        }
        m
    }

    fn load_one(&mut self, item: &LoadoutItem) {
        let key = item.icon.trim();
        if key.is_empty() || self.sources.contains_key(key) {
            return;
        }
        let bytes = crate::icons::icon_png_bytes(key)
            .map(|b| b.to_vec())
            .or_else(|| std::fs::read(icon_path(key)).ok());
        match bytes
            .and_then(|b| image::load_from_memory(&b).ok())
            .map(|img| img.to_rgba8())
        {
            Some(img) => {
                self.sources.insert(key.to_string(), img);
            }
            None => self.load_failures.push(key.to_string()),
        }
    }

    pub fn has(&self, icon_key: &str) -> bool {
        self.sources.contains_key(icon_key.trim())
    }

    /// 只加载一个目标（闭集对照实验用）。
    #[cfg(test)]
    pub fn load_one_for_test(&mut self, item: &LoadoutItem) {
        self.load_one(item);
    }

    /// 从磁盘加载一个图标作为模板（诊断/对照实验用）。
    #[cfg(test)]
    pub fn load_icon_from_file(&mut self, key: &str, path: &std::path::Path) -> bool {
        let Ok(bytes) = std::fs::read(path) else {
            return false;
        };
        let Ok(img) = image::load_from_memory(&bytes) else {
            return false;
        };
        self.sources.insert(key.to_string(), img.to_rgba8());
        true
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.sources.len()
    }

    /// 取得（必要时渲染）画布为 `canvas_w × canvas_h`、资源画布边长为 `icon_size` 的模板。
    fn template(
        &self,
        key: &str,
        canvas_w: u32,
        canvas_h: u32,
        icon_size: u32,
    ) -> Option<Arc<PreparedTemplate>> {
        let cache_key = TemplateKey {
            icon: key.to_string(),
            canvas_w: canvas_w as u16,
            canvas_h: canvas_h as u16,
            icon_size: icon_size as u16,
        };
        if let Some(cached) = self
            .templates
            .lock()
            .ok()
            .and_then(|cache| cache.get(&cache_key).cloned())
        {
            return Some(cached);
        }
        let source = self.sources.get(key)?;
        let built = Arc::new(build_template(key, source, canvas_w, canvas_h, icon_size)?);
        if let Ok(mut cache) = self.templates.lock() {
            cache.insert(cache_key, built.clone());
        }
        Some(built)
    }

    /// 该格子类别下参与比较的模板集合（按 Booster 分类切分）。
    fn templates_for(
        &self,
        kind: CellKind,
        canvas_w: u32,
        canvas_h: u32,
        icon_size: u32,
    ) -> Vec<Arc<PreparedTemplate>> {
        let mut out = Vec::with_capacity(self.sources.len());
        for key in self.sources.keys() {
            if CellKind::key_is_booster(key) != kind.is_booster() {
                continue;
            }
            if let Some(t) = self.template(key, canvas_w, canvas_h, icon_size) {
                out.push(t);
            }
        }
        out
    }

    /// 格子边长 → 资源画布边长（模板画布与格子等大，图标按此边长居中绘制）。
    fn icon_size(cell: ImageRect, kind: CellKind) -> u32 {
        let side = cell.w.min(cell.h).max(8) as f32;
        ((side * kind.canvas_scale()).round() as u32).clamp(6, side as u32)
    }

    /// 空槽前置检查（相对标准差路径，不需要完整掩码）。
    fn cell_is_empty(&self, frame: &CapturedFrame, cell: ImageRect) -> bool {
        CellFeatures::from_frame_inset(frame, cell, EMPTY_INSET_RATIO)
            .map(|f| f.is_empty_like())
            .unwrap_or(true)
    }

    /// 取格子的前景掩码（空槽 / 坏裁剪 / 无法采样 → None）。
    /// 取一格的掩码（经空槽与包围盒 sanity 过滤）。
    ///
    /// 供目录键控分类层复用，避免它在自己那侧重复实现空槽/坏裁剪判定。
    pub fn cell_masks_pub(&self, frame: &CapturedFrame, cell: ImageRect) -> Option<CellMasks> {
        self.cell_masks(frame, cell)
    }

    fn cell_masks(&self, frame: &CapturedFrame, cell: ImageRect) -> Option<CellMasks> {
        if self.cell_is_empty(frame, cell) {
            return None;
        }
        let masks = CellMasks::from_frame(frame, cell)?;
        (!masks.is_empty_like() && foreground_bbox_sane(&masks)).then_some(masks)
    }

    // ─── 对外接口 ───

    /// 指定目标在指定格子上的最佳分数（hover / 选中验证、搜索确认用）。
    pub fn score_cell_for(
        &self,
        frame: &CapturedFrame,
        cell: ImageRect,
        item: &LoadoutItem,
        kind: CellKind,
    ) -> Option<f32> {
        let masks = self.cell_masks(frame, cell)?;
        let icon_size = Self::icon_size(cell, kind);
        let score = self.best_score_for(item.icon.trim(), &masks, kind, icon_size);
        score.is_finite().then_some(score)
    }

    /// 整库判别：返回 (最佳键, 分数, 与次优的间隔)。
    pub fn classify_cell_all(
        &self,
        frame: &CapturedFrame,
        cell: ImageRect,
        kind: CellKind,
    ) -> Option<(String, f32, f32)> {
        let masks = self.cell_masks(frame, cell)?;
        let mut scored = self.score_all(&masks, cell, kind);
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let (best_key, best) = scored.first()?.clone();
        let second = scored.get(1).map(|(_, s)| *s).unwrap_or(0.0);
        Some((best_key, best, (best - second).max(0.0)))
    }

    /// 整库排名（识别失败时打印「差多少、被谁挤掉」）。
    pub fn top_candidates(
        &self,
        frame: &CapturedFrame,
        cell: ImageRect,
        kind: CellKind,
        limit: usize,
    ) -> Vec<(String, f32)> {
        let Some(masks) = self.cell_masks(frame, cell) else {
            return Vec::new();
        };
        let mut scored = self.score_all(&masks, cell, kind);
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        scored
    }

    /// 在候选格子里找目标。
    pub fn find_in_cells(
        &self,
        frame: &CapturedFrame,
        cells: &[ListCell],
        item: &LoadoutItem,
        kind: CellKind,
    ) -> Option<Match> {
        let mut scored: Vec<(f32, &ListCell)> = Vec::new();
        for cell in cells {
            if let Some(score) = self.score_cell_for(frame, cell.rect, item, kind) {
                if score.is_finite() {
                    scored.push((score, cell));
                }
            }
        }
        if scored.is_empty() {
            return None;
        }
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        let (best, cell) = scored[0];
        let second = scored.get(1).map(|(s, _)| *s).unwrap_or(0.0);
        Some(Match {
            item: item.clone(),
            rect: cell.rect,
            score: best,
            margin: (best - second).max(0.0),
            cell_row: cell.row,
            cell_col: cell.col,
        })
    }

    /// 在给定候选集合里判别：返回 (候选下标, 分数, 与次优候选的差)。
    pub fn classify_cell(
        &self,
        frame: &CapturedFrame,
        cell: ImageRect,
        candidates: &[&LoadoutItem],
        kind: CellKind,
    ) -> Option<(usize, f32, f32)> {
        let masks = self.cell_masks(frame, cell)?;
        let icon_size = Self::icon_size(cell, kind);
        let mut scored: Vec<(usize, f32)> = Vec::new();
        for (index, item) in candidates.iter().enumerate() {
            let score = self.best_score_for(item.icon.trim(), &masks, kind, icon_size);
            if score.is_finite() {
                scored.push((index, score));
            }
        }
        if scored.is_empty() {
            return None;
        }
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let (index, best) = scored[0];
        let second = scored.get(1).map(|(_, s)| *s).unwrap_or(0.0);
        Some((index, best, (best - second).max(0.0)))
    }

    /// 该目标是否是所在格子里「最像的候选」：返回 (目标分数, 与最强对手的差)。
    pub fn confirm_target_is_best(
        &self,
        frame: &CapturedFrame,
        cell: ImageRect,
        item: &LoadoutItem,
        kind: CellKind,
    ) -> Option<(f32, f32)> {
        let masks = self.cell_masks(frame, cell)?;
        let icon_size = Self::icon_size(cell, kind);
        let own_key = item.icon.trim();
        let own_score = self.best_score_for(own_key, &masks, kind, icon_size);
        let mut best_other = f32::NEG_INFINITY;
        for key in self.sources.keys() {
            if key == own_key || CellKind::key_is_booster(key) != kind.is_booster() {
                continue;
            }
            let s = self.best_score_for(key, &masks, kind, icon_size);
            if s > best_other {
                best_other = s;
            }
        }
        let margin = if best_other.is_finite() {
            own_score - best_other
        } else {
            f32::INFINITY
        };
        Some((own_score, margin))
    }

    /// 指定资源画布边长下整库打分（比例标定用）。
    #[cfg(test)]
    pub fn score_all_with_glyph_height(
        &self,
        frame: &CapturedFrame,
        cell: ImageRect,
        kind: CellKind,
        glyph_h: u32,
    ) -> Vec<(String, f32)> {
        let Some(masks) = CellMasks::from_frame(frame, cell) else {
            return Vec::new();
        };
        let canvas_w = masks.width as u32;
        let canvas_h = masks.height as u32;
        let icon_size = glyph_h.clamp(6, canvas_w.min(canvas_h));
        self.templates_for(kind, canvas_w, canvas_h, icon_size)
            .iter()
            .map(|t| (t.key.clone(), best_score(t, &masks)))
            .collect()
    }

    /// 单模板最佳分数（包围盒中心对齐 + ±1px 精修 + 尺寸 ±1）。
    fn best_score_for(&self, key: &str, masks: &CellMasks, _kind: CellKind, icon_size: u32) -> f32 {
        let canvas_w = masks.width as u32;
        let canvas_h = masks.height as u32;
        let side = canvas_w.min(canvas_h);
        let mut best = f32::NEG_INFINITY;
        for delta in -REFINE_SCALE_RADIUS..=REFINE_SCALE_RADIUS {
            let size = icon_size.saturating_add_signed(delta);
            if size < 6 || size > side {
                continue;
            }
            let Some(template) = self.template(key, canvas_w, canvas_h, size) else {
                continue;
            };
            let score = best_score(&template, masks);
            if score.is_finite() && score > best {
                best = score;
            }
        }
        best
    }

    /// 整库打分。
    fn score_all(&self, masks: &CellMasks, cell: ImageRect, kind: CellKind) -> Vec<(String, f32)> {
        let icon_size = Self::icon_size(cell, kind);
        let canvas_w = masks.width as u32;
        let canvas_h = masks.height as u32;
        self.templates_for(kind, canvas_w, canvas_h, icon_size)
            .iter()
            .map(|t| (t.key.clone(), best_score(t, masks)))
            .collect()
    }
}

/// 单模板对格子的最佳分数：先按前景包围盒把模板**归一化映射**到格子坐标系，再在 ±1px 邻域取最大。
///
/// 为什么是包围盒归一化而不是"固定画布 + 居中"：
/// 检测出的格子矩形本身有 ±2px 的边长误差与约 −6px 的系统横向偏差（实机实测），
/// 而游戏卡片在格子里的真实渲染比例也无法从截图外部确知；
/// 包围盒归一化同时消除平移与尺度误差，只保留**形状与内部结构**的差异 ——
/// 卡片内部「白字图标 + 分类色剪影」的相对大小与位置仍然完整保留，判别信息不丢。
fn best_score(template: &PreparedTemplate, masks: &CellMasks) -> f32 {
    best_score_with_offsets(template, masks, SEARCH_OFFSETS.iter().copied())
}

/// 在给定的**有界**偏移候选集合内取最大分。
///
/// 偏移集合由调用方给出，且必然是常数级大小（见 [`alignment_offsets`]）；
/// 同一个 cell 的所有候选模板必须传入**同一个**集合，保证公平性
/// （plan3 REQ-MATCH-003 / P1.4）。
fn best_score_with_offsets(
    template: &PreparedTemplate,
    masks: &CellMasks,
    offsets: impl IntoIterator<Item = (i32, i32)>,
) -> f32 {
    if template.foreground_px() == 0 || masks.bbox.is_none() {
        return f32::NEG_INFINITY;
    }
    let mut best = f32::NEG_INFINITY;
    for (dx, dy) in offsets {
        let score = score_masks(template, masks, dx, dy);
        if score.is_finite() && score > best {
            best = score;
        }
    }
    best
}

/// 诊断/对照实验入口：把偏移半径作为参数暴露给探针。
///
/// 生产路径不调用它；它用于验证「扩大有界偏移邻域」是否让正确模板获益多于
/// 错误模板（plan3 §10 停止条件 3），而无需在生产常量上反复试参数。
#[cfg(test)]
fn score_with_offset_radius(template: &PreparedTemplate, masks: &CellMasks, radius: i32) -> f32 {
    best_score_with_offsets(template, masks, alignment_offsets(radius))
}

/// 前景包围盒是否可信：太小说明没提取到卡片（当成空槽/坏裁剪弃权），
/// 太大（几乎占满格子）说明把边框或格间分隔线也吞进来了。
fn foreground_bbox_sane(masks: &CellMasks) -> bool {
    let Some((x0, y0, x1, y1)) = masks.bbox else {
        return false;
    };
    let w = (x1 - x0 + 1) as f32 / masks.width as f32;
    let h = (y1 - y0 + 1) as f32 / masks.height as f32;
    (MIN_CARD_RATIO..=MAX_CARD_RATIO).contains(&w) && (MIN_CARD_RATIO..=MAX_CARD_RATIO).contains(&h)
}

/// 把模板掩码按「模板前景包围盒 → 格子前景包围盒」映射重采样到格子坐标系。
///
/// **保持纵横比**：把宽扁形状拉伸成正方形后，"宽条" 与 "任何宽条" 的 Dice 都会很高
/// （实机实测：正确项 0.45、错误项 0.96），纵横比本身是主要判别特征。
/// 做法是按 min(尺度) 等比缩放后居中放置。
fn map_template(template: &PreparedTemplate, masks: &CellMasks, dx: i32, dy: i32) -> MappedMasks {
    let (bx0, by0, bx1, by1) = template.bbox;
    let (bw, bh) = ((bx1 - bx0 + 1) as f32, (by1 - by0 + 1) as f32);
    let qb = masks.bbox.expect("调用方已确认格子存在前景");
    let (qw, qh) = ((qb.2 - qb.0 + 1) as f32, (qb.3 - qb.1 + 1) as f32);
    let canvas_w = template.canvas_w as usize;
    let canvas_h = template.canvas_h as usize;

    let mut white = vec![false; masks.width * masks.height];
    let mut color = vec![false; masks.width * masks.height];
    let (mut white_px, mut color_px) = (0usize, 0usize);

    // 等比尺度 + 居中（+ 精修偏移）
    let scale = (qw / bw).min(qh / bh);
    let placed_w = (bw * scale).round().max(1.0);
    let placed_h = (bh * scale).round().max(1.0);
    let origin_x = (qb.0 as f32 + (qw - placed_w) / 2.0).round() as i32 + dx;
    let origin_y = (qb.1 as f32 + (qh - placed_h) / 2.0).round() as i32 + dy;

    for ty in 0..(placed_h as i32) {
        for tx in 0..(placed_w as i32) {
            let src_x = (bx0 + (tx as f32 / scale).floor() as i32).clamp(0, canvas_w as i32 - 1);
            let src_y = (by0 + (ty as f32 / scale).floor() as i32).clamp(0, canvas_h as i32 - 1);
            let index = src_y as usize * canvas_w + src_x as usize;
            let x = origin_x + tx;
            let y = origin_y + ty;
            if x < 0 || y < 0 || x >= masks.width as i32 || y >= masks.height as i32 {
                continue;
            }
            let q_index = y as usize * masks.width + x as usize;
            if template.white[index] {
                white[q_index] = true;
                white_px += 1;
            }
            if template.color[index] {
                color[q_index] = true;
                color_px += 1;
            }
        }
    }
    MappedMasks {
        white,
        color,
        white_px,
        color_px,
    }
}

/// 映射到格子坐标系后的模板掩码。
struct MappedMasks {
    white: Vec<bool>,
    color: Vec<bool>,
    white_px: usize,
    color_px: usize,
}

impl MappedMasks {
    fn foreground_px(&self) -> usize {
        self.white_px + self.color_px
    }
}

/// 三个掩码 Dice 项的加权分数（∈[0,1]）。
///
/// * 分类色剪影 Dice —— 主项：色度把彩色字形从暗色格子底与灰色 UI 里分出来；
/// * 白字图标 Dice —— 次项：保留卡片内部「白字在上、剪影在下」的结构信息；
/// * 整体前景 Dice —— 兜底项：避免两项都因阈值边界而同时归零。
///
/// 权重按模板实际拥有的部分归一化（有些资源没有白字部分）。
fn score_masks(template: &PreparedTemplate, masks: &CellMasks, dx: i32, dy: i32) -> f32 {
    let mapped = map_template(template, masks, dx, dy);
    let dice = |hit: usize, a: usize, b: usize| -> f32 {
        if a + b == 0 {
            0.0
        } else {
            2.0 * hit as f32 / (a + b) as f32
        }
    };
    let mut color_hit = 0usize;
    let mut white_hit = 0usize;
    let mut shape_hit = 0usize;
    // 模板「白 ∪ 彩」与查询白区的命中数（灰度帧专用，见下）。
    let mut white_or_color_hit = 0usize;
    let (mut color_q, mut white_q, mut shape_q) = (0usize, 0usize, 0usize);
    for index in 0..masks.white.len() {
        let q_white = masks.white[index];
        let q_color = masks.color[index];
        let t_white = mapped.white[index];
        let t_color = mapped.color[index];
        if q_white {
            white_q += 1;
        }
        if q_color {
            color_q += 1;
        }
        if q_white || q_color {
            shape_q += 1;
        }
        if t_white && q_white {
            white_hit += 1;
        }
        if t_color && q_color {
            color_hit += 1;
        }
        if (t_white || t_color) && q_white {
            white_or_color_hit += 1;
        }
        if (t_white || t_color) && (q_white || q_color) {
            shape_hit += 1;
        }
    }

    let mut weight_sum = 0.0f32;
    let mut score = 0.0f32;
    if template.color_px > 0 && color_q > 0 {
        score += MASK_WEIGHT_COLOR * dice(color_hit, mapped.color_px, color_q);
        weight_sum += MASK_WEIGHT_COLOR;
    }
    if template.white_px > 0 && white_q > 0 {
        score += MASK_WEIGHT_WHITE * dice(white_hit, mapped.white_px, white_q);
        weight_sum += MASK_WEIGHT_WHITE;
    }
    if template.foreground_px() > 0 && shape_q > 0 {
        score += MASK_WEIGHT_SHAPE * dice(shape_hit, mapped.foreground_px(), shape_q);
        weight_sum += MASK_WEIGHT_SHAPE;
    }
    // 通道对齐：查询帧**完全没有彩色像素**时（灰度截图 / 合成夹具），模板的彩色字形
    // 在这帧里只能表现为「亮而不彩」，即落进查询白区，`MASK_WEIGHT_COLOR` 项因没有
    // 可比对象而缺席。此时若只看「模板白区 vs 查询白区」，模板彩区就没有任何约束
    // （实测模板白区 77px 对查询白区 371px，上限 0.34，正确目标 eagle_airstrike 被压到
    // 0.636）；因此在**原有白字项之外**追加一项「模板 白∪彩 vs 查询白区」，
    // 权重沿用 `MASK_WEIGHT_WHITE`（不引入新常数），由 `weight_sum` 自动归一化。
    // 彩色通道存在的帧（真实游戏截图实测 color≈400~575px）完全不进入这个分支，
    // 生产行为不变。
    // ponytail: 该分支按「整帧有无彩色像素」选择，不逐格自适应；
    // 若将来出现彩色/灰度混合帧，再按格分别选择口径。
    if color_q == 0 && template.color_px > 0 && white_q > 0 {
        score += MASK_WEIGHT_WHITE
            * dice(
                white_or_color_hit,
                mapped.white_px + mapped.color_px,
                white_q,
            );
        weight_sum += MASK_WEIGHT_WHITE;
    }
    if weight_sum <= 0.0 {
        return f32::NEG_INFINITY;
    }
    if std::env::var_os("H2AC_SCORE_TRACE").is_some() {
        eprintln!(
            "[trace] cq={color_q} wq={white_q} tcolor={} twhite={} twc_hit={white_or_color_hit} whisk={white_hit} shk={shape_hit} -> {:.3}",
            mapped.color_px,
            mapped.white_px,
            score / weight_sum
        );
    }

    score / weight_sum
}

// ─── 单格灰度特征（空槽相对标准差路径） ───

/// 单格特征：原生分辨率的灰度 + 梯度（用于空槽判定与诊断）。
#[derive(Debug, Clone)]
pub struct CellFeatures {
    gray: Vec<f32>,
    gradient: Vec<f32>,
    width: usize,
    height: usize,
    /// 相对标准差：空槽接近 0
    pub relative_std: f32,
}

impl CellFeatures {
    pub fn from_frame(frame: &CapturedFrame, cell: ImageRect) -> Option<Self> {
        Self::from_frame_inset(frame, cell, 0.0)
    }

    /// 与 [`Self::from_frame`] 相同，但四边先按比例内缩。
    pub fn from_frame_inset(
        frame: &CapturedFrame,
        cell: ImageRect,
        inset_ratio: f32,
    ) -> Option<Self> {
        Self::from_frame_with(frame, cell, inset_ratio, CELL_BLUR_RADIUS)
    }

    /// 完整构造：显式指定内缩比例与平滑半径（标定探针需要独立扫描这两个参数）。
    pub fn from_frame_with(
        frame: &CapturedFrame,
        cell: ImageRect,
        inset_ratio: f32,
        blur: i32,
    ) -> Option<Self> {
        let rect = inset_rect(cell, inset_ratio)
            .clamp_to_frame(frame.rgba.width() as i32, frame.rgba.height() as i32)?;
        let width = rect.w.max(1) as usize;
        let height = rect.h.max(1) as usize;
        if width < 8 || height < 8 {
            return None;
        }
        let mut gray = Vec::with_capacity(width * height);
        for y in 0..height {
            for x in 0..width {
                let p = frame
                    .rgba
                    .get_pixel((rect.x + x as i32) as u32, (rect.y + y as i32) as u32)
                    .0;
                gray.push(luma601(p[0], p[1], p[2]));
            }
        }
        let n = gray.len() as f32;
        let mean = gray.iter().sum::<f32>() / n;
        let var = gray.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / n;
        let std = var.sqrt();
        let gray = if blur > 0 {
            box_blur(&gray, width, height, blur)
        } else {
            gray
        };
        let gradient = sobel_magnitude(&gray, width, height);
        Some(Self {
            gray,
            gradient,
            width,
            height,
            relative_std: std / mean.max(1.0),
        })
    }

    pub fn is_empty_like(&self) -> bool {
        self.relative_std < EMPTY_RELATIVE_STD
    }
}

// ─── 辅助 ───

/// 四边按比例内缩的矩形（比例按宽高各自计算，夹在 0~0.45 之间）。
///
/// 内缩系数集中在这里：禁止在业务代码里散落 `rect.inset(6)` 这类魔法数字。
pub fn inset_rect(rect: ImageRect, ratio: f32) -> ImageRect {
    let ratio = if ratio.is_finite() {
        ratio.clamp(0.0, 0.45)
    } else {
        0.0
    };
    let dx = (rect.w as f32 * ratio).round() as i32;
    let dy = (rect.h as f32 * ratio).round() as i32;
    ImageRect::new(
        rect.x + dx,
        rect.y + dy,
        (rect.w - 2 * dx).max(1),
        (rect.h - 2 * dy).max(1),
    )
}

fn icon_path(key: &str) -> std::path::PathBuf {
    crate::util::app_dir()
        .join("assets/icons")
        .join(format!("{key}.png"))
}

fn luma601(r: u8, g: u8, b: u8) -> f32 {
    0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32
}

/// 由图标源构建模板：整张资源 → 缩放到 `icon_size` 正方形 → 居中合成到格子画布 → 分类成掩码。
///
/// 三条与「包围盒归一化 + 亮度余弦」旧实现的关键区别：
///   * 画布与格子等大、资源按**游戏实测比例**居中，不按 alpha 包围盒缩放；
///   * 只保留**形状**（白字 / 分类色两个掩码），不比较亮度绝对值
///     —— 实机彩色字形 luma≈226、资源彩色部分 luma≈146，亮度通道没有判别力；
///   * 掩码来自资源 alpha ∩ 分类结果，天然剔除格子边框、选中框与格间缝隙。
fn build_template(
    key: &str,
    source: &RgbaImage,
    canvas_w: u32,
    canvas_h: u32,
    icon_size: u32,
) -> Option<PreparedTemplate> {
    build_template_with(
        key,
        source,
        canvas_w,
        canvas_h,
        icon_size,
        PixelThresholds::PRODUCTION,
    )
}

/// 参数化模板构建：生产传 [`PixelThresholds::PRODUCTION`]，探针传候选组合。
fn build_template_with(
    key: &str,
    source: &RgbaImage,
    canvas_w: u32,
    canvas_h: u32,
    icon_size: u32,
    thresholds: PixelThresholds,
) -> Option<PreparedTemplate> {
    if canvas_w < 8 || canvas_h < 8 {
        return None;
    }
    let icon_size = icon_size.min(canvas_w.min(canvas_h));
    if icon_size < 6 {
        return None;
    }
    let scaled = resize_rgba(source, icon_size, icon_size);
    let mut white = vec![false; (canvas_w * canvas_h) as usize];
    let mut color = vec![false; (canvas_w * canvas_h) as usize];
    let offset_x = ((canvas_w - icon_size) / 2) as usize;
    let offset_y = ((canvas_h - icon_size) / 2) as usize;
    for y in 0..icon_size as usize {
        for x in 0..icon_size as usize {
            let p = scaled.get_pixel(x as u32, y as u32).0;
            if p[3] < ALPHA_MASK_THRESHOLD {
                continue;
            }
            let index = (y + offset_y) * canvas_w as usize + x + offset_x;
            match classify_pixel(p[0], p[1], p[2], thresholds) {
                PixelClass::White => white[index] = true,
                PixelClass::Color => color[index] = true,
                PixelClass::Background => {}
            }
        }
    }
    let white_px = white.iter().filter(|v| **v).count();
    let color_px = color.iter().filter(|v| **v).count();
    if white_px + color_px < MIN_COMPONENT_AREA {
        return None;
    }
    let mut bbox = (i32::MAX, i32::MAX, -1i32, -1i32);
    for y in 0..canvas_h as i32 {
        for x in 0..canvas_w as i32 {
            let index = (y as u32 * canvas_w + x as u32) as usize;
            if white[index] || color[index] {
                bbox.0 = bbox.0.min(x);
                bbox.1 = bbox.1.min(y);
                bbox.2 = bbox.2.max(x);
                bbox.3 = bbox.3.max(y);
            }
        }
    }
    Some(PreparedTemplate {
        key: key.to_string(),
        canvas_w: canvas_w as u16,
        canvas_h: canvas_h as u16,
        icon_size: icon_size as u16,
        white,
        color,
        white_px,
        color_px,
        bbox,
    })
}

/// 掩码内**单位 L2 归一化**：去均值后除以平方和的平方根 → 与另一向量点积即余弦相似度。
///
/// 仅单测使用（生产路径不做向量化余弦打分）。
#[cfg(test)]
fn unit_normalize(values: &[f32]) -> Option<Vec<f32>> {
    let n = values.len() as f32;
    if n < 4.0 {
        return None;
    }
    let mean = values.iter().sum::<f32>() / n;
    let norm = values
        .iter()
        .map(|v| (v - mean) * (v - mean))
        .sum::<f32>()
        .sqrt();
    if !norm.is_finite() || norm <= 1e-6 {
        return None;
    }
    Some(values.iter().map(|v| (v - mean) / norm).collect())
}

/// 均值平滑（抑制格子底纹这类高频纹理）。
fn box_blur(src: &[f32], width: usize, height: usize, radius: i32) -> Vec<f32> {
    if radius <= 0 {
        return src.to_vec();
    }
    let mut out = vec![0.0f32; src.len()];
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let mut sum = 0.0f32;
            let mut count = 0.0f32;
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    let sx = x + dx;
                    let sy = y + dy;
                    if sx >= 0 && sy >= 0 && sx < width as i32 && sy < height as i32 {
                        sum += src[sy as usize * width + sx as usize];
                        count += 1.0;
                    }
                }
            }
            out[y as usize * width + x as usize] = sum / count.max(1.0);
        }
    }
    out
}

/// Sobel 梯度幅值。
fn sobel_magnitude(gray: &[f32], width: usize, height: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; width * height];
    if width < 3 || height < 3 {
        return out;
    }
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let at = |dx: usize, dy: usize| gray[(y + dy - 1) * width + (x + dx - 1)];
            let gx = -at(0, 0) + at(2, 0) - 2.0 * at(0, 1) + 2.0 * at(2, 1) - at(0, 2) + at(2, 2);
            let gy = -at(0, 0) - 2.0 * at(1, 0) - at(2, 0) + at(0, 2) + 2.0 * at(1, 2) + at(2, 2);
            out[y * width + x] = (gx * gx + gy * gy).sqrt();
        }
    }
    out
}

/// 预乘 alpha 的 box 缩放（避免透明边缘产生黑色光晕）。
pub fn resize_rgba(src: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    if width == 0 || height == 0 || src.width() == 0 || src.height() == 0 {
        return RgbaImage::new(1, 1);
    }
    let sw = src.width() as f32;
    let sh = src.height() as f32;
    let mut out = RgbaImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let sx0 = (x as f32 * sw / width as f32).floor() as u32;
            let sx1 = (((x + 1) as f32 * sw / width as f32).ceil() as u32).min(src.width());
            let sy0 = (y as f32 * sh / height as f32).floor() as u32;
            let sy1 = (((y + 1) as f32 * sh / height as f32).ceil() as u32).min(src.height());
            let (mut r, mut g, mut b, mut a, mut n) = (0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32);
            for sy in sy0..sy1.max(sy0 + 1) {
                for sx in sx0..sx1.max(sx0 + 1) {
                    if sx >= src.width() || sy >= src.height() {
                        continue;
                    }
                    let p = src.get_pixel(sx, sy).0;
                    let w = p[3] as f32 / 255.0;
                    r += p[0] as f32 * w;
                    g += p[1] as f32 * w;
                    b += p[2] as f32 * w;
                    a += p[3] as f32;
                    n += 1.0;
                }
            }
            if n <= 0.0 {
                continue;
            }
            let alpha = a / n;
            let weight = alpha / 255.0;
            let (rr, gg, bb) = if weight > 1e-3 {
                (r / (n * weight), g / (n * weight), b / (n * weight))
            } else {
                (0.0, 0.0, 0.0)
            };
            out.put_pixel(
                x,
                y,
                image::Rgba([
                    rr.clamp(0.0, 255.0) as u8,
                    gg.clamp(0.0, 255.0) as u8,
                    bb.clamp(0.0, 255.0) as u8,
                    alpha.clamp(0.0, 255.0) as u8,
                ]),
            );
        }
    }
    out
}

#[cfg(test)]
impl IconMatcher {
    /// 对照实验：不同偏移半径下同一 cell 的**全部**候选模板分数。
    ///
    /// 用途是验证 plan3 §10 停止条件 3 ——「正确模板的提升必须多于错误模板」。
    /// 只观察，不改生产常量，也不改生产阈值。
    pub(crate) fn score_matrix_at_radius(
        &self,
        masks: &CellMasks,
        kind: CellKind,
        radius: i32,
    ) -> Vec<(String, f32)> {
        let canvas_w = masks.width as u32;
        let canvas_h = masks.height as u32;
        let side = canvas_w.min(canvas_h) as i32;
        let icon_size = ((side as f32) * kind.canvas_scale())
            .round()
            .clamp(6.0, side as f32) as u32;
        self.templates_for(kind, canvas_w, canvas_h, icon_size)
            .iter()
            .map(|t| (t.key.clone(), score_with_offset_radius(t, masks, radius)))
            .collect()
    }

    /// 诊断用：在给定像素分类阈值下，对整库候选重算分数（plan4 P1.3）。
    ///
    /// 关键点：**掩码与模板必须用同一套阈值**重新生成，
    /// 否则两侧的 white/color 分类口径不一致，对照失去意义。
    /// 只观察，不改生产常量，也不改缓存（走独立的临时模板）。
    pub(crate) fn score_matrix_with_thresholds(
        &self,
        frame: &CapturedFrame,
        cell: ImageRect,
        kind: CellKind,
        thresholds: PixelThresholds,
    ) -> Vec<(String, f32)> {
        let Some(masks) = CellMasks::from_frame_thresholds(frame, cell, thresholds) else {
            return Vec::new();
        };
        let canvas_w = masks.width as u32;
        let canvas_h = masks.height as u32;
        let side = canvas_w.min(canvas_h) as i32;
        let icon_size = ((side as f32) * kind.canvas_scale())
            .round()
            .clamp(6.0, side as f32) as u32;
        let mut out = Vec::with_capacity(self.sources.len());
        for key in self.sources.keys() {
            if CellKind::key_is_booster(key) != kind.is_booster() {
                continue;
            }
            let Some(source) = self.sources.get(key) else {
                continue;
            };
            let Some(t) =
                build_template_with(key, source, canvas_w, canvas_h, icon_size, thresholds)
            else {
                continue;
            };
            out.push((key.clone(), best_score(&t, &masks)));
        }
        out
    }

    /// 诊断用：拿到指定键在该格子几何下的模板摘要（与生产完全同一条生成路径）。
    ///
    /// 返回扁平摘要而不是 `PreparedTemplate` 本身，
    /// 这样 `tests.rs` 不需要把内部类型提升为 `pub(crate)`。
    pub(crate) fn template_probe_summary(
        &self,
        masks: &CellMasks,
        kind: CellKind,
        key: &str,
    ) -> Option<TemplateProbeSummary> {
        let side = masks.width.min(masks.height) as i32;
        let icon_size = ((side as f32) * kind.canvas_scale())
            .round()
            .clamp(6.0, side as f32) as u32;
        let template = self.template(key, masks.width as u32, masks.height as u32, icon_size)?;
        let mapped = map_template(&template, masks, 0, 0);
        Some(TemplateProbeSummary {
            key: key.to_string(),
            icon_size: template.icon_size,
            canvas: (template.canvas_w, template.canvas_h),
            bbox: template.bbox,
            white_px: template.white.iter().filter(|v| **v).count(),
            color_px: template.color.iter().filter(|v| **v).count(),
            mapped_white: mapped.white,
            mapped_color: mapped.color,
        })
    }
}

/// 模板诊断摘要（plan4 P0.2「模板」一节所需的全部字段）。
#[cfg(test)]
pub(crate) struct TemplateProbeSummary {
    pub key: String,
    pub icon_size: u16,
    pub canvas: (u16, u16),
    pub bbox: (i32, i32, i32, i32),
    pub white_px: usize,
    pub color_px: usize,
    /// 该模板在目标格坐标系下映射后的 white / color 通道。
    pub mapped_white: Vec<bool>,
    pub mapped_color: Vec<bool>,
}

#[cfg(test)]
impl CellMasks {
    pub(crate) fn probe_bbox(&self) -> Option<(i32, i32, i32, i32)> {
        self.bbox
    }
}

#[cfg(test)]
impl CellMasks {
    pub(crate) fn grid(&self) -> (&[bool], &[bool], usize, usize) {
        (&self.white, &self.color, self.width, self.height)
    }

    pub(crate) fn component_infos(&self) -> &[ComponentInfo] {
        &self.components
    }

    /// 取指定包围盒内的子网格（白字 / 分类色两通道）。
    pub(crate) fn sub_grid(
        &self,
        bbox: (i32, i32, i32, i32),
    ) -> (Vec<bool>, Vec<bool>, usize, usize) {
        let w = (bbox.2 - bbox.0 + 1).max(1) as usize;
        let h = (bbox.3 - bbox.1 + 1).max(1) as usize;
        let mut white = vec![false; w * h];
        let mut color = vec![false; w * h];
        for y in bbox.1..=bbox.3 {
            for x in bbox.0..=bbox.2 {
                if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
                    continue;
                }
                let s = y as usize * self.width + x as usize;
                let d = (y - bbox.1) as usize * w + (x - bbox.0) as usize;
                white[d] = self.white[s];
                color[d] = self.color[s];
            }
        }
        (white, color, w, h)
    }
}

/// 模板的两个通道（诊断用）。
#[cfg(test)]
pub(crate) struct TemplateParts {
    pub white: Vec<bool>,
    pub color: Vec<bool>,
    pub width: usize,
    pub height: usize,
    pub bbox: (i32, i32, i32, i32),
}

#[cfg(test)]
impl IconMatcher {
    /// 取指定键在给定格子几何下的模板通道（诊断/对照实验用）。
    pub(crate) fn template_parts(
        &self,
        frame: &CapturedFrame,
        cell: ImageRect,
        key: &str,
        glyph_h: u32,
    ) -> Option<TemplateParts> {
        let masks = CellMasks::from_frame(frame, cell)?;
        let canvas_w = masks.width as u32;
        let canvas_h = masks.height as u32;
        let icon_size = glyph_h.clamp(6, canvas_w.min(canvas_h));
        let template = self.template(key, canvas_w, canvas_h, icon_size)?;
        Some(TemplateParts {
            white: template.white.clone(),
            color: template.color.clone(),
            width: template.canvas_w as usize,
            height: template.canvas_h as usize,
            bbox: template.bbox,
        })
    }

    /// 诊断：把格子与模板的两个掩码导出成 PNG（给人看的）并返回文本 ASCII 图（给日志/文本模型看）。
    pub(crate) fn debug_dump_pair(
        &self,
        frame: &CapturedFrame,
        cell: ImageRect,
        key: &str,
        glyph_h: u32,
        path: &std::path::Path,
    ) -> String {
        let Some(masks) = CellMasks::from_frame(frame, cell) else {
            return String::from("(格子掩码提取失败)");
        };
        let canvas_w = masks.width as u32;
        let canvas_h = masks.height as u32;
        let icon_size = glyph_h.clamp(6, canvas_w.min(canvas_h));
        let Some(template) = self.template(key, canvas_w, canvas_h, icon_size) else {
            return String::from("(模板构建失败)");
        };
        let w = canvas_w as usize;
        let h = canvas_h as usize;
        let mut out = image::GrayImage::new((w as u32 + 2) * 2, (h as u32 + 2) * 2);
        let put = |out: &mut image::GrayImage, white: &[bool], color: &[bool], ox: u32, oy: u32| {
            for y in 0..h {
                for x in 0..w {
                    let index = y * w + x;
                    let v = if white.get(index).copied().unwrap_or(false) {
                        255
                    } else if color.get(index).copied().unwrap_or(false) {
                        128
                    } else {
                        0
                    };
                    out.put_pixel(ox + x as u32, oy + y as u32, image::Luma([v]));
                }
            }
        };
        put(&mut out, &masks.white, &masks.color, 1, 1);
        put(&mut out, &template.white, &template.color, w as u32 + 2, 1);
        let _ = out.save(path);
        let score = best_score(&template, &masks);
        format!(
            "格子掩码 (#=白字 *=分类色 . =背景):\n{}\n模板掩码:\n{}\nscore={score:.3} 模板前景={}px 格子前景={}px",
            masks.ascii(2),
            template_ascii(&template, 2),
            template.foreground_px(),
            masks.foreground_px()
        )
    }

    /// 诊断：打印每格掩码统计与逐模板的 Dice 分解（不需要看图）。
    pub(crate) fn debug_pair_stats(
        &self,
        frame: &CapturedFrame,
        cell: ImageRect,
        keys: &[&str],
        glyph_h: u32,
    ) {
        let Some(masks) = CellMasks::from_frame(frame, cell) else {
            eprintln!("格子掩码提取失败");
            return;
        };
        eprintln!(
            "格子前景: white={}px color={}px bbox={:?}",
            masks.white_px, masks.color_px, masks.bbox
        );
        for (id, c) in masks.components.iter().enumerate() {
            eprintln!(
                "  域{id}: area={} bbox=({},{},{},{}) white={} color={} kept={}",
                c.area, c.bbox.0, c.bbox.1, c.bbox.2, c.bbox.3, c.white_px, c.color_px, c.kept
            );
        }
        eprintln!("{}", masks.ascii(2));
        let canvas_w = masks.width as u32;
        let canvas_h = masks.height as u32;
        let icon_size = glyph_h.clamp(6, canvas_w.min(canvas_h));
        for key in keys {
            let Some(template) = self.template(key, canvas_w, canvas_h, icon_size) else {
                eprintln!("{key}: 模板构建失败");
                continue;
            };
            let score = best_score(&template, &masks);
            let mapped = map_template(&template, &masks, 0, 0);
            eprintln!(
                "{key}: 模板 white={}px color={}px bbox={:?} icon_size={} | 映射后 white={}px color={}px | score={score:.3}",
                template.white_px,
                template.color_px,
                template.bbox,
                template.icon_size,
                mapped.white_px,
                mapped.color_px
            );
            if *key == keys[0] {
                eprintln!("模板掩码:\n{}", template_ascii(&template, 2));
            }
        }
    }
}

#[cfg(test)]
fn template_ascii(template: &PreparedTemplate, step: usize) -> String {
    let w = template.canvas_w as usize;
    let h = template.canvas_h as usize;
    let step = step.max(1);
    let mut out = String::new();
    let mut y = 0;
    while y < h {
        let mut x = 0;
        while x < w {
            let index = y * w + x;
            out.push(if template.white[index] {
                '#'
            } else if template.color[index] {
                '*'
            } else {
                '.'
            });
            x += step;
        }
        out.push('\n');
        y += step;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_normalize_is_zero_mean_unit_norm() {
        let z = unit_normalize(&[1.0, 2.0, 3.0, 4.0]).expect("归一化");
        let mean = z.iter().sum::<f32>() / z.len() as f32;
        assert!(mean.abs() < 1e-6, "均值应为 0");
        let norm = z.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5, "L2 范数应为 1，实际 {norm}");
    }

    #[test]
    fn constant_values_have_no_direction() {
        assert!(unit_normalize(&[5.0, 5.0, 5.0, 5.0]).is_none());
    }

    #[test]
    fn resize_keeps_alpha_and_avoids_halo() {
        let mut src = RgbaImage::new(4, 4);
        for (x, _, p) in src.enumerate_pixels_mut() {
            let a = if x < 2 { 255 } else { 0 };
            *p = image::Rgba([200, 40, 40, a]);
        }
        let out = resize_rgba(&src, 2, 2);
        assert_eq!(out.dimensions(), (2, 2));
        let left = out.get_pixel(0, 0).0;
        assert!(left[3] > 200, "不透明区域 alpha 应保留");
        assert!(left[0] > 150, "颜色不应被透明像素拉黑");
    }

    #[test]
    fn pixel_classification_separates_white_and_category_color() {
        // 实机实测：白字图标 (248,248,243)、分类色图形 (170,250,251)
        assert_eq!(
            classify_pixel(248, 248, 243, PixelThresholds::PRODUCTION),
            PixelClass::White
        );
        assert_eq!(
            classify_pixel(170, 250, 251, PixelThresholds::PRODUCTION),
            PixelClass::Color
        );
        // 格子底色（暗灰）与亮边框（无彩色亮线）：
        assert_eq!(
            classify_pixel(80, 80, 78, PixelThresholds::PRODUCTION),
            PixelClass::Background
        );
        assert_eq!(
            classify_pixel(210, 210, 205, PixelThresholds::PRODUCTION),
            PixelClass::White
        );
    }

    #[test]
    fn inset_rect_clamps_ratio() {
        let r = ImageRect::new(10, 20, 100, 50);
        let inset = inset_rect(r, 0.10);
        assert_eq!(inset.x, 20);
        assert_eq!(inset.y, 25);
        assert_eq!(inset.w, 80);
        assert_eq!(inset.h, 40);
        // 非法比例退化为不内缩
        assert_eq!(inset_rect(r, f32::NAN), r);
    }

    #[test]
    fn search_offsets_match_alignment_offsets() {
        // 生产用的 SEARCH_OFFSETS 必须与 alignment_offsets(1) 逐项一致，
        // 否则「生产偏移」与「实验偏移」会各自漂移，对照实验失去意义。
        assert_eq!(SEARCH_OFFSETS.to_vec(), alignment_offsets(1));
    }

    #[test]
    fn alignment_offsets_are_bounded_and_deterministic() {
        // 候选数必须是常数级：radius=2 → 5×5，且 (0,0) 永远最优先
        assert_eq!(alignment_offsets(2).len(), 25);
        assert_eq!(alignment_offsets(1).len(), 9);
        assert_eq!(alignment_offsets(0), vec![(0, 0)]);
        assert_eq!(alignment_offsets(2)[0], (0, 0));
        // 超过硬上限会被夹住，防止有人把它当盲搜入口
        assert_eq!(
            alignment_offsets(99).len(),
            alignment_offsets(ALIGN_OFFSET_RADIUS).len()
        );
        // 确定性：两次调用顺序一致
        assert_eq!(alignment_offsets(2), alignment_offsets(2));
        // 距离单调不减（近的先试，保证同分时选最小位移）
        let dists: Vec<i32> = alignment_offsets(2)
            .iter()
            .map(|(x, y)| x.abs() + y.abs())
            .collect();
        assert!(
            dists.windows(2).all(|w| w[0] <= w[1]),
            "偏移应按距离从小到大"
        );
    }
}
