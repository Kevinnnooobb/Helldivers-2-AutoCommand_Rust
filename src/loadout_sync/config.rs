// Loadout Sync 自动化参数 —— 写入主 Config（config.json）的 loadout_sync 段。
//
// plan7 之后：识别、导航、滚动、时序全部由参考实现原文承担，其阈值是参考项目在
// 真实游戏里标定的常量（见 `src/vision/*`、`src/loadout/direct_select.rs`），
// 不通过 H2AC 配置暴露。因此这里**只保留仍然生效的开关**。
//
// 已删除的旧调参字段（滚动量 / 识别阈值 / 各级超时 / 重试次数 / 覆盖已填配装）：
// 它们随自研路径一起归档，留着只会让人改了配置却毫无效果。
// 旧 config.json 里的这些字段会被 serde 直接忽略，读取不会失败。
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoadoutSyncConfig {
    /// 任务结束时保存调试图到 `loadout_sync_debug/`（成功与失败都保存）：默认关闭
    #[serde(default)]
    pub debug_screenshots: bool,
}

impl Default for LoadoutSyncConfig {
    fn default() -> Self {
        Self {
            debug_screenshots: false,
        }
    }
}

impl LoadoutSyncConfig {
    /// 兼容入口：字段已全部是布尔开关，无需归一化（保留方法以免调用方改动）。
    pub fn sanitize(self) -> Self {
        self
    }
}
