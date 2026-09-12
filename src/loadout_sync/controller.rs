// Loadout Sync 控制器 —— 显式状态机 + 工作线程执行。
//
// 为什么是状态机而不是一个巨大的 apply_loadout()：
//   * 每一步都有独立的超时、重试上限与失败错误类型；
//   * 每一步都能被取消（取消检查贯穿所有循环）；
//   * 视觉不成立时立即停止，绝不「随机继续点」。
//
// 状态机只依赖 SyncEnv（窗口/截图/输入/时钟），因此可以在单元测试里用
// mock 环境跑完整个流程，不需要真实游戏。
//
// 动作权威（plan6 §5.3）：
//   * 整轮装配**只有**一条动作路径 —— [`SyncState::DirectSelecting`] 交给
//     `direct_select` 会话；
//   * 本文件不再包含模板评分、滚动位移推导、hover 阈值或点击后图标再识别；
//   * controller 只负责窗口、取消、日志、生命周期与失败诊断。
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::loadout_sync::capture::CapturedFrame;
use crate::loadout_sync::config::LoadoutSyncConfig;
use crate::loadout_sync::direct_select::classify::CatalogClassifier;
use crate::loadout_sync::direct_select::real_io::RealDirectSelectIo;
use crate::loadout_sync::direct_select::session::{self, SessionPlan};
use crate::loadout_sync::error::LoadoutSyncError;
use crate::loadout_sync::input::SyncInput;
use crate::loadout_sync::selection::LoadoutSyncSelection;
use crate::loadout_sync::state::{
    log_line, status_from_error, LoadoutSyncShared, SyncEvent, SyncLogLevel, SyncStatus,
};
use crate::loadout_sync::types::{Calibration, GameWindowInfo};
use crate::vision::reference_catalog::IconCatalog;

// ─── 状态 ───

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncState {
    Idle,
    Preparing,
    ValidatingSelection,
    FindingGameWindow,
    /// 整轮装配交给 plan6 的 `direct_select` 会话（唯一动作路径）。
    DirectSelecting,
    Completed,
    Failed,
    Cancelled,
}

impl SyncState {
    pub fn name(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Preparing => "Preparing",
            Self::ValidatingSelection => "ValidatingSelection",
            Self::FindingGameWindow => "FindingGameWindow",
            Self::DirectSelecting => "DirectSelecting",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

// ─── 环境抽象（真实实现 / 测试 mock） ───

pub trait SyncEnv {
    /// 找到游戏窗口
    fn find_window(&mut self) -> Result<GameWindowInfo, LoadoutSyncError>;
    /// 游戏窗口是否处于前台（绝不主动抢焦点，因此必须由环境回答）
    fn window_is_foreground(&mut self, win: &GameWindowInfo) -> bool;
    fn capture(
        &mut self,
        win: &GameWindowInfo,
        want_color: bool,
    ) -> Result<CapturedFrame, LoadoutSyncError>;
    fn input(&mut self) -> &mut dyn SyncInput;
    fn now_ms(&mut self) -> u64;
    /// 环境自带的等待原语（direct_select 使用自己的轮询节奏，此处留给测试/调试实现）
    #[allow(dead_code)]
    fn sleep_ms(&mut self, ms: u64);
    /// 保存调试截图（debug 关闭时为空实现）
    fn save_debug(&mut self, name: &str, frame: &CapturedFrame, note: &str);
}

// ─── 报告器（状态 + 日志） ───

pub struct Reporter {
    shared: Arc<LoadoutSyncShared>,
    tx: std::sync::mpsc::Sender<SyncEvent>,
}

impl Reporter {
    pub fn new(shared: Arc<LoadoutSyncShared>, tx: std::sync::mpsc::Sender<SyncEvent>) -> Self {
        Self { shared, tx }
    }

    pub fn log(&self, level: SyncLogLevel, msg: impl AsRef<str>) {
        let _ = self.tx.send(SyncEvent::Log(level, log_line(msg.as_ref())));
    }

    pub fn info(&self, msg: impl AsRef<str>) {
        self.log(SyncLogLevel::Info, msg);
    }

    pub fn warn(&self, msg: impl AsRef<str>) {
        self.log(SyncLogLevel::Warn, msg);
    }

    pub fn error(&self, msg: impl AsRef<str>) {
        self.log(SyncLogLevel::Error, msg);
    }

    pub fn stage(&self, state: SyncState, step: usize, target: impl Into<String>) {
        self.shared.set_stage(state.name(), step, target);
    }

    pub fn detail(&self, msg: impl Into<String>) {
        self.shared.set_detail(msg);
    }

    pub fn cancelled(&self) -> bool {
        self.shared.is_cancelled()
    }

    /// 取消标志的独立句柄（供 `direct_select` 的 IO 在自己的循环里检查取消）。
    ///
    /// 返回 `Arc` 而不是借用：调用方通常在持有 `&mut self` 的同时把它交给
    /// 一个持有 `&mut dyn SyncEnv` 的 IO，借用 self 字段会与后续字段写入冲突。
    pub fn cancel_check_handle(&self) -> Arc<std::sync::atomic::AtomicBool> {
        self.shared.cancel_handle()
    }

    pub fn cancel_check(&self) -> Result<(), LoadoutSyncError> {
        if self.cancelled() {
            Err(LoadoutSyncError::Cancelled)
        } else {
            Ok(())
        }
    }
}

// ─── 任务描述 ───

#[derive(Debug, Clone)]
pub struct SyncJob {
    pub selection: LoadoutSyncSelection,
    pub params: LoadoutSyncConfig,
    pub calibration: Calibration,
    /// 视觉管线配置（供 `vision` CLI 探针复用同一份配置；装配路径不再读取它）。
    #[allow(dead_code)]
    pub vision: crate::vision::config::VisionConfig,
    pub debug_screenshots: bool,
}

// ─── 状态机 ───

pub struct Machine {
    job: SyncJob,
    reporter: Reporter,
    state: SyncState,
    window: Option<GameWindowInfo>,
    frame: Option<CapturedFrame>,
    /// 阶段超时（毫秒，基于 SyncEnv 时钟：测试里可虚拟推进，不依赖真实等待）
    stage_deadline_ms: u64,
    stage_timeout_ms: u64,
    deadline_armed: bool,
    started_ms: Option<u64>,
    /// 审计轨迹（测试断言 + 调试日志）
    pub transitions: Vec<SyncState>,
}

const POLL_MS: u64 = 80;

impl Machine {
    pub fn new(job: SyncJob, reporter: Reporter) -> Self {
        Self {
            job,
            reporter,
            state: SyncState::Idle,
            window: None,
            frame: None,
            stage_deadline_ms: 0,
            stage_timeout_ms: 0,
            deadline_armed: false,
            started_ms: None,
            transitions: Vec::new(),
        }
    }

    pub fn state(&self) -> SyncState {
        self.state
    }

    fn goto(&mut self, state: SyncState) {
        #[cfg(test)]
        eprintln!("[state] -> {}", state.name());
        self.transitions.push(state);
        self.state = state;
        self.stage_timeout_ms = self.stage_timeout(state).as_millis() as u64;
        // 真正的时间戳在下一步 advance() 里用 SyncEnv 时钟打点
        self.deadline_armed = false;
    }

    fn stage_timeout(&self, state: SyncState) -> Duration {
        let p = &self.job.params;
        let ms = match state {
            SyncState::FindingGameWindow => 1_000,
            // direct_select 会话在**一次** `advance` 里跑完整轮装配，
            // 因此它的阶段预算就是整轮预算，不能按单步的 2s 计算。
            SyncState::DirectSelecting => p.total_timeout_ms,
            _ => 2_000,
        };
        Duration::from_millis(ms)
    }

    // ─── 主推进 ───

    pub fn advance(&mut self, env: &mut dyn SyncEnv) -> Result<(), LoadoutSyncError> {
        self.reporter.cancel_check()?;
        let now = env.now_ms();
        if !self.deadline_armed {
            self.stage_deadline_ms = now + self.stage_timeout_ms;
            self.deadline_armed = true;
        }
        let started = *self.started_ms.get_or_insert(now);
        if now.saturating_sub(started) > self.job.params.total_timeout_ms {
            return Err(LoadoutSyncError::Timeout {
                stage: "Total",
                ms: self.job.params.total_timeout_ms,
            });
        }
        if now > self.stage_deadline_ms
            && !matches!(
                self.state,
                SyncState::Idle | SyncState::Completed | SyncState::Failed | SyncState::Cancelled
            )
        {
            return Err(LoadoutSyncError::Timeout {
                stage: self.state.name(),
                ms: self.stage_timeout_ms,
            });
        }

        match self.state {
            SyncState::Idle => self.step_preparing(env),
            SyncState::Preparing => self.step_preparing(env),
            SyncState::ValidatingSelection => self.step_validating(),
            SyncState::FindingGameWindow => self.step_finding_window(env),
            SyncState::DirectSelecting => self.step_direct_select(env),
            SyncState::Completed | SyncState::Failed | SyncState::Cancelled => Ok(()),
        }
    }

    // ─── 各状态实现 ───

    fn step_preparing(&mut self, _env: &mut dyn SyncEnv) -> Result<(), LoadoutSyncError> {
        self.reporter.info("Triggered — 开始自动装配");
        let p = &self.job.params;
        self.reporter.detail(format!(
            "scroll={} probe={} threshold={:.2} max_scroll={} max_retry={}",
            p.scroll_delta,
            p.scroll_probe_delta,
            p.recognition_threshold,
            p.max_scroll_attempts,
            p.max_retry_attempts
        ));
        if self.job.debug_screenshots {
            self.reporter
                .info("调试模式：失败时会保存截图到 screenshots/loadout_sync/");
        }
        self.goto(SyncState::ValidatingSelection);
        Ok(())
    }

    fn step_validating(&mut self) -> Result<(), LoadoutSyncError> {
        self.reporter.stage(SyncState::ValidatingSelection, 0, "");
        self.reporter.info("读取 H2AC Slot 06~10");
        for (i, item) in self.job.selection.stratagems.iter().enumerate() {
            match item {
                Some(item) => self.reporter.info(format!("S{}: {}", i + 1, item.name)),
                None => self.reporter.info(format!("S{}: (未配置)", i + 1)),
            }
        }
        match &self.job.selection.booster {
            Some(b) => self.reporter.info(format!("Booster: {}", b.name)),
            None => self.reporter.info("Booster: (未配置，将跳过)"),
        }
        // 本地校验：4 个 Stratagem 必须齐全且不重复；Booster 可空。
        // 绝不做「自动左移」或猜测用户意图。
        self.job.selection.validate()?;
        self.goto(SyncState::FindingGameWindow);
        Ok(())
    }

    fn step_finding_window(&mut self, env: &mut dyn SyncEnv) -> Result<(), LoadoutSyncError> {
        self.reporter.stage(SyncState::FindingGameWindow, 0, "");
        self.reporter.info("Finding HELLDIVERS 2");
        let win = env.find_window()?;
        if !env.window_is_foreground(&win) {
            // 不抢焦点、不 Alt+Tab：直接失败并提示用户
            return Err(LoadoutSyncError::GameNotForeground);
        }
        self.reporter
            .info(format!("Window detected: {}x{}", win.width(), win.height()));
        self.window = Some(win);
        self.reporter.info("动作路径: direct_select（plan6）");
        self.goto(SyncState::DirectSelecting);
        Ok(())
    }

    /// 把整轮装配交给 `direct_select` 会话 —— **唯一**的动作路径。
    ///
    /// 这里**只做**：读 preset、装载目录、跑会话、上报诊断、把结论转成
    /// `Completed` / `Err`。所有「识别 → 规划 → 有限输入 → 状态验证」都在
    /// `direct_select` 里，controller 不再参与。
    fn step_direct_select(&mut self, env: &mut dyn SyncEnv) -> Result<(), LoadoutSyncError> {
        self.reporter.stage(SyncState::DirectSelecting, 0, "");
        let win = self.window.ok_or(LoadoutSyncError::GameNotFound)?;

        // ── 目录 → 分类器 ──
        let root = crate::vision::reference_catalog::default_root();
        let t0 = Instant::now();
        let catalog = IconCatalog::load(&root).map_err(|e| LoadoutSyncError::UnexpectedState {
            detail: format!(
                "参考图标目录加载失败（{}）: {}",
                root.display(),
                e.message()
            ),
        })?;
        let load_ms = t0.elapsed().as_millis();
        let classifier = CatalogClassifier::load(catalog);
        let build_ms = t0.elapsed().as_millis();
        self.reporter.detail(format!(
            "catalog {} 条可分类 / {} 条无法解析到当前图标键；加载 {}ms，模板构建累计 {}ms",
            classifier.classifiable(),
            classifier.unresolved.len(),
            load_ms,
            build_ms
        ));
        if !classifier.unresolved.is_empty() {
            self.reporter.warn(format!(
                "参考目录中未映射到当前图标键（不会作为目标）: {}",
                classifier.unresolved.join(", ")
            ));
        }

        // ── preset → 目录 ID（绝不猜：解析不出来就整轮拒绝） ──
        let mut stratagems: Vec<String> = Vec::new();
        for item in self.job.selection.stratagems.iter().flatten() {
            let id = classifier.catalog_id_for_icon(&item.icon).ok_or_else(|| {
                LoadoutSyncError::UnexpectedState {
                    detail: format!(
                        "参考目录里没有 {}（icon={}），direct_select 无法装配",
                        item.name, item.icon
                    ),
                }
            })?;
            self.reporter
                .info(format!("期望: {} → 目录 {}", item.name, id));
            if !stratagems.iter().any(|s| s == id) {
                stratagems.push(id.to_string());
            }
        }
        let booster = match self.job.selection.booster.as_ref() {
            Some(b) => {
                let id = classifier.catalog_id_for_icon(&b.icon).ok_or_else(|| {
                    LoadoutSyncError::UnexpectedState {
                        detail: format!("参考目录里没有 Booster {}（icon={}）", b.name, b.icon),
                    }
                })?;
                self.reporter
                    .info(format!("期望: {} → 目录 {}", b.name, id));
                Some(id.to_string())
            }
            None => None,
        };
        let plan = SessionPlan {
            stratagems,
            booster,
        };

        // ── 跑会话 ──
        let cancel = self.reporter.cancel_check_handle();
        let calibration = self.job.calibration;
        let mut log = |line: &str| self.reporter.info(line.to_string());
        let result = {
            let mut io = RealDirectSelectIo::with_calibration(env, win, calibration, classifier);
            io = io.with_cancel(&cancel);
            let r = session::run(&mut io, &plan, &mut log);
            // 让失败截图仍能拿到 direct_select 看到的那一帧
            self.frame = io.last_frame().cloned();
            r
        };

        match result {
            Ok(report) => {
                self.reporter.detail(report.summary());
                self.reporter
                    .info(format!("阶段: {}", report.phases.join(" → ")));
                if report.is_success() {
                    self.goto(SyncState::Completed);
                    Ok(())
                } else {
                    Err(LoadoutSyncError::UnexpectedState {
                        detail: format!(
                            "direct_select 未完成（已确认 {}/{}）: {}",
                            report.selected.len(),
                            report.expected,
                            report.failures.join(" / ")
                        ),
                    })
                }
            }
            Err(error) => {
                self.reporter.detail(error.report.summary());
                self.reporter.info(format!(
                    "direct_select 已确认 {}/{} 项后安全停止",
                    error.report.selected.len(),
                    error.report.expected
                ));
                Err(LoadoutSyncError::UnexpectedState {
                    detail: format!("direct_select 安全停止: {error}"),
                })
            }
        }
    }
}

// ─── 驱动 ───

/// 运行整个流程；任何错误都会先释放输入，再返回状态。
pub fn run(
    env: &mut dyn SyncEnv,
    job: SyncJob,
    shared: Arc<LoadoutSyncShared>,
    tx: std::sync::mpsc::Sender<SyncEvent>,
) -> SyncStatus {
    let reporter = Reporter::new(shared.clone(), tx.clone());
    let total_timeout_ms = job.params.total_timeout_ms;
    let mut machine = Machine::new(job, reporter);
    machine.goto(SyncState::Preparing);
    // 硬上限：状态机自身的超时依赖 SyncEnv 时钟，若某个状态循环不推进时钟
    // （或环境实现有问题）就会死循环，因此这里再加「迭代次数 + 真实墙钟」双保险。
    let max_iterations = (total_timeout_ms / POLL_MS).max(64) * 8;
    let wall_deadline =
        std::time::Instant::now() + Duration::from_millis(total_timeout_ms * 3 + 10_000);
    let mut iterations: u64 = 0;
    let status = loop {
        iterations += 1;
        if iterations > max_iterations {
            let err = LoadoutSyncError::UnexpectedState {
                detail: format!("状态机迭代超过上限（{max_iterations}）"),
            };
            env.input().release_all();
            machine
                .reporter
                .error(format!("ERROR: {} — {}", err.code(), err.message()));
            break status_from_error(&err);
        }
        if std::time::Instant::now() > wall_deadline {
            let err = LoadoutSyncError::Timeout {
                stage: "WallClock",
                ms: total_timeout_ms * 3 + 10_000,
            };
            env.input().release_all();
            machine
                .reporter
                .error(format!("ERROR: {} — {}", err.code(), err.message()));
            break status_from_error(&err);
        }
        let state = machine.state();
        if state.is_terminal() {
            break match state {
                SyncState::Completed => SyncStatus::Succeeded,
                SyncState::Cancelled => SyncStatus::Cancelled,
                _ => SyncStatus::Failed {
                    code: "UnexpectedState".into(),
                    message: "自动装配失败：内部状态异常。".into(),
                },
            };
        }
        match machine.advance(env) {
            Ok(()) => continue,
            Err(err) => {
                // 异常路径也必须释放所有按键/鼠标，绝不留按下状态
                env.input().release_all();
                machine
                    .reporter
                    .error(format!("ERROR: {} — {}", err.code(), err.message()));
                if let Some(hint) = err.hint() {
                    machine.reporter.warn(hint);
                }
                if machine.job.debug_screenshots {
                    if let Some(frame) = machine.frame.clone() {
                        env.save_debug(
                            &format!("failed_{}", machine.state.name()),
                            &frame,
                            err.code(),
                        );
                    }
                }
                machine.reporter.info("Automation aborted");
                // 审计轨迹：失败/取消也是一次明确的状态迁移（便于日志与测试追踪）
                machine.goto(if err.is_cancelled() {
                    SyncState::Cancelled
                } else {
                    SyncState::Failed
                });
                break status_from_error(&err);
            }
        }
    };
    // 正常/取消路径同样释放输入
    env.input().release_all();
    if matches!(status, SyncStatus::Succeeded) {
        machine.reporter.info("Completed");
    } else if matches!(status, SyncStatus::Cancelled) {
        machine.reporter.info("Cancelled by user");
    }
    shared.finish(status.clone());
    let _ = tx.send(SyncEvent::Finished(status.clone()));
    status
}

// ─── 真实环境实现 ───

pub struct RealEnv {
    input: crate::loadout_sync::input::LoadoutInputController,
    started: Instant,
    debug_dir: Option<std::path::PathBuf>,
    debug_overlay: bool,
}

impl RealEnv {
    pub fn new(debug_screenshots: bool, debug_overlay: bool) -> Self {
        Self {
            input: crate::loadout_sync::input::LoadoutInputController::new(),
            started: Instant::now(),
            debug_dir: debug_screenshots
                .then(|| crate::util::app_dir().join("screenshots/loadout_sync")),
            debug_overlay,
        }
    }
}

impl Drop for RealEnv {
    fn drop(&mut self) {
        // 任务结束就释放截图会话：WGC 会话存活期间系统会在被捕获窗口周围显示捕获指示框，
        // 缓存到下一次任务既无必要也不礼貌；下次任务开始时按需重建。
        crate::loadout_sync::wgc::invalidate();
    }
}

impl SyncEnv for RealEnv {
    fn find_window(&mut self) -> Result<GameWindowInfo, LoadoutSyncError> {
        crate::loadout_sync::window::find_game_window()
    }

    fn window_is_foreground(&mut self, win: &GameWindowInfo) -> bool {
        crate::loadout_sync::window::is_foreground(win.hwnd)
    }

    fn capture(
        &mut self,
        win: &GameWindowInfo,
        want_color: bool,
    ) -> Result<CapturedFrame, LoadoutSyncError> {
        crate::loadout_sync::capture::capture_client_area(win, want_color || self.debug_overlay)
    }

    fn input(&mut self) -> &mut dyn SyncInput {
        &mut self.input
    }

    fn now_ms(&mut self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn sleep_ms(&mut self, ms: u64) {
        std::thread::sleep(Duration::from_millis(ms));
    }

    fn save_debug(&mut self, name: &str, frame: &CapturedFrame, note: &str) {
        let Some(dir) = &self.debug_dir else { return };
        let path = dir.join(format!("{name}.png"));
        let _ = frame.save_png(&path);
        crate::util::log_to_file(
            "loadout_sync.log",
            &format!("debug screenshot: {} ({note})", path.display()),
        );
    }
}
