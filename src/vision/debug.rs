// 调试产物（需求 §23 / §24）。
//
// 约束：
//   * **禁止每帧写盘**：只有 debug level != Off、或显式捕获、或识别失败时才落盘；
//   * 每次运行一个时间戳目录，目录数量有上限（超限删除最旧的）；
//   * 产物是可读的 PNG + `result.json`，让「看图 → 定位失败阶段」成为可能。
use std::path::{Path, PathBuf};

use image::{GrayImage, Luma, RgbaImage};
use serde::Serialize;

use crate::loadout_sync::types::ImageRect;

use super::config::{DebugConfig, VisionDebugLevel};
use super::error::VisionError;
use super::recognize::VisionOutput;
use super::roi::{draw_cross, draw_rect, save_gray_png, save_png};

/// 一次调试会话（对应一个时间戳目录）。
#[derive(Debug, Clone)]
pub struct DebugSession {
    dir: PathBuf,
    level: VisionDebugLevel,
}

impl DebugSession {
    /// 创建会话目录并清理超限的旧会话。
    pub fn create(cfg: &DebugConfig, label: &str) -> Result<Self, VisionError> {
        let stamp = timestamp();
        let dir = cfg.dump_dir.join(format!("{stamp}_{label}"));
        std::fs::create_dir_all(&dir).map_err(|e| VisionError::Io {
            path: dir.display().to_string(),
            detail: format!("创建调试目录失败: {e}"),
        })?;
        prune_sessions(&cfg.dump_dir, cfg.max_dump_sessions);
        Ok(Self {
            dir,
            level: cfg.level,
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// 本次会话的调试级别（仅诊断/测试使用）。
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn level(&self) -> VisionDebugLevel {
        self.level
    }

    /// 落盘整次运行的全部产物。
    pub fn dump_output(&self, output: &VisionOutput) -> Result<(), VisionError> {
        save_png(&output.roi_image, &self.dir.join("roi.png"))?;

        let icon_boxes: Vec<(usize, ImageRect)> = output
            .traces
            .iter()
            .filter_map(|t| {
                output
                    .recognition
                    .detections
                    .iter()
                    .find(|d| d.slot == t.slot)
                    .and_then(|d| d.bbox)
                    .map(|b| (t.slot, b))
            })
            .collect();
        let grid = super::roi::overlay_slots(
            &output.roi_image,
            &output.geometry,
            &output.plan,
            &icon_boxes,
        );
        save_png(&grid, &self.dir.join("grid.png"))?;

        if self.level.per_cell_artifacts() {
            for trace in &output.traces {
                self.dump_cell(output, trace.slot)?;
            }
        }

        let report = DebugReport::from_output(output);
        let json = serde_json::to_string_pretty(&report).map_err(|e| VisionError::Io {
            path: self.dir.join("result.json").display().to_string(),
            detail: format!("序列化失败: {e}"),
        })?;
        std::fs::write(self.dir.join("result.json"), json).map_err(|e| VisionError::Io {
            path: self.dir.join("result.json").display().to_string(),
            detail: format!("写入失败: {e}"),
        })?;

        let mut text = output.topk_report().join("\n");
        text.push('\n');
        text.push_str(&output.recognition.summary_lines().join("\n"));
        std::fs::write(self.dir.join("report.txt"), text).map_err(|e| VisionError::Io {
            path: self.dir.join("report.txt").display().to_string(),
            detail: format!("写入失败: {e}"),
        })?;
        Ok(())
    }

    fn dump_cell(&self, output: &VisionOutput, slot: usize) -> Result<(), VisionError> {
        let Some(trace) = output.traces.iter().find(|t| t.slot == slot) else {
            return Ok(());
        };
        let prefix = format!("slot_{slot:02}");
        // 原始格子（含边框）与内裁剪结果（识别输入）
        save_png(
            &trace.cell_image,
            &self.dir.join(format!("{prefix}_raw.png")),
        )?;
        save_png(
            &trace.inner_image,
            &self.dir.join(format!("{prefix}_inner.png")),
        )?;
        save_gray_png(
            &mask_image(&trace.masks.orange, trace.masks.width, trace.masks.height),
            &self.dir.join(format!("{prefix}_orange.png")),
        )?;
        save_gray_png(
            &mask_image(&trace.masks.white, trace.masks.width, trace.masks.height),
            &self.dir.join(format!("{prefix}_white.png")),
        )?;
        save_gray_png(
            &mask_image(&trace.masks.combined, trace.masks.width, trace.masks.height),
            &self.dir.join(format!("{prefix}_mask.png")),
        )?;
        save_png(
            &components_image(trace),
            &self.dir.join(format!("{prefix}_components.png")),
        )?;
        if self.level.trace_scores() {
            save_gray_png(
                &score_image(
                    &trace.masks.scores.combined,
                    trace.masks.width,
                    trace.masks.height,
                ),
                &self.dir.join(format!("{prefix}_score.png")),
            )?;
        }
        if let Some(icon) = &trace.icon {
            save_png(
                &icon.normalized_rgb,
                &self.dir.join(format!("{prefix}_normalized.png")),
            )?;
            save_gray_png(
                &icon.normalized_mask,
                &self.dir.join(format!("{prefix}_normalized_mask.png")),
            )?;
            save_gray_png(
                &icon.normalized_edge,
                &self.dir.join(format!("{prefix}_edge.png")),
            )?;
        }
        Ok(())
    }
}

/// 掩码 → PNG（255 = 前景）。
pub fn mask_image(mask: &[bool], width: usize, height: usize) -> GrayImage {
    let mut img = GrayImage::new(width as u32, height as u32);
    for y in 0..height {
        for x in 0..width {
            let v = if mask.get(y * width + x).copied().unwrap_or(false) {
                255
            } else {
                0
            };
            img.put_pixel(x as u32, y as u32, Luma([v]));
        }
    }
    img
}

/// 前景分数图 → PNG（0~1 线性映射到 0~255）。
pub fn score_image(scores: &[f32], width: usize, height: usize) -> GrayImage {
    let mut img = GrayImage::new(width as u32, height as u32);
    for y in 0..height {
        for x in 0..width {
            let v = scores
                .get(y * width + x)
                .copied()
                .unwrap_or(0.0)
                .clamp(0.0, 1.0);
            img.put_pixel(x as u32, y as u32, Luma([(v * 255.0).round() as u8]));
        }
    }
    img
}

/// 连通域可视化：保留分量绿框、拒绝分量红框（在内裁剪图上绘制）。
pub fn components_image(trace: &super::recognize::CellTrace) -> RgbaImage {
    let mut img = trace.inner_image.clone();
    for component in &trace.analysis.components {
        let color = if component.kept {
            [80, 255, 120, 255]
        } else {
            [255, 80, 80, 255]
        };
        draw_rect(&mut img, component.bbox, color, (0, 0));
        let c = component.bbox.center();
        draw_cross(&mut img, c, 2, color, (0, 0));
    }
    if let Some(bbox) = trace.analysis.bbox {
        draw_rect(&mut img, bbox, [0, 200, 255, 255], (0, 0));
    }
    img
}

/// 时间戳目录名（本地时间不可用时退回计数器，绝不 panic）。
fn timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => format!("{:010}", d.as_secs()),
        Err(_) => "0000000000".to_string(),
    }
}

/// 删除超限的旧会话目录（按名称排序，名称前缀是零填充时间戳）。
fn prune_sessions(root: &Path, max_sessions: usize) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    while dirs.len() > max_sessions.max(1) {
        let oldest = dirs.remove(0);
        let _ = std::fs::remove_dir_all(oldest);
    }
}

// ─── JSON 报告（需求 §24 的 result.json） ───

#[derive(Debug, Serialize)]
pub struct DebugReport {
    pub screen_size: [u32; 2],
    pub roi: [i32; 4],
    pub plan: String,
    pub geometry_source: String,
    pub elapsed_ms: u128,
    pub from_cache: bool,
    pub slots: Vec<DebugSlot>,
    pub recognitions: Vec<DebugRecognition>,
}

#[derive(Debug, Serialize)]
pub struct DebugSlot {
    pub slot: usize,
    pub row: u32,
    pub col: u32,
    pub kind: String,
    pub cell: [i32; 4],
    pub inner: [i32; 4],
    pub bbox: Option<[i32; 4]>,
    pub foreground_ratio: f32,
    pub components_kept: usize,
}

#[derive(Debug, Serialize)]
pub struct DebugRecognition {
    pub slot: usize,
    pub stratagem_id: Option<String>,
    pub display_name: Option<String>,
    pub confidence: f32,
    pub method: String,
    pub template_score: f32,
    pub margin: f32,
    pub geometry_score: f32,
    pub outcome: String,
    pub unknown_reason: Option<String>,
    pub alternatives: Vec<DebugAlternative>,
}

#[derive(Debug, Serialize)]
pub struct DebugAlternative {
    pub stratagem_id: String,
    pub score: f32,
}

impl DebugReport {
    pub fn from_output(output: &VisionOutput) -> Self {
        use super::recognize::DetectionOutcome;
        let rec = &output.recognition;
        let slots = output
            .traces
            .iter()
            .map(|t| DebugSlot {
                slot: t.slot,
                row: 0,
                col: 0,
                kind: "unknown".to_string(),
                cell: rect_array(t.cell),
                inner: rect_array(t.inner_crop.rect),
                bbox: t.icon.as_ref().map(|i| rect_array(i.bbox_frame)),
                foreground_ratio: t.masks.foreground_ratio(),
                components_kept: t.analysis.kept().count(),
            })
            .collect();
        let recognitions = rec
            .detections
            .iter()
            .map(|d| {
                let (
                    id,
                    confidence,
                    method,
                    template_score,
                    margin,
                    geometry_score,
                    alternatives,
                    unknown_reason,
                    outcome,
                ) = match &d.outcome {
                    DetectionOutcome::Recognized {
                        id,
                        confidence,
                        method,
                        template_score,
                        margin,
                        geometry_score,
                        alternatives,
                        ..
                    } => (
                        Some(id.as_str().to_string()),
                        *confidence,
                        method.label().to_string(),
                        *template_score,
                        *margin,
                        *geometry_score,
                        alternatives
                            .iter()
                            .map(|a| DebugAlternative {
                                stratagem_id: a.id.as_str().to_string(),
                                score: a.score,
                            })
                            .collect(),
                        None,
                        "recognized".to_string(),
                    ),
                    DetectionOutcome::Unknown {
                        reason,
                        best,
                        best_score,
                        margin,
                    } => (
                        best.as_ref().map(|b| b.as_str().to_string()),
                        0.0,
                        "unknown".to_string(),
                        *best_score,
                        *margin,
                        0.0,
                        Vec::new(),
                        Some(reason.label().to_string()),
                        format!("unknown:{}", reason.label()),
                    ),
                    DetectionOutcome::Empty => (
                        None,
                        0.0,
                        "none".to_string(),
                        0.0,
                        0.0,
                        0.0,
                        Vec::new(),
                        None,
                        "empty".to_string(),
                    ),
                };
                DebugRecognition {
                    slot: d.slot,
                    display_name: id.as_ref().and_then(|k| {
                        super::id::StratagemId::new(k)
                            .display_name()
                            .map(str::to_string)
                    }),
                    stratagem_id: id,
                    confidence,
                    method,
                    template_score,
                    margin,
                    geometry_score,
                    outcome,
                    unknown_reason,
                    alternatives,
                }
            })
            .collect();
        Self {
            screen_size: [rec.screen.0, rec.screen.1],
            roi: rect_array(rec.roi),
            plan: format!("{:?}", rec.plan),
            geometry_source: format!("{:?}", rec.geometry_source),
            elapsed_ms: rec.elapsed_ms,
            from_cache: rec.from_cache,
            slots,
            recognitions,
        }
    }
}

fn rect_array(r: ImageRect) -> [i32; 4] {
    [r.x, r.y, r.w, r.h]
}
