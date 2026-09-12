// 模板库与多度量相似度（需求 §15 / §16）。
//
// 第一识别器（第一阶段唯一必需的识别器）：
//   查询图标（`IconImage` 的归一化掩码）→ 与模板库逐模板比较 → top-k 候选。
//
// 四个互补度量（任何一个单独都不够，权重可配置）：
//   1. mask        —— 前景 IoU（主体形状）
//   2. edge        —— 轮廓一致性（对 ±1px 抗锯齿差异容忍）
//   3. shape       —— Hu 矩（对局部缺失/内部镂空稳健）
//   4. perceptual  —— 感知哈希（整体版式，抗噪声）
//
// 模板来源：H2AC 内嵌图标资源（`icons::icon_png_bytes`）或
// `assets/stratagems/{id}/` 目录（需求 §15 的数据库布局）。同一 ID 允许多个变体。
use std::collections::HashMap;

use image::RgbaImage;

use crate::icons;

use super::config::{NormalizationConfig, TemplateConfig, TemplateVariant, TemplateWeights};
use super::error::VisionError;
use super::id::StratagemId;
use super::normalize::IconImage;

/// 图标资源 alpha 掩码阈值（与 `loadout_sync::matcher` 同口径）。
pub const ALPHA_MASK_THRESHOLD: u8 = 64;

// ─── 位掩码 ───

/// 紧凑位掩码（128×128 → 256 个 u64），匹配时只做 popcount，避免逐像素分支。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BitMask {
    words: Vec<u64>,
    pub width: usize,
    pub height: usize,
}

impl BitMask {
    pub fn new(width: usize, height: usize) -> Self {
        let words = (width * height).div_ceil(64).max(1);
        Self {
            words: vec![0; words],
            width,
            height,
        }
    }

    /// 由布尔向量构造掩码（仅测试使用）。
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn from_bools(values: &[bool], width: usize, height: usize) -> Self {
        let mut mask = Self::new(width, height);
        for (i, v) in values.iter().enumerate().take(width * height) {
            if *v {
                mask.set_index(i);
            }
        }
        mask
    }

    #[inline]
    pub fn index(&self, x: usize, y: usize) -> usize {
        y * self.width + x
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        let i = self.index(x, y);
        (self.words[i / 64] >> (i % 64)) & 1 == 1
    }

    /// 置位第 i 个 bit（仅测试装配用）。
    #[cfg(test)]
    #[allow(dead_code)]
    fn set_index(&mut self, i: usize) {
        self.words[i / 64] |= 1u64 << (i % 64);
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, value: bool) {
        if x >= self.width || y >= self.height {
            return;
        }
        let i = self.index(x, y);
        if value {
            self.words[i / 64] |= 1u64 << (i % 64);
        } else {
            self.words[i / 64] &= !(1u64 << (i % 64));
        }
    }

    pub fn count(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// `self & other` 的位数。
    pub fn and_count(&self, other: &Self) -> usize {
        self.words
            .iter()
            .zip(&other.words)
            .map(|(a, b)| (a & b).count_ones() as usize)
            .sum()
    }

    /// `self & !other` 的位数。
    pub fn andnot_count(&self, other: &Self) -> usize {
        self.words
            .iter()
            .zip(&other.words)
            .map(|(a, b)| (a & !b).count_ones() as usize)
            .sum()
    }

    /// 膨胀（半径 1 → 3×3）。
    pub fn dilate(&self, radius: i32) -> Self {
        self.morph(radius, true)
    }

    /// 腐蚀。
    pub fn erode(&self, radius: i32) -> Self {
        self.morph(radius, false)
    }

    fn morph(&self, radius: i32, dilate: bool) -> Self {
        if radius <= 0 {
            return self.clone();
        }
        let mut out = Self::new(self.width, self.height);
        for y in 0..self.height as i32 {
            for x in 0..self.width as i32 {
                // 膨胀：初始 false，遇到任一前景即置位；
                // 腐蚀：初始 true，遇到任一背景即清位。
                let mut hit = !dilate;
                'scan: for dy in -radius..=radius {
                    for dx in -radius..=radius {
                        let (nx, ny) = (x + dx, y + dy);
                        let inside =
                            nx >= 0 && ny >= 0 && nx < self.width as i32 && ny < self.height as i32;
                        let v = inside && self.get(nx as usize, ny as usize);
                        if dilate && v {
                            hit = true;
                            break 'scan;
                        }
                        if !dilate && !v {
                            hit = false;
                            break 'scan;
                        }
                    }
                }
                if hit {
                    out.set(x as usize, y as usize, true);
                }
            }
        }
        out
    }

    /// 形态学轮廓（前景 ∧ ¬腐蚀）—— 轮廓相似度用它，避免直接比较亮度边缘。
    pub fn boundary(&self) -> Self {
        let eroded = self.erode(1);
        let mut out = Self::new(self.width, self.height);
        for (i, (a, b)) in self.words.iter().zip(&eroded.words).enumerate() {
            out.words[i] = a & !b;
        }
        out
    }
}

// ─── 查询形状 ───

/// 查询图标在「形状域」的表示：掩码 + 轮廓 + 膨胀掩码 + Hu 矩 + 感知哈希。
#[derive(Debug, Clone, PartialEq)]
pub struct QueryShape {
    pub size: u32,
    pub mask: BitMask,
    pub boundary: BitMask,
    pub dilated: BitMask,
    pub hu: [f64; 7],
    pub hash: u64,
    pub foreground_px: usize,
    pub foreground_ratio: f32,
}

impl QueryShape {
    pub fn from_mask(mask: BitMask) -> Self {
        let size = mask.width as u32;
        let boundary = mask.boundary();
        let dilated = mask.dilate(1);
        let foreground_px = mask.count();
        let foreground_ratio = foreground_px as f32 / (mask.width * mask.height).max(1) as f32;
        Self {
            size,
            hu: hu_moments(&mask),
            hash: perceptual_hash(&mask),
            mask,
            boundary,
            dilated,
            foreground_px,
            foreground_ratio,
        }
    }

    /// 从归一化图标产物构造查询（阈值来自 `NormalizationConfig::mask_threshold`）。
    pub fn from_icon(icon: &IconImage, threshold: u8) -> Self {
        let (w, h) = icon.normalized_mask.dimensions();
        let mut mask = BitMask::new(w as usize, h as usize);
        for y in 0..h {
            for x in 0..w {
                if icon.normalized_mask.get_pixel(x, y)[0] >= threshold {
                    mask.set(x as usize, y as usize, true);
                }
            }
        }
        Self::from_mask(mask)
    }
}

// ─── 模板 ───

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MethodScores {
    pub mask: f32,
    pub edge: f32,
    pub shape: f32,
    pub perceptual: f32,
}

impl MethodScores {
    /// 按权重归一化后的总分。
    pub fn fuse(&self, weights: &TemplateWeights) -> f32 {
        let sum = (weights.mask + weights.edge + weights.shape + weights.perceptual).max(1e-6);
        (weights.mask * self.mask
            + weights.edge * self.edge
            + weights.shape * self.shape
            + weights.perceptual * self.perceptual)
            / sum
    }
}

/// 一个已准备好的模板（掩码域）。
#[derive(Debug, Clone)]
pub struct PreparedTemplate {
    pub id: StratagemId,
    pub variant: TemplateVariant,
    pub mask: BitMask,
    pub boundary: BitMask,
    pub dilated: BitMask,
    pub hu: [f64; 7],
    pub hash: u64,
    pub foreground_px: usize,
}

/// 单次模板匹配结果。
#[derive(Debug, Clone, PartialEq)]
pub struct TemplateMatch {
    pub id: StratagemId,
    pub score: f32,
    pub methods: MethodScores,
    pub variant: TemplateVariant,
}

/// 模板库（进程内只加载一次，需求 §25）。
#[derive(Debug, Default)]
pub struct TemplateDb {
    templates: Vec<PreparedTemplate>,
    /// 加载失败的图标键（诊断用，不静默）
    pub failures: Vec<String>,
}

impl TemplateDb {
    /// 已加载的模板条目数与加载失败列表（CLI 启动时报告，避免模板静默缺失）。
    pub fn report(&self) -> String {
        match self.failures.len() {
            0 => format!("模板库：{} 个模板，全部加载成功", self.templates.len()),
            n => format!(
                "模板库：{} 个模板，{} 个资源加载失败（前 3 条：{}）",
                self.templates.len(),
                n,
                self.failures
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        }
    }

    /// 仅测试与诊断使用的计数（生产路径用 `report()`）。
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.templates.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }

    /// 仅测试使用：模板库覆盖到的 ID 列表。
    #[cfg(test)]
    pub fn ids(&self) -> Vec<StratagemId> {
        let mut ids: Vec<StratagemId> = self.templates.iter().map(|t| t.id.clone()).collect();
        ids.sort();
        ids.dedup();
        ids
    }

    /// 两个 ID 的模板是否近乎重复（取各自最佳掩码的 IoU）。
    ///
    /// 资源库中确实存在同一字形被登记成两个 ID 的情况（实测
    /// `orbital_precision_strike` 与 `seaf_artillery` 的 alpha 掩码 IoU ≈ 0.96），
    /// 此时任何阈值都无法把二者分开，必须按「歧义」处理而不是任选其一。
    pub fn near_duplicate_pair(&self, a: &StratagemId, b: &StratagemId, iou: f32) -> bool {
        let best_mask = |id: &StratagemId| {
            self.templates
                .iter()
                .filter(|t| &t.id == id)
                .max_by_key(|t| t.foreground_px)
                .map(|t| &t.mask)
        };
        match (best_mask(a), best_mask(b)) {
            (Some(ma), Some(mb)) => {
                let inter = ma.and_count(mb);
                let union = ma.count() + mb.count() - inter;
                union > 0 && inter as f32 / union as f32 >= iou
            }
            _ => false,
        }
    }

    /// 加载模板库：内嵌图标资源（可选）+ 额外模板目录（可选）。
    pub fn load(
        template_cfg: &TemplateConfig,
        norm_cfg: &NormalizationConfig,
    ) -> Result<Self, VisionError> {
        let mut db = TemplateDb::default();
        if template_cfg.use_embedded_library {
            for key in icons::all_icon_keys() {
                let Some(bytes) = icons::icon_png_bytes(key) else {
                    db.failures.push(format!("{key}: 内嵌图标缺失"));
                    continue;
                };
                match image::load_from_memory(bytes) {
                    Ok(img) => db.add_source(
                        &StratagemId::new(key),
                        &img.to_rgba8(),
                        template_cfg,
                        norm_cfg,
                    ),
                    Err(e) => db.failures.push(format!("{key}: 解码失败 {e}")),
                }
            }
        }
        if let Some(dir) = &template_cfg.extra_dir {
            db.load_directory(dir, template_cfg, norm_cfg)?;
        }
        if db.templates.is_empty() {
            return Err(VisionError::NoTemplates);
        }
        Ok(db)
    }

    /// 额外模板目录：`{dir}/{id}/raw/*.png` 或 `{dir}/{id}/variants/*.png`。
    pub fn load_directory(
        &mut self,
        dir: &std::path::Path,
        template_cfg: &TemplateConfig,
        norm_cfg: &NormalizationConfig,
    ) -> Result<(), VisionError> {
        let entries = std::fs::read_dir(dir).map_err(|e| VisionError::TemplateDatabase {
            path: dir.display().to_string(),
            detail: format!("无法读取目录: {e}"),
        })?;
        for entry in entries.flatten() {
            let id_dir = entry.path();
            if !id_dir.is_dir() {
                continue;
            }
            let Some(name) = id_dir.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let id = StratagemId::new(name);
            for sub in ["raw", "variants", "."] {
                let sub_dir = id_dir.join(sub);
                if !sub_dir.is_dir() {
                    continue;
                }
                let Ok(files) = std::fs::read_dir(&sub_dir) else {
                    continue;
                };
                for file in files.flatten() {
                    let path = file.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("png") {
                        continue;
                    }
                    match std::fs::read(&path)
                        .map_err(|e| e.to_string())
                        .and_then(|bytes| {
                            image::load_from_memory(&bytes)
                                .map(|i| i.to_rgba8())
                                .map_err(|e| e.to_string())
                        }) {
                        Ok(img) => self.add_source(&id, &img, template_cfg, norm_cfg),
                        Err(e) => self.failures.push(format!("{}: {e}", path.display())),
                    }
                }
            }
        }
        Ok(())
    }

    /// 从一张源图构建该 ID 的全部变体模板。
    pub fn add_source(
        &mut self,
        id: &StratagemId,
        source: &RgbaImage,
        template_cfg: &TemplateConfig,
        norm_cfg: &NormalizationConfig,
    ) {
        for variant in &template_cfg.variants {
            match prepare_template(id, source, *variant, template_cfg, norm_cfg) {
                Some(template) => self.templates.push(template),
                None => self
                    .failures
                    .push(format!("{}[{}]: 掩码为空", id, variant.label())),
            }
        }
    }

    /// 整库排名（按 ID 聚合，取该 ID 的最佳变体）。
    ///
    /// `booster`：Some(true) 只比 Booster 模板；Some(false) 只比战备模板；None 全比。
    pub fn rank(
        &self,
        query: &QueryShape,
        template_cfg: &TemplateConfig,
        booster: Option<bool>,
        limit: usize,
    ) -> Vec<TemplateMatch> {
        let weights = &template_cfg.weights;
        let mut best: HashMap<&StratagemId, TemplateMatch> = HashMap::new();
        for template in &self.templates {
            if let Some(want) = booster {
                if template.id.is_booster() != want {
                    continue;
                }
            }
            let methods = compare(query, template);
            let score = methods.fuse(weights);
            let candidate = TemplateMatch {
                id: template.id.clone(),
                score,
                methods,
                variant: template.variant,
            };
            match best.get(&template.id) {
                Some(existing) if existing.score >= candidate.score => {}
                _ => {
                    best.insert(&template.id, candidate);
                }
            }
        }
        let mut ranked: Vec<TemplateMatch> = best.into_values().collect();
        ranked.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        ranked.truncate(limit);
        ranked
    }
}

/// 四个度量的实际计算。
fn compare(query: &QueryShape, template: &PreparedTemplate) -> MethodScores {
    let inter = query.mask.and_count(&template.mask);
    let union = query.foreground_px + template.foreground_px - inter;
    let mask_score = if union == 0 {
        0.0
    } else {
        inter as f32 / union as f32
    };

    let boundary_q = query.boundary.count();
    let boundary_t = template.boundary.count();
    let edge_score = if boundary_q + boundary_t == 0 {
        0.0
    } else {
        // 允许 ±1px 的抗锯齿差异：落在对方膨胀掩码内即算命中
        let miss_q = query.boundary.andnot_count(&template.dilated);
        let miss_t = template.boundary.andnot_count(&query.dilated);
        1.0 - (miss_q + miss_t) as f32 / (boundary_q + boundary_t) as f32
    };

    let mut diff = 0.0f64;
    for (a, b) in query.hu.iter().zip(&template.hu) {
        diff += (a - b).abs();
    }
    let shape_score = (7.0 / (7.0 + diff)) as f32;

    let hamming = (query.hash ^ template.hash).count_ones() as f32;
    let perceptual_score = 1.0 - hamming / 64.0;

    MethodScores {
        mask: mask_score.clamp(0.0, 1.0),
        edge: edge_score.clamp(0.0, 1.0),
        shape: shape_score.clamp(0.0, 1.0),
        perceptual: perceptual_score.clamp(0.0, 1.0),
    }
}

/// 由源图构建一个变体模板。
fn prepare_template(
    id: &StratagemId,
    source: &RgbaImage,
    variant: TemplateVariant,
    cfg: &TemplateConfig,
    norm: &NormalizationConfig,
) -> Option<PreparedTemplate> {
    let (sw, sh) = (source.width() as usize, source.height() as usize);
    if sw < 4 || sh < 4 {
        return None;
    }
    let subject = subject_mask(source);
    let bbox = subject_bbox(&subject, sw, sh)?;
    let (bx, by, bw, bh) = bbox;

    let size = norm.size.max(16) as usize;
    let padding = norm.padding_ratio.clamp(0.0, 0.40) as f64;
    let variant_scale = match variant {
        TemplateVariant::Normal => 1.0f64,
        TemplateVariant::Brightness => 1.0,
        TemplateVariant::Scale => (1.0 - cfg.scale_delta.clamp(0.0, 0.3) as f64).max(0.5),
    };
    let inner = size as f64 * (1.0 - 2.0 * padding) * variant_scale;
    let scale = inner / bw.max(bh).max(1) as f64;
    let placed_w = ((bw as f64 * scale).round() as usize).clamp(1, size);
    let placed_h = ((bh as f64 * scale).round() as usize).clamp(1, size);
    let x0 = (size - placed_w) / 2;
    let y0 = (size - placed_h) / 2;

    let mut mask = BitMask::new(size, size);
    for dy in 0..placed_h {
        for dx in 0..placed_w {
            let sx = bx + (dx as f64 / scale) as usize;
            let sy = by + (dy as f64 / scale) as usize;
            if sx < sw && sy < sh && subject[sy * sw + sx] {
                mask.set(x0 + dx, y0 + dy, true);
            }
        }
    }

    // Brightness 变体：用 ±1px 形态学近似「阈值/亮度偏移导致的字形胖瘦差异」。
    // （掩码域本身已经对亮度不变，因此这里模拟的是阈值边界移动，而不是灰度缩放。）
    if matches!(variant, TemplateVariant::Brightness) {
        mask = mask.dilate(1);
    }

    let foreground_px = mask.count();
    if foreground_px < cfg.min_foreground_px {
        return None;
    }
    let boundary = mask.boundary();
    let dilated = mask.dilate(1);
    let hu = hu_moments(&mask);
    let hash = perceptual_hash(&mask);
    Some(PreparedTemplate {
        id: id.clone(),
        variant,
        mask,
        boundary,
        dilated,
        hu,
        hash,
        foreground_px,
    })
}

/// 源图的「主体」判定：优先 alpha；alpha 几乎全不透明时退回颜色规则。
fn subject_mask(source: &RgbaImage) -> Vec<bool> {
    let (w, h) = (source.width() as usize, source.height() as usize);
    let mut mask = vec![false; w * h];
    let mut opaque = 0usize;
    for y in 0..h {
        for x in 0..w {
            let p = source.get_pixel(x as u32, y as u32).0;
            if p[3] >= ALPHA_MASK_THRESHOLD {
                mask[y * w + x] = true;
                opaque += 1;
            }
        }
    }
    if opaque as f32 / (w * h).max(1) as f32 > 0.97 {
        // 无透明通道的美术图：用「非背景」颜色规则（高色度或高亮度）
        for y in 0..h {
            for x in 0..w {
                let p = source.get_pixel(x as u32, y as u32).0;
                let max = p[0].max(p[1]).max(p[2]) as i32;
                let min = p[0].min(p[1]).min(p[2]) as i32;
                let chroma = max - min;
                let luma = 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32;
                mask[y * w + x] = chroma > 60 || luma > 150.0;
            }
        }
    }
    mask
}

/// 主体外接框（x, y, w, h）；全背景返回 None。
fn subject_bbox(
    mask: &[bool],
    width: usize,
    height: usize,
) -> Option<(usize, usize, usize, usize)> {
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
    for y in 0..height {
        for x in 0..width {
            if mask[y * width + x] {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    (x0 != usize::MAX).then(|| (x0, y0, x1 - x0 + 1, y1 - y0 + 1))
}

/// 7 个 Hu 不变矩（取对数，符号保留）。
pub fn hu_moments(mask: &BitMask) -> [f64; 7] {
    let (w, h) = (mask.width, mask.height);
    let mut m = [0.0f64; 7]; // m00 m10 m01 m20 m11 m02 m30.. 实际用下面的数组
    let mut raw = [0.0f64; 10]; // 00 10 01 20 11 02 30 21 12 03
    for y in 0..h {
        for x in 0..w {
            if !mask.get(x, y) {
                continue;
            }
            let (xf, yf) = (x as f64, y as f64);
            raw[0] += 1.0;
            raw[1] += xf;
            raw[2] += yf;
            raw[3] += xf * xf;
            raw[4] += xf * yf;
            raw[5] += yf * yf;
            raw[6] += xf * xf * xf;
            raw[7] += xf * xf * yf;
            raw[8] += xf * yf * yf;
            raw[9] += yf * yf * yf;
        }
    }
    if raw[0] <= 0.0 {
        return [0.0; 7];
    }
    let cx = raw[1] / raw[0];
    let cy = raw[2] / raw[0];
    // 中心矩
    let mu20 = raw[3] - cx * raw[1];
    let mu02 = raw[5] - cy * raw[2];
    let mu11 = raw[4] - cx * raw[2];
    let mu30 = raw[6] - 3.0 * cx * raw[3] + 2.0 * cx * cx * raw[1];
    let mu03 = raw[9] - 3.0 * cy * raw[5] + 2.0 * cy * cy * raw[2];
    let mu21 = raw[7] - 2.0 * cx * raw[4] - cy * raw[3] + 2.0 * cx * cx * raw[2];
    let mu12 = raw[8] - 2.0 * cy * raw[4] - cx * raw[5] + 2.0 * cy * cy * raw[1];

    // 归一化中心矩 eta_pq = mu_pq / m00^(1 + (p+q)/2) —— 尺度不变
    let m00 = raw[0].max(1e-9);
    let norm = |p: u32, q: u32| m00.powf(1.0 + (p + q) as f64 / 2.0);
    let (n20, n02, n11) = (mu20 / norm(2, 0), mu02 / norm(0, 2), mu11 / norm(1, 1));
    let (n30, n21, n12, n03) = (
        mu30 / norm(3, 0),
        mu21 / norm(2, 1),
        mu12 / norm(1, 2),
        mu03 / norm(0, 3),
    );

    m[0] = n20 + n02;
    m[1] = (n20 - n02).powi(2) + 4.0 * n11 * n11;
    m[2] = (n30 - 3.0 * n12).powi(2) + (3.0 * n21 - n03).powi(2);
    m[3] = (n30 + n12).powi(2) + (n21 + n03).powi(2);
    m[4] = (n30 - 3.0 * n12) * (n30 + n12) * ((n30 + n12).powi(2) - 3.0 * (n21 + n03).powi(2))
        + (3.0 * n21 - n03) * (n21 + n03) * (3.0 * (n30 + n12).powi(2) - (n21 + n03).powi(2));
    m[5] = (n20 - n02) * ((n30 + n12).powi(2) - (n21 + n03).powi(2))
        + 4.0 * n11 * (n30 + n12) * (n21 + n03);
    m[6] = (3.0 * n21 - n03) * (n30 + n12) * ((n30 + n12).powi(2) - 3.0 * (n21 + n03).powi(2))
        - (n30 - 3.0 * n12) * (n21 + n03) * (3.0 * (n30 + n12).powi(2) - (n21 + n03).powi(2));

    let mut out = [0.0f64; 7];
    for (i, v) in m.iter().enumerate() {
        let abs = v.abs().max(1e-12);
        out[i] = v.signum() * abs.ln();
    }
    out
}

/// 64 位感知哈希（8×8 均值阈值）。
pub fn perceptual_hash(mask: &BitMask) -> u64 {
    const CELLS: usize = 8;
    let (w, h) = (mask.width, mask.height);
    let mut values = [0.0f64; CELLS * CELLS];
    for (ci, value) in values.iter_mut().enumerate() {
        let (cx, cy) = (ci % CELLS, ci / CELLS);
        let x0 = cx * w / CELLS;
        let x1 = ((cx + 1) * w / CELLS).max(x0 + 1);
        let y0 = cy * h / CELLS;
        let y1 = ((cy + 1) * h / CELLS).max(y0 + 1);
        let mut sum = 0.0f64;
        let mut count = 0.0f64;
        for y in y0..y1.min(h) {
            for x in x0..x1.min(w) {
                sum += if mask.get(x, y) { 1.0 } else { 0.0 };
                count += 1.0;
            }
        }
        *value = sum / count.max(1.0);
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let mut hash = 0u64;
    for (i, v) in values.iter().enumerate() {
        if *v > mean {
            hash |= 1u64 << i;
        }
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask_from(rows: &[&str]) -> BitMask {
        let h = rows.len();
        let w = rows[0].len();
        let mut mask = BitMask::new(w, h);
        for (y, row) in rows.iter().enumerate() {
            for (x, ch) in row.chars().enumerate() {
                if ch == '#' {
                    mask.set(x, y, true);
                }
            }
        }
        mask
    }

    fn cfg() -> TemplateConfig {
        TemplateConfig::default()
    }

    fn norm() -> NormalizationConfig {
        NormalizationConfig::default()
    }

    #[test]
    fn bitmask_ops_are_consistent() {
        let a = mask_from(&["##..", "##..", "....", "...."]);
        let b = mask_from(&["....", ".##.", ".##.", "...."]);
        assert_eq!(a.count(), 4);
        // a 的右下角 (1,1) 与 b 的左上角重叠，其余不重叠
        assert_eq!(a.and_count(&b), 1);
        assert_eq!(a.andnot_count(&b), 3);
        assert_eq!(b.andnot_count(&a), 3);
        let dilated = a.dilate(1);
        assert!(dilated.count() > a.count());
        assert!(dilated.get(2, 2));
        let eroded = a.erode(1);
        assert!(!eroded.get(0, 0), "角像素应被腐蚀");
        // 2×2 的全部像素都是轮廓；4×4 才有内部像素
        assert_eq!(a.boundary().count(), 4);
        let big = mask_from(&["####", "####", "####", "####"]);
        let boundary = big.boundary();
        assert!(boundary.get(0, 0) && !boundary.get(1, 1));
        assert_eq!(boundary.count(), 12);
    }

    #[test]
    fn hu_moments_are_scale_invariant_in_shape() {
        let small = mask_from(&["####", "####", "####", "####"]);
        let big = mask_from(&[
            "########", "########", "########", "########", "########", "########", "########",
            "########",
        ]);
        let (a, b) = (hu_moments(&small), hu_moments(&big));
        // 同形状不同尺寸：log|Hu1| 差异应很小
        assert!(
            (a[0] - b[0]).abs() < 0.05,
            "尺度不变性失败: {} vs {}",
            a[0],
            b[0]
        );
        let other = mask_from(&["####", "####", "....", "...."]);
        let c = hu_moments(&other);
        assert!((a[0] - c[0]).abs() > 0.05, "不同形状应有差异");
    }

    #[test]
    fn perceptual_hash_is_stable_for_identical_masks() {
        let m = mask_from(&["####", "##..", "##..", "####"]);
        assert_eq!(perceptual_hash(&m), perceptual_hash(&m.clone()));
        let other = mask_from(&["....", ".###", "###.", "...."]);
        assert_ne!(perceptual_hash(&m), perceptual_hash(&other));
    }

    #[test]
    fn embedded_library_loads_with_variants() {
        let db = TemplateDb::load(&cfg(), &norm()).expect("模板库加载");
        assert!(db.len() > 90, "内嵌库应加载 90+ 模板，实际 {}", db.len());
        assert!(db.ids().len() > 60, "ID 数应 60+，实际 {}", db.ids().len());
        assert!(!db.is_empty());
    }

    #[test]
    fn template_matches_itself_with_top1() {
        let db = TemplateDb::load(&cfg(), &norm()).expect("模板库加载");
        let target = db
            .templates
            .iter()
            .find(|t| t.variant == TemplateVariant::Normal)
            .expect("至少一个 Normal 模板");
        let query = QueryShape::from_mask(target.mask.clone());
        let ranked = db.rank(&query, &cfg(), None, 3);
        assert_eq!(ranked[0].id, target.id, "自身模板必须是第一名");
        assert!(
            ranked[0].score > 0.95,
            "自匹配分数应接近 1，实际 {}",
            ranked[0].score
        );
        assert!(ranked.len() >= 2);
    }

    #[test]
    fn template_matching_survives_small_scale_change() {
        let db = TemplateDb::load(&cfg(), &norm()).expect("模板库加载");
        let target = db
            .templates
            .iter()
            .find(|t| t.variant == TemplateVariant::Normal)
            .expect("至少一个 Normal 模板");
        // 把模板放大 8% 后再做查询（模拟格子尺寸估计误差）
        let (w, h) = (target.mask.width, target.mask.height);
        let mut scaled = BitMask::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let sx = ((x as f32 - w as f32 / 2.0) / 1.08 + w as f32 / 2.0) as i32;
                let sy = ((y as f32 - h as f32 / 2.0) / 1.08 + h as f32 / 2.0) as i32;
                if sx >= 0 && sy >= 0 && (sx as usize) < w && (sy as usize) < h {
                    if target.mask.get(sx as usize, sy as usize) {
                        scaled.set(x, y, true);
                    }
                }
            }
        }
        let query = QueryShape::from_mask(scaled);
        let ranked = db.rank(&query, &cfg(), None, 3);
        assert_eq!(
            ranked[0].id, target.id,
            "8% 缩放后仍应命中同一 ID（实际 {} {}）",
            ranked[0].id, ranked[0].score
        );
    }

    #[test]
    fn mask_iou_dominates_for_grossly_different_shapes() {
        let db = TemplateDb::load(&cfg(), &norm()).expect("模板库加载");
        let target = db.templates.first().expect("模板");
        let mut bar = BitMask::new(128, 128);
        for y in 60..68 {
            for x in 0..128 {
                bar.set(x, y, true);
            }
        }
        let query = QueryShape::from_mask(bar);
        let _ = target;
        let ranked = db.rank(&query, &cfg(), None, 1);
        assert!(
            ranked[0].score < 0.8,
            "无意义的宽条不应拿到高分，实际 {}",
            ranked[0].score
        );
    }

    #[test]
    fn method_scores_fuse_by_weights() {
        let scores = MethodScores {
            mask: 1.0,
            edge: 0.0,
            shape: 1.0,
            perceptual: 0.0,
        };
        let weights = TemplateWeights {
            mask: 0.5,
            edge: 0.5,
            shape: 0.0,
            perceptual: 0.0,
        };
        assert!((scores.fuse(&weights) - 0.5).abs() < 1e-6);
        let half = MethodScores::default().fuse(&weights);
        assert_eq!(half, 0.0);
    }

    #[test]
    fn source_mask_falls_back_for_opaque_art() {
        // 全不透明 + 高对比图案：alpha 路径会全选，必须退回颜色规则
        let mut img = RgbaImage::new(8, 8);
        for y in 0..8u32 {
            for x in 0..8u32 {
                let inside = (2..6).contains(&x) && (2..6).contains(&y);
                let color = if inside {
                    [230, 120, 40, 255]
                } else {
                    [10, 10, 10, 255]
                };
                img.put_pixel(x, y, image::Rgba(color));
            }
        }
        let mask = subject_mask(&img);
        assert_eq!(mask.iter().filter(|v| **v).count(), 16);
    }
}
