// 视觉层错误 —— 每个变体都携带足够上下文，禁止 `unwrap()` / 裸 `Error`。
//
// 与项目现有风格一致：手写 `Display` / `std::error::Error`，不引入 thiserror/anyhow
// （需求 §26/§27：能用现有依赖就不要新增依赖）。
use std::fmt;

use crate::loadout_sync::types::ImageRect;

#[derive(Debug, Clone, PartialEq)]
pub enum VisionError {
    /// 捕获帧尺寸为 0（未初始化或后端异常）。
    ZeroSizedFrame,
    /// 标定 ROI 在缩放后超出画面（非 16:9 裁剪 / UI 缩放异常）。
    RoiOutsideFrame {
        roi: ImageRect,
        frame_w: u32,
        frame_h: u32,
    },
    /// 网格几何非法（格子边长、行列数等）。
    InvalidGrid { detail: String },
    /// 格子矩形不可用（越界 / 非正尺寸）。
    InvalidCell { slot: usize, rect: ImageRect },
    /// 图标模板加载失败。
    TemplateLoad { icon: String, detail: String },
    /// 模板库为空（无内嵌资源且磁盘目录缺省）。
    NoTemplates,
    /// 图像处理阶段失败（附带阶段名，便于定位）。
    ImageProcessing { stage: &'static str, detail: String },
    /// 文件系统读写失败。
    Io { path: String, detail: String },
    /// 模板数据库目录结构非法。
    TemplateDatabase { path: String, detail: String },
    /// CLI 参数错误。
    Usage { detail: String },
}

impl VisionError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ZeroSizedFrame => "VisionZeroSizedFrame",
            Self::RoiOutsideFrame { .. } => "VisionRoiOutsideFrame",
            Self::InvalidGrid { .. } => "VisionInvalidGrid",
            Self::InvalidCell { .. } => "VisionInvalidCell",
            Self::TemplateLoad { .. } => "VisionTemplateLoad",
            Self::NoTemplates => "VisionNoTemplates",
            Self::ImageProcessing { .. } => "VisionImageProcessing",
            Self::Io { .. } => "VisionIo",
            Self::TemplateDatabase { .. } => "VisionTemplateDatabase",
            Self::Usage { .. } => "VisionUsage",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::ZeroSizedFrame => "视觉输入帧尺寸为 0".to_string(),
            Self::RoiOutsideFrame {
                roi,
                frame_w,
                frame_h,
            } => format!(
                "标定 ROI ({},{},{},{}) 超出画面 {frame_w}x{frame_h}（可能为非 16:9 裁剪或 UI 缩放异常）",
                roi.x, roi.y, roi.w, roi.h
            ),
            Self::InvalidGrid { detail } => format!("网格几何非法：{detail}"),
            Self::InvalidCell { slot, rect } => format!(
                "槽位 {slot} 的格子矩形不可用：({},{},{},{})",
                rect.x, rect.y, rect.w, rect.h
            ),
            Self::TemplateLoad { icon, detail } => format!("图标模板「{icon}」加载失败：{detail}"),
            Self::NoTemplates => "模板库为空：内嵌图标不可用且未配置额外模板目录".to_string(),
            Self::ImageProcessing { stage, detail } => format!("图像处理失败（{stage}）：{detail}"),
            Self::Io { path, detail } => format!("文件读写失败「{path}」：{detail}"),
            Self::TemplateDatabase { path, detail } => {
                format!("模板目录「{path}」结构非法：{detail}")
            }
            Self::Usage { detail } => format!("命令行参数错误：{detail}"),
        }
    }
}

impl fmt::Display for VisionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for VisionError {}

impl From<VisionError> for crate::loadout_sync::error::LoadoutSyncError {
    fn from(value: VisionError) -> Self {
        crate::loadout_sync::error::LoadoutSyncError::CaptureFailed {
            detail: value.message(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_stable_and_unique() {
        let all = [
            VisionError::ZeroSizedFrame,
            VisionError::RoiOutsideFrame {
                roi: ImageRect::new(0, 0, 1, 1),
                frame_w: 1,
                frame_h: 1,
            },
            VisionError::InvalidGrid {
                detail: String::new(),
            },
            VisionError::InvalidCell {
                slot: 0,
                rect: ImageRect::new(0, 0, 1, 1),
            },
            VisionError::TemplateLoad {
                icon: String::new(),
                detail: String::new(),
            },
            VisionError::NoTemplates,
            VisionError::ImageProcessing {
                stage: "test",
                detail: String::new(),
            },
            VisionError::Io {
                path: String::new(),
                detail: String::new(),
            },
            VisionError::TemplateDatabase {
                path: String::new(),
                detail: String::new(),
            },
            VisionError::Usage {
                detail: String::new(),
            },
        ];
        let mut codes: Vec<&str> = all.iter().map(|e| e.code()).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), all.len(), "错误码必须稳定且唯一");
        for error in &all {
            assert!(!error.message().is_empty(), "错误必须带上下文");
        }
    }
}
