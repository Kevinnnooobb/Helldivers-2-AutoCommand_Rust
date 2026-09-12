// Loadout Sync 的 GUI 侧胶水层 —— AppModel 只负责「启动任务 / 取消任务 / 显示状态」。
//
// 视觉算法、截图、鼠标注入全部在 loadout_sync 工作线程里，GUI 主线程不被阻塞。
use crate::loadout_sync::controller::{self, SyncJob};
use crate::loadout_sync::selection::LoadoutSyncSelection;
use crate::loadout_sync::state::{SyncEvent, SyncLogLevel};
use crate::loadout_sync::types::Calibration;
use crate::H2ACApp;
use crate::LogKind;

impl H2ACApp {
    /// 读取 H2AC 下排 Slot 06~10 → LoadoutSyncSelection（不做任何排序/左移）。
    pub fn loadout_sync_selection(&self) -> LoadoutSyncSelection {
        LoadoutSyncSelection::from_slots(&self.model.slots, &self.model.plugin_slots)
    }

    /// 当前是否有可执行的目标（用于 GUI 按钮可用性）。
    pub fn loadout_sync_ready(&self) -> bool {
        self.loadout_sync_selection().validate().is_ok()
    }

    pub fn loadout_sync_running(&self) -> bool {
        self.model.loadout_sync.is_running()
    }

    /// 快捷键 / 按钮统一入口：运行中再次触发 = 取消（禁止并发第二个任务）。
    pub fn toggle_loadout_sync(&mut self) {
        if self.model.loadout_sync.is_running() {
            self.cancel_loadout_sync();
        } else {
            self.start_loadout_sync();
        }
    }

    /// 唯一入口：启动自动装配（读取 H2AC Slot06~10）。
    ///
    /// 热键 / 主界面按钮 / 浮窗按钮都走这里 → 只有一个 Loadout Sync Controller，
    /// 不存在「正常模式一套、紧凑模式一套」的分叉（§2 / §15）。
    pub fn start_loadout_sync(&mut self) -> bool {
        let selection = self.loadout_sync_selection();
        self.start_loadout_sync_with(selection)
    }

    /// 以指定选择模型启动自动装配（紧凑预设草稿会走这条路径）。
    pub fn start_loadout_sync_with(&mut self, selection: LoadoutSyncSelection) -> bool {
        if let Err(e) = selection.validate() {
            self.log(LogKind::Warn, format!("[LoadoutSync] {}", e.message()));
            if let Some(hint) = e.hint() {
                self.log(LogKind::Info, format!("[LoadoutSync] {hint}"));
            }
            self.model.loadout_sync.shared().finish(
                crate::loadout_sync::state::SyncStatus::Failed {
                    code: e.code().to_string(),
                    message: e.message(),
                },
            );
            return false;
        }
        // 每次任务开始都重建截图会话：分辨率/显示模式/窗口切换后必须重新绑定
        crate::loadout_sync::wgc::invalidate();
        let cfg = self.model.config.loadout_sync.clone().sanitize();
        let job = SyncJob {
            selection,
            params: cfg.clone(),
            calibration: Calibration::default(),
            vision: self.model.config.vision.clone(),
            debug_screenshots: cfg.debug_screenshots || cfg.debug_overlay || self.model.debug_mode,
        };
        let started = self.model.loadout_sync.start(move |shared, tx| {
            let mut env = controller::RealEnv::new(job.debug_screenshots, cfg.debug_overlay);
            controller::run(&mut env, job, shared, tx);
        });
        if started {
            self.log(LogKind::Info, "[LoadoutSync] Triggered — 自动装配开始");
            true
        } else {
            self.log(LogKind::Warn, "[LoadoutSync] 已有任务在运行，忽略重复触发");
            false
        }
    }

    pub fn cancel_loadout_sync(&mut self) {
        if self.model.loadout_sync.is_running() {
            self.model.loadout_sync.request_cancel();
            self.log(LogKind::Warn, "[LoadoutSync] 已请求取消…");
        }
    }

    /// 每帧消费工作线程回传的日志与最终状态。
    pub fn poll_loadout_sync(&mut self) {
        for event in self.model.loadout_sync.drain_events() {
            match event {
                SyncEvent::Log(level, text) => {
                    let kind = match level {
                        SyncLogLevel::Info => LogKind::Info,
                        SyncLogLevel::Warn => LogKind::Warn,
                        SyncLogLevel::Error => LogKind::Warn,
                    };
                    self.log(kind, text);
                }
                SyncEvent::Finished(status) => match status {
                    crate::loadout_sync::state::SyncStatus::Succeeded => {
                        self.log(LogKind::Exec, "[LoadoutSync] SYNC COMPLETE — 装配完成");
                    }
                    crate::loadout_sync::state::SyncStatus::Cancelled => {
                        self.log(LogKind::Warn, "[LoadoutSync] 已取消");
                    }
                    crate::loadout_sync::state::SyncStatus::Failed { code, message } => {
                        self.log(LogKind::Warn, format!("[LoadoutSync] {code}: {message}"));
                    }
                    _ => {}
                },
            }
        }
        self.model.loadout_sync.reap();
    }
}
