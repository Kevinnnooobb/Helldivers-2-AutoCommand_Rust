//! 确定性回放（plan6 §6.2）：用脚本化帧序列驱动完整状态机。
//!
//! ## 为什么需要它
//!
//! 真实游戏不可用时，唯一能回归「完整装配状态机」的手段是
//! **把每一帧的识别结果脚本化**，然后跑真实的
//! [`crate::loadout_sync::direct_select::machine::DirectSelectMachine`]。
//!
//! 关键点：这里**不复制**状态机逻辑，只替换 IO。因此回放通过就意味着
//! 状态推进、硬上限、失败分支都通过了；它**不能**替代真实游戏验收
//! （纹理、动画、输入时序都不在覆盖范围内）—— 报告里必须这样标注。
use std::collections::VecDeque;

use image::{Rgba, RgbaImage};

use crate::loadout_sync::direct_select::machine::{DirectSelectIo, DirectSelectOutcome};
use crate::loadout_sync::reference_vision::{
    Classification, ItemKind, ObservedSlot, RoiObservation, SlotKind,
};
use crate::loadout_sync::types::ImageRect;

/// 一帧的脚本化内容。
#[derive(Debug, Clone)]
pub struct ScriptedFrame {
    pub slots: Vec<ObservedSlot>,
    /// 槽位边框亮度（与该帧 `slots` 一一对应）
    pub border: Vec<u8>,
}

impl ScriptedFrame {
    /// 构造一帧：给定 (row, col, item_id 或 None)。
    pub fn new(entries: &[(u32, u32, Option<&str>)]) -> Self {
        let slots = entries
            .iter()
            .map(|(row, col, id)| ObservedSlot {
                row: *row,
                col: *col,
                rect: ImageRect::new(*col as i32 * 100, *row as i32 * 100, 90, 90),
                kind: SlotKind::ListStratagem,
                occupied: true,
                classification: id.map(|i| Classification::new(i, 0.90, 0.20, 1.0)),
            })
            .collect();
        let border = vec![120u8; entries.len()];
        Self { slots, border }
    }

    /// 指定某个槽位的边框亮度（模拟 hover 高亮 / 选中后变暗）。
    pub fn with_border(mut self, index: usize, value: u8) -> Self {
        if index < self.border.len() {
            self.border[index] = value;
        }
        self
    }

    fn to_observation(&self, fingerprint_tag: u8) -> RoiObservation {
        RoiObservation::new(self.slots.clone(), 832.0, vec![fingerprint_tag; 8])
    }

    fn to_image(&self) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(1024, 1024, Rgba([10, 10, 10, 255]));
        for (slot, v) in self.slots.iter().zip(self.border.iter()) {
            let r = slot.rect;
            let (w, h) = (img.width() as i32, img.height() as i32);
            let mut put = |x: i32, y: i32| {
                if x >= 0 && y >= 0 && x < w && y < h {
                    img.put_pixel(x as u32, y as u32, Rgba([*v, *v, *v, 255]));
                }
            };
            for x in r.x..=r.right() {
                put(x, r.y);
                put(x, r.bottom());
            }
            for y in r.y..=r.bottom() {
                put(r.x, y);
                put(r.right(), y);
            }
        }
        img
    }
}

/// 脚本化 IO：按顺序播放帧，并记录所有发出的输入。
///
/// 默认**不模拟点击后的视觉变化**，因此点击后必然观察到 `Unchanged`
/// —— 这正是要回归的「安全失败」路径。
/// 需要回归成功路径时用 [`ReplayIo::with_click_effect`] 显式打开。
pub struct ReplayIo {
    /// 帧队列；耗尽后重复最后一帧
    frames: VecDeque<ScriptedFrame>,
    last: Option<ScriptedFrame>,
    /// 每次 observe 返回的指纹标签（用于区分不同页面）
    fingerprints: VecDeque<u8>,
    last_fingerprint: u8,
    pub moves: Vec<(i32, i32)>,
    pub clicks: u32,
    pub scrolls: Vec<i32>,
    pub home_after_final_click: bool,
    last_move: Option<(i32, i32)>,
    now: u64,
    /// 脚本耗尽后是否允许继续（否则 observe 返回 Err 触发 IO 失败）
    repeat_last: bool,
    /// 每帧重复播放的次数（>1 时模拟「光标稳定停在某个槽位上」的多帧）
    repeat_each: usize,
    repeat_left: usize,
    /// 点击后是否把「光标所在槽位」的边框调暗（模拟游戏撤掉 hover 高亮）
    click_effect: bool,
    /// 点击后是否切换指纹（模拟视口变化）
    click_changes_fingerprint: bool,
    /// 是否按**光标位置**动态高亮边框（与真实游戏一致）
    cursor_highlight: bool,
    /// 点击后要调暗的槽位索引
    dimmed_after_click: Option<usize>,
    /// 会话级场景（Home ↔ List 自动切换）
    scene: Option<Scene>,
}

impl ReplayIo {
    pub fn new(frames: Vec<ScriptedFrame>) -> Self {
        Self {
            frames: frames.into(),
            last: None,
            fingerprints: VecDeque::new(),
            last_fingerprint: 0,
            moves: Vec::new(),
            clicks: 0,
            scrolls: Vec::new(),
            home_after_final_click: false,
            last_move: None,
            now: 0,
            repeat_last: true,
            repeat_each: 1,
            repeat_left: 0,
            click_effect: false,
            click_changes_fingerprint: false,
            cursor_highlight: false,
            dimmed_after_click: None,
            scene: None,
        }
    }

    /// 打开「点击后光标所在槽位变暗」的模拟（选中成功的视觉证据）。
    pub fn with_click_effect(mut self) -> Self {
        self.click_effect = true;
        self
    }

    /// 打开「点击后视口位移」的模拟（选中成功的另一种视觉证据）。
    /// 指定每一帧的指纹标签（用于让「视口移动」可判定）。
    pub fn with_fingerprints(mut self, tags: Vec<u8>) -> Self {
        self.fingerprints = tags.into();
        self
    }

    /// 每帧重复播放 `n` 次：用于让 hover 有足够稳定帧确认。
    /// 仅测试与诊断使用。
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn with_repeat_each(mut self, n: usize) -> Self {
        self.repeat_each = n.max(1);
        self
    }

    /// 按**光标位置**动态高亮边框（与真实游戏一致）。
    ///
    /// 这是让回放可信的关键：真实游戏里**只有光标停驻的那个槽位**会画出
    /// hover 高亮，其余槽位是暗基线。用静态亮度脚本无法表达这一点，
    /// 于是「同一页有多个目标」时 hover 判据永远无法对第二个目标成立。
    pub fn with_cursor_highlight(mut self) -> Self {
        self.cursor_highlight = true;
        self
    }

    /// 找到「帧内包含该点」的槽位索引。
    fn slot_at(&self, point: (i32, i32)) -> Option<usize> {
        let f = self.last.as_ref()?;
        f.slots.iter().position(|s| {
            let r = s.rect;
            point.0 >= r.x && point.0 <= r.right() && point.1 >= r.y && point.1 <= r.bottom()
        })
    }

    fn next_frame(&mut self) -> Option<ScriptedFrame> {
        if self.repeat_left > 0 {
            self.repeat_left -= 1;
            return self.last.clone();
        }
        if let Some(scene) = self.scene.as_mut() {
            // Home 帧**不在这里前进**：Home 状态的推进由一次「选中已确认」触发
            // （见 `is_home_settled`），否则同一次选择会被推进两格。
            let popped = match scene.mode {
                SceneMode::Home => scene.home.front().cloned(),
                SceneMode::List => scene.list.pop_front(),
            };
            if let Some(f) = popped {
                self.repeat_left = self.repeat_each.saturating_sub(1);
                self.last = Some(f.clone());
                return Some(f);
            }
            return self.last.clone();
        }
        if let Some(f) = self.frames.pop_front() {
            self.repeat_left = self.repeat_each.saturating_sub(1);
            self.last = Some(f.clone());
            if let Some(t) = self.fingerprints.pop_front() {
                self.last_fingerprint = t;
            }
            return Some(f);
        }
        if self.repeat_last {
            return self.last.clone();
        }
        None
    }
}

/// 场景模式：Home 与列表两套帧脚本，按点击自动切换（会话级回放用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneMode {
    Home,
    List,
}

/// 会话级场景脚本。
///
/// * `home`：依次经过的 Home 状态（每次选完一个目标前进一格）；
/// * `pending_lists`：每次「从 Home 点开列表」时取用的一段列表脚本
///   （战备列表与 Booster 列表内容不同，必须分别给）。
struct Scene {
    mode: SceneMode,
    home: VecDeque<ScriptedFrame>,
    list: VecDeque<ScriptedFrame>,
    pending_lists: VecDeque<Vec<ScriptedFrame>>,
}

impl ReplayIo {
    /// 打开会话级场景：Home 帧序列 + 每次进入列表时依次取用的列表脚本。
    ///
    /// 与 [`ReplayIo::new`] 的单队列模式互斥；场景模式下帧来源由 `mode` 决定。
    pub fn with_scene(mut self, home: Vec<ScriptedFrame>, lists: Vec<Vec<ScriptedFrame>>) -> Self {
        self.scene = Some(Scene {
            mode: SceneMode::Home,
            home: home.into(),
            list: VecDeque::new(),
            pending_lists: lists.into_iter().collect(),
        });
        self.last = self.scene.as_ref().and_then(|s| s.home.front().cloned());
        self
    }

    /// 当前场景模式（非场景回放返回 `None`）。
    pub fn scene_mode(&self) -> Option<SceneMode> {
        self.scene.as_ref().map(|s| s.mode)
    }
}

/// 会话级回放：把 `SessionIo` 也实现出来，从而能跑完整装配会话。
impl crate::loadout_sync::direct_select::session::SessionIo for ReplayIo {
    fn click_at(&mut self, point: (i32, i32)) -> Result<(), String> {
        // 与真实 IO 一致：先移动再点击
        self.move_cursor(point)?;
        self.clicks += 1;
        self.now += 5;
        // Home → 点开列表：取下一段列表脚本
        if let Some(scene) = self.scene.as_mut() {
            if scene.mode == SceneMode::Home {
                scene.mode = SceneMode::List;
                let next = scene.pending_lists.pop_front().unwrap_or_default();
                scene.list = next.into();
                self.last = scene.list.front().cloned();
                // 新开的列表是一次全新的渲染：上一次点击留下的变暗标记必须清掉，
                // 否则它会永久压暗新列表的同一个槽位，让 hover 永远无法确认。
                self.dimmed_after_click = None;
            }
        }
        Ok(())
    }

    fn list_is_open(&mut self, kind: ItemKind) -> Result<bool, String> {
        let Some(scene) = self.scene.as_ref() else {
            return Ok(false);
        };
        if scene.mode != SceneMode::List {
            return Ok(false);
        }
        let want = SlotKind::list_kind_for(kind);
        Ok(self
            .last
            .as_ref()
            .is_some_and(|f| f.slots.iter().any(|s| s.kind == want)))
    }

    fn poll_ms(&mut self) -> u64 {
        // 确定性回放不真实等待：时间由 `now` 虚拟推进
        0
    }
}

impl DirectSelectIo for ReplayIo {
    fn observe(&mut self) -> Result<RoiObservation, String> {
        self.now += 10;
        let Some(f) = self.next_frame() else {
            return Err("脚本帧耗尽".to_string());
        };
        Ok(f.to_observation(self.last_fingerprint))
    }

    fn capture_rgba(&mut self) -> Result<RgbaImage, String> {
        let Some(f) = self.last.clone() else {
            return Err("没有可用帧".to_string());
        };
        if !self.cursor_highlight {
            return Ok(f.to_image());
        }
        // 与真实游戏一致：光标所在槽位高亮（220），其余为暗基线（120）；
        // 已点击过的那个槽位变暗（80）—— 游戏在选中后撤掉 hover 高亮。
        let hovered = self.last_move.and_then(|p| self.slot_at(p));
        let mut frame = f;
        for (i, b) in frame.border.iter_mut().enumerate() {
            *b = if Some(i) == hovered { 220 } else { 120 };
        }
        if let Some(i) = self.dimmed_after_click {
            if i < frame.border.len() {
                frame.border[i] = 80;
            }
        }
        Ok(frame.to_image())
    }

    fn move_cursor(&mut self, point: (i32, i32)) -> Result<(), String> {
        self.moves.push(point);
        self.last_move = Some(point);
        self.now += 5;
        Ok(())
    }

    fn click(&mut self) -> Result<(), String> {
        self.clicks += 1;
        self.now += 5;
        if self.click_effect {
            // 记录「被选中的那个槽位」，之后它的边框会变暗
            self.dimmed_after_click = self.last_move.and_then(|p| self.slot_at(p));
        }
        if self.click_changes_fingerprint {
            self.last_fingerprint = self.last_fingerprint.wrapping_add(1);
        }
        Ok(())
    }

    fn scroll(&mut self, delta: i32) -> Result<(), String> {
        self.scrolls.push(delta);
        self.now += 5;
        Ok(())
    }

    fn now_ms(&mut self) -> u64 {
        self.now
    }

    fn is_home_settled(&mut self) -> Result<bool, String> {
        // 回放时钟必须在这里也前进，否则状态机的超时判定永远不成立
        self.now += 50;
        // 场景模式：进入列表后，一次「选中已确认」才意味着游戏关掉列表回到 Home，
        // 于是这里同时完成「列表 → Home」的切换与 Home 状态前进一格。
        if let Some(scene) = self.scene.as_mut() {
            if scene.mode == SceneMode::List {
                scene.mode = SceneMode::Home;
                if scene.home.len() > 1 {
                    scene.home.pop_front();
                }
                self.last = scene.home.front().cloned();
            }
            return Ok(scene.mode == SceneMode::Home);
        }
        // 空目标列表时（clicks == 0）Home 本就应该是稳定的
        let ready = self.clicks > 0 || self.last.is_some();
        Ok(ready && self.home_after_final_click)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loadout_sync::direct_select::machine::{
        DirectSelectFailure, DirectSelectMachine, DirectSelectPhase,
    };

    /// 一页 4 行 × 1 列，只放一个目标。
    fn page(rows: &[Option<&str>]) -> ScriptedFrame {
        let entries: Vec<(u32, u32, Option<&str>)> = rows
            .iter()
            .enumerate()
            .map(|(i, id)| (i as u32, 0, *id))
            .collect();
        ScriptedFrame::new(&entries)
    }

    #[test]
    fn happy_path_selects_every_visible_target() {
        // 四个目标全部在同一页可见。
        // 只有**光标停驻**的那个槽位会亮（模拟游戏画出的 hover 边框），
        // 其余槽位是暗基线 —— 这正是 hover 判据依赖的结构。
        let f = page(&[
            Some("machine-gun"),
            Some("autocannon"),
            Some("stalwart"),
            Some("railgun"),
        ]);
        let mut io = ReplayIo::new(vec![f])
            .with_click_effect()
            .with_cursor_highlight();
        io.home_after_final_click = true;
        let targets = vec![
            "machine-gun".to_string(),
            "autocannon".to_string(),
            "stalwart".to_string(),
            "railgun".to_string(),
        ];
        let mut m = DirectSelectMachine::new(targets, ItemKind::Stratagem);
        let out = m.run(&mut io);

        match &out {
            DirectSelectOutcome::Succeeded { selected } => assert_eq!(selected.len(), 4),
            other => panic!("期望成功，实际 {other:?}（轨迹 {:?}）", m.trace().phases),
        }
        // 每个目标一次点击，绝无重复点击
        assert_eq!(io.clicks, 4, "应恰好点击 4 次");
        assert_eq!(m.trace().clicks, 4);
        assert!(!m.trace().phases.contains(&DirectSelectPhase::Failed));
        // 光标必须移到过槽位中心
        assert_eq!(io.moves.len(), 4);
    }

    #[test]
    fn target_absent_from_every_page_fails_safely_within_the_wheel_cap() {
        // 只有一页内容，目标不在其中 → 必须先翻页，翻不动后安全失败
        let f = page(&[Some("machine-gun"), Some("autocannon")]);
        let mut io = ReplayIo::new(vec![f]).with_fingerprints(vec![1]);
        let targets = vec!["nonexistent".to_string()];
        let mut m = DirectSelectMachine::new(targets, ItemKind::Stratagem);
        let out = m.run(&mut io);

        assert!(!out.is_success(), "找不到目标绝不能报成功");
        match out.failure() {
            Some(DirectSelectFailure::TargetNotFound { .. }) => {}
            other => panic!("期望 TargetNotFound，实际 {other:?}"),
        }
        assert_eq!(io.clicks, 0, "未确认目标时绝不允许点击");
        assert!(m.trace().wheel_inputs <= 20, "滚轮输入必须受硬上限约束");
    }

    #[test]
    fn unconfirmed_hover_never_clicks() {
        // 目标可见，但所有槽位边框都一样亮 → 无法建立 hover 证据
        let f = page(&[Some("machine-gun"), Some("autocannon")]);
        let f = f.with_border(0, 120).with_border(1, 120);
        let mut io = ReplayIo::new(vec![f]);
        let targets = vec!["machine-gun".to_string()];
        let mut m = DirectSelectMachine::new(targets, ItemKind::Stratagem);
        let out = m.run(&mut io);

        assert!(!out.is_success());
        assert_eq!(io.clicks, 0, "hover 未确认时绝不允许点击");
        match out.failure() {
            Some(DirectSelectFailure::HoverNotConfirmed { .. }) => {}
            other => panic!("期望 HoverNotConfirmed，实际 {other:?}"),
        }
    }

    #[test]
    fn unselected_after_click_retries_then_fails_within_the_cap() {
        // 目标可见、hover 能确认，但点击后亮度不变 → 视口没动 + 没变暗
        let f = page(&[Some("machine-gun"), Some("autocannon")]);
        let f = f.with_border(0, 220).with_border(1, 120);
        let mut io = ReplayIo::new(vec![f])
            .with_fingerprints(vec![1])
            .with_click_effect()
            .with_cursor_highlight();
        io.home_after_final_click = false;
        let targets = vec!["machine-gun".to_string()];
        let mut m = DirectSelectMachine::new(targets, ItemKind::Stratagem);
        let out = m.run(&mut io);

        assert!(!out.is_success(), "未观察到选中绝不能报成功");
        // 点击次数必须受 MAX_TARGET_CLICK_ATTEMPTS 约束
        assert!(io.clicks <= 3, "点击次数 {} 超过硬上限 3", io.clicks);
    }

    #[test]
    fn page_turn_advances_the_map_and_finds_a_target_on_a_later_page() {
        // 第一页不含目标；第二页才有。
        // 状态机在进入循环前会先观察一帧，因此这里需要足够多的帧
        // 让「翻页 → 新页 → hover → 点击」都能拿到正确的帧。
        let first = page(&[Some("stalwart"), Some("railgun")]);
        let second = page(&[Some("machine-gun"), Some("autocannon")]);
        let mut io = ReplayIo::new(vec![first, second])
            .with_fingerprints(vec![1, 2])
            .with_click_effect()
            .with_cursor_highlight();
        io.home_after_final_click = true;
        let targets = vec!["machine-gun".to_string()];
        let mut m = DirectSelectMachine::new(targets, ItemKind::Stratagem);
        let out = m.run(&mut io);

        match &out {
            DirectSelectOutcome::Succeeded { selected } => {
                assert_eq!(selected, &["machine-gun".to_string()])
            }
            other => panic!("期望成功，实际 {other:?}（轨迹 {:?}）", m.trace().phases),
        }
        assert_eq!(io.clicks, 1);
        assert_eq!(io.clicks, 1);
    }
    #[test]
    fn no_success_is_reported_when_home_never_settles() {
        // 至少两个槽位，否则无法建立 hover baseline（参考实现同样要求）
        let f = page(&[Some("machine-gun"), Some("autocannon")]);
        let mut io = ReplayIo::new(vec![f])
            .with_click_effect()
            .with_cursor_highlight();
        io.home_after_final_click = false; // 永远不回 Home
        let targets = vec!["machine-gun".to_string()];
        let mut m = DirectSelectMachine::new(targets, ItemKind::Stratagem);
        let out = m.run(&mut io);

        assert!(!out.is_success(), "Home 未确认时绝不能报成功");
        match out.failure() {
            Some(DirectSelectFailure::TerminalHomeNotReached { .. }) => {}
            // 也可能因为点击后状态没变而更早失败 —— 两者都是安全失败
            Some(DirectSelectFailure::SelectionUnchanged { .. }) => {}
            other => panic!("期望安全失败，实际 {other:?}"),
        }
    }

    #[test]
    fn empty_target_list_settles_home_and_succeeds() {
        let f = page(&[Some("machine-gun")]);
        let mut io = ReplayIo::new(vec![f])
            .with_click_effect()
            .with_cursor_highlight();
        io.home_after_final_click = true;
        let mut m = DirectSelectMachine::new(Vec::new(), ItemKind::Stratagem);
        let out = m.run(&mut io);
        assert!(out.is_success());
        assert_eq!(io.clicks, 0);
    }

    #[test]
    fn booster_targets_use_booster_slots_only() {
        // 战备槽 + Booster 槽；目标是 Booster
        let f = ScriptedFrame {
            slots: vec![
                ObservedSlot {
                    row: 0,
                    col: 0,
                    rect: ImageRect::new(0, 0, 90, 90),
                    kind: SlotKind::ListStratagem,
                    occupied: true,
                    classification: Some(Classification::new("machine-gun", 0.9, 0.2, 1.0)),
                },
                ObservedSlot {
                    row: 0,
                    col: 1,
                    rect: ImageRect::new(200, 0, 90, 90),
                    kind: SlotKind::ListBooster,
                    occupied: true,
                    classification: Some(Classification::new(
                        "experimental-infusion",
                        0.9,
                        0.2,
                        1.0,
                    )),
                },
            ],
            border: vec![120, 220],
        };
        let mut io = ReplayIo::new(vec![f])
            .with_click_effect()
            .with_cursor_highlight();
        io.home_after_final_click = true;
        let mut m =
            DirectSelectMachine::new(vec!["experimental-infusion".to_string()], ItemKind::Booster);
        let out = m.run(&mut io);
        match &out {
            DirectSelectOutcome::Succeeded { selected } => {
                assert_eq!(selected, &["experimental-infusion".to_string()])
            }
            other => panic!("期望成功，实际 {other:?}（轨迹 {:?}）", m.trace().phases),
        }
        // 必须点到 Booster 槽（col=1 → x 中心 245），不能点到战备槽
        assert_eq!(io.moves[0], (245, 45));
    }

    #[test]
    fn stratagem_targets_never_land_on_booster_slots() {
        let f = ScriptedFrame {
            slots: vec![ObservedSlot {
                row: 0,
                col: 0,
                rect: ImageRect::new(0, 0, 90, 90),
                kind: SlotKind::ListBooster,
                occupied: true,
                classification: Some(Classification::new("machine-gun", 0.9, 0.2, 1.0)),
            }],
            border: vec![220],
        };
        let mut io = ReplayIo::new(vec![f]);
        let mut m = DirectSelectMachine::new(vec!["machine-gun".to_string()], ItemKind::Stratagem);
        let out = m.run(&mut io);
        assert!(!out.is_success(), "战备不得落在 Booster 槽上");
        assert_eq!(io.clicks, 0);
    }

    // ─── 会话级回放（plan6 §6.2：HomeEmpty → 列表 → 选中 → HomeFilled） ───

    fn home_frame(filled: &[Option<&str>], booster: Option<&str>) -> ScriptedFrame {
        let mut slots: Vec<ObservedSlot> = filled
            .iter()
            .enumerate()
            .map(|(i, id)| ObservedSlot {
                row: 0,
                col: i as u32,
                rect: ImageRect::new(i as i32 * 100, 600, 90, 90),
                kind: SlotKind::HomeStratagem,
                occupied: id.is_some(),
                classification: id.map(|x| Classification::new(x, 0.90, 0.20, 1.0)),
            })
            .collect();
        slots.push(ObservedSlot {
            row: 0,
            col: crate::loadout_sync::direct_select::home_activation::BOOSTER_COL,
            rect: ImageRect::new(400, 600, 90, 90),
            kind: SlotKind::HomeBooster,
            occupied: booster.is_some(),
            classification: booster.map(|x| Classification::new(x, 0.90, 0.20, 1.0)),
        });
        let border = vec![120u8; slots.len()];
        ScriptedFrame { slots, border }
    }

    /// 列表帧：目标 + 一个陪衬项（hover 判据需要 ≥2 个槽位才有中位基线）。
    fn list_frame(ids: &[&str], kind: SlotKind) -> ScriptedFrame {
        let slots: Vec<ObservedSlot> = ids
            .iter()
            .enumerate()
            .map(|(i, id)| ObservedSlot {
                row: i as u32,
                col: 0,
                rect: ImageRect::new(0, i as i32 * 100, 90, 90),
                kind,
                occupied: true,
                classification: Some(Classification::new(*id, 0.90, 0.20, 1.0)),
            })
            .collect();
        let border = vec![120u8; slots.len()];
        ScriptedFrame { slots, border }
    }

    #[test]
    fn session_replay_assembles_four_stratagems_and_a_booster() {
        use crate::loadout_sync::direct_select::session::{self, SessionPlan};
        let home = vec![
            home_frame(&[None, None, None, None], None),
            home_frame(&[Some("quasar_cannon"), None, None, None], None),
            home_frame(
                &[Some("quasar_cannon"), Some("eagle_airstrike"), None, None],
                None,
            ),
            home_frame(
                &[
                    Some("quasar_cannon"),
                    Some("eagle_airstrike"),
                    Some("railgun"),
                    None,
                ],
                None,
            ),
            home_frame(
                &[
                    Some("quasar_cannon"),
                    Some("eagle_airstrike"),
                    Some("railgun"),
                    Some("autocannon"),
                ],
                None,
            ),
            home_frame(
                &[
                    Some("quasar_cannon"),
                    Some("eagle_airstrike"),
                    Some("railgun"),
                    Some("autocannon"),
                ],
                Some("supply_pack"),
            ),
        ];
        // 每次「点开槽位」对应一段列表脚本：四个战备列表 + 一个 Booster 列表
        let lists = vec![
            vec![list_frame(
                &["quasar_cannon", "or_bital_laser_placeholder"],
                SlotKind::ListStratagem,
            )],
            vec![list_frame(
                &["eagle_airstrike", "or_bital_laser_placeholder"],
                SlotKind::ListStratagem,
            )],
            vec![list_frame(
                &["railgun", "or_bital_laser_placeholder"],
                SlotKind::ListStratagem,
            )],
            vec![list_frame(
                &["autocannon", "or_bital_laser_placeholder"],
                SlotKind::ListStratagem,
            )],
            vec![list_frame(
                &["supply_pack", "experimental_infusion_placeholder"],
                SlotKind::ListBooster,
            )],
        ];
        let mut io = ReplayIo::new(Vec::new())
            .with_scene(home, lists)
            .with_cursor_highlight()
            .with_click_effect();
        let plan = SessionPlan {
            stratagems: vec![
                "quasar_cannon".to_string(),
                "eagle_airstrike".to_string(),
                "railgun".to_string(),
                "autocannon".to_string(),
            ],
            booster: Some("supply_pack".to_string()),
        };
        let mut logs: Vec<String> = Vec::new();
        let report = {
            let mut log = |l: &str| logs.push(l.to_string());
            session::run(&mut io, &plan, &mut log).expect("会话应成功")
        };
        assert_eq!(
            report.selected,
            vec![
                "quasar_cannon",
                "eagle_airstrike",
                "railgun",
                "autocannon",
                "supply_pack"
            ],
            "日志: {logs:?}"
        );
        assert!(report.is_success(), "报告: {report:?}");
        assert_eq!(report.expected, 5);
        assert_eq!(report.opened_lists, 5, "每个目标都要重新打开一个空槽");
        // 目标都在首页可见 → 不允许任何滚轮输入（无谓滚动即视为回归）
        assert_eq!(report.wheel_inputs, 0, "不应滚动");
        // 每次点击前都必须先移动光标（不能点「上一次」的位置）
        assert!(io.moves.len() >= 5, "移动次数 {}", io.moves.len());
        // 选完后光标最终停在槽位上，且会话报告回到 Home
        assert_eq!(io.scene_mode(), Some(SceneMode::Home));
    }

    #[test]
    fn session_replay_stops_safely_when_the_target_never_appears() {
        use crate::loadout_sync::direct_select::session::{self, SessionPlan};
        let home = vec![
            home_frame(&[None, None, None, None], None),
            home_frame(&[None, None, None, None], None),
        ];
        let lists = vec![vec![list_frame(
            &["railgun", "autocannon"],
            SlotKind::ListStratagem,
        )]];
        let mut io = ReplayIo::new(Vec::new())
            .with_scene(home, lists)
            .with_cursor_highlight()
            .with_click_effect();
        let plan = SessionPlan {
            stratagems: vec!["quasar_cannon".to_string()],
            booster: None,
        };
        let mut log = |_l: &str| {};
        let err = session::run(&mut io, &plan, &mut log).expect_err("目标不在列表里必须安全失败");
        assert!(err.contains("quasar_cannon"), "错误信息: {err}");
        assert_eq!(err.report.expected, 1);
        assert_eq!(err.report.opened_lists, 1);
        assert!(!err.report.failures.is_empty());
        // 安全底线：不得对不存在的目标点击
        assert_eq!(io.clicks, 1, "只允许「点开列表」那一次点击");
    }

    #[test]
    fn session_replay_never_opens_an_occupied_home_slot() {
        use crate::loadout_sync::direct_select::session::{self, SessionPlan};
        // 四个战备槽全满（且全部认不出 → classification 为 None）；
        // 仍然不得把已填槽位当空槽去点。
        let mut full = home_frame(&[None, None, None, None], None);
        for slot in full.slots.iter_mut().filter(|s| !s.kind.is_booster()) {
            slot.occupied = true;
            slot.classification = None;
        }
        let mut io = ReplayIo::new(Vec::new()).with_scene(vec![full], Vec::new());
        let plan = SessionPlan {
            stratagems: vec!["quasar_cannon".to_string()],
            booster: None,
        };
        let mut log = |_l: &str| {};
        let err = session::run(&mut io, &plan, &mut log).expect_err("没有空槽必须安全失败");
        assert!(err.contains("空槽"), "错误信息: {err}");
        assert_eq!(err.report.expected, 1);
        assert_eq!(err.report.opened_lists, 0);
        assert!(!err.report.failures.is_empty());
        assert_eq!(io.clicks, 0, "不得点击任何槽位");
    }
}
