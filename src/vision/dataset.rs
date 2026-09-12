// 数据集生成工具（需求 §22）。
//
// 目的：**不需要人工逐张裁剪**。给定一批截图，按标定自动切槽、分割、归一化，
// 并按识别结果归档到 `dataset/{stratagem_id}/{raw,mask,normalized,edge}/`。
//
// 无法确定 ID 的样本（弃权）不会丢弃：落到 `_unlabeled/` 并写入
// `pending_labels.json` 供人工补标（人工只需填 ID，不需要裁图）。
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::error::VisionError;
use super::recognize::{DetectionOutcome, RecognizeTarget, VisionEngine};
use super::roi::{load_frame, save_gray_png, save_png};

/// 数据集生成参数。
#[derive(Debug, Clone)]
pub struct DatasetOptions {
    pub out_dir: PathBuf,
    pub target: RecognizeTarget,
    /// 是否额外输出 `preview/`（每个槽位一张带 bbox 的可视化图）
    pub write_preview: bool,
    /// 弃权样本最多保留多少个（防止一堆垃圾把目录撑爆）
    pub max_unlabeled: usize,
}

impl Default for DatasetOptions {
    fn default() -> Self {
        Self {
            out_dir: PathBuf::from("dataset"),
            target: RecognizeTarget::Loadout,
            write_preview: false,
            max_unlabeled: 200,
        }
    }
}

/// 一个待人工标注的槽位。
#[derive(Debug, Clone, Serialize)]
pub struct PendingLabel {
    pub screenshot: String,
    pub slot: usize,
    pub reason: String,
    pub best_candidate: Option<String>,
    pub best_score: f32,
    pub raw_path: String,
}

/// 生成报告（同时作为 CLI 输出与落盘的 `dataset_report.json`）。
#[derive(Debug, Clone, Serialize)]
pub struct DatasetReport {
    pub screenshots: usize,
    pub slots: usize,
    pub written: usize,
    pub empty_slots: usize,
    pub unknown_slots: usize,
    pub per_id: BTreeMap<String, usize>,
    pub pending: Vec<PendingLabel>,
    pub failures: Vec<String>,
}

impl DatasetReport {
    pub fn summary(&self) -> String {
        let mut lines = vec![format!(
            "数据集生成完成：{} 张截图 / {} 个槽位 → 写入 {} 个样本（空槽 {}，弃权 {}）",
            self.screenshots, self.slots, self.written, self.empty_slots, self.unknown_slots
        )];
        if !self.per_id.is_empty() {
            lines.push("每 ID 样本数：".to_string());
            for (id, count) in &self.per_id {
                lines.push(format!("  {id}: {count}"));
            }
        }
        if !self.pending.is_empty() {
            lines.push(format!(
                "待人工标注 {} 个（见 pending_labels.json）",
                self.pending.len()
            ));
        }
        for failure in &self.failures {
            lines.push(format!("  ! {failure}"));
        }
        lines.join("\n")
    }
}

/// 批量生成数据集。
pub fn generate(
    engine: &VisionEngine,
    screenshots: &[PathBuf],
    options: &DatasetOptions,
) -> Result<DatasetReport, VisionError> {
    std::fs::create_dir_all(&options.out_dir).map_err(|e| VisionError::Io {
        path: options.out_dir.display().to_string(),
        detail: format!("创建输出目录失败: {e}"),
    })?;
    let mut report = DatasetReport {
        screenshots: 0,
        slots: 0,
        written: 0,
        empty_slots: 0,
        unknown_slots: 0,
        per_id: BTreeMap::new(),
        pending: Vec::new(),
        failures: Vec::new(),
    };
    for path in screenshots {
        let frame = match load_frame(path) {
            Ok(frame) => frame,
            Err(e) => {
                report.failures.push(e.message());
                continue;
            }
        };
        report.screenshots += 1;
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("frame")
            .to_string();
        let output = match engine.run(&frame, options.target) {
            Ok(output) => output,
            Err(e) => {
                report
                    .failures
                    .push(format!("{}: {}", path.display(), e.message()));
                continue;
            }
        };
        for (trace, det) in output.traces.iter().zip(&output.recognition.detections) {
            report.slots += 1;
            match &det.outcome {
                DetectionOutcome::Empty => report.empty_slots += 1,
                DetectionOutcome::Recognized { id, .. } => {
                    let entry = report.per_id.entry(id.as_str().to_string()).or_insert(0);
                    *entry += 1;
                    write_sample(
                        &options.out_dir,
                        id.as_str(),
                        &stem,
                        det.slot,
                        trace,
                        options.write_preview,
                    )?;
                    report.written += 1;
                }
                DetectionOutcome::Unknown {
                    reason,
                    best,
                    best_score,
                    ..
                } => {
                    report.unknown_slots += 1;
                    if report.pending.len() < options.max_unlabeled {
                        let dir = options.out_dir.join("_unlabeled");
                        write_sample(&dir, "", &stem, det.slot, trace, options.write_preview)?;
                        report.pending.push(PendingLabel {
                            screenshot: path.display().to_string(),
                            slot: det.slot,
                            reason: reason.label().to_string(),
                            best_candidate: best.as_ref().map(|b| b.as_str().to_string()),
                            best_score: *best_score,
                            raw_path: dir
                                .join(format!("{stem}_slot{:02}_raw.png", det.slot))
                                .display()
                                .to_string(),
                        });
                    }
                }
            }
        }
    }
    write_pending(&options.out_dir, &report.pending)?;
    let json = serde_json::to_string_pretty(&report).map_err(|e| VisionError::Io {
        path: options.out_dir.display().to_string(),
        detail: format!("序列化报告失败: {e}"),
    })?;
    std::fs::write(options.out_dir.join("dataset_report.json"), json).map_err(|e| {
        VisionError::Io {
            path: options.out_dir.display().to_string(),
            detail: format!("写入报告失败: {e}"),
        }
    })?;
    Ok(report)
}

/// 写一个样本：`{root}/{id}/{raw,mask,normalized,edge}/...`
fn write_sample(
    root: &Path,
    id: &str,
    stem: &str,
    slot: usize,
    trace: &super::recognize::CellTrace,
    preview: bool,
) -> Result<(), VisionError> {
    let base = if id.is_empty() {
        root.to_path_buf()
    } else {
        root.join(id)
    };
    let name = format!("{stem}_slot{slot:02}");
    let Some(icon) = &trace.icon else {
        return Ok(());
    };
    save_png(&icon.raw, &base.join("raw").join(format!("{name}.png")))?;
    save_gray_png(&icon.mask, &base.join("mask").join(format!("{name}.png")))?;
    save_png(
        &icon.normalized_rgb,
        &base.join("normalized").join(format!("{name}.png")),
    )?;
    save_gray_png(
        &icon.normalized_edge,
        &base.join("edge").join(format!("{name}.png")),
    )?;
    save_gray_png(
        &icon.normalized_mask,
        &base.join("normalized").join(format!("{name}_mask.png")),
    )?;
    if preview {
        save_png(
            &super::debug::components_image(trace),
            &base.join("preview").join(format!("{name}.png")),
        )?;
    }
    Ok(())
}

fn write_pending(root: &Path, pending: &[PendingLabel]) -> Result<(), VisionError> {
    if pending.is_empty() {
        return Ok(());
    }
    let json = serde_json::to_string_pretty(pending).map_err(|e| VisionError::Io {
        path: root.display().to_string(),
        detail: format!("序列化待标注列表失败: {e}"),
    })?;
    std::fs::write(root.join("pending_labels.json"), json).map_err(|e| VisionError::Io {
        path: root.join("pending_labels.json").display().to_string(),
        detail: format!("写入失败: {e}"),
    })
}

/// 收集目录下所有 PNG/JPG 截图（排序保证可复现）。
pub fn collect_screenshots(dir: &Path) -> Result<Vec<PathBuf>, VisionError> {
    let entries = std::fs::read_dir(dir).map_err(|e| VisionError::Io {
        path: dir.display().to_string(),
        detail: format!("无法读取截图目录: {e}"),
    })?;
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && matches!(
                    p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()),
                    Some(ref ext) if ext == "png" || ext == "jpg" || ext == "jpeg"
                )
        })
        .collect();
    paths.sort();
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loadout_sync::types::ImageRect;
    use crate::vision::config::VisionConfig;
    use image::{Rgba, RgbaImage};

    fn engine() -> VisionEngine {
        VisionEngine::new(VisionConfig::default()).expect("引擎")
    }

    /// 生成一张「空配装界面」截图并落盘，供数据集工具测试使用。
    fn write_empty_home(path: &Path) {
        let layout = crate::vision::calibration::StratagemLayout::default();
        let geom = layout.resolve(2560, 1440).expect("几何");
        let mut img = RgbaImage::new(2560, 1440);
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
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("目录");
        }
        img.save(path).expect("保存");
    }

    #[test]
    fn collect_screenshots_filters_by_extension_and_sorts() {
        let dir = std::env::temp_dir().join("h2ac_dataset_collect");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("目录");
        for name in ["b.png", "a.PNG", "note.txt"] {
            std::fs::write(dir.join(name), b"x").expect("写");
        }
        let found = collect_screenshots(&dir).expect("收集");
        assert_eq!(found.len(), 2, "只收 PNG/JPG");
        assert!(found[0].ends_with("a.PNG"), "必须排序");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_screenshots_produce_no_samples_but_a_report() {
        let dir = std::env::temp_dir().join("h2ac_dataset_empty");
        let out = dir.join("out");
        let _ = std::fs::remove_dir_all(&dir);
        write_empty_home(&dir.join("frame.png"));
        let engine = engine();
        let screenshots = collect_screenshots(&dir).expect("收集");
        let report = generate(
            &engine,
            &screenshots,
            &DatasetOptions {
                out_dir: out.clone(),
                ..DatasetOptions::default()
            },
        )
        .expect("生成");
        assert_eq!(report.screenshots, 1);
        assert_eq!(report.empty_slots, 5);
        assert_eq!(report.written, 0);
        assert!(out.join("dataset_report.json").is_file());
        assert!(report.summary().contains("数据集生成完成"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_reported_not_panicking() {
        let engine = engine();
        let report = generate(
            &engine,
            &[PathBuf::from("这个文件不存在.png")],
            &DatasetOptions::default(),
        )
        .expect("生成");
        assert_eq!(report.screenshots, 0);
        assert_eq!(report.failures.len(), 1);
        assert!(report.failures[0].contains("读取截图失败"));
    }
}
