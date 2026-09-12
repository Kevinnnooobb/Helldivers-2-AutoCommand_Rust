// plan7 偏离说明（已登记到 reference-pins.json 的 deviations）：
// 本文件以参考实现 `hd2-preset-helper-0.1.4/src/automation.rs` 为基准，
// 仅**追加**取消钩子（`cancel` 字段 / `with_cancel` / `ensure_running` 与各方法开头的
// 一次检查），未改动任何既有签名与语义。原因：参考实现自身没有取消概念，而 H2AC 有
// 全局取消热键；参考状态机的所有循环（翻页 / hover / 点击确认 / 等待 Home）都以
// `capture()` 为步进，因此在捕获与输入前各检查一次即可让取消在一步之内生效，
// 且不必改动任何判定逻辑。
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, bail};
use image::RgbaImage;

use crate::capture::CaptureRegion;
use crate::input::{InputSession, Key};
use crate::window::WindowTarget;

pub struct AutomationSession<'a> {
    region: CaptureRegion<'a>,
    input: InputSession,
    /// plan7 追加：外部取消标志（`None` = 不检查，与参考行为完全一致）。
    cancel: Option<Arc<AtomicBool>>,
}

impl<'a> AutomationSession<'a> {
    pub fn new(region: CaptureRegion<'a>, target: WindowTarget) -> Result<Self> {
        Self::with_cancel(region, target, None)
    }

    /// plan7 追加：带取消标志构造。
    pub fn with_cancel(
        region: CaptureRegion<'a>,
        target: WindowTarget,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Result<Self> {
        Ok(Self {
            region,
            input: InputSession::new(target)?,
            cancel,
        })
    }

    /// plan7 追加：取消检查。命中即 `bail!("automation cancelled")`，
    /// 由 H2AC `controller` 归类为 `LoadoutSyncError::Cancelled`。
    fn ensure_running(&self) -> Result<()> {
        if self
            .cancel
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::SeqCst))
        {
            bail!("automation cancelled");
        }
        Ok(())
    }

    pub fn capture(&mut self) -> Result<RgbaImage> {
        self.ensure_running()?;
        self.region.capture()
    }

    pub fn move_cursor(&mut self, point: (u32, u32)) -> Result<()> {
        self.ensure_running()?;
        self.input.move_cursor(self.region.map_to_client(point))
    }

    pub fn click(&mut self, point: (u32, u32), hold_ms: u64) -> Result<()> {
        self.ensure_running()?;
        self.input.click(self.region.map_to_client(point), hold_ms)
    }

    pub fn click_current(&mut self, hold_ms: u64) -> Result<()> {
        self.ensure_running()?;
        self.input.click_current(hold_ms)
    }

    pub fn scroll(&mut self, delta: i32) -> Result<()> {
        self.ensure_running()?;
        self.input.scroll(delta)
    }

    pub fn tap_key(&mut self, key: Key, hold_ms: u64) -> Result<()> {
        self.ensure_running()?;
        self.input.tap_key(key, hold_ms)
    }
}
