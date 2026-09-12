// 识别与置信度融合（需求 §19 / §20 / §21 / §22 / §33）。
//
// 这一层负责把「几何 + 分割 + 模板」的证据合成一个**可解释**的结论，并且
// 在证据不足时明确弃权（Unknown）——错误识别一个战备比返回 Unknown 危险得多。
//
// 接口契约：`recognize_loadout()` 只读截图，返回 `Vec<StratagemDetection>`；
// 本模块不做任何输入模拟（不按键、不点击、不改 loadout），自动化由上层的 adapter 负责。
//
// plan6 §5.3 之后，vision 管线不再是装配动作路径：它由 `vision` CLI、
// 数据集生成器和真实截图 fixture 测试驱动。这里保留完整 API（几何 + 分割 +
// 模板融合 + 弃权原因），供底层适配契约验证与人工探针使用，因此豁免 dead_code 告警。
#![allow(dead_code)]
use std::sync::Mutex;
use std::time::Instant;

use image::RgbaImage;

use crate::loadout_sync::capture::CapturedFrame;
use crate::loadout_sync::types::ImageRect;

use super::calibration::{FrameGeometry, StratagemLayout};
use super::components::{analyze as analyze_components, ComponentAnalysis};
#[cfg(test)]
use super::config::VisionDebugLevel;
use super::config::{RecognitionConfig, VisionConfig};
use super::crop::{inner_crop, InnerCrop};
use super::error::VisionError;
use super::geometry::{self, GeometryDetection};
use super::grid::{self, GeometrySource, SlotKind, SlotPlan, SlotPlanKind};
use super::id::StratagemId;
use super::normalize::{build_icon_image, IconImage};
use super::roi;
use super::segment::{closing, opening, opening_preserve_thin, segment_cell, ForegroundMasks};
use super::template::{MethodScores, QueryShape, TemplateDb, TemplateMatch};

/// 得出该结论所依赖的识别器（需求 §19）。
///
/// 只保留真正会被产出的两种：当前阶段唯一的识别器是模板匹配，
/// `Fusion` 用于模板与几何/其它识别器共同给出结论的情况。
/// embedding / classifier 尚未接入（权重为 0），因此不再保留它们的空壳变体。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecognitionMethod {
    Template,
    /// 模板 + 几何 + 其它识别器的融合
    Fusion,
}

impl RecognitionMethod {
    pub fn label(self) -> &'static str {
        match self {
            Self::Template => "template",
            Self::Fusion => "fusion",
        }
    }
}

/// 备选（top-k）。
#[derive(Debug, Clone, PartialEq)]
pub struct Alternative {
    pub id: StratagemId,
    pub score: f32,
    pub methods: MethodScores,
}

/// 弃权原因（必须能解释，禁止只返回一个“未知”）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnknownReason {
    /// 空格子（列表尾部的空槽 / 未装备）
    EmptySlot,
    /// 裁剪结果不可用（尺寸/越界）
    BadCrop,
    /// 掩码退化（前景过少/过多，无法构成图标）
    InvalidMask,
    /// 置信度不足
    LowConfidence,
    /// top-1 与 top-2 差距过小（歧义）
    Ambiguous,
    /// 该图标不在模板库中（Mod / 新战备）
    UnsupportedIcon,
    /// 不认识的界面状态
    UnexpectedUiState,
}

impl UnknownReason {
    pub fn label(self) -> &'static str {
        match self {
            Self::EmptySlot => "empty_slot",
            Self::BadCrop => "bad_crop",
            Self::InvalidMask => "invalid_mask",
            Self::LowConfidence => "low_confidence",
            Self::Ambiguous => "ambiguous",
            Self::UnsupportedIcon => "unsupported_icon",
            Self::UnexpectedUiState => "unexpected_ui_state",
        }
    }
}

/// 单个槽位的判定结果。
#[derive(Debug, Clone, PartialEq)]
pub enum DetectionOutcome {
    /// 识别成功
    Recognized {
        id: StratagemId,
        /// 融合后的置信度（0~1）
        confidence: f32,
        method: RecognitionMethod,
        /// 可分解的证据
        template_score: f32,
        margin: f32,
        geometry_score: f32,
        methods: MethodScores,
        alternatives: Vec<Alternative>,
    },
    /// 空格子（不算失败，也不算识别）
    Empty,
    /// 弃权
    Unknown {
        reason: UnknownReason,
        best: Option<StratagemId>,
        best_score: f32,
        margin: f32,
    },
}

impl DetectionOutcome {
    pub fn is_recognized(&self) -> bool {
        matches!(self, Self::Recognized { .. })
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, Self::Unknown { .. })
    }

    /// 该槽位的 ID（未识别/空格为 None）。
    pub fn id(&self) -> Option<&StratagemId> {
        match self {
            Self::Recognized { id, .. } => Some(id),
            _ => None,
        }
    }

    /// 日志/UI 用的单行摘要。
    pub fn summary(&self) -> String {
        match self {
            Self::Recognized {
                id,
                confidence,
                method,
                margin,
                ..
            } => format!(
                "{} (confidence={confidence:.3}, method={}, margin={margin:.3})",
                id,
                method.label()
            ),
            Self::Empty => "Empty".to_string(),
            Self::Unknown {
                reason,
                best,
                best_score,
                margin,
            } => format!(
                "Unknown({}) best={} score={best_score:.3} margin={margin:.3}",
                reason.label(),
                best.as_ref().map(|b| b.as_str()).unwrap_or("-")
            ),
        }
    }
}

/// 一个槽位的识别结果（需求 §19 的 `RecognitionResult` + 需求 §43 的 bbox/top-k）。
#[derive(Debug, Clone, PartialEq)]
pub struct StratagemDetection {
    /// 槽位序号（home：0..=3 战备，4 = Booster；列表：行优先）
    pub slot: usize,
    pub row: u32,
    pub col: u32,
    pub kind: SlotKind,
    /// 槽位外框（帧像素）
    pub cell: ImageRect,
    /// 内裁剪后的图标区（帧像素）
    pub inner: ImageRect,
    /// 图标主体包围盒（帧像素）；None = 未提取到主体
    pub bbox: Option<ImageRect>,
    pub outcome: DetectionOutcome,
    /// 前景像素占比（空槽判定的直接依据）
    pub foreground_ratio: f32,
    /// 保留的连通域数量
    pub components_kept: usize,
}

/// 整次识别的结果。
#[derive(Debug, Clone, PartialEq)]
pub struct LoadoutRecognition {
    pub screen: (u32, u32),
    pub roi: ImageRect,
    pub plan: SlotPlanKind,
    pub geometry_source: GeometrySource,
    pub detections: Vec<StratagemDetection>,
    pub elapsed_ms: u128,
    /// 本次结果来自缓存（截图未变化，需求 §25）
    pub from_cache: bool,
}

impl LoadoutRecognition {
    pub fn stratagems(&self) -> impl Iterator<Item = &StratagemDetection> {
        self.detections.iter().filter(|d| !d.kind.is_booster())
    }

    pub fn booster(&self) -> Option<&StratagemDetection> {
        self.detections.iter().find(|d| d.kind.is_booster())
    }

    /// 已识别的战备（不含 Booster）。
    pub fn recognized_stratagems(&self) -> Vec<&StratagemDetection> {
        self.stratagems()
            .filter(|d| d.outcome.is_recognized())
            .collect()
    }

    pub fn unknown_reasons(&self) -> Vec<(usize, UnknownReason)> {
        self.detections
            .iter()
            .filter_map(|d| match &d.outcome {
                DetectionOutcome::Unknown { reason, .. } => Some((d.slot, *reason)),
                _ => None,
            })
            .collect()
    }

    /// 人类可读的逐槽摘要（CLI 与日志共用，避免两处格式化逻辑）。
    pub fn summary_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for det in &self.detections {
            let name = det
                .outcome
                .id()
                .and_then(|id| id.display_name())
                .unwrap_or("");
            let label = if det.kind.is_booster() {
                "Booster".to_string()
            } else {
                format!("Slot {}", det.slot)
            };
            lines.push(format!(
                "{label}: {} {}",
                det.outcome.summary(),
                if name.is_empty() {
                    String::new()
                } else {
                    format!("[{name}]")
                }
            ));
        }
        lines
    }
}

/// 单格的完整中间产物（调试与失败归因用，需求 §24）。
#[derive(Debug, Clone)]
pub struct CellTrace {
    pub slot: usize,
    pub cell: ImageRect,
    pub inner_crop: InnerCrop,
    /// 完整格子图（含被裁掉的边框，调试对照用）
    pub cell_image: RgbaImage,
    /// 内裁剪后的识别输入图
    pub inner_image: RgbaImage,
    pub masks: ForegroundMasks,
    pub analysis: ComponentAnalysis,
    pub icon: Option<IconImage>,
    pub ranked: Vec<TemplateMatch>,
    pub geometry_score: f32,
    /// 几何层的内容检查结果（独立交叉证据）
    pub content_hint: bool,
}

/// 一次运行的完整输出（结果 + 可落盘的中间产物）。
#[derive(Debug, Clone)]
pub struct VisionOutput {
    pub recognition: LoadoutRecognition,
    pub geometry: FrameGeometry,
    pub plan: SlotPlan,
    /// 几何检测的原始结果（固定锚点 + 边线验证分数，调试与阈值诊断用）
    pub detection: GeometryDetection,
    pub roi_image: RgbaImage,
    pub traces: Vec<CellTrace>,
}

impl VisionOutput {
    /// 逐格 top-k 文本报告（需求 §23 的 debug overlay 文本形态）。
    pub fn topk_report(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (trace, det) in self.traces.iter().zip(&self.recognition.detections) {
            out.push(format!("[Slot {}]", trace.slot));
            out.push(format!("  Prediction: {}", det.outcome.summary()));
            out.push(format!(
                "  Foreground: {:.4} | components kept: {} | bbox: {:?} | geometry score: {:.3} | content hint: {}",
                det.foreground_ratio, det.components_kept, det.bbox, trace.geometry_score, trace.content_hint
            ));
            out.push("  Top-K:".to_string());
            for (i, m) in trace.ranked.iter().enumerate() {
                let name = m.id.display_name().unwrap_or("");
                out.push(format!(
                    "    {}. {} {:.3} [mask {:.3} edge {:.3} shape {:.3} phash {:.3}] {}",
                    i + 1,
                    m.id,
                    m.score,
                    m.methods.mask,
                    m.methods.edge,
                    m.methods.shape,
                    m.methods.perceptual,
                    name
                ));
            }
        }
        out
    }
}

/// 截图指纹（需求 §25：画面未变化则复用上次结果）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameDigest(u64);

impl FrameDigest {
    /// 对 ROI 区域做稀疏采样后 FNV-1a 散列（足够区分画面变化，成本极低）。
    pub fn of(frame: &CapturedFrame, roi: ImageRect) -> Self {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut mix = |byte: u8| {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100_0000_01b3);
        };
        mix((frame.width() & 0xff) as u8);
        mix((frame.height() & 0xff) as u8);
        let step = 4.max(roi.w / 64).max(1);
        let mut y = roi.y.max(0);
        while y < roi.bottom().min(frame.height() as i32) {
            let mut x = roi.x.max(0);
            while x < roi.right().min(frame.width() as i32) {
                let p = frame.rgba.get_pixel(x as u32, y as u32).0;
                mix(p[0]);
                mix(p[1]);
                mix(p[2]);
                x += step;
            }
            y += step;
        }
        Self(hash)
    }
}

/// 识别目标：home 战备栏，或可滚动的战备列表。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecognizeTarget {
    /// 配装主界面（4 战备 + Booster）——`recognize_loadout()`
    Loadout,
    /// 当前可见的列表网格
    List,
}

/// 缓存条目。
struct CacheEntry {
    digest: FrameDigest,
    target: RecognizeTarget,
    recognition: LoadoutRecognition,
}

/// 视觉识别引擎：模板库只加载一次，识别可重复调用（需求 §25）。
pub struct VisionEngine {
    cfg: VisionConfig,
    layout: StratagemLayout,
    templates: TemplateDb,
    cache: Mutex<Option<CacheEntry>>,
}

impl VisionEngine {
    pub fn new(cfg: VisionConfig) -> Result<Self, VisionError> {
        let cfg = cfg.sanitize();
        let layout = StratagemLayout::from_config(&cfg.layout);
        let templates = TemplateDb::load(&cfg.template, &cfg.normalization)?;
        Ok(Self {
            cfg,
            layout,
            templates,
            cache: Mutex::new(None),
        })
    }

    pub fn config(&self) -> &VisionConfig {
        &self.cfg
    }

    /// 模板库加载情况（CLI 启动时打印，避免模板静默缺失）。
    pub fn template_report(&self) -> String {
        self.templates.report()
    }

    /// 以下访问器仅供测试与诊断使用（生产路径不读取引擎内部状态）。
    #[cfg(test)]
    pub fn layout(&self) -> &StratagemLayout {
        &self.layout
    }

    #[cfg(test)]
    pub fn templates(&self) -> &TemplateDb {
        &self.templates
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub fn debug_level(&self) -> VisionDebugLevel {
        self.cfg.debug.level
    }

    /// 清空结果缓存（仅测试使用：生产路径的缓存由帧指纹自动失效）。
    #[cfg(test)]
    pub fn clear_cache(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            *cache = None;
        }
    }

    /// 只做几何：返回当前帧的 ROI 与槽位计划（调试与 ROI 可视化用，需求 §5）。
    #[cfg(test)]
    pub fn plan(
        &self,
        frame: &CapturedFrame,
        target: RecognizeTarget,
    ) -> Result<(FrameGeometry, SlotPlan), VisionError> {
        let (geom, plan, _) = self.analyze(frame, target)?;
        Ok((geom, plan))
    }

    /// 几何分析：ROI 解析 → canonical 化 → 固定锚点 + 边线验证 → 槽位计划。
    pub fn analyze(
        &self,
        frame: &CapturedFrame,
        target: RecognizeTarget,
    ) -> Result<(FrameGeometry, SlotPlan, GeometryDetection), VisionError> {
        let geom = self.layout.resolve(frame.width(), frame.height())?;
        let roi_image = roi::extract_roi(frame, &geom)?;
        let (detection, plan) = match target {
            RecognizeTarget::Loadout => {
                let detection =
                    geometry::detect_home(&roi_image, &self.cfg.geometry, &self.cfg.segmentation);
                let plan = if detection.verified() {
                    grid::plan_from_home(&detection, &geom)?
                } else if self.cfg.geometry.fallback_to_calibration {
                    grid::calibration_home_plan(&self.layout, &geom)?
                } else {
                    return Err(VisionError::InvalidGrid {
                        detail: "home 几何未通过边线验证（固定锚点 + 验证）".into(),
                    });
                };
                (detection, plan)
            }
            RecognizeTarget::List => {
                let detection = geometry::detect_list(&roi_image, &self.cfg.geometry);
                let plan = if detection.verified() {
                    grid::plan_from_list(&detection, &geom)?
                } else if self.cfg.geometry.fallback_to_calibration {
                    let rows: Vec<i32> = (0..self.cfg.geometry.list_max_rows.min(4) as i32)
                        .map(|row| self.layout.list.origin_y + row * self.layout.list.row_pitch)
                        .collect();
                    grid::calibration_list_plan(&self.layout, &geom, &rows)?
                } else {
                    return Err(VisionError::InvalidGrid {
                        detail: "列表几何未通过边线验证（固定锚点 + 验证）".into(),
                    });
                };
                (detection, plan)
            }
        };
        Ok((geom, plan, detection))
    }

    /// 识别当前战备栏（需求 §33 的 `recognize_loadout()`）。
    pub fn recognize_loadout(&self, frame: &CapturedFrame) -> Result<VisionOutput, VisionError> {
        self.run(frame, RecognizeTarget::Loadout)
    }

    /// 识别当前可见的列表网格。
    pub fn recognize_list(&self, frame: &CapturedFrame) -> Result<VisionOutput, VisionError> {
        self.run(frame, RecognizeTarget::List)
    }

    /// 完整管线（需求 §二 的流程）：
    /// Calibration → ROI → Grid → Inner crop → Segmentation → Morphology →
    /// Components → BBox → Normalization → Template → Fusion → StratagemId。
    pub fn run(
        &self,
        frame: &CapturedFrame,
        target: RecognizeTarget,
    ) -> Result<VisionOutput, VisionError> {
        if frame.width() == 0 || frame.height() == 0 {
            return Err(VisionError::ZeroSizedFrame);
        }
        let (geom, plan, detection) = self.analyze(frame, target)?;
        let digest = FrameDigest::of(frame, geom.roi);

        // 缓存命中：画面与目标都没变 → 直接复用（需求 §25）
        if let Ok(cache) = self.cache.lock() {
            if let Some(entry) = cache.as_ref() {
                if entry.digest == digest && entry.target == target {
                    let mut recognition = entry.recognition.clone();
                    recognition.from_cache = true;
                    let roi_image = roi::extract_roi(frame, &geom)?;
                    return Ok(VisionOutput {
                        recognition,
                        geometry: geom,
                        plan,
                        detection,
                        roi_image,
                        traces: Vec::new(),
                    });
                }
            }
        }

        let started = Instant::now();
        let roi_image = roi::extract_roi(frame, &geom)?;
        let mut detections = Vec::with_capacity(plan.slots.len());
        let mut traces = Vec::with_capacity(plan.slots.len());
        for slot in &plan.slots {
            let (detection, trace) = self.process_slot(frame, slot)?;
            detections.push(detection);
            traces.push(trace);
        }
        let recognition = LoadoutRecognition {
            screen: (frame.width(), frame.height()),
            roi: geom.roi,
            plan: plan.kind,
            geometry_source: plan.geometry_source,
            detections,
            elapsed_ms: started.elapsed().as_millis(),
            from_cache: false,
        };
        if let Ok(mut cache) = self.cache.lock() {
            *cache = Some(CacheEntry {
                digest,
                target,
                recognition: recognition.clone(),
            });
        }
        Ok(VisionOutput {
            recognition,
            geometry: geom,
            plan,
            detection,
            roi_image,
            traces,
        })
    }

    /// 单格处理：分割 → 形态学 → 连通域 → 空槽判定 → 归一化 → 模板 → 融合。
    fn process_slot(
        &self,
        frame: &CapturedFrame,
        slot: &grid::Slot,
    ) -> Result<(StratagemDetection, CellTrace), VisionError> {
        let crop = inner_crop(slot.rect, &self.cfg.layout.crop);
        let cell_image = roi::crop_rect(frame, slot.rect)?;
        let inner_image = roi::crop_rect(frame, crop.rect)?;
        let mut masks = segment_cell(&inner_image, &self.cfg.segmentation);

        // 形态学：小核（默认 3×3、迭代 1 次）——需求 §11 明令禁止大核。
        // 默认使用「保形开运算」：实机图标笔画只有 2px 宽，各向同性 3×3 腐蚀会把
        // 整条笔画抹掉，只剩铜色底座。详见 `segment::opening_preserve_thin`。
        let (w, h) = (masks.width, masks.height);
        if self.cfg.morphology.open_preserve_thin {
            opening_preserve_thin(
                &mut masks.combined,
                w,
                h,
                self.cfg.morphology.open_iterations,
            );
        } else {
            opening(
                &mut masks.combined,
                w,
                h,
                self.cfg.morphology.open_radius,
                self.cfg.morphology.open_iterations,
            );
        }
        closing(
            &mut masks.combined,
            w,
            h,
            self.cfg.morphology.close_radius,
            self.cfg.morphology.close_iterations,
        );

        let analysis = analyze_components(&masks, &self.cfg.component);
        let booster = slot.kind.is_booster();
        let mut trace = CellTrace {
            slot: slot.index,
            cell: slot.rect,
            inner_crop: crop,
            cell_image,
            inner_image,
            masks,
            analysis,
            icon: None,
            ranked: Vec::new(),
            geometry_score: 0.0,
            content_hint: slot.content_hint,
        };

        // 空槽判定（需求 §21）：让空槽根本进不了分类器
        if self.is_empty_slot(&trace) {
            return Ok((
                StratagemDetection {
                    slot: slot.index,
                    row: slot.row,
                    col: slot.col,
                    kind: slot.kind,
                    cell: slot.rect,
                    inner: crop.rect,
                    bbox: None,
                    outcome: DetectionOutcome::Empty,
                    foreground_ratio: trace.masks.foreground_ratio(),
                    components_kept: trace.analysis.kept().count(),
                },
                trace,
            ));
        }

        let Some(bbox) = trace.analysis.bbox else {
            return Ok((
                unknown_detection(
                    slot,
                    &crop,
                    &trace,
                    UnknownReason::InvalidMask,
                    None,
                    0.0,
                    0.0,
                ),
                trace,
            ));
        };

        let icon = match build_icon_image(
            &trace.inner_image,
            crop.rect,
            bbox,
            &trace.masks,
            &self.cfg.normalization,
        ) {
            Ok(icon) => icon,
            Err(_) => {
                return Ok((
                    unknown_detection(slot, &crop, &trace, UnknownReason::BadCrop, None, 0.0, 0.0),
                    trace,
                ))
            }
        };

        let query = QueryShape::from_icon(
            &icon,
            (self.cfg.normalization.mask_threshold * 255.0).round() as u8,
        );
        if query.foreground_px < self.cfg.template.min_foreground_px {
            let detection = unknown_detection(
                slot,
                &crop,
                &trace,
                UnknownReason::InvalidMask,
                None,
                0.0,
                0.0,
            );
            trace.icon = Some(icon);
            return Ok((detection, trace));
        }

        let ranked = self.templates.rank(
            &query,
            &self.cfg.template,
            Some(booster),
            self.cfg.recognition.top_k.max(2),
        );
        let geometry_score = geometry_score(&icon, &trace.analysis, &self.cfg.recognition);
        let bbox_frame = icon.bbox_frame;
        trace.geometry_score = geometry_score;
        trace.icon = Some(icon);
        trace.ranked = ranked.clone();

        let outcome = self.decide(&ranked, geometry_score);
        Ok((
            StratagemDetection {
                slot: slot.index,
                row: slot.row,
                col: slot.col,
                kind: slot.kind,
                cell: slot.rect,
                inner: crop.rect,
                bbox: Some(bbox_frame),
                outcome,
                foreground_ratio: trace.masks.foreground_ratio(),
                components_kept: trace.analysis.kept().count(),
            },
            trace,
        ))
    }

    /// 空槽判定：三条互相独立的证据（前景占比 / 最大连通域 / 中心活动度）。
    fn is_empty_slot(&self, trace: &CellTrace) -> bool {
        let cfg = &self.cfg.recognition;
        let fg_ratio = trace.masks.foreground_ratio();
        if fg_ratio < cfg.empty_foreground_ratio {
            return true;
        }
        if trace.analysis.bbox.is_none() {
            return true;
        }
        let (w, h) = (trace.masks.width, trace.masks.height);
        let center = trace.masks.ratio_in(w / 4, h / 4, w * 3 / 4, h * 3 / 4);
        center < cfg.empty_center_ratio
            && trace.analysis.largest_kept_ratio() < cfg.empty_max_component_ratio
    }

    /// 置信度融合与弃权判定（需求 §19 / §20）。
    fn decide(&self, ranked: &[TemplateMatch], geometry_score: f32) -> DetectionOutcome {
        let cfg: &RecognitionConfig = &self.cfg.recognition;
        let Some(best) = ranked.first() else {
            return DetectionOutcome::Unknown {
                reason: UnknownReason::UnsupportedIcon,
                best: None,
                best_score: 0.0,
                margin: 0.0,
            };
        };
        // 间隔必须相对「**非等价**的次优候选」计算。
        //
        // 资源库里存在同一字形被登记成两个 ID 的情况（实测
        // `orbital_precision_strike` / `seaf_artillery` 的掩码 IoU ≈ 0.96），
        // 这种候选之间 IoU 极高、分数必然几乎相同，若直接取第 2 名算 margin，
        // 正确但同形的一对会被判成「歧义」而永久弃权。等价候选之间不构成证据冲突。
        let second = ranked
            .iter()
            .skip(1)
            .find(|m| {
                !self
                    .templates
                    .near_duplicate_pair(&best.id, &m.id, cfg.dup_mask_iou)
            })
            .map(|m| m.score)
            .unwrap_or(0.0);
        let margin = (best.score - second).max(0.0);
        let alternatives: Vec<Alternative> = ranked
            .iter()
            .skip(1)
            .map(|m| Alternative {
                id: m.id.clone(),
                score: m.score,
                methods: m.methods,
            })
            .collect();

        // 融合：模板 + 几何（embedding / classifier 权重默认为 0，未启用）
        let weights = cfg.weights;
        let available =
            (weights.template + weights.embedding + weights.classifier + weights.geometry)
                .max(1e-6);
        let used = if cfg.allow_template_only {
            weights.template + weights.geometry
        } else {
            weights.template
        }
        .max(1e-6);
        let confidence = (weights.template * best.score + weights.geometry * geometry_score) / used;
        let _ = available;

        // 闸门 1：外观证据必须自身达标（几何分不允许把不达标的外观抬过门槛）
        if best.score < cfg.min_confidence || confidence < cfg.min_confidence {
            return DetectionOutcome::Unknown {
                reason: UnknownReason::LowConfidence,
                best: Some(best.id.clone()),
                best_score: best.score,
                margin,
            };
        }
        // 闸门 2：与次优必须有足够间隔（0.81 vs 0.80 视为歧义）
        if margin < cfg.min_margin {
            return DetectionOutcome::Unknown {
                reason: UnknownReason::Ambiguous,
                best: Some(best.id.clone()),
                best_score: best.score,
                margin,
            };
        }
        DetectionOutcome::Recognized {
            id: best.id.clone(),
            confidence: confidence.clamp(0.0, 1.0),
            method: if weights.embedding > 0.0 || weights.classifier > 0.0 {
                RecognitionMethod::Fusion
            } else {
                RecognitionMethod::Template
            },
            template_score: best.score,
            margin,
            geometry_score,
            methods: best.methods,
            alternatives,
        }
    }
}

fn unknown_detection(
    slot: &grid::Slot,
    crop: &InnerCrop,
    trace: &CellTrace,
    reason: UnknownReason,
    best: Option<StratagemId>,
    best_score: f32,
    margin: f32,
) -> StratagemDetection {
    StratagemDetection {
        slot: slot.index,
        row: slot.row,
        col: slot.col,
        kind: slot.kind,
        cell: slot.rect,
        inner: crop.rect,
        bbox: None,
        outcome: DetectionOutcome::Unknown {
            reason,
            best,
            best_score,
            margin,
        },
        foreground_ratio: trace.masks.foreground_ratio(),
        components_kept: trace.analysis.kept().count(),
    }
}

/// 几何有效性分数（0~1）：裁剪质量本身就是可信度的一部分（需求 §19 的 geometric validity）。
///
/// 注意：它不是类别证据，只能**降低**可信度；判定时也不允许它把外观证据抬过门槛。
pub fn geometry_score(
    icon: &IconImage,
    analysis: &ComponentAnalysis,
    cfg: &RecognitionConfig,
) -> f32 {
    let _ = cfg;
    let area_ratio =
        (icon.bbox.w * icon.bbox.h) as f32 / (analysis.width * analysis.height).max(1) as f32;
    let bbox_score = if area_ratio < 0.10 {
        smoothstep(0.02, 0.10, area_ratio)
    } else if area_ratio > 0.95 {
        1.0 - smoothstep(0.95, 1.0, area_ratio)
    } else {
        1.0
    };
    let aspect = icon.bbox.w.max(icon.bbox.h) as f32 / icon.bbox.w.min(icon.bbox.h).max(1) as f32;
    let aspect_score = 1.0 - smoothstep(1.5, 6.0, aspect);
    let fg = icon.normalized_foreground_ratio;
    let fill_score = if fg < 0.02 {
        smoothstep(0.005, 0.02, fg)
    } else if fg > 0.75 {
        1.0 - smoothstep(0.75, 0.95, fg)
    } else {
        1.0
    };
    (bbox_score * aspect_score * fill_score).clamp(0.0, 1.0)
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if (edge1 - edge0).abs() <= f32::EPSILON {
        return if x >= edge1 { 1.0 } else { 0.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loadout_sync::capture::CaptureBackend;
    use crate::loadout_sync::types::ScreenPoint;
    use image::{Rgba, RgbaImage};

    fn frame_from(img: RgbaImage) -> CapturedFrame {
        let gray = image::DynamicImage::ImageRgba8(img.clone()).to_luma8();
        CapturedFrame {
            gray,
            rgba: img,
            origin: ScreenPoint { x: 0, y: 0 },
            backend: CaptureBackend::Wgc,
        }
    }

    /// 造一帧：左侧配装面板位置画上游戏风格的空槽（暗底 + 亮边框）。
    fn empty_home_frame(w: u32, h: u32) -> CapturedFrame {
        let layout = StratagemLayout::default();
        let geom = layout.resolve(w, h).expect("几何");
        let mut img = RgbaImage::new(w, h);
        for (_, _, p) in img.enumerate_pixels_mut() {
            *p = Rgba([45, 48, 52, 255]);
        }
        for slot in [
            ImageRect::new(9, 636, 106, 106),
            ImageRect::new(122, 636, 106, 106),
            ImageRect::new(236, 636, 106, 106),
            ImageRect::new(349, 636, 106, 106),
            ImageRect::new(458, 636, 106, 106),
        ] {
            let rect = geom.roi_rect(slot);
            for y in rect.y..rect.bottom() {
                for x in rect.x..rect.right() {
                    if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
                        continue;
                    }
                    let edge = x == rect.x
                        || y == rect.y
                        || x == rect.right() - 1
                        || y == rect.bottom() - 1;
                    let color = if edge {
                        [214, 214, 210, 255]
                    } else {
                        [78, 82, 88, 255]
                    };
                    img.put_pixel(x as u32, y as u32, Rgba(color));
                }
            }
        }
        frame_from(img)
    }

    /// 在某个 home 槽位里画一个橙色方块图标。
    fn home_frame_with_slot(
        slot_index: usize,
        glyph: impl Fn(u32, u32) -> [u8; 4],
    ) -> CapturedFrame {
        let (w, h) = (2560u32, 1440u32);
        let layout = StratagemLayout::default();
        let geom = layout.resolve(w, h).expect("几何");
        let mut img = RgbaImage::new(w, h);
        for (_, _, p) in img.enumerate_pixels_mut() {
            *p = Rgba([45, 48, 52, 255]);
        }
        let slot_rel = ImageRect::new(
            layout.home.column_x(slot_index as u32),
            layout.home.origin_y,
            layout.home.cell_size,
            layout.home.cell_size,
        );
        let rect = geom.roi_rect(slot_rel);
        for y in rect.y..rect.bottom() {
            for x in rect.x..rect.right() {
                let local_x = (x - rect.x) as u32;
                let local_y = (y - rect.y) as u32;
                let color = glyph(local_x, local_y);
                img.put_pixel(x as u32, y as u32, Rgba(color));
            }
        }
        frame_from(img)
    }

    fn engine() -> VisionEngine {
        VisionEngine::new(VisionConfig::default()).expect("引擎")
    }

    #[test]
    fn engine_loads_templates_once() {
        let engine = engine();
        assert!(engine.templates().len() > 90);
        assert_eq!(engine.debug_level(), VisionDebugLevel::Off);
    }

    #[test]
    fn home_plan_covers_four_slots_and_booster() {
        let engine = engine();
        let frame = empty_home_frame(2560, 1440);
        let (geom, plan) = engine.plan(&frame, RecognizeTarget::Loadout).expect("几何");
        assert_eq!(geom.roi, ImageRect::new(64, 480, 576, 832));
        assert_eq!(plan.slots.len(), 5);
        assert_eq!(plan.cols, 4);
        assert!(plan.booster().is_some());
    }

    #[test]
    fn empty_home_slots_are_detected_as_empty() {
        let engine = engine();
        let frame = empty_home_frame(2560, 1440);
        let output = engine.recognize_loadout(&frame).expect("识别");
        assert_eq!(output.recognition.detections.len(), 5);
        for det in &output.recognition.detections {
            assert!(
                det.outcome.is_empty(),
                "空槽必须判为空，实际 {}",
                det.outcome.summary()
            );
        }
    }

    #[test]
    fn orange_glyph_is_recognized_with_confidence_and_bbox() {
        let engine = engine();
        // 橙色实心方块：模板库中「实心方块」不存在，因此结果必须是可解释的
        // （识别到某个 ID 或明确弃权），但绝不能是空槽。
        let frame = home_frame_with_slot(0, |x, y| {
            if (30..76).contains(&x) && (30..76).contains(&y) {
                [232, 160, 90, 255]
            } else {
                [78, 82, 88, 255]
            }
        });
        let output = engine.recognize_loadout(&frame).expect("识别");
        let det = &output.recognition.detections[0];
        assert!(!det.outcome.is_empty(), "有内容不得判空槽");
        assert!(det.bbox.is_some(), "必须给出图标 bbox");
        assert!(det.foreground_ratio > 0.05);
        match &det.outcome {
            DetectionOutcome::Recognized { confidence, .. } => {
                assert!(*confidence > 0.0 && *confidence <= 1.0);
            }
            DetectionOutcome::Unknown { reason, .. } => {
                assert!(matches!(
                    reason,
                    UnknownReason::LowConfidence
                        | UnknownReason::Ambiguous
                        | UnknownReason::UnsupportedIcon
                ));
            }
            DetectionOutcome::Empty => unreachable!(),
        }
    }

    #[test]
    fn identical_frames_hit_the_cache() {
        let engine = engine();
        let frame = empty_home_frame(2560, 1440);
        let first = engine.recognize_loadout(&frame).expect("首次");
        assert!(!first.recognition.from_cache);
        let second = engine.recognize_loadout(&frame).expect("二次");
        assert!(second.recognition.from_cache, "同画面应命中缓存");
        assert_eq!(
            first.recognition.detections, second.recognition.detections,
            "缓存结果必须与首次一致"
        );
    }

    #[test]
    fn frame_change_invalidates_the_cache() {
        let engine = engine();
        let frame = empty_home_frame(2560, 1440);
        let _ = engine.recognize_loadout(&frame).expect("首次");
        let changed = home_frame_with_slot(1, |x, y| {
            if (30..76).contains(&x) && (30..76).contains(&y) {
                [232, 160, 90, 255]
            } else {
                [78, 82, 88, 255]
            }
        });
        let output = engine.recognize_loadout(&changed).expect("变化后");
        assert!(!output.recognition.from_cache, "画面变化必须重算");
    }

    #[test]
    fn digest_is_stable_and_sensitive() {
        let a = empty_home_frame(2560, 1440);
        let b = empty_home_frame(2560, 1440);
        let roi = ImageRect::new(64, 480, 576, 832);
        assert_eq!(FrameDigest::of(&a, roi), FrameDigest::of(&b, roi));
        let c = home_frame_with_slot(0, |_, _| [232, 160, 90, 255]);
        assert_ne!(FrameDigest::of(&a, roi), FrameDigest::of(&c, roi));
    }

    #[test]
    fn zero_sized_frame_is_rejected() {
        let engine = engine();
        let frame = frame_from(RgbaImage::new(1, 1));
        assert!(engine.recognize_loadout(&frame).is_err());
    }

    #[test]
    fn non_16_9_frame_still_resolves_geometry() {
        let engine = engine();
        let frame = empty_home_frame(1921, 1076);
        let output = engine.recognize_loadout(&frame).expect("1921x1076");
        assert_eq!(output.recognition.detections.len(), 5);
        assert!(output.geometry.roi.w > 400);
    }

    #[test]
    fn summary_lines_are_human_readable() {
        let engine = engine();
        let frame = empty_home_frame(2560, 1440);
        let output = engine.recognize_loadout(&frame).expect("识别");
        let lines = output.recognition.summary_lines();
        assert_eq!(lines.len(), 5);
        assert!(lines[0].starts_with("Slot 0: Empty"));
        assert!(lines[4].starts_with("Booster: Empty"));
    }

    #[test]
    fn abort_reasons_are_reported_for_low_confidence() {
        // 极低阈值下必然接受；极高阈值下必须弃权并说明原因
        let mut cfg = VisionConfig::default();
        cfg.recognition.min_confidence = 0.99;
        cfg.recognition.min_margin = 0.0;
        let engine = VisionEngine::new(cfg).expect("引擎");
        let frame = home_frame_with_slot(0, |x, y| {
            if (30..76).contains(&x) && (30..76).contains(&y) {
                [232, 160, 90, 255]
            } else {
                [78, 82, 88, 255]
            }
        });
        let output = engine.recognize_loadout(&frame).expect("识别");
        match &output.recognition.detections[0].outcome {
            DetectionOutcome::Unknown { reason, best, .. } => {
                assert_eq!(*reason, UnknownReason::LowConfidence);
                assert!(best.is_some(), "弃权时必须报告最佳候选");
            }
            other => panic!("期望 LowConfidence 弃权，实际 {}", other.summary()),
        }
    }
}
