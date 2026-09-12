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
//! 3. **取消与结论**：取消标志交给参考 `AutomationSession::with_cancel`
//!    （每次捕获/输入前检查一次，故取消在一步之内生效），并把参考的 anyhow 错误
//!    按阶段归类回 H2AC 的具体错误类型。
//!
//! 输入释放由参考 `InputSession` 的 `Drop` 保证（异常、取消、权限失败都会走到）。

use std::sync::Arc;
use std::sync::mpsc::Sender;
use std::sync::{Mutex, atomic::Ordering};
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

    // ── 捕获会话 + 标定 ROI + 自动化会话（全部参考原文，仅追加取消钩子）──
    reporter.stage("Capture", 0, "");
    let mut capture_session = CaptureSessionManager::new();
    let capture = capture_session
        .get_or_create(&window)
        .map_err(|error| classify_reference_error("Capture", &error))?;
    let region = bind_loadout_region(capture, runtime.calibration())
        .map_err(|error| classify_reference_error("Capture", &error))?;
    let mut automation =
        AutomationSession::with_cancel(region, window, Some(shared.cancel_handle()))
            .map_err(|error| classify_reference_error("Input", &error))?;

    // ── 界面状态：Home 三态由参考 detect_ui_state 判定 ──
    reporter.stage("Scanning", 1, "");
    let initial = match scan_loadout_home(&mut automation, &runtime) {
        Ok(observation) => observation,
        Err(error) => {
            reporter.warn(format!("配装 Home 扫描失败: {error:#}"));
            return Err(classify_reference_error("Scanning", &error));
        }
    };
    let ui_state = detect_ui_state(&initial);
    reporter.info(format!("界面状态: {}", ui_state.label()));
    reporter.detail(format!("界面: {}", ui_state.label()));
    if let Err(error) = check_cancel(shared) {
        return Err(error);
    }

    // ── S3：参考事件 → H2AC 日志/进度 ──
    let events = event_sink(reporter);

    // ── 装配主体（单独成函数：失败时仍能拿到 automation 抓一帧调试图）──
    let outcome = assemble(
        &runtime,
        &mut automation,
        &events,
        reporter,
        ui_state,
        &stratagems,
        booster.as_deref(),
        shared,
    );

    if job.debug_screenshots {
        save_debug_frame(&mut automation, reporter, outcome.is_ok());
    }

    outcome
}

/// 按参考 `detect_ui_state` 的结果决定动作。本版本只做**装配**（不实现参考的保存路径）。
#[allow(clippy::too_many_arguments)]
fn assemble(
    runtime: &RecognizerRuntime,
    automation: &mut AutomationSession<'_>,
    events: &AppEventSink,
    reporter: &Reporter,
    ui_state: UiState,
    stratagems: &[String],
    booster: Option<&str>,
    shared: &LoadoutSyncShared,
) -> Result<(), LoadoutSyncError> {
    match ui_state {
        UiState::HomeEmpty => {
            reporter.stage("Stratagems", 1, "");
            apply_empty_loadout_preset(runtime, automation, events, stratagems, true)
                .map_err(|error| classify_reference_error("Stratagems", &error))?;
            check_cancel(shared)?;

            if let Some(booster) = booster {
                reporter.stage("Booster", 5, "");
                apply_booster_from_home(runtime, automation, events, std::slice::from_ref(&booster.to_string()))
                    .map_err(|error| classify_reference_error("Booster", &error))?;
            }
            Ok(())
        }
        UiState::HomeFilled => {
            // 只做装配：参考的 HomeFilled 分支是"保存预设"，本版本不实现。
            reporter.warn("配装界面已填满：请先清空四个战备槽再自动装配");
            Err(LoadoutSyncError::LoadoutNotEmpty { filled: 4 })
        }
        UiState::HomeMixed => {
            reporter.warn("配装界面部分填充：拒绝装配，避免误点到错误目标");
            Err(LoadoutSyncError::UnexpectedState {
                detail: "配装界面部分填充（HomeMixed），已按安全策略拒绝".into(),
            })
        }
        UiState::List(_) | UiState::Unknown => {
            reporter.warn("未检测到配装 Home 界面（可能停在列表或其它界面）");
            Err(LoadoutSyncError::LoadoutHomeNotDetected { score: 0.0 })
        }
    }
}

/// 参考 `AppEvent` → H2AC 日志/进度。
fn event_sink(reporter: &Reporter) -> AppEventSink {
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
}

/// 阶段边界的取消检查（细粒度取消由 `AutomationSession` 的取消钩子承担）。
fn check_cancel(shared: &LoadoutSyncShared) -> Result<(), LoadoutSyncError> {
    if shared.is_cancelled() {
        Err(LoadoutSyncError::Cancelled)
    } else {
        Ok(())
    }
}

/// 把参考实现的 anyhow 错误按**阶段 + 消息链**归类回 H2AC 的具体错误类型。
///
/// 审阅意见：此前所有失败都塌成 `UnexpectedState`，权限 / 输入 / 超时等类型全部丢失。
/// 这里沿 `error.chain()` 逐层匹配关键字：命中具体类型就返回具体类型，
/// 都不命中才退回 `UnexpectedState`（并保留完整消息链，日志里不会丢信息）。
fn classify_reference_error(stage: &'static str, error: &anyhow::Error) -> LoadoutSyncError {
    let detail = error
        .chain()
        .map(|cause| cause.to_string())
        .collect::<Vec<_>>()
        .join(" → ");
    let top = error.to_string();
    let lower = detail.to_ascii_lowercase();

    if lower.contains("automation cancelled") {
        return LoadoutSyncError::Cancelled;
    }
    if lower.contains("no foreground window") {
        return LoadoutSyncError::GameNotFound;
    }
    if lower.contains("not the foreground")
        || lower.contains("lost focus")
        || lower.contains("moved or resized")
        || lower.contains("identity changed")
        || lower.contains("no longer available")
        || lower.contains("is not the foreground window")
    {
        return LoadoutSyncError::GameWindowInvalid { detail };
    }
    if lower.contains("access is denied")
        || lower.contains("permission")
        || lower.contains("unauthorized")
        || lower.contains("elevation")
    {
        return LoadoutSyncError::PermissionError;
    }
    if lower.contains("capture")
        || lower.contains("dwm")
        || lower.contains("roi")
        || lower.contains("client rect")
        || lower.contains("client origin")
        || lower.contains("empty roi")
        || lower.contains("create capture session")
    {
        return LoadoutSyncError::CaptureFailed { detail };
    }
    if lower.contains("sendinput")
        || lower.contains("input")
        || lower.contains("cursor")
        || lower.contains("click")
        || lower.contains("wheel")
        || lower.contains("keyboard")
        || lower.contains("hotkey")
    {
        return LoadoutSyncError::InputError { detail };
    }
    if lower.contains("timeout") || lower.contains("timed out") {
        return LoadoutSyncError::Timeout {
            stage,
            ms: first_number(&lower).unwrap_or(0),
        };
    }
    if lower.contains("viewport")
        || lower.contains("shared landmark")
        || lower.contains("page turn")
        || lower.contains("wheel inputs")
    {
        return LoadoutSyncError::ViewportNavigationFailed { detail };
    }
    if lower.contains("did not stabilize") || lower.contains("hover") {
        return LoadoutSyncError::HoverVerificationFailed {
            name: top,
            score: 0.0,
        };
    }
    if lower.contains("neither moved nor dimmed")
        || lower.contains("remained unchanged")
        || lower.contains("did not return to a confirmed loadout home")
        || lower.contains("could not be tracked")
    {
        return LoadoutSyncError::SelectionVerificationFailed { name: top };
    }
    if lower.contains("not found") {
        return LoadoutSyncError::TargetNotFound { name: top };
    }
    if lower.contains("not recognized") || lower.contains("could not be recognized") {
        return LoadoutSyncError::RecognitionUnsupported {
            name: top,
            score: 0.0,
        };
    }
    LoadoutSyncError::UnexpectedState {
        detail: format!("{stage}: {detail}"),
    }
}

/// 取消息里第一个整数（参考的超时消息形如 "timed out after 4000 ms"）。
fn first_number(text: &str) -> Option<u64> {
    let digits: String = text
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// 调试帧落盘（H2AC 侧能力；参考栈不落盘）。
///
/// 成功与失败都会尝试抓一帧，文件名带状态后缀 —— 与 `config.rs` 的说明一致。
/// 取消路径下不落盘：取消钩子在捕获前生效，避免取消后还去抓帧。
fn save_debug_frame(automation: &mut AutomationSession<'_>, reporter: &Reporter, succeeded: bool) {
    let label = if succeeded { "ok" } else { "failed" };
    let Ok(image) = automation.capture() else {
        reporter.warn(format!("调试帧抓取失败（{label}）"));
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
    let path = dir.join(format!("loadout_sync_{label}_{stamp}.png"));
    match image.save(&path) {
        Ok(()) => reporter.info(format!("调试帧: {}", path.display())),
        Err(error) => reporter.warn(format!("调试帧保存失败: {error}")),
    }
}

/// 取消标志的只读检查（保留给未来的参考循环边界接缝；当前由 automation 钩子承担）。
#[allow(dead_code)]
fn cancelled(shared: &LoadoutSyncShared) -> bool {
    shared.is_cancelled() && shared.cancel_handle().load(Ordering::SeqCst)
}
