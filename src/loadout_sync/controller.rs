//! controller —— H2AC 侧的任务生命周期（plan7）。
//!
//! ## 动作权威
//!
//! 整轮装配**完全**由参考实现原文执行：
//! `loadout::apply_empty_loadout_preset` / `loadout::apply_booster_from_home`
//! （`src/loadout/direct_select.rs`），捕获与输入来自参考
//! `capture::CaptureSessionManager` + `automation::AutomationSession`，
//! 识别来自参考 `vision::RecognizerRuntime`。
//!
//! 本文件**不含**任何识别、导航、悬停或点击判定，只做三件 H2AC 特有的事：
//!
//! 1. **S1 目标翻译**：把 H2AC 的 Slot06~10 目标翻成参考目录 `item_id`；
//!    目录里没有的目标（例如任务战备）**整轮拒答**，绝不退化成"滚动到找到为止"。
//! 2. **S3 事件桥**：把参考 `AppEvent` 桥到 H2AC 的日志与进度。
//! 3. **取消与结论**：在阶段边界检查取消标志，并把结果翻成 `SyncStatus`。
//!
//! 输入释放由参考 `InputSession` 的 `Drop` 保证（异常、取消、权限失败都会走到）。

use std::sync::atomic::Ordering;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::app_events::{AppEvent, AppEventSink};
use crate::automation::AutomationSession;
use crate::capture::CaptureSessionManager;
use crate::game_window::find_game_window;
use crate::loadout::{
    UiState, apply_booster_from_home, apply_empty_loadout_preset, bind_loadout_region,
    detect_ui_state, scan_loadout_home,
};
use crate::loadout_sync::catalog_bridge;
use crate::loadout_sync::config::LoadoutSyncConfig;
use crate::loadout_sync::error::LoadoutSyncError;
use crate::loadout_sync::selection::LoadoutSyncSelection;
use crate::loadout_sync::state::{
    LoadoutSyncShared, SyncEvent, SyncLogLevel, SyncStatus, log_line, status_from_error,
};
use crate::vision::RecognizerRuntime;

/// 一次装配任务的输入（H2AC 侧）。
pub struct SyncJob {
    pub selection: LoadoutSyncSelection,
    pub params: LoadoutSyncConfig,
    pub debug_screenshots: bool,
}

/// 日志与进度上报（线程安全：`AppEventSink` 要求闭包 `Send + Sync`）。
#[derive(Clone)]
struct Reporter {
    shared: Arc<LoadoutSyncShared>,
    tx: Arc<Mutex<Sender<SyncEvent>>>,
}

impl Reporter {
    fn new(shared: Arc<LoadoutSyncShared>, tx: Sender<SyncEvent>) -> Self {
        Self {
            shared,
            tx: Arc::new(Mutex::new(tx)),
        }
    }

    fn log(&self, level: SyncLogLevel, text: String) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(SyncEvent::Log(level, text));
        }
    }

    fn info(&self, text: impl AsRef<str>) {
        self.log(SyncLogLevel::Info, log_line(text.as_ref()));
    }

    fn warn(&self, text: impl AsRef<str>) {
        self.log(SyncLogLevel::Warn, log_line(text.as_ref()));
    }

    fn stage(&self, stage: &'static str, step: usize, target: impl Into<String>) {
        self.shared.set_stage(stage, step, target);
    }

    fn detail(&self, text: impl Into<String>) {
        self.shared.set_detail(text);
    }
}

/// 工作线程入口：唯一动作路径。
pub fn run(job: SyncJob, shared: Arc<LoadoutSyncShared>, tx: Sender<SyncEvent>) {
    let reporter = Reporter::new(shared.clone(), tx.clone());
    let started = Instant::now();
    let outcome = run_inner(&job, &reporter, &shared);

    // 取消优先于结果：取消时一律报 Cancelled（哪怕参考链路返回了其它错误）。
    let status = if shared.is_cancelled() {
        SyncStatus::Cancelled
    } else {
        match outcome {
            Ok(()) => SyncStatus::Succeeded,
            Err(error) => status_from_error(&error),
        }
    };

    match &status {
        SyncStatus::Succeeded => reporter.info(format!(
            "装配完成，用时 {:.1}s",
            started.elapsed().as_secs_f32()
        )),
        SyncStatus::Cancelled => {
            reporter.warn("已取消：鼠标按键状态由参考 InputSession 的 Drop 释放");
        }
        SyncStatus::Failed { code, message } => {
            reporter.warn(format!("失败 {code}: {message}"));
        }
        _ => {}
    }

    shared.finish(status.clone());
    if let Ok(tx) = reporter.tx.lock() {
        let _ = tx.send(SyncEvent::Finished(status));
    }
}

fn run_inner(
    job: &SyncJob,
    reporter: &Reporter,
    shared: &LoadoutSyncShared,
) -> Result<(), LoadoutSyncError> {
    // ── 参考运行时：内嵌目录（assets/reference/icons）+ 内嵌标定（data/calibration.json）──
    reporter.stage("Preparing", 0, "");
    let runtime = RecognizerRuntime::load().map_err(|error| LoadoutSyncError::UnexpectedState {
        detail: format!("参考运行时加载失败（内嵌目录/标定）: {error:#}"),
    })?;
    let catalog = runtime.icon_catalog().clone();
    reporter.info(format!("参考目录已加载：{} 条", catalog.iter().count()));

    // ── S1：H2AC 目标 → 参考目录 item_id（解析不出即整轮拒答）──
    let mut stratagems: Vec<String> = Vec::new();
    for item in job.selection.stratagems.iter().flatten() {
        let id = catalog_bridge::resolve_item_id(&catalog, &item.icon).ok_or_else(|| {
            LoadoutSyncError::UnexpectedState {
                detail: format!("参考目录里没有 {}（icon={}），无法装配", item.name, item.icon),
            }
        })?;
        reporter.info(format!("目标: {} → {id}", item.name));
        if !stratagems.iter().any(|existing| existing == id) {
            stratagems.push(id.to_string());
        }
    }
    let booster = match job.selection.booster.as_ref() {
        Some(item) => {
            let id = catalog_bridge::resolve_item_id(&catalog, &item.icon).ok_or_else(|| {
                LoadoutSyncError::UnexpectedState {
                    detail: format!("参考目录里没有 Booster {}（icon={}）", item.name, item.icon),
                }
            })?;
            reporter.info(format!("目标: {} → {id}", item.name));
            Some(id.to_string())
        }
        None => None,
    };

    // ── 窗口：参考实现要求游戏是前台窗口且标题匹配 ──
    reporter.stage("Window", 0, "");
    reporter.info("查找 HELLDIVERS 2 窗口");
    let window = match find_game_window() {
        Ok(window) => window,
        Err(error) => {
            reporter.warn(format!("未找到前台游戏窗口: {error:#}"));
            return Err(LoadoutSyncError::GameNotForeground);
        }
    };

    // ── 捕获会话 + 标定 ROI + 自动化会话（全部参考原文）──
    reporter.stage("Capture", 0, "");
    let mut capture_session = CaptureSessionManager::new();
    let capture = capture_session
        .get_or_create(&window)
        .map_err(|error| LoadoutSyncError::CaptureFailed {
            detail: format!("{error:#}"),
        })?;
    let region = bind_loadout_region(capture, runtime.calibration()).map_err(|error| {
        LoadoutSyncError::CaptureFailed {
            detail: format!("标定 ROI 解析失败: {error:#}"),
        }
    })?;
    let mut automation =
        AutomationSession::new(region, window).map_err(|error| LoadoutSyncError::InputError {
            detail: format!("{error:#}"),
        })?;

    // ── 界面状态：Home 三态由参考 detect_ui_state 判定 ──
    reporter.stage("Scanning", 1, "");
    let initial = match scan_loadout_home(&mut automation, &runtime) {
        Ok(observation) => observation,
        Err(error) => {
            reporter.warn(format!("配装 Home 扫描失败: {error:#}"));
            return Err(LoadoutSyncError::LoadoutHomeNotDetected { score: 0.0 });
        }
    };
    let ui_state = detect_ui_state(&initial);
    reporter.info(format!("界面状态: {}", ui_state.label()));
    reporter.detail(format!("界面: {}", ui_state.label()));
    if shared.is_cancelled() {
        return Err(LoadoutSyncError::Cancelled);
    }

    // ── S3：参考事件 → H2AC 日志/进度 ──
    let events = {
        let reporter = reporter.clone();
        AppEventSink::new(move |event: AppEvent| match event {
            AppEvent::ListSelectionStarted {
                item_kind,
                requested_items,
            } => reporter.info(format!("开始选择 {} ×{requested_items}", item_kind.label())),
            AppEvent::ItemSelected { item_id } => {
                reporter.info(format!("已确认选中: {item_id}"));
            }
            AppEvent::UiStateDetected { state } => {
                reporter.detail(format!("界面: {state}"));
            }
            AppEvent::PresetDone { preset, warning } => {
                if let Some(warning) = warning {
                    reporter.warn(format!("{preset}: {warning}"));
                }
            }
            other => reporter.detail(format!("{other:?}")),
        })
    };

    match ui_state {
        UiState::HomeEmpty => {
            reporter.stage("Stratagems", 1, "");
            apply_empty_loadout_preset(&runtime, &mut automation, &events, &stratagems, true)
                .map_err(|error| LoadoutSyncError::UnexpectedState {
                    detail: format!("装配战备失败: {error:#}"),
                })?;
            if shared.is_cancelled() {
                return Err(LoadoutSyncError::Cancelled);
            }

            if let Some(booster) = booster.as_ref() {
                reporter.stage("Booster", 5, "");
                apply_booster_from_home(
                    &runtime,
                    &mut automation,
                    &events,
                    std::slice::from_ref(booster),
                )
                .map_err(|error| LoadoutSyncError::UnexpectedState {
                    detail: format!("装配 Booster 失败: {error:#}"),
                })?;
            }
        }
        UiState::HomeFilled => {
            // D2：本次只做装配，不实现参考的"保存预设"路径。
            reporter.warn("配装界面已填满：请先清空四个战备槽再自动装配");
            return Err(LoadoutSyncError::LoadoutNotEmpty { filled: 4 });
        }
        UiState::HomeMixed => {
            reporter.warn("配装界面部分填充：拒绝装配，避免误点到错误目标");
            return Err(LoadoutSyncError::UnexpectedState {
                detail: "配装界面部分填充（HomeMixed），已按安全策略拒绝".into(),
            });
        }
        UiState::List(_) | UiState::Unknown => {
            reporter.warn("未检测到配装 Home 界面（可能停在列表或其它界面）");
            return Err(LoadoutSyncError::LoadoutHomeNotDetected { score: 0.0 });
        }
    }

    if shared.is_cancelled() {
        return Err(LoadoutSyncError::Cancelled);
    }

    if job.debug_screenshots {
        save_debug_frame(&mut automation, reporter);
    }

    if job.params.debug_overlay {
        reporter.warn("debug_overlay 已随旧视觉管线移除（参考栈不提供叠加图）");
    }
    Ok(())
}

/// 调试帧落盘（H2AC 侧能力；参考栈不落盘，这里在阶段末补抓一帧）。
fn save_debug_frame(automation: &mut AutomationSession<'_>, reporter: &Reporter) {
    let Ok(image) = automation.capture() else {
        reporter.warn("调试帧抓取失败");
        return;
    };
    let dir = crate::util::app_dir().join("loadout_sync_debug");
    if let Err(error) = std::fs::create_dir_all(&dir) {
        reporter.warn(format!("调试帧目录创建失败: {error}"));
        return;
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0);
    let path = dir.join(format!("loadout_sync_{stamp}.png"));
    match image.save(&path) {
        Ok(()) => reporter.info(format!("调试帧: {}", path.display())),
        Err(error) => reporter.warn(format!("调试帧保存失败: {error}")),
    }
}

/// 取消标志的只读检查（供将来放进参考循环边界的接缝使用）。
#[allow(dead_code)]
fn cancelled(shared: &LoadoutSyncShared) -> bool {
    shared.is_cancelled() && shared.cancel_handle().load(Ordering::SeqCst)
}
