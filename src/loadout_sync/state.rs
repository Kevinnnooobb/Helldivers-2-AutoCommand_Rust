// Loadout Sync 运行状态 —— GUI 线程与工作线程之间唯一的共享状态。
//
// 设计：工作线程只写、GUI 只读；取消用 AtomicBool；日志经 channel 回传，
// 避免工作线程触碰 egui/AppModel（GUI 主线程绝不阻塞）。
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::loadout_sync::error::LoadoutSyncError;

pub const TOTAL_STEPS: usize = 5;

#[derive(Debug, Clone, PartialEq, Default)]
pub enum SyncStatus {
    #[default]
    Idle,
    Running,
    Succeeded,
    Failed {
        code: String,
        message: String,
    },
    Cancelled,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SyncSnapshot {
    pub status: SyncStatus,
    /// 当前状态机状态名（日志/调试用）
    pub stage: &'static str,
    /// 第几个目标（1..=5）
    pub step: usize,
    pub total: usize,
    pub target: String,
    pub detail: String,
}

impl Default for SyncSnapshot {
    fn default() -> Self {
        Self {
            status: SyncStatus::Idle,
            stage: "Idle",
            step: 0,
            total: TOTAL_STEPS,
            target: String::new(),
            detail: String::new(),
        }
    }
}

impl SyncSnapshot {
    pub fn is_running(&self) -> bool {
        matches!(self.status, SyncStatus::Running)
    }

    /// GUI 顶栏显示文本。
    pub fn label(&self) -> String {
        match &self.status {
            SyncStatus::Idle => "IDLE".to_string(),
            SyncStatus::Running => {
                if self.target.is_empty() {
                    format!("SYNCING {}/{}", self.step.max(1), self.total)
                } else {
                    format!(
                        "SYNCING {}/{} · {}",
                        self.step.max(1),
                        self.total,
                        self.target
                    )
                }
            }
            SyncStatus::Succeeded => "SYNC COMPLETE".to_string(),
            SyncStatus::Failed { .. } => "SYNC FAILED".to_string(),
            SyncStatus::Cancelled => "SYNC CANCELLED".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncLogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone)]
pub enum SyncEvent {
    Log(SyncLogLevel, String),
    Finished(SyncStatus),
}

/// 工作线程与 GUI 共享的状态。
#[derive(Debug)]
pub struct LoadoutSyncShared {
    snapshot: Mutex<SyncSnapshot>,
    /// 取消标志。用 `Arc` 持有是为了能给 `direct_select` 的 IO 一份**同一对象**的
    /// 句柄（`begin()` 的就地复位对它同样生效）。
    cancel: Arc<AtomicBool>,
    finished: AtomicBool,
}

impl LoadoutSyncShared {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            snapshot: Mutex::new(SyncSnapshot::default()),
            cancel: Arc::new(AtomicBool::new(false)),
            finished: AtomicBool::new(true),
        })
    }

    /// 取消标志的共享句柄（同一个 `AtomicBool`，不是副本）。
    pub fn cancel_handle(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    pub fn snapshot(&self) -> SyncSnapshot {
        self.snapshot.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn with<R>(&self, f: impl FnOnce(&mut SyncSnapshot) -> R) -> R {
        let mut guard = self.snapshot.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut guard)
    }

    pub fn begin(&self) {
        self.cancel.store(false, Ordering::SeqCst);
        self.finished.store(false, Ordering::SeqCst);
        self.with(|s| {
            *s = SyncSnapshot {
                status: SyncStatus::Running,
                stage: "Preparing",
                step: 0,
                total: TOTAL_STEPS,
                target: String::new(),
                detail: String::new(),
            };
        });
    }

    pub fn set_stage(&self, stage: &'static str, step: usize, target: impl Into<String>) {
        let target = target.into();
        self.with(|s| {
            s.stage = stage;
            if step > 0 {
                s.step = step;
            }
            s.target = target;
        });
    }

    pub fn set_detail(&self, detail: impl Into<String>) {
        let detail = detail.into();
        self.with(|s| s.detail = detail);
    }

    pub fn finish(&self, status: SyncStatus) {
        self.finished.store(true, Ordering::SeqCst);
        self.with(|s| {
            s.status = status;
            s.target.clear();
        });
    }

    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::SeqCst)
    }

    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    pub fn reset(&self) {
        self.cancel.store(false, Ordering::SeqCst);
        self.finished.store(true, Ordering::SeqCst);
        self.with(|s| *s = SyncSnapshot::default());
    }
}

/// GUI 侧持有的句柄：启动/取消/读取状态/消费日志。
#[derive(Default)]
pub struct LoadoutSyncHandle {
    shared: Option<Arc<LoadoutSyncShared>>,
    events: Option<Receiver<SyncEvent>>,
    worker: Option<JoinHandle<()>>,
}

impl LoadoutSyncHandle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn shared(&self) -> Arc<LoadoutSyncShared> {
        match &self.shared {
            Some(s) => s.clone(),
            None => {
                // 尚未启动过：返回一个独立的空状态（GUI 首帧读取用）
                LoadoutSyncShared::new()
            }
        }
    }

    pub fn snapshot(&self) -> SyncSnapshot {
        self.shared
            .as_ref()
            .map(|s| s.snapshot())
            .unwrap_or_default()
    }

    pub fn is_running(&self) -> bool {
        self.snapshot().is_running()
    }

    /// 启动工作线程；已在运行时返回 false（调用方应转为取消语义）。
    pub fn start<F>(&mut self, job: F) -> bool
    where
        F: FnOnce(Arc<LoadoutSyncShared>, Sender<SyncEvent>) + Send + 'static,
    {
        if self.is_running() {
            return false;
        }
        self.reap();
        let shared = LoadoutSyncShared::new();
        shared.begin();
        let (tx, rx) = mpsc::channel();
        let shared_for_thread = shared.clone();
        let handle = std::thread::Builder::new()
            .name("loadout-sync".into())
            .spawn(move || job(shared_for_thread, tx))
            .ok();
        self.shared = Some(shared);
        self.events = Some(rx);
        self.worker = handle;
        true
    }

    pub fn request_cancel(&self) {
        if let Some(s) = &self.shared {
            s.request_cancel();
        }
    }

    /// 取出所有未消费的事件（GUI 每帧调用一次）。
    pub fn drain_events(&mut self) -> Vec<SyncEvent> {
        let mut out = Vec::new();
        if let Some(rx) = &self.events {
            while let Ok(e) = rx.try_recv() {
                out.push(e);
            }
        }
        out
    }

    /// 回收已结束的工作线程（避免句柄泄漏）。
    pub fn reap(&mut self) {
        let finished = self
            .shared
            .as_ref()
            .map(|s| s.is_finished())
            .unwrap_or(true);
        if finished {
            if let Some(handle) = self.worker.take() {
                let _ = handle.join();
            }
        }
    }
}

/// 统一日志文本格式：[LoadoutSync] ...
pub fn log_line(msg: &str) -> String {
    format!("[LoadoutSync] {msg}")
}

/// 错误 → (日志文本, 状态) 的统一转换。
pub fn status_from_error(err: &LoadoutSyncError) -> SyncStatus {
    if err.is_cancelled() {
        SyncStatus::Cancelled
    } else {
        SyncStatus::Failed {
            code: err.code().to_string(),
            message: err.message(),
        }
    }
}
