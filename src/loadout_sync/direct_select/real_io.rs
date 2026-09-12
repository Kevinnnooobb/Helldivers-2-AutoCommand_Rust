//! 把 `direct_select` 状态机接到真实游戏环境
//! （参考实现 `AutomationSession` 的对应物）。
//!
//! ## 职责划分
//!
//! * [`SyncEnv`]：既有项目的窗口/截图/输入/时钟抽象（已存在，不改）；
//! * [`GameUIRecognizer`]：既有项目的逐帧几何识别（不改）；
//! * [`CatalogClassifier`]：目录键控分类（本迁移新增的适配层）；
//! * 本模块：把上面三者包成 [`DirectSelectIo`]，**只做适配，不做判定**。
//!
//! ## 安全约束（plan6 §2）
//!
//! 1. 未确认目标槽位不得点击 —— 由状态机保证（见 `machine.rs`）；
//! 2. 未确认 hover 不得点击 —— 同上；
//! 3. 所有点击都发生在 `move_cursor` 之后，且只点一次（状态机持有 `click` 的调用权）；
//! 4. 帧坐标 → 屏幕坐标必须走 `CapturedFrame::origin`（与既有 controller 同一转换），
//!    **不允许**自乘 scale 或裸坐标；
//! 5. 本模块自身不重试、不吞错误：失败一律向上返回，由状态机决定有界重试。
use crate::loadout_sync::capture::CapturedFrame;
use crate::loadout_sync::controller::SyncEnv;
use crate::loadout_sync::direct_select::classify::CatalogClassifier;
use crate::loadout_sync::direct_select::frame::image_fingerprint;
use crate::loadout_sync::direct_select::machine::DirectSelectIo;
use crate::loadout_sync::recognizer::{Expect, GameUIRecognizer};
use crate::loadout_sync::reference_vision::{
    ItemKind, ObservedSlot, RoiObservation, SlotKind, UiState as RefUiState,
};
use crate::loadout_sync::types::{GameWindowInfo, ImageRect, ScreenPoint, UiState};

/// 真实环境的 `DirectSelectIo` 实现。
pub struct RealDirectSelectIo<'a> {
    env: &'a mut dyn SyncEnv,
    window: GameWindowInfo,
    recognizer: GameUIRecognizer,
    classifier: CatalogClassifier,
    /// 最近一帧（供 `capture_rgba` 复用，避免重复截图）
    last_frame: Option<CapturedFrame>,
    /// 最近一次观察到的参考层界面状态（诊断与拒答原因用）
    last_ui: Option<RefUiState>,
    cancel: Option<&'a std::sync::atomic::AtomicBool>,
}

impl<'a> RealDirectSelectIo<'a> {
    /// 用既有标定构造（`GameUIRecognizer::new`）。
    pub fn with_calibration(
        env: &'a mut dyn SyncEnv,
        window: GameWindowInfo,
        calibration: crate::loadout_sync::types::Calibration,
        classifier: CatalogClassifier,
    ) -> Self {
        Self {
            env,
            window,
            recognizer: GameUIRecognizer::new(calibration),
            classifier,
            last_frame: None,
            last_ui: None,
            cancel: None,
        }
    }

    /// 接上取消标志（来自 `LoadoutSyncShared`）。
    pub fn with_cancel(mut self, flag: &'a std::sync::atomic::AtomicBool) -> Self {
        self.cancel = Some(flag);
        self
    }

    /// 最近一次观察到的参考层界面状态。
    pub fn observed_state(&self) -> Option<RefUiState> {
        self.last_ui
    }

    /// 最近一次抓到的帧（失败截图用）。
    pub fn last_frame(&self) -> Option<&CapturedFrame> {
        self.last_frame.as_ref()
    }

    /// 抓一帧并缓存。
    fn grab(&mut self) -> Result<CapturedFrame, String> {
        let frame = self
            .env
            .capture(&self.window, true)
            .map_err(|e| format!("截图失败: {}", e.message()))?;
        self.last_frame = Some(frame.clone());
        Ok(frame)
    }

    /// 帧坐标 → 屏幕坐标（与既有 controller 相同：加 `frame.origin`）。
    fn to_screen(&self, point: (i32, i32)) -> Option<ScreenPoint> {
        let origin = self.last_frame.as_ref()?.origin;
        let rect = ImageRect::new(point.0, point.1, 1, 1).to_screen(origin);
        Some(ScreenPoint {
            x: rect.x,
            y: rect.y,
        })
    }

    /// 识别当前界面，并把槽位分类成 `ObservedSlot`。
    fn observe_inner(&mut self) -> Result<RoiObservation, String> {
        let frame = self.grab()?;
        let recognition = self.recognizer.recognize(&frame, Expect::Any);
        let roi_height = recognition
            .geom
            .map(|g| g.roi.h as f32)
            .unwrap_or(frame.height() as f32);

        match recognition.state {
            UiState::LoadoutHome => {
                let slots = recognition
                    .slots
                    .ok_or_else(|| "识别为 Home 但没有槽位几何".to_string())?;
                let mut out: Vec<ObservedSlot> = Vec::new();
                for (i, region) in slots.stratagems.iter().enumerate() {
                    out.push(self.classify_slot(
                        &frame,
                        region.rect,
                        i as u32,
                        0,
                        SlotKind::HomeStratagem,
                        ItemKind::Stratagem,
                    ));
                }
                out.push(self.classify_slot(
                    &frame,
                    slots.booster.rect,
                    0,
                    crate::loadout_sync::direct_select::home_activation::BOOSTER_COL,
                    SlotKind::HomeBooster,
                    ItemKind::Booster,
                ));
                // Home 还分「有空槽」与「已填满」：这决定下一步该不该找空槽。
                // 用 `ObservedSlot::is_empty()`（= 既有识别器的空槽判据）判定，
                // 不用「认不出」代替「空」。
                self.last_ui = Some(if out.iter().any(|s| s.is_empty()) {
                    RefUiState::HomeEmpty
                } else {
                    RefUiState::HomeFilled
                });
                Ok(self.finish_observation(&frame, roi_height, out))
            }
            UiState::StratagemList | UiState::BoosterList => {
                // 战备列表与 Booster 列表是两个不同的列表：格子类型必须跟着走。
                // 用 `ListStratagem` 表示 Booster 列表会让
                // `SlotKind::is_selectable_item_for(Booster)` 全部为假 ——
                // Booster 将永远找不到目标（曾是这样）。
                let (item_kind, slot_kind) = match recognition.state {
                    UiState::BoosterList => (ItemKind::Booster, SlotKind::ListBooster),
                    _ => (ItemKind::Stratagem, SlotKind::ListStratagem),
                };
                let grid = recognition
                    .grid
                    .ok_or_else(|| "识别为列表但没有网格".to_string())?;
                let mut out: Vec<ObservedSlot> = Vec::new();
                for cell in &grid.cells {
                    out.push(self.classify_slot(
                        &frame, cell.rect, cell.row, cell.col, slot_kind, item_kind,
                    ));
                }
                self.last_ui = Some(match item_kind {
                    ItemKind::Booster => RefUiState::BoosterList,
                    ItemKind::Stratagem => RefUiState::StratagemList,
                });
                Ok(self.finish_observation(&frame, roi_height, out))
            }
            other => Err(format!("当前界面无法用于列表导航: {other:?}")),
        }
    }

    /// 分类单个槽位。
    fn classify_slot(
        &self,
        frame: &CapturedFrame,
        rect: ImageRect,
        row: u32,
        col: u32,
        kind: SlotKind,
        item_kind: ItemKind,
    ) -> ObservedSlot {
        // 空槽判据的唯一权威是既有识别器；本迁移不改它的阈值。
        // 列表里的格子必然有内容，不做这个判据（逐格 `cell_is_empty` 也毫无意义）。
        let occupied = kind.is_list() || !self.recognizer.cell_is_empty(frame, rect);
        // Home 与 List 使用不同的资源渲染比例，必须区分
        let classification = if kind.is_list() {
            self.classifier.classify_cell(frame, rect, item_kind)
        } else {
            self.classifier.classify_cell_home(frame, rect, item_kind)
        };
        ObservedSlot {
            row,
            col,
            rect,
            kind,
            occupied,
            classification,
        }
    }

    fn finish_observation(
        &self,
        frame: &CapturedFrame,
        roi_height: f32,
        slots: Vec<ObservedSlot>,
    ) -> RoiObservation {
        let fingerprint = image_fingerprint(&frame.rgba);
        RoiObservation::new(slots, roi_height, fingerprint)
    }
}

impl DirectSelectIo for RealDirectSelectIo<'_> {
    fn observe(&mut self) -> Result<RoiObservation, String> {
        self.observe_inner()
    }

    fn capture_rgba(&mut self) -> Result<image::RgbaImage, String> {
        // Pixel evidence must describe the state after the preceding action.
        // Reusing the cached observation frame would hide hover and selection
        // transitions from the verifier.
        self.grab()?;
        self.last_frame
            .as_ref()
            .map(|f| f.rgba.clone())
            .ok_or_else(|| "没有可用帧".to_string())
    }

    fn move_cursor(&mut self, point: (i32, i32)) -> Result<(), String> {
        let screen = self
            .to_screen(point)
            .ok_or_else(|| format!("帧坐标 {point:?} 无法映射到屏幕坐标"))?;
        self.env
            .input()
            .move_to(screen)
            .map_err(|e| format!("移动光标失败: {}", e.message()))
    }

    fn click(&mut self) -> Result<(), String> {
        self.env
            .input()
            .click_left()
            .map_err(|e| format!("点击失败: {}", e.message()))
    }

    fn scroll(&mut self, delta: i32) -> Result<(), String> {
        self.env
            .input()
            .scroll(delta)
            .map_err(|e| format!("滚动失败: {}", e.message()))
    }

    fn now_ms(&mut self) -> u64 {
        self.env.now_ms()
    }

    fn is_home_settled(&mut self) -> Result<bool, String> {
        let frame = self.grab()?;
        Ok(matches!(
            self.recognizer.recognize(&frame, Expect::Any).state,
            UiState::LoadoutHome
        ))
    }

    fn cancelled(&mut self) -> bool {
        self.cancel
            .map(|c| c.load(std::sync::atomic::Ordering::Relaxed))
            .unwrap_or(false)
    }
}

impl crate::loadout_sync::direct_select::session::SessionIo for RealDirectSelectIo<'_> {
    fn click_at(&mut self, point: (i32, i32)) -> Result<(), String> {
        // 先移动再点击：绝不点击「上一次」的位置
        self.move_cursor(point)?;
        self.click()
    }

    fn list_is_open(&mut self, kind: ItemKind) -> Result<bool, String> {
        let frame = self.grab()?;
        let state = self.recognizer.recognize(&frame, Expect::Any).state;
        Ok(match kind {
            ItemKind::Stratagem => matches!(state, UiState::StratagemList),
            ItemKind::Booster => matches!(state, UiState::BoosterList),
        })
    }

    fn ui_state(&mut self) -> Option<RefUiState> {
        self.observed_state()
    }
}
