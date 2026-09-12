//! 参考实现的 Home 激活
//! （`hd2-preset-helper-0.1.4/src/loadout/direct_select/home_activation.rs`）。
//!
//! ## 职责
//!
//! 从 Home 界面进入目标列表：
//!
//! 1. 在 Home 上找到**要填的空槽**（战备）或 **Booster 六边形**；
//! 2. 点击它的中心；
//! 3. 等待目标列表 UI 出现（有超时）。
//!
//! ## 迁移要点
//!
//! 参考实现把「点哪里」与「点完之后等什么」分开：
//! `wait_for_home_booster_target` 先等一个**稳定的 Home 布局**，
//! 再从该布局取 Booster 槽中心；`open_slot_list` 才真正点击。
//!
//! 这样做的意义：**点击坐标来自当前帧实测几何**，而不是标定先验。
//! 标定先验会有系统性偏差（实机实测约 −6px 横向），
//! 直接拿来点击会点偏到相邻格子 —— 那是最危险的错误（会装错战备）。
//!
//! 本模块是纯逻辑：点击与截图由 trait 注入。
use crate::loadout_sync::direct_select::click_plan::SlotRef;
use crate::loadout_sync::reference_vision::{ItemKind, SlotKind};

/// 列表打开的超时（参考实现 `LIST_OPEN_TIMEOUT = 1500ms`）。
pub const LIST_OPEN_TIMEOUT_MS: u64 = 1500;
/// 点击按住时长（参考实现 `CLICK_HOLD_MS`）。
pub const CLICK_HOLD_MS: u64 = 40;

/// 要打开的列表目标：类型 + **当前帧实测**的点击点。
#[derive(Debug, Clone, PartialEq)]
pub struct HomeOpenTarget {
    pub item_kind: ItemKind,
    /// 点击点（屏幕坐标，由调用方从帧坐标换算）
    pub point: (i32, i32),
}

/// 从 Home 槽位里挑出「该类型第一个可填的槽」。
///
/// 类型来自调用方给出的 [`SlotKind`]（不依赖列号约定，因此对布局变化更稳健），
/// 战备按 `row`/`col` 顺序取第一个，Booster 只认 Booster 槽。
/// 找不到时返回 `None` —— 调用方必须停止，**不得**回退到标定先验坐标。
pub fn find_home_slot_kinded(slots: &[(SlotRef, SlotKind)], kind: ItemKind) -> Option<&SlotRef> {
    let mut candidates: Vec<&(SlotRef, SlotKind)> = slots
        .iter()
        .filter(|(s, k)| s.selectable && k.is_selectable_item_for(kind))
        .collect();
    candidates.sort_by_key(|(s, _)| (s.row, s.col));
    candidates
        .into_iter()
        .find(|(s, _)| !s.occupied)
        .map(|(s, _)| s)
}

/// Home 上 Booster 槽的列号约定（战备 0..=3，Booster 4）。
///
/// 仅用于把 Home 槽位标记成 `ObservedSlot`（`real_io`）与断言，
/// 类型判定本身一律走 [`find_home_slot_kinded`] 的显式 `SlotKind`。
pub const BOOSTER_COL: u32 = 4;

/// 一次点击 + 等待的结论。
#[derive(Debug, Clone, PartialEq)]
pub enum ActivationOutcome {
    /// 列表已打开
    ListOpened,
    /// 超时仍未看到目标列表
    Timeout { elapsed_ms: u64 },
    /// 无法评估（几何不可用）
    NotEvaluable { detail: String },
}

impl ActivationOutcome {
    pub fn label(&self) -> &'static str {
        match self {
            Self::ListOpened => "list_opened",
            Self::Timeout { .. } => "timeout",
            Self::NotEvaluable { .. } => "not_evaluable",
        }
    }
}

/// Home 激活所需的 I/O 注入。
pub trait HomeDriver {
    /// 点击一个点（屏幕坐标），按住 `hold_ms`。
    fn click(&mut self, point: (i32, i32), hold_ms: u64) -> Result<(), String>;
    /// 当前是否已显示目标类型的列表。
    fn list_is_open(&mut self, kind: ItemKind) -> Result<bool, String>;
    /// 当前时间（毫秒）。
    fn now_ms(&mut self) -> u64;
    /// 轮询间隔。
    fn poll_ms(&mut self) -> u64 {
        POLL_MS
    }
    /// 是否被取消。
    fn cancelled(&mut self) -> bool {
        false
    }
}

/// 轮询间隔。
pub const POLL_MS: u64 = 16;

/// 打开目标列表：点击 + 有界等待。
///
/// 点击点**必须**来自当前帧实测几何（调用方传入），不得用标定先验。
pub fn open_slot_list(
    driver: &mut dyn HomeDriver,
    target: HomeOpenTarget,
) -> Result<ActivationOutcome, String> {
    driver.click(target.point, CLICK_HOLD_MS)?;
    let start = driver.now_ms();
    let kind = target.item_kind;
    loop {
        if driver.cancelled() {
            return Ok(ActivationOutcome::NotEvaluable {
                detail: "已取消".to_string(),
            });
        }
        match driver.list_is_open(kind) {
            Ok(true) => return Ok(ActivationOutcome::ListOpened),
            Ok(false) => {}
            Err(detail) => {
                return Ok(ActivationOutcome::NotEvaluable { detail });
            }
        }
        let elapsed = driver.now_ms().saturating_sub(start);
        if elapsed >= LIST_OPEN_TIMEOUT_MS {
            return Ok(ActivationOutcome::Timeout {
                elapsed_ms: elapsed,
            });
        }
        std::thread::sleep(std::time::Duration::from_millis(driver.poll_ms()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loadout_sync::reference_vision::Classification;
    use crate::loadout_sync::types::ImageRect;

    fn home_slot(col: u32, x: i32, occupied: bool) -> SlotRef {
        SlotRef {
            row: 0,
            col,
            rect: ImageRect::new(x, 600, 80, 80),
            selectable: true,
            occupied,
            classification: occupied.then(|| Classification::new("machine-gun", 0.9, 0.2, 1.0)),
        }
    }

    /// 四个战备槽（0..3）+ Booster 槽（col=4）。
    ///
    /// 返回 `(SlotRef, SlotKind)`，与生产路径（`real_io` 构造 `ObservedSlot`
    /// 后经 `slots_of` 转换）是同一形状。
    fn home_layout(occupied: [bool; 4]) -> Vec<(SlotRef, SlotKind)> {
        let mut v: Vec<(SlotRef, SlotKind)> = occupied
            .iter()
            .enumerate()
            .map(|(i, occ)| {
                (
                    home_slot(i as u32, 10 + i as i32 * 100, *occ),
                    SlotKind::HomeStratagem,
                )
            })
            .collect();
        v.push((
            home_slot(BOOSTER_COL, 10 + 4 * 100, false),
            SlotKind::HomeBooster,
        ));
        v
    }

    /// 测试辅助：等价于生产路径的「找空槽 → 取中心」。
    fn stratagem_target(slots: &[(SlotRef, SlotKind)]) -> Option<(ItemKind, (i32, i32))> {
        find_home_slot_kinded(slots, ItemKind::Stratagem).map(|s| (ItemKind::Stratagem, s.center()))
    }

    /// 测试辅助：等价于生产路径的「找空的 Booster 槽 → 取中心」。
    fn booster_target(slots: &[(SlotRef, SlotKind)]) -> Option<(ItemKind, (i32, i32))> {
        find_home_slot_kinded(slots, ItemKind::Booster).map(|s| (ItemKind::Booster, s.center()))
    }

    #[test]
    fn picks_first_empty_stratagem_slot() {
        let slots = home_layout([true, true, false, false]);
        let t = stratagem_target(&slots).expect("应找到空槽");
        assert_eq!(t.0, ItemKind::Stratagem);
        // 第 3 个槽（col=2）→ 中心 x = 10+200+40 = 250
        assert_eq!(t.1, (250, 640));
    }

    #[test]
    fn booster_slot_is_never_used_for_stratagems() {
        // 战备全满，只剩 Booster 槽空 → 战备目标应为 None（不得占用 Booster 槽）
        let slots = home_layout([true, true, true, true]);
        assert!(stratagem_target(&slots).is_none());
        // Booster 目标可以找到
        let b = booster_target(&slots).expect("Booster 槽应可用");
        assert_eq!(b.0, ItemKind::Booster);
    }

    #[test]
    fn kinded_lookup_does_not_depend_on_column_convention() {
        // 显式给出 SlotKind：即使布局把 Booster 放在 col 0 也能正确区分
        let strat = SlotRef {
            row: 0,
            col: 0,
            rect: ImageRect::new(0, 0, 80, 80),
            selectable: true,
            occupied: false,
            classification: None,
        };
        let boost = SlotRef {
            row: 0,
            col: 1,
            rect: ImageRect::new(200, 0, 80, 80),
            selectable: true,
            occupied: false,
            classification: None,
        };
        let pairs = vec![
            (strat, SlotKind::HomeStratagem),
            (boost, SlotKind::HomeBooster),
        ];
        let s = find_home_slot_kinded(&pairs, ItemKind::Stratagem).expect("战备槽");
        assert_eq!(s.rect.x, 0);
        let b = find_home_slot_kinded(&pairs, ItemKind::Booster).expect("Booster 槽");
        assert_eq!(b.rect.x, 200);
    }

    #[test]
    fn no_empty_slot_means_no_target() {
        let slots = home_layout([true, true, true, true]);
        assert!(stratagem_target(&slots).is_none());
    }

    #[test]
    fn occupied_slots_are_never_clicked() {
        // 第一个槽已占用 → 必须跳到下一个空槽，而不是点已占用的槽
        let slots = home_layout([true, false, false, false]);
        let t = stratagem_target(&slots).expect("应找到空槽");
        assert_eq!(t.1 .0, 10 + 100 + 40, "应选第二个槽");
    }

    #[test]
    fn occupied_but_unrecognized_slot_is_not_treated_as_empty() {
        // Mod 替换图标 / 分割失败：有内容但分类为 None。
        // 这种槽位不得被当作空槽（否则会点开已填槽位并覆盖）。
        let unknown = SlotRef {
            row: 0,
            col: 0,
            rect: ImageRect::new(0, 600, 80, 80),
            selectable: true,
            occupied: true,
            classification: None,
        };
        let empty = SlotRef {
            row: 0,
            col: 1,
            rect: ImageRect::new(100, 600, 80, 80),
            selectable: true,
            occupied: false,
            classification: None,
        };
        let unknown_pair = (unknown.clone(), SlotKind::HomeStratagem);
        assert!(find_home_slot_kinded(&[unknown_pair.clone()], ItemKind::Stratagem).is_none());
        let pairs = vec![unknown_pair, (empty, SlotKind::HomeStratagem)];
        let picked = find_home_slot_kinded(&pairs, ItemKind::Stratagem).expect("应选到真正的空槽");
        assert_eq!(picked.center().0, 100 + 40);
        // 有内容但认不出的槽位也不得被当成 Booster 槽
        let only_unknown = vec![(unknown, SlotKind::HomeStratagem)];
        assert!(find_home_slot_kinded(&only_unknown, ItemKind::Booster).is_none());
    }

    // ── 驱动测试 ──

    struct ScriptedHome {
        open_after: u32,
        ticks: u32,
        now: u64,
        pub clicks: Vec<((i32, i32), u64)>,
    }

    impl HomeDriver for ScriptedHome {
        fn click(&mut self, point: (i32, i32), hold_ms: u64) -> Result<(), String> {
            self.clicks.push((point, hold_ms));
            Ok(())
        }
        fn list_is_open(&mut self, _kind: ItemKind) -> Result<bool, String> {
            self.ticks += 1;
            Ok(self.ticks > self.open_after)
        }
        fn now_ms(&mut self) -> u64 {
            self.now += 20;
            self.now
        }
        fn poll_ms(&mut self) -> u64 {
            0
        }
    }

    #[test]
    fn open_slot_list_clicks_once_then_confirms() {
        let mut d = ScriptedHome {
            open_after: 2,
            ticks: 0,
            now: 0,
            clicks: Vec::new(),
        };
        let out = open_slot_list(
            &mut d,
            HomeOpenTarget {
                item_kind: ItemKind::Stratagem,
                point: (250, 640),
            },
        )
        .expect("执行");
        assert_eq!(out.label(), "list_opened", "实际 {out:?}");
        // 必须只点一次 —— 重复点击是最危险的错误输入
        assert_eq!(d.clicks.len(), 1);
        assert_eq!(d.clicks[0], ((250, 640), CLICK_HOLD_MS));
    }

    #[test]
    fn open_slot_list_times_out_without_retrying_clicks() {
        let mut d = ScriptedHome {
            open_after: u32::MAX,
            ticks: 0,
            now: 0,
            clicks: Vec::new(),
        };
        let out = open_slot_list(
            &mut d,
            HomeOpenTarget {
                item_kind: ItemKind::Booster,
                point: (500, 640),
            },
        )
        .expect("执行");
        match out {
            ActivationOutcome::Timeout { elapsed_ms } => {
                assert!(elapsed_ms >= LIST_OPEN_TIMEOUT_MS)
            }
            other => panic!("期望 Timeout，实际 {other:?}"),
        }
        assert_eq!(d.clicks.len(), 1, "超时也不得重复点击");
    }

    #[test]
    fn open_slot_list_reports_io_failure_instead_of_pretending_success() {
        struct Failing;
        impl HomeDriver for Failing {
            fn click(&mut self, _p: (i32, i32), _h: u64) -> Result<(), String> {
                Err("输入注入失败".to_string())
            }
            fn list_is_open(&mut self, _k: ItemKind) -> Result<bool, String> {
                Ok(false)
            }
            fn now_ms(&mut self) -> u64 {
                0
            }
        }
        let err = open_slot_list(
            &mut Failing,
            HomeOpenTarget {
                item_kind: ItemKind::Stratagem,
                point: (0, 0),
            },
        )
        .expect_err("输入失败必须向上报错");
        assert!(err.contains("输入注入失败"));
    }
}
