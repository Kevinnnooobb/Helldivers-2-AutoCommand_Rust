// 视觉识别配置 —— 全部阈值 / 权重 / 几何参数的唯一归属地。
//
// 设计约束（对应需求 §4 / §8 / §28 / §40）：
//   * 业务代码里禁止出现 `x = 105` / `0.42` 这类裸数字，一律引用本模块的字段；
//   * 全部字段可序列化（写进 config.json 的 `vision` 段），
//   * `sanitize()` 对非法值自愈（手改配置不允许让识别器“零阈值乱猜”或除零）。
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::loadout_sync::types::{ImageRect, RoiAnchor, ScaleAxis};

/// Bump when the serialized vision configuration changes incompatibly.
pub const VISION_CONFIG_SCHEMA_VERSION: u32 = 1;
/// Bump when prepared template/cache inputs or scoring semantics change.
pub const TEMPLATE_CACHE_SCHEMA_VERSION: u32 = 1;

// ─── 调试级别 ───

/// 调试产物级别。默认 `Off`：正常运行绝不写盘（需求 §24）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VisionDebugLevel {
    /// 不产出任何调试数据
    #[default]
    Off,
    /// 只产出最终结果（识别摘要 + 失败原因）
    Basic,
    /// 每个阶段一份产物（ROI / grid / 每格 mask / bbox / normalized / top-k）
    Detailed,
    /// Detailed + 逐格中间掩码（orange / white / components）与原始分数分解
    Trace,
}

impl VisionDebugLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Basic => "Basic",
            Self::Detailed => "Detailed",
            Self::Trace => "Trace",
        }
    }

    /// 是否需要写盘。
    pub fn writes_artifacts(self) -> bool {
        !matches!(self, Self::Off)
    }

    /// 是否需要每格的中间产物。
    pub fn per_cell_artifacts(self) -> bool {
        matches!(self, Self::Detailed | Self::Trace)
    }

    /// 是否需要分数分解（最啰嗦的一级）。
    pub fn trace_scores(self) -> bool {
        matches!(self, Self::Trace)
    }
}

// ─── 版面（StratagemLayout 的可序列化形式） ───

/// 一个网格（home 槽位区 / 列表区）的几何：全部坐标是**相对 ROI 左上角的参考像素**。
///
/// `column_offsets` 为空时用 `origin_x + col * pitch()` 推导；
/// 实机列距并非严格等距（实测 9/122/236/349，间距 113/114/113），
/// 因此默认提供精确列坐标，pitch 只作为兜底与行距来源。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GridProfile {
    pub origin_x: i32,
    pub origin_y: i32,
    pub cell_size: i32,
    pub cell_gap: i32,
    pub columns: u32,
    /// 固定行数；0 表示行数由滚动视口动态决定（列表区）。
    pub rows: u32,
    #[serde(default)]
    pub column_offsets: Vec<i32>,
    pub row_pitch: i32,
}

impl GridProfile {
    /// 标称列距（列坐标缺失时的兜底）。
    pub fn pitch(&self) -> i32 {
        (self.cell_size + self.cell_gap).max(1)
    }

    /// 第 `col` 列的左边界（相对 ROI，参考像素）。
    pub fn column_x(&self, col: u32) -> i32 {
        match self.column_offsets.get(col as usize) {
            Some(x) => *x,
            None => self.origin_x + col as i32 * self.pitch(),
        }
    }

    /// 第 `row` 行的上边界（相对 ROI，参考像素）。
    pub fn row_y(&self, row: u32) -> i32 {
        self.origin_y + row as i32 * self.row_pitch
    }

    /// 某格在参考坐标（ROI 内部）中的矩形。
    pub fn cell_rect(&self, row: u32, col: u32) -> ImageRect {
        ImageRect::new(
            self.column_x(col),
            self.row_y(row),
            self.cell_size,
            self.cell_size,
        )
    }
}

/// Booster 六边形槽的几何（以槽位外接方框为基准的比例参数）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoosterProfile {
    /// Booster 列左边界相对 home 第 4 列左边界的名义距离（参考像素）。
    pub column_offset: i32,
    pub hex_w_ratio: f32,
    pub hex_h_ratio: f32,
    pub center_dx: f32,
    pub center_dy: f32,
}

/// 内裁剪配置（需求 §5）。
///
/// 四边可分别用**百分比**（相对格子边长）或**像素**表达；
/// 优先级：像素 > 百分比（显式像素是调试时最可控的表达）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct InnerCropConfig {
    pub left_pct: f32,
    pub right_pct: f32,
    pub top_pct: f32,
    pub bottom_pct: f32,
    pub left_px: i32,
    pub right_px: i32,
    pub top_px: i32,
    pub bottom_px: i32,
    /// 单元格外框（白边框 / 黄选中框）额定宽度占格子边长的比例，
    /// 用于把「名义边框」之外的区域整体再收一次，避免把边框算进前景。
    pub border_ratio: f32,
}

impl Default for InnerCropConfig {
    fn default() -> Self {
        // 实机：列表格边框是亮白细线（luma 210+），选中框是黄色描边；
        // 10% 的四边内缩在实测里刚好切掉边框且不切图标（旧实现的 BORDER_INSET_RATIO 同源）。
        Self {
            left_pct: 0.10,
            right_pct: 0.10,
            top_pct: 0.10,
            bottom_pct: 0.10,
            left_px: 0,
            right_px: 0,
            top_px: 0,
            bottom_px: 0,
            border_ratio: 0.04,
        }
    }
}

/// 版面标定：参考坐标系 + ROI + 各网格几何 + 内裁剪（需求 §4 / §5）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutConfig {
    pub reference_w: u32,
    pub reference_h: u32,
    /// 参考帧坐标系里的战备面板 ROI（与 `loadout_sync::Calibration::roi` 同义）。
    pub panel_roi: ImageRect,
    pub scale_axis: ScaleAxis,
    pub anchor: RoiAnchor,
    /// home（配装主界面）4 个战备槽。
    pub home: GridProfile,
    /// 战备选择列表（可滚动）的列几何；行由视口动态给出。
    pub list: GridProfile,
    pub booster: BoosterProfile,
    /// 内裁剪：格 → 图标区。
    pub crop: InnerCropConfig,
    /// 是否用 `loadout_sync::recognizer` 的逐帧实测几何（边线响应）修正标定先验。
    pub use_verified_geometry: bool,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        // 数值来源：hd2-preset-helper 参考实现的 canonical ROI 几何（与其实机标定一致），
        // 单位是「ROI 参考坐标」（ROI 本身为 576×832，见 panel_roi）。
        // 槽位 104 + 间距 9 → 行距/列距 113，与实机 WGC 帧实测一致。
        Self {
            reference_w: 2560,
            reference_h: 1440,
            panel_roi: ImageRect::new(64, 480, 576, 832),
            scale_axis: ScaleAxis::Fit,
            anchor: RoiAnchor::TopCenter,
            home: GridProfile {
                origin_x: 11,
                origin_y: 636,
                cell_size: 104,
                cell_gap: 9,
                columns: 4,
                rows: 1,
                column_offsets: vec![11, 124, 237, 350],
                row_pitch: 113,
            },
            list: GridProfile {
                origin_x: 77,
                origin_y: 120,
                cell_size: 104,
                cell_gap: 9,
                columns: 4,
                rows: 0,
                column_offsets: vec![77, 190, 304, 417],
                row_pitch: 113,
            },
            booster: BoosterProfile {
                column_offset: 107,
                hex_w_ratio: 0.877,
                hex_h_ratio: 0.755,
                center_dx: -0.024,
                center_dy: 0.009,
            },
            crop: InnerCropConfig::default(),
            use_verified_geometry: true,
        }
    }
}

// ─── 几何检测（参考项目「固定锚点 + 边线响应验证」口径） ───

/// 参考实现 hd2-preset-helper 的几何检测参数（全部有实测来源）。
///
/// 工作方式（与参考实现完全一致）：
///   * 候选位置**固定**（标定给出的列/行区间），不做盲搜；
///   * 每个候选位置用「积分图 + 三带亮度窗口」测上下左右四条边的响应，
///     并通过边框一致性（36 段亮度离散度）打折 —— 这就是「验证」；
///   * 列表行在 y 区间内逐像素打分后，用 DP 在「最小行距」硬约束下挑最优行集合。
///
/// 所有坐标都是 ROI 参考像素（canonical ROI = 576×832）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GeometryConfig {
    /// canonical ROI 宽（参考实现固定 576）
    pub canonical_w: u32,
    /// canonical ROI 高（参考实现固定 832）
    pub canonical_h: u32,
    /// 槽位边长（参考实现 SLOT_SIZE_I32 = 104）
    pub slot_size: i32,
    /// 四列左边界（参考实现 LIST_COLS）
    pub list_cols: [i32; 4],
    /// home 四列左边界（参考实现 HOME_COLS）
    pub home_cols: [i32; 4],
    /// home Booster 列左边界（参考实现 HOME_BOOSTER_X = 457）
    pub home_booster_x: i32,
    /// home 行扫描区间（参考实现 620..=652，只允许 1 行）
    pub home_y_min: i32,
    pub home_y_max: i32,
    pub home_max_rows: usize,
    pub home_min_slots: usize,
    /// 列表行扫描区间（参考实现 120..=700，最多 10 行）
    pub list_y_min: i32,
    pub list_y_max: i32,
    pub list_max_rows: usize,
    pub list_min_slots: usize,
    /// 行距硬约束（参考实现 ROW_MIN_DIST = 113）
    pub row_min_dist: i32,
    /// 行候选的最低分（参考实现 ROW_THRESHOLD = 0.16）
    pub row_threshold: f32,
    /// 槽位最低分（参考实现 MIN_SLOT_SCORE = 0.26）
    pub min_slot_score: f32,
    /// 上下边线最低响应（参考实现 MIN_HORIZONTAL_EDGE = 0.24）
    pub min_horizontal_edge: f32,
    /// 左右边线最低响应（参考实现 MIN_SIDE = 0.16）
    pub min_side: f32,
    /// 水平三带窗口：窗口宽 / 线宽 / 侧带宽（参考实现 48 / 3 / 7）
    pub h_window: i32,
    pub h_line_h: i32,
    pub h_side_h: i32,
    /// 垂直三带窗口：窗口高 / 线宽 / 侧带宽（参考实现 48 / 3 / 7）
    pub v_window: i32,
    pub v_line_w: i32,
    pub v_side_w: i32,
    /// 响应归一化的数值保护项（参考实现 Z_EPS = 0.12）
    pub z_eps: f32,
    /// 响应 → 分数 的线性映射端点（参考实现 H 0.18/0.75、V 0.16/0.70）
    pub h_response_thr: f32,
    pub h_response_hi: f32,
    pub v_response_thr: f32,
    pub v_response_hi: f32,
    /// 边线带搜索半径（参考实现 EDGE_BAND = 1）
    pub edge_band: i32,
    /// 水平边线的中心偏置权重（参考实现 H_EDGE_CENTER_WEIGHT = 2.0）
    pub h_edge_center_weight: f32,
    /// 垂直边线取 top-k 平均的 k（参考实现 EDGE_BAND_TOPK = 2）
    pub edge_band_topk: usize,
    /// 每条边分成的段数（参考实现 SEGMENT_BINS = 10）
    pub segment_bins: usize,
    /// 垂直边跳过的中间段数（参考实现 V_SEGMENT_SKIP_CENTER_BINS = 2）
    pub v_segment_skip_center_bins: usize,
    /// 单段命中阈值（参考实现 SEGMENT_MIN_BIN_SCORE = 0.22）
    pub segment_min_bin_score: f32,
    /// 段均值与段命中率的组合权重（参考实现 0.65 / 0.35）
    pub segment_mean_weight: f32,
    pub segment_active_weight: f32,
    /// 槽位总分权重（参考实现 0.55 / 0.25 / 0.15 / 0.05）
    pub tb_min_weight: f32,
    pub tb_mean_weight: f32,
    pub side_mean_weight: f32,
    pub side_best_weight: f32,
    /// 边框一致性：参与统计的 36 段里裁掉最偏的 N 段（参考实现 8）
    pub border_uniformity_trim: usize,
    /// 边框一致性：中位数下限 / 相对标准差的好、坏端点 / 最大惩罚（参考实现 0.08 / 0.10 / 0.30 / 0.60）
    pub border_uniformity_luma_floor: f32,
    pub border_uniformity_good: f32,
    pub border_uniformity_bad: f32,
    pub border_uniformity_max_penalty: f32,
    /// home 槽位内容检查：内缩像素 / 均值下限 / 相对标准差下限（参考实现 19 / 0.08 / 0.15）
    pub home_content_inset: i32,
    pub home_content_mean_floor: f32,
    pub home_content_min_relative_std: f32,
    /// Booster 六边形内容区（参考实现 0.5000 / 0.5048 / 0.4250×0.90）
    pub booster_hex_center_x: f32,
    pub booster_hex_center_y: f32,
    pub booster_content_side_len: f32,
    /// Booster 判定为「已装备」所需的最小黄色占比（参考实现 0.40）
    pub booster_min_yellow_ratio: f32,
    /// 验证失败时是否回退到纯标定坐标（回退结果会被标记为未验证，score = 0）。
    ///
    /// 默认 true：调试与界面预览仍需一个可用坐标；自动化侧可以检查
    /// `SlotPlan::geometry_source` 决定是否拒绝。
    pub fallback_to_calibration: bool,
}

impl Default for GeometryConfig {
    fn default() -> Self {
        Self {
            canonical_w: 576,
            canonical_h: 832,
            slot_size: 104,
            list_cols: [77, 190, 304, 417],
            home_cols: [11, 124, 237, 350],
            home_booster_x: 457,
            home_y_min: 620,
            home_y_max: 652,
            home_max_rows: 1,
            home_min_slots: 4,
            list_y_min: 120,
            list_y_max: 700,
            list_max_rows: 10,
            list_min_slots: 1,
            row_min_dist: 113,
            row_threshold: 0.16,
            min_slot_score: 0.26,
            min_horizontal_edge: 0.24,
            min_side: 0.16,
            h_window: 48,
            h_line_h: 3,
            h_side_h: 7,
            v_window: 48,
            v_line_w: 3,
            v_side_w: 7,
            z_eps: 0.12,
            h_response_thr: 0.18,
            h_response_hi: 0.75,
            v_response_thr: 0.16,
            v_response_hi: 0.70,
            edge_band: 1,
            h_edge_center_weight: 2.0,
            edge_band_topk: 2,
            segment_bins: 10,
            v_segment_skip_center_bins: 2,
            segment_min_bin_score: 0.22,
            segment_mean_weight: 0.65,
            segment_active_weight: 0.35,
            tb_min_weight: 0.55,
            tb_mean_weight: 0.25,
            side_mean_weight: 0.15,
            side_best_weight: 0.05,
            border_uniformity_trim: 8,
            border_uniformity_luma_floor: 0.08,
            border_uniformity_good: 0.10,
            border_uniformity_bad: 0.30,
            border_uniformity_max_penalty: 0.60,
            home_content_inset: 19,
            home_content_mean_floor: 0.08,
            home_content_min_relative_std: 0.15,
            booster_hex_center_x: 0.5000,
            booster_hex_center_y: 0.5048,
            booster_content_side_len: 0.4250 * 0.90,
            booster_min_yellow_ratio: 0.40,
            fallback_to_calibration: true,
        }
    }
}

impl GeometryConfig {
    /// 每条边参与一致性统计的段数（与参考实现一致：4×10 − 2×2 = 36）。
    pub fn border_segments(&self) -> usize {
        self.segment_bins * 4 - 2 * self.v_segment_skip_center_bins
    }

    /// 裁掉最偏的 N 段后剩下的段数。
    pub fn border_retained(&self) -> usize {
        self.border_segments()
            .saturating_sub(self.border_uniformity_trim)
            .max(1)
    }
}

// ─── 分割（需求 §8 / §9 / §10） ───

/// HSV 阈值（H ∈ [0,360) 度，S/V ∈ [0,1]）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HsvRange {
    pub h_min: f32,
    pub h_max: f32,
    pub s_min: f32,
    pub s_max: f32,
    pub v_min: f32,
    pub v_max: f32,
}

impl HsvRange {
    /// 色相是否命中（支持跨 0° 的区间，例如 350°~20°）。
    pub fn matches(&self, h: f32, s: f32, v: f32) -> bool {
        if s < self.s_min || s > self.s_max || v < self.v_min || v > self.v_max {
            return false;
        }
        if self.h_min <= self.h_max {
            h >= self.h_min && h <= self.h_max
        } else {
            h >= self.h_min || h <= self.h_max
        }
    }
}

/// Lab 参考色及其最大容忍距离（CIELAB，D65）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LabReference {
    pub l: f32,
    pub a: f32,
    pub b: f32,
    /// 距离 <= `distance_full` 得满分，>= `distance_zero` 得 0 分，中间平滑过渡。
    pub distance_full: f32,
    pub distance_zero: f32,
}

/// 归一化色度参考（r,g,b 各自除以和）+ 亮度斜坡 + 色度距离 —— 参考实现的 `ColorProfile`。
///
/// 与 Lab 距离的区别：这里比较的是「色度方向」而不是绝对色差，
/// 因此对整体亮度缩放（HDR / 不同地图光照）几乎不敏感。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ChromaProfile {
    pub chroma: [f32; 3],
    pub luma_low: f32,
    pub luma_full: f32,
    /// 色度距离 <= distance_full 得满分，>= distance_zero 得 0 分
    pub distance_full: f32,
    pub distance_zero: f32,
    /// 判定「命中该颜色」的分数下限
    pub min_likeness: f32,
}

impl ChromaProfile {
    /// 参考实现的 Booster 黄色（255,222,38）。
    pub const BOOSTER_YELLOW: Self = Self {
        chroma: [255.0 / 515.0, 222.0 / 515.0, 38.0 / 515.0],
        luma_low: 75.0,
        luma_full: 160.0,
        distance_full: 0.025,
        distance_zero: 0.100,
        min_likeness: 0.5,
    };
}

/// 前景评分的四路权重（会自动按可用通道归一化）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScoreWeights {
    pub orange: f32,
    pub white: f32,
    pub contrast: f32,
    pub center: f32,
    /// 相对亮度证据（相对格内稳健背景的亮度超额）——实机图标主体是「浅灰字形 +
    /// 铜色底座」，两者色度差异极大但都比格底亮，绝对颜色阈值必然漏掉其中之一。
    #[serde(default = "default_luma_weight")]
    pub luma: f32,
}

fn default_luma_weight() -> f32 {
    0.55
}

fn default_background_percentile() -> f32 {
    0.25
}

fn default_luma_delta() -> f32 {
    0.10
}

impl Default for ScoreWeights {
    fn default() -> Self {
        Self {
            orange: 0.30,
            white: 0.22,
            contrast: 0.13,
            center: 0.10,
            luma: default_luma_weight(),
        }
    }
}

/// 分割参数。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SegmentationConfig {
    /// 橙色/桃色前景（战备图标的橙色主体，H 约 15°~45°）。
    pub orange_hsv: HsvRange,
    /// 橙色前景的 Lab 参考（H2AC 资源实测 #E8A05A 一族）。
    pub orange_lab: LabReference,
    /// 白色/浅色前景（低饱和 + 高亮）。
    pub white_value_min: f32,
    pub white_saturation_max: f32,
    pub white_lab: LabReference,
    /// 局部对比度：与局部均值的差异（抑制大面积底色、保留内部结构）。
    pub contrast_radius: i32,
    pub contrast_threshold: f32,
    pub contrast_gain: f32,
    /// 背景亮度分位数（0~0.5）：低于该分位的像素被视为「格子底色」，
    /// 取该分位作为背景亮度参考。实机格子底色是暗色，图标主体明显更亮。
    #[serde(default = "default_background_percentile")]
    pub background_percentile: f32,
    /// 相对背景亮度需要超出的最小值（过度区宽度为其一半）。
    #[serde(default = "default_luma_delta")]
    pub luma_delta: f32,
    /// 中心先验：越靠近格子中心权重越高（sigma 为格子边长比例）。
    pub center_sigma: f32,
    pub weights: ScoreWeights,
    /// 前景判定阈值（前景分数 > 该值）。
    pub foreground_threshold: f32,
    /// 采样区四边内缩（去掉贴边 UI）。
    pub border_inset_ratio: f32,
    /// Booster 黄色判定（参考实现口径：色度方向 + 亮度斜坡）
    pub booster_yellow: ChromaProfile,
}

impl Default for SegmentationConfig {
    fn default() -> Self {
        Self {
            orange_hsv: HsvRange {
                h_min: 12.0,
                h_max: 48.0,
                s_min: 0.28,
                s_max: 1.0,
                v_min: 0.35,
                v_max: 1.0,
            },
            orange_lab: LabReference {
                l: 74.0,
                a: 20.0,
                b: 44.0,
                distance_full: 34.0,
                distance_zero: 92.0,
            },
            // 真实 1920x1080 HDR 截图中的浅色图标通常只有约
            // 150~170/255 的值；过高的门槛会把整张图标误判为空槽。
            white_value_min: 0.45,
            // 白字图标通常低饱和；分类色图形由 HSV/Lab 与局部对比度
            // 共同提供证据，不依赖单一亮度阈值。
            white_saturation_max: 0.22,
            white_lab: LabReference {
                l: 92.0,
                a: 0.0,
                b: 4.0,
                distance_full: 18.0,
                distance_zero: 54.0,
            },
            contrast_radius: 3,
            contrast_threshold: 0.04,
            contrast_gain: 2.2,
            background_percentile: default_background_percentile(),
            luma_delta: default_luma_delta(),
            center_sigma: 0.62,
            weights: ScoreWeights::default(),
            foreground_threshold: 0.30,
            border_inset_ratio: 0.08,
            booster_yellow: ChromaProfile::BOOSTER_YELLOW,
        }
    }
}

// ─── 形态学（需求 §11） ───

/// 形态学参数：默认只用 3×3 核、迭代 1 次（需求明令禁止 >5×5 的默认核）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MorphologyConfig {
    /// 开运算核半径（1 → 3×3）。
    pub open_radius: i32,
    /// 闭运算核半径（1 → 3×3）。
    pub close_radius: i32,
    pub open_iterations: u32,
    pub close_iterations: u32,
    /// 只清除孤立噪点、保留细笔画（实机图标笔画只有 2px 宽，各向同性开运算
    /// 会把整条笔画腐蚀掉；该项为 true 时用「孤立点清除」代替 3×3 开运算）。
    #[serde(default = "default_true")]
    pub open_preserve_thin: bool,
}

fn default_true() -> bool {
    true
}

impl Default for MorphologyConfig {
    fn default() -> Self {
        Self {
            open_radius: 1,
            close_radius: 1,
            open_iterations: 1,
            close_iterations: 1,
            open_preserve_thin: true,
        }
    }
}

// ─── 连通域（需求 §12） ───

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ComponentConfig {
    pub min_area_px: usize,
    pub min_area_ratio: f32,
    /// 面积分数的「满分」占比：达到该占比即得满分，避免过大分量获得额外奖励。
    pub good_area_ratio: f32,
    pub max_area_ratio: f32,
    pub min_fill_ratio: f32,
    pub max_aspect_ratio: f32,
    /// 线伪影判定：细边上限 + 长度占格子比例上限。
    pub line_thin_max: i32,
    pub line_length_ratio: f32,
    /// 分量打分权重。
    pub weight_area: f32,
    pub weight_center: f32,
    pub weight_shape: f32,
    pub weight_color: f32,
    /// 多分量合并：分量间距（含边框）小于该比例 × 格子边长时合并。
    pub merge_distance_ratio: f32,
    /// 合并后允许的最大分量数（超过则只保留分数最高的若干个）。
    pub max_merged_components: usize,
    /// 分量接受下限。
    pub min_component_score: f32,
}

impl Default for ComponentConfig {
    fn default() -> Self {
        Self {
            min_area_px: 12,
            min_area_ratio: 0.0025,
            good_area_ratio: 0.08,
            max_area_ratio: 0.92,
            min_fill_ratio: 0.10,
            max_aspect_ratio: 6.0,
            line_thin_max: 3,
            line_length_ratio: 0.55,
            weight_area: 0.30,
            weight_center: 0.25,
            weight_shape: 0.20,
            weight_color: 0.25,
            merge_distance_ratio: 0.22,
            max_merged_components: 8,
            min_component_score: 0.30,
        }
    }
}

// ─── 归一化（需求 §13 / §14） ───

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NormalizationConfig {
    /// 归一化画布边长（需求指定 128）。
    pub size: u32,
    /// 图标包围盒相对画布的留白比例（等比缩放后居中）。
    pub padding_ratio: f32,
    /// 边缘图阈值（Sobel 幅值归一化后）。
    pub edge_threshold: f32,
    /// 掩码前景判定阈值（前景分数）。
    pub mask_threshold: f32,
}

impl Default for NormalizationConfig {
    fn default() -> Self {
        Self {
            size: 128,
            padding_ratio: 0.06,
            edge_threshold: 0.18,
            mask_threshold: 0.35,
        }
    }
}

// ─── 模板（需求 §15 / §16） ───

/// 多度量权重（会自动按可用度量归一化）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TemplateWeights {
    /// 掩码 IoU / Dice（形状主体）
    pub mask: f32,
    /// 边缘图相似度（内部结构）
    pub edge: f32,
    /// Hu 矩形状相似度（对局部缺失稳健）
    pub shape: f32,
    /// 感知哈希相似度（整体版式）
    pub perceptual: f32,
}

impl Default for TemplateWeights {
    fn default() -> Self {
        Self {
            mask: 0.45,
            edge: 0.25,
            shape: 0.15,
            perceptual: 0.15,
        }
    }
}

/// 模板变体：同一战备的不同渲染状态（需求 §15 要求每图标多模板）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateVariant {
    /// 常规渲染
    Normal,
    /// 亮度偏移（HDR / 不同地图光照的近似）
    Brightness,
    /// 缩放偏移（格子边长误差的近似）
    Scale,
}

impl TemplateVariant {
    pub fn all() -> &'static [TemplateVariant] {
        &[
            TemplateVariant::Normal,
            TemplateVariant::Brightness,
            TemplateVariant::Scale,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Brightness => "brightness",
            Self::Scale => "scale",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplateConfig {
    /// 模板来源：内嵌 `assets/icons` 图标库。
    pub use_embedded_library: bool,
    /// 额外模板目录（`assets/stratagems/{id}/`，需求 §15 的数据库布局）。
    pub extra_dir: Option<PathBuf>,
    pub variants: Vec<TemplateVariant>,
    /// 变体的亮度偏移量（±）。
    pub brightness_delta: f32,
    /// 变体的缩放偏移比例（±）。
    pub scale_delta: f32,
    pub weights: TemplateWeights,
    /// 掩码 Iou 计算前对两者取并集的最小前景像素数（低于则判模板不可用）。
    pub min_foreground_px: usize,
}

impl Default for TemplateConfig {
    fn default() -> Self {
        Self {
            use_embedded_library: true,
            extra_dir: None,
            variants: vec![
                TemplateVariant::Normal,
                TemplateVariant::Brightness,
                TemplateVariant::Scale,
            ],
            brightness_delta: 0.18,
            scale_delta: 0.06,
            weights: TemplateWeights::default(),
            min_foreground_px: 24,
        }
    }
}

// ─── 识别与融合（需求 §19 / §20 / §21） ───

/// 各识别器的融合权重（缺项的识别器按可用性归一化）。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FusionWeights {
    pub template: f32,
    pub embedding: f32,
    pub classifier: f32,
    /// 几何有效性（裁剪/包围盒/前景比例是否合理）—— 不是类别证据，但影响最终可信度。
    pub geometry: f32,
}

impl Default for FusionWeights {
    fn default() -> Self {
        Self {
            template: 0.80,
            embedding: 0.00,
            classifier: 0.00,
            geometry: 0.20,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RecognitionConfig {
    /// 接受为「已识别」的最低融合置信度。
    pub min_confidence: f32,
    /// top-1 与 top-2 的最小间隔（低于即判歧义，必须弃权）。
    pub min_margin: f32,
    /// 视为「同一字形的重复登记」的掩码 IoU 下限：达到该值的候选互不算证据冲突，
    /// margin 会跳过它们去和真正的下一个不同形状候选比较。
    #[serde(default = "default_dup_mask_iou")]
    pub dup_mask_iou: f32,
    /// 返回多少个备选。
    pub top_k: usize,
    /// 空槽判定：前景像素占比下限。
    pub empty_foreground_ratio: f32,
    /// 空槽判定：最大连通域面积占格子面积的下限。
    pub empty_max_component_ratio: f32,
    /// 空槽判定：中心区域（格子中央 50%）前景占比下限。
    pub empty_center_ratio: f32,
    pub weights: FusionWeights,
    /// 允许只靠模板匹配给结论（第一阶段必须为 true）。
    pub allow_template_only: bool,
}

impl Default for RecognitionConfig {
    fn default() -> Self {
        Self {
            min_confidence: 0.62,
            min_margin: 0.035,
            dup_mask_iou: default_dup_mask_iou(),
            top_k: 3,
            empty_foreground_ratio: 0.012,
            empty_max_component_ratio: 0.010,
            empty_center_ratio: 0.004,
            weights: FusionWeights::default(),
            allow_template_only: true,
        }
    }
}

fn default_dup_mask_iou() -> f32 {
    0.93
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DebugConfig {
    pub level: VisionDebugLevel,
    /// 产物根目录（默认 exe 目录下 `vision_debug/`）。
    #[serde(default = "default_dump_dir")]
    pub dump_dir: PathBuf,
    /// 识别失败时即使 level=Basic/Detailed 之外也落盘一次。
    pub dump_on_failure: bool,
    /// 单次运行的产物目录数量上限（防止磁盘被写满）。
    pub max_dump_sessions: usize,
}

fn default_dump_dir() -> PathBuf {
    crate::util::app_dir().join("vision_debug")
}

impl Default for DebugConfig {
    fn default() -> Self {
        Self {
            level: VisionDebugLevel::Off,
            dump_dir: default_dump_dir(),
            dump_on_failure: true,
            max_dump_sessions: 20,
        }
    }
}

// ─── 总配置 ───

/// 视觉识别系统总配置（需求 §28）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct VisionConfig {
    /// Version of the persisted vision configuration schema.
    #[serde(default = "default_vision_config_schema_version")]
    pub schema_version: u32,
    /// Version namespace for prepared template data and recognition caches.
    #[serde(default = "default_template_cache_schema_version")]
    pub template_cache_version: u32,
    #[serde(default)]
    pub layout: LayoutConfig,
    #[serde(default)]
    pub geometry: GeometryConfig,
    #[serde(default)]
    pub segmentation: SegmentationConfig,
    #[serde(default)]
    pub morphology: MorphologyConfig,
    #[serde(default)]
    pub component: ComponentConfig,
    #[serde(default)]
    pub normalization: NormalizationConfig,
    #[serde(default)]
    pub template: TemplateConfig,
    #[serde(default)]
    pub recognition: RecognitionConfig,
    #[serde(default)]
    pub debug: DebugConfig,
}

impl VisionConfig {
    /// 归一化非法值：任何一项异常都不允许让识别器「零阈值接受」或除零。
    pub fn sanitize(mut self) -> Self {
        // Older config files deserialize with their historical defaults; always
        // normalize them into the current namespace before recognition starts.
        self.schema_version = VISION_CONFIG_SCHEMA_VERSION;
        self.template_cache_version = TEMPLATE_CACHE_SCHEMA_VERSION;

        // 几何检测：数值关系必须自洽（否则边线窗口会退化）
        let g = &mut self.geometry;
        g.canonical_w = g.canonical_w.clamp(64, 8192);
        g.canonical_h = g.canonical_h.clamp(64, 8192);
        g.slot_size = g.slot_size.clamp(8, 2048);
        g.edge_band = g.edge_band.clamp(0, 4);
        g.h_line_h = g.h_line_h.clamp(1, 31);
        g.h_side_h = g.h_side_h.clamp(1, 63);
        g.h_window = g.h_window.clamp(g.h_line_h, 512);
        g.v_line_w = g.v_line_w.clamp(1, 31);
        g.v_side_w = g.v_side_w.clamp(1, 63);
        g.v_window = g.v_window.clamp(g.v_line_w, 512);
        g.segment_bins = g.segment_bins.clamp(2, 64);
        g.v_segment_skip_center_bins = g
            .v_segment_skip_center_bins
            .min(g.segment_bins.saturating_sub(1));
        if g.border_uniformity_trim >= g.border_segments() {
            g.border_uniformity_trim = 0;
        }

        g.home_max_rows = g.home_max_rows.clamp(1, 8);
        g.list_max_rows = g.list_max_rows.clamp(1, 32);
        g.home_min_slots = g.home_min_slots.clamp(1, 4);
        g.list_min_slots = g.list_min_slots.clamp(1, 4);
        if g.home_y_max < g.home_y_min {
            g.home_y_max = g.home_y_min;
        }
        if g.list_y_max < g.list_y_min {
            g.list_y_max = g.list_y_min;
        }
        g.row_min_dist = g.row_min_dist.clamp(4, 1024);
        g.row_threshold = clamp01(g.row_threshold);
        g.min_slot_score = clamp01(g.min_slot_score);
        g.min_horizontal_edge = clamp01(g.min_horizontal_edge);
        g.min_side = clamp01(g.min_side);
        g.h_response_hi = g.h_response_hi.max(g.h_response_thr + 1e-3);
        g.v_response_hi = g.v_response_hi.max(g.v_response_thr + 1e-3);
        g.z_eps = g.z_eps.max(1e-3);
        g.segment_mean_weight = g.segment_mean_weight.max(0.0);
        g.segment_active_weight = g.segment_active_weight.max(0.0);
        if g.segment_mean_weight + g.segment_active_weight <= 0.0 {
            g.segment_mean_weight = 0.65;
            g.segment_active_weight = 0.35;
        }
        g.booster_content_side_len = g.booster_content_side_len.clamp(0.05, 0.49);
        g.booster_min_yellow_ratio = clamp01(g.booster_min_yellow_ratio);

        // 版面：参考尺寸与格子必须为正
        self.layout.reference_w = self.layout.reference_w.clamp(320, 16_384);
        self.layout.reference_h = self.layout.reference_h.clamp(240, 16_384);
        sanitize_grid(&mut self.layout.home);
        sanitize_grid(&mut self.layout.list);
        if self.layout.panel_roi.w <= 0 || self.layout.panel_roi.h <= 0 {
            self.layout.panel_roi = LayoutConfig::default().panel_roi;
        }
        let crop = &mut self.layout.crop;
        crop.left_pct = clamp01(crop.left_pct);
        crop.right_pct = clamp01(crop.right_pct);
        crop.top_pct = clamp01(crop.top_pct);
        crop.bottom_pct = clamp01(crop.bottom_pct);
        crop.border_ratio = clamp01(crop.border_ratio);
        // 四边合计内缩不得超过格子的一半，否则内裁剪会退化成空矩形
        for pair in [
            (crop.left_pct, crop.right_pct),
            (crop.top_pct, crop.bottom_pct),
        ] {
            if pair.0 + pair.1 + 2.0 * crop.border_ratio > 0.75 {
                crop.left_pct = 0.10;
                crop.right_pct = 0.10;
                crop.top_pct = 0.10;
                crop.bottom_pct = 0.10;
                crop.border_ratio = 0.04;
                break;
            }
        }
        for px in [
            &mut crop.left_px,
            &mut crop.right_px,
            &mut crop.top_px,
            &mut crop.bottom_px,
        ] {
            *px = (*px).clamp(0, 512);
        }

        // 分割
        let seg = &mut self.segmentation;
        seg.border_inset_ratio = seg.border_inset_ratio.clamp(0.0, 0.35);
        seg.contrast_radius = seg.contrast_radius.clamp(0, 12);
        seg.contrast_threshold = clamp01(seg.contrast_threshold);
        seg.contrast_gain = seg.contrast_gain.clamp(0.0, 8.0);
        seg.center_sigma = seg.center_sigma.clamp(0.15, 3.0);
        seg.foreground_threshold = clamp01(seg.foreground_threshold).max(0.05);
        seg.white_value_min = clamp01(seg.white_value_min);
        seg.white_saturation_max = clamp01(seg.white_saturation_max);
        for w in [
            &mut seg.weights.orange,
            &mut seg.weights.white,
            &mut seg.weights.contrast,
            &mut seg.weights.center,
        ] {
            *w = w.max(0.0);
        }
        if seg.weights.orange + seg.weights.white + seg.weights.contrast + seg.weights.center <= 0.0
        {
            seg.weights = ScoreWeights::default();
        }

        // 形态学：默认核不得超过 5×5（半径 2）
        self.morphology.open_radius = self.morphology.open_radius.clamp(0, 2);
        self.morphology.close_radius = self.morphology.close_radius.clamp(0, 2);
        self.morphology.open_iterations = self.morphology.open_iterations.clamp(0, 3);
        self.morphology.close_iterations = self.morphology.close_iterations.clamp(0, 3);

        // 连通域
        let comp = &mut self.component;
        comp.min_area_px = comp.min_area_px.clamp(1, 100_000);
        comp.min_area_ratio = clamp01(comp.min_area_ratio);
        comp.good_area_ratio = comp.good_area_ratio.clamp(comp.min_area_ratio, 1.0);
        comp.max_area_ratio = comp
            .max_area_ratio
            .clamp(comp.min_area_ratio.max(0.05), 1.0);
        comp.min_fill_ratio = clamp01(comp.min_fill_ratio);
        comp.max_aspect_ratio = comp.max_aspect_ratio.clamp(1.0, 50.0);
        comp.line_thin_max = comp.line_thin_max.clamp(1, 8);
        comp.line_length_ratio = clamp01(comp.line_length_ratio);
        comp.merge_distance_ratio = clamp01(comp.merge_distance_ratio);
        comp.max_merged_components = comp.max_merged_components.clamp(1, 32);
        comp.min_component_score = clamp01(comp.min_component_score);
        for w in [
            &mut comp.weight_area,
            &mut comp.weight_center,
            &mut comp.weight_shape,
            &mut comp.weight_color,
        ] {
            *w = w.max(0.0);
        }

        // 归一化
        self.normalization.size = self.normalization.size.clamp(16, 512);
        self.normalization.padding_ratio = clamp01(self.normalization.padding_ratio);
        self.normalization.edge_threshold = clamp01(self.normalization.edge_threshold);
        self.normalization.mask_threshold = clamp01(self.normalization.mask_threshold);

        // 模板
        self.template.brightness_delta = self.template.brightness_delta.clamp(0.0, 0.6);
        self.template.scale_delta = self.template.scale_delta.clamp(0.0, 0.30);
        self.template.weights = sanitize_template_weights(self.template.weights);
        if self.template.variants.is_empty() {
            self.template.variants = TemplateConfig::default().variants;
        }

        // 识别
        let rec = &mut self.recognition;
        rec.min_confidence = clamp01(rec.min_confidence).clamp(0.05, 0.99);
        rec.min_margin = rec.min_margin.clamp(0.0, 0.5);
        rec.top_k = rec.top_k.clamp(1, 10);
        rec.empty_foreground_ratio = clamp01(rec.empty_foreground_ratio);
        rec.empty_max_component_ratio = clamp01(rec.empty_max_component_ratio);
        rec.empty_center_ratio = clamp01(rec.empty_center_ratio);
        rec.weights = sanitize_fusion_weights(rec.weights);

        // 调试
        self.debug.max_dump_sessions = self.debug.max_dump_sessions.clamp(1, 200);
        self
    }
}

fn default_vision_config_schema_version() -> u32 {
    VISION_CONFIG_SCHEMA_VERSION
}

fn default_template_cache_schema_version() -> u32 {
    TEMPLATE_CACHE_SCHEMA_VERSION
}

/// 模板权重归一化：全 0 或非有限值回退默认，避免「零权重 → 分数恒 0」。
pub fn sanitize_template_weights(w: TemplateWeights) -> TemplateWeights {
    let values = [w.mask, w.edge, w.shape, w.perceptual];
    if values.iter().any(|v| !v.is_finite() || *v < 0.0) || values.iter().sum::<f32>() <= 0.0 {
        return TemplateWeights::default();
    }
    w
}

/// 融合权重归一化（同上）。
pub fn sanitize_fusion_weights(w: FusionWeights) -> FusionWeights {
    let values = [w.template, w.embedding, w.classifier, w.geometry];
    if values.iter().any(|v| !v.is_finite() || *v < 0.0) || values.iter().sum::<f32>() <= 0.0 {
        return FusionWeights::default();
    }
    w
}

fn sanitize_grid(grid: &mut GridProfile) {
    grid.cell_size = grid.cell_size.clamp(8, 1_024);
    grid.cell_gap = grid.cell_gap.clamp(0, 512);
    grid.columns = grid.columns.clamp(1, 16);
    grid.rows = grid.rows.min(64);
    grid.row_pitch = grid.row_pitch.clamp(grid.cell_size, 2_048);
    grid.column_offsets.truncate(grid.columns as usize);
    for x in &mut grid.column_offsets {
        *x = (*x).clamp(-4_096, 16_384);
    }
}

fn clamp01(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_matches_measured_geometry() {
        let cfg = LayoutConfig::default();
        // 与参考实现 hd2-preset-helper 的 canonical 几何一致
        assert_eq!(cfg.home.column_x(0), 11);
        assert_eq!(cfg.home.column_x(3), 350);
        assert_eq!(cfg.list.column_x(1), 190);
        assert_eq!(cfg.home.cell_rect(0, 2).w, 104);
        assert_eq!(cfg.booster.column_offset, 107);
        // 列坐标缺失时退回 origin + col * pitch
        let mut g = cfg.list.clone();
        g.column_offsets.clear();
        assert_eq!(g.column_x(2), 77 + 2 * (104 + 9));
    }

    #[test]
    fn sanitize_repairs_illegal_values() {
        let mut cfg = VisionConfig::default();
        cfg.layout.crop.left_pct = f32::NAN;
        cfg.segmentation.foreground_threshold = -3.0;
        cfg.segmentation.weights = ScoreWeights {
            orange: 0.0,
            white: 0.0,
            contrast: 0.0,
            center: 0.0,
            luma: 0.0,
        };
        cfg.morphology.open_radius = 99;
        cfg.recognition.min_confidence = f32::INFINITY;
        cfg.recognition.top_k = 0;
        cfg.template.weights = TemplateWeights {
            mask: -1.0,
            edge: 0.0,
            shape: 0.0,
            perceptual: 0.0,
        };
        let cfg = cfg.sanitize();
        assert_eq!(cfg.layout.crop.left_pct, 0.0);
        assert!(cfg.segmentation.foreground_threshold >= 0.05);
        assert_eq!(cfg.segmentation.weights, ScoreWeights::default());
        assert_eq!(cfg.morphology.open_radius, 2);
        assert!(cfg.recognition.min_confidence.is_finite());
        assert_eq!(cfg.recognition.top_k, 1);
        assert_eq!(cfg.template.weights, TemplateWeights::default());
    }

    #[test]
    fn over_inset_crop_falls_back_to_default() {
        let mut cfg = VisionConfig::default();
        cfg.layout.crop.left_pct = 0.40;
        cfg.layout.crop.right_pct = 0.40;
        let cfg = cfg.sanitize();
        assert_eq!(cfg.layout.crop.left_pct, 0.10);
    }

    #[test]
    fn hsv_range_wraps_across_zero() {
        let range = HsvRange {
            h_min: 350.0,
            h_max: 20.0,
            s_min: 0.2,
            s_max: 1.0,
            v_min: 0.2,
            v_max: 1.0,
        };
        assert!(range.matches(5.0, 0.5, 0.5));
        assert!(range.matches(355.0, 0.5, 0.5));
        assert!(!range.matches(180.0, 0.5, 0.5));
        assert!(!range.matches(5.0, 0.01, 0.5));
    }

    #[test]
    fn config_roundtrips_through_json() {
        let cfg = VisionConfig::default().sanitize();
        let json = serde_json::to_string(&cfg).expect("序列化");
        let back: VisionConfig = serde_json::from_str(&json).expect("反序列化");
        assert_eq!(cfg, back);
    }

    #[test]
    fn legacy_config_is_migrated_to_current_versions() {
        let legacy: VisionConfig = serde_json::from_str("{}").expect("legacy config");
        assert_eq!(legacy.schema_version, VISION_CONFIG_SCHEMA_VERSION);
        assert_eq!(legacy.template_cache_version, TEMPLATE_CACHE_SCHEMA_VERSION);

        let mut changed = legacy;
        changed.schema_version = 0;
        changed.template_cache_version = 0;
        let migrated = changed.sanitize();
        assert_eq!(migrated.schema_version, VISION_CONFIG_SCHEMA_VERSION);
        assert_eq!(
            migrated.template_cache_version,
            TEMPLATE_CACHE_SCHEMA_VERSION
        );
    }
}
