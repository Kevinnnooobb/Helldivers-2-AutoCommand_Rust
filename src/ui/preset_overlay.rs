// 紧凑模式 = 游戏内配装预设 Overlay（取代原来的 554x56 迷你执行条）
//
// 布局：上排 TASK（Slot01~05，战斗中呼叫用，立即写盘）
//      下排 LOADOUT（Slot06~10 = 游戏 S1~S4 + Booster，装配成功后才写盘）
// 视觉语言沿用 H2AC 的 HUD 风格（切角面板 / 角括号 / 扫描线 / Saira 字体）。
//
// 该视图与主窗口共用同一个 eframe 窗口：自动化期间窗口 alpha=0 + 点击穿透，
// 因此这里必须继续渲染（事件循环与全局热键派发依赖它）。
use eframe::egui::{
    self, Align2, Color32, Context, CornerRadius, CursorIcon, Pos2, Rect, Sense, Stroke, Ui, Vec2,
};

use crate::compact_mode::preset::{FIRST_H2AC_SLOT, PRESET_SLOTS};
use crate::config::SLOT_COUNT;
use crate::loadout_sync::state::SyncStatus;
use crate::theme::*;
use crate::widgets::*;
use crate::H2ACApp;
use crate::LogKind;

const TITLE_H: f32 = 30.0;
const SECTION_H: f32 = 18.0;
const ROW_H: f32 = 38.0;
const ROW_GAP: f32 = 3.0;
const FOOTER_H: f32 = 46.0;
const PAD: f32 = 6.0;

/// 槽位在浮窗里的角色
#[derive(Clone, Copy, PartialEq)]
enum SlotRole {
    /// 上排 TASK：战斗中呼叫，立即持久化
    Task,
    /// 下排 LOADOUT：写入预设草稿，装配成功后才持久化
    Loadout(usize),
}

impl SlotRole {
    fn of(slot: usize) -> Self {
        match crate::compact_mode::preset::draft_index_of(slot) {
            Some(index) => Self::Loadout(index),
            None => Self::Task,
        }
    }
}

/// 下排槽位对应的游戏侧语义
fn loadout_tag(preset_index: usize) -> &'static str {
    match preset_index {
        0 => "S1",
        1 => "S2",
        2 => "S3",
        3 => "S4",
        _ => "BOOSTER",
    }
}

pub fn show_preset_overlay(app: &mut H2ACApp, ctx: &Context) {
    // 每帧校准窗口透明度/点击穿透（egui 的窗口指令是延迟生效的）
    app.ensure_overlay_window_state();
    let m = app.model.metrics;
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(BG_DEEP).inner_margin(0.0))
        .show(ctx, |ui| {
            let full = ui.available_rect_before_wrap();
            ui.advance_cursor_after_rect(full);
            scanlines(ui.painter(), full, &m);

            let panel = full.shrink(2.0);
            paint_chamfer(
                ui.painter(),
                panel,
                10.0,
                BG_PANEL,
                Stroke::new(1.0, GOLD_DIM),
            );
            corner_brackets(ui.painter(), panel.shrink(3.0), 8.0, GOLD_DIM);

            let title = Rect::from_min_size(panel.min, Vec2::new(panel.width(), TITLE_H));
            render_title_row(app, ui, title, &m);
            let footer = Rect::from_min_max(
                Pos2::new(panel.left(), panel.bottom() - FOOTER_H),
                panel.max,
            );
            let body = Rect::from_min_max(
                Pos2::new(panel.left(), title.bottom()),
                Pos2::new(panel.right(), footer.top()),
            );

            match app.compact_editor_slot() {
                Some(slot) => render_selector(app, ui, body, slot, &m),
                None => render_slot_list(app, ui, body, &m),
            }
            render_footer(app, ui, footer, &m);
        });

    // 拖动结束后记录位置，下次唤出保持同一位置
    if ctx.input(|i| i.pointer.any_released()) {
        app.save_overlay_position();
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        if app.compact_editor_slot().is_some() {
            app.cancel_compact_choice();
        } else {
            app.toggle_compact_overlay(ctx);
        }
    }
    // 槽位热键捕获弹窗（在主界面触发后切到浮窗时仍要可见）
    crate::ui::modals::render_capture_modal(app, ctx, &m);
}

fn render_title_row(app: &mut H2ACApp, ui: &mut Ui, rect: Rect, m: &UiMetrics) {
    let p = ui.painter();
    let drag = ui.interact(rect, ui.id().with("ov_drag"), Sense::drag());
    if drag.drag_started_by(egui::PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    p.hline(
        rect.left()..=rect.right(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, LINE),
    );
    p.add(egui::Shape::convex_polygon(
        chamfer_points(Rect::from_min_size(rect.min, Vec2::new(5.0, TITLE_H)), 3.0),
        GOLD,
        Stroke::NONE,
    ));
    p.text(
        Pos2::new(rect.left() + 14.0, rect.top() + 9.0),
        Align2::LEFT_TOP,
        "H2AC-RS",
        m.hud_b(14.0),
        GOLD,
    );
    let profile = if app.model.current_profile.is_empty() {
        "PRESET".to_string()
    } else {
        format!("PRESET · {}", app.model.current_profile)
    };
    p.text(
        Pos2::new(rect.left() + 88.0, rect.top() + 11.0),
        Align2::LEFT_TOP,
        profile,
        m.hud(11.0),
        TEXT_SUB,
    );

    let mut bar = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_max(
                Pos2::new(rect.right() - 150.0, rect.top() + 2.0),
                Pos2::new(rect.right() - 6.0, rect.bottom() - 2.0),
            ))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    bar.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if glyph_button(ui, Glyph::Minimize, 22.0, "隐藏浮窗（不退出程序）").clicked() {
            app.toggle_compact_overlay(ui.ctx());
        }
        if glyph_button(ui, Glyph::Restore, 22.0, "返回主界面").clicked() {
            app.save_overlay_position();
            app.model.view_mode = crate::state::ViewMode::Main;
            app.apply_view_mode(ui.ctx());
        }
        if hud_button(ui, "透明", Vec2::new(38.0, 22.0), m, TEXT_SUB, false).clicked() {
            app.cycle_overlay_opacity(ui.ctx());
        }
    });
}

/// 全部 10 个槽位：上排 TASK + 下排 LOADOUT
fn render_slot_list(app: &mut H2ACApp, ui: &mut Ui, body: Rect, m: &UiMetrics) {
    // 10 个槽位 + 2 个分组标题必须全部可见：行高按剩余高度反推（上限 ROW_H），
    // 避免固定行高把最后一格（Slot10 = Booster）挤出可视区而静默丢失。
    let header_total = 2.0 * (SECTION_H + 2.0);
    let avail =
        (body.height() - 2.0 * PAD - header_total - (SLOT_COUNT as f32 - 1.0) * ROW_GAP - 2.0)
            .max(0.0);
    let row_h = (avail / SLOT_COUNT as f32).min(ROW_H);

    let mut y = body.top() + PAD;
    y = render_section_header(
        ui,
        body,
        y,
        "TASK · 战斗中呼叫（Slot 01~05）",
        "由快捷键/主界面执行，不参与自动装配",
        m,
    );

    let hint_edit = "右键选择";
    for slot in 0..SLOT_COUNT {
        if slot == FIRST_H2AC_SLOT {
            y = render_section_header(
                ui,
                body,
                y,
                "LOADOUT · 自动装配（Slot 06~10）",
                "S1~S4 + Booster · 装配成功后保存",
                m,
            );
        }
        let row = Rect::from_min_size(
            Pos2::new(body.left() + PAD, y),
            Vec2::new(body.width() - PAD * 2.0, row_h),
        );
        if row.bottom() > body.bottom() - PAD {
            // 防御：正常尺寸下不会触发（行高已按可用高度反推）
            break;
        }
        render_slot_row(app, ui, row, slot, m, hint_edit);
        y = row.bottom() + ROW_GAP;
    }
}

fn render_section_header(
    ui: &mut Ui,
    body: Rect,
    y: f32,
    title: &str,
    hint: &str,
    m: &UiMetrics,
) -> f32 {
    let p = ui.painter();
    let rect = Rect::from_min_size(
        Pos2::new(body.left() + PAD, y),
        Vec2::new(body.width() - PAD * 2.0, SECTION_H),
    );
    p.text(
        Pos2::new(rect.left(), rect.top() + 2.0),
        Align2::LEFT_TOP,
        title,
        m.hud_b(10.0),
        GOLD_MID,
    );
    p.text(
        Pos2::new(rect.right(), rect.top() + 3.0),
        Align2::RIGHT_TOP,
        hint,
        m.hud(8.5),
        TEXT_DIM,
    );
    rect.bottom() + 2.0
}

fn render_slot_row(
    app: &mut H2ACApp,
    ui: &mut Ui,
    rect: Rect,
    slot: usize,
    m: &UiMetrics,
    hint: &str,
) {
    let resp = ui.interact(rect, ui.id().with(("ov_row", slot)), Sense::click());
    let p = ui.painter_at(rect);
    let role = SlotRole::of(slot);

    // 上排读普通槽位；下排读预设草稿（未保存的编辑立刻可见）
    let (name, icon, category, filled) = match role {
        SlotRole::Task => {
            let filled = app.slot_filled(slot);
            (
                app.slot_name(slot).unwrap_or_default().to_string(),
                app.slot_icon(slot).unwrap_or_default().to_string(),
                app.slot_category(slot).unwrap_or_default().to_string(),
                filled,
            )
        }
        SlotRole::Loadout(index) => app.model.compact_preset.draft.entries[index]
            .as_ref()
            .map(|e| {
                let cat = app
                    .plugins
                    .stratagems
                    .iter()
                    .find(|p| p.name == e.item.name)
                    .map(|p| p.category.clone())
                    .unwrap_or_default();
                (e.item.name.clone(), e.item.icon.clone(), cat, true)
            })
            .unwrap_or_default(),
    };

    let hovered = resp.hovered();
    let bg = if hovered {
        BG_HOVER
    } else if filled {
        BG_RAISED
    } else {
        BG_PANEL
    };
    let border = if hovered {
        Stroke::new(1.0, GOLD)
    } else if matches!(role, SlotRole::Loadout(_)) {
        Stroke::new(1.0, GOLD_DIM)
    } else {
        Stroke::new(1.0, LINE)
    };
    paint_chamfer(&p, rect, m.chamfer(), bg, border);

    // 槽位号
    p.text(
        Pos2::new(rect.left() + 7.0, rect.center().y),
        Align2::LEFT_CENTER,
        format!("{:02}", slot + 1),
        m.hud_b(14.0),
        if filled { GOLD } else { TEXT_DIM },
    );
    // 游戏侧语义（仅下排）
    if let SlotRole::Loadout(index) = role {
        p.text(
            Pos2::new(rect.left() + 32.0, rect.center().y),
            Align2::LEFT_CENTER,
            loadout_tag(index),
            m.hud(8.5),
            GOLD_MID,
        );
    }
    // 图标
    let icon_rect = Rect::from_center_size(
        Pos2::new(rect.left() + 68.0, rect.center().y),
        Vec2::splat(26.0),
    );
    if filled {
        if let Some(tex) = app.model.icons.get(icon.as_str()) {
            p.image(
                tex.id(),
                icon_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            p.text(
                icon_rect.center(),
                Align2::CENTER_CENTER,
                "?",
                m.hud(12.0),
                TEXT_DIM,
            );
        }
    } else {
        paint_chamfer(&p, icon_rect, 4.0, BG_DEEP, Stroke::new(1.0, LINE));
    }
    // 名称
    if filled {
        let font = m.fit_font(&p, &name, rect.width() - 150.0, &[12.5, 11.5, 10.0], false);
        p.text(
            Pos2::new(rect.left() + 86.0, rect.center().y),
            Align2::LEFT_CENTER,
            &name,
            font,
            TEXT,
        );
        let strip = Rect::from_min_max(
            Pos2::new(rect.right() - 4.0, rect.top() + 5.0),
            Pos2::new(rect.right() - 2.0, rect.bottom() - 5.0),
        );
        p.rect_filled(strip, CornerRadius::ZERO, category_color(&category));
    } else {
        p.text(
            Pos2::new(rect.left() + 86.0, rect.center().y),
            Align2::LEFT_CENTER,
            "未配置",
            m.hud(11.0),
            TEXT_DIM,
        );
    }
    // 悬停提示
    p.text(
        Pos2::new(rect.right() - 8.0, rect.center().y),
        Align2::RIGHT_CENTER,
        if hovered { hint } else { "" },
        m.hud(9.0),
        TEXT_DIM,
    );

    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if resp.clicked() || resp.secondary_clicked() {
        app.open_compact_selector(slot);
    }
}

fn render_footer(app: &mut H2ACApp, ui: &mut Ui, rect: Rect, m: &UiMetrics) {
    let p = ui.painter();
    p.hline(
        rect.left()..=rect.right(),
        rect.top() + 0.5,
        Stroke::new(1.0, LINE),
    );
    let running = app.model.loadout_sync.is_running();
    let hint = if running {
        format!(
            "自动装配进行中 · 浮窗只读 · 取消 {}",
            app.model.config.loadout_sync_cancel_hotkey.to_uppercase()
        )
    } else {
        format!(
            "右键选择 · 自动装配 {} · 取消 {}",
            app.model.config.loadout_sync_hotkey.to_uppercase(),
            app.model.config.loadout_sync_cancel_hotkey.to_uppercase()
        )
    };
    p.text(
        Pos2::new(rect.left() + 10.0, rect.top() + 5.0),
        Align2::LEFT_TOP,
        hint,
        m.hud(9.5),
        TEXT_DIM,
    );

    let snapshot = app.model.loadout_sync.snapshot();
    let (text, color) = match &snapshot.status {
        SyncStatus::Running => (
            format!(
                "SYNCING {}/{} · {}",
                snapshot.step.max(1),
                snapshot.total,
                snapshot.target
            ),
            GOLD,
        ),
        SyncStatus::Succeeded => ("SYNC COMPLETE — 已保存".to_string(), OK),
        SyncStatus::Failed { code, message } => (format!("{code}: {message}"), DANGER),
        SyncStatus::Cancelled => ("已取消（预设未保存）".to_string(), TEXT_SUB),
        SyncStatus::Idle => {
            let line = app.model.compact_preset.status_line();
            if line.is_empty() {
                (
                    format!("LOADOUT {}", app.model.compact_preset.draft.summary()),
                    TEXT_SUB,
                )
            } else {
                let color = match app.model.compact_preset.outcome {
                    crate::compact_mode::preset::OverlayOutcome::Success => OK,
                    crate::compact_mode::preset::OverlayOutcome::Failed(_) => DANGER,
                    _ => TEXT_SUB,
                };
                (line, color)
            }
        }
    };
    let font = m.fit_font(p, &text, rect.width() - 20.0, &[11.0, 10.0, 9.0], false);
    p.text(
        Pos2::new(rect.left() + 10.0, rect.top() + 22.0),
        Align2::LEFT_TOP,
        text,
        font,
        color,
    );
}

/// 选择器视图：复用现有战备库（分类 / 搜索 / 图标），不新建数据库。
fn render_selector(app: &mut H2ACApp, ui: &mut Ui, body: Rect, slot: usize, m: &UiMetrics) {
    let mut area = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(body.shrink2(Vec2::new(8.0, 6.0)))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    area.spacing_mut().item_spacing.y = 4.0;

    let role = SlotRole::of(slot);
    let title = match role {
        SlotRole::Task => format!("选择 Slot {:02} · TASK 战备", slot + 1),
        SlotRole::Loadout(index) => format!(
            "选择 Slot {:02} · {}",
            slot + 1,
            if loadout_tag(index) == "BOOSTER" {
                "Booster".to_string()
            } else {
                format!("游戏 Stratagem {}", index + 1)
            }
        ),
    };
    area.label(egui::RichText::new(title).font(m.hud_b(12.0)).color(GOLD));

    let mut search = app
        .model
        .compact_preset
        .editor
        .as_ref()
        .map(|e| e.search.clone())
        .unwrap_or_default();
    area.horizontal(|ui| {
        let resp = ui.add(
            egui::TextEdit::singleline(&mut search)
                .hint_text(
                    egui::RichText::new("搜索名称/型号")
                        .font(m.hud(11.0))
                        .color(TEXT_DIM),
                )
                .font(m.hud(11.0))
                .desired_width(150.0),
        );
        if resp.changed() {
            if let Some(editor) = app.model.compact_preset.editor.as_mut() {
                editor.search = search.clone();
            }
        }
        let current = app
            .model
            .compact_preset
            .editor
            .as_ref()
            .and_then(|e| e.category.clone());
        let label = current.clone().unwrap_or_else(|| "全部".into());
        egui::ComboBox::from_id_salt("ov_cat")
            .width(110.0)
            .selected_text(egui::RichText::new(label).font(m.hud(11.0)))
            .show_ui(ui, |ui| {
                let mut pick: Option<Option<String>> = None;
                if ui.selectable_label(current.is_none(), "全部").clicked() {
                    pick = Some(None);
                }
                for cat in app.lib_categories() {
                    let selected = current.as_deref() == Some(cat.as_str());
                    if ui.selectable_label(selected, cat.as_str()).clicked() {
                        pick = Some(Some(cat));
                    }
                }
                if let Some(next) = pick {
                    if let Some(editor) = app.model.compact_preset.editor.as_mut() {
                        editor.category = next;
                    }
                }
            });
    });

    let entries: Vec<crate::stratagems::StratagemRef<'_>> = {
        let search_text = app
            .model
            .compact_preset
            .editor
            .as_ref()
            .map(|e| e.search.clone())
            .unwrap_or_default();
        if !search_text.trim().is_empty() {
            app.lib_search(&search_text)
        } else {
            match app
                .model
                .compact_preset
                .editor
                .as_ref()
                .and_then(|e| e.category.clone())
            {
                Some(cat) => app.lib_by_category(&cat),
                None => {
                    let mut all = Vec::new();
                    for cat in app.library.categories.clone() {
                        all.extend(app.lib_by_category(&cat));
                    }
                    for p in &app.plugins.stratagems {
                        all.push(crate::stratagems::StratagemRef::Plugin(p));
                    }
                    all
                }
            }
        }
    };

    let list_h = (area.available_rect_before_wrap().height() - 30.0).max(60.0);
    let mut choice: Option<Option<crate::compact_mode::preset::PresetEntry>> = None;
    egui::ScrollArea::vertical()
        .max_height(list_h)
        .auto_shrink([false, false])
        .show(&mut area, |ui| {
            for entry in &entries {
                let (name, model_name, icon) = (
                    entry.name().to_string(),
                    entry.model().to_string(),
                    entry.icon().to_string(),
                );
                let (resp, painter) =
                    ui.allocate_painter(Vec2::new(ui.available_width(), 30.0), Sense::click());
                let rect = resp.rect;
                let hovered = resp.hovered();
                if hovered {
                    paint_chamfer(&painter, rect, 4.0, BG_HOVER, Stroke::NONE);
                }
                let icon_rect = Rect::from_center_size(
                    Pos2::new(rect.left() + 18.0, rect.center().y),
                    Vec2::splat(22.0),
                );
                if let Some(tex) = app.model.icons.get(icon.as_str()) {
                    painter.image(
                        tex.id(),
                        icon_rect,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                let font = m.fit_font(
                    &painter,
                    &name,
                    rect.width() - 60.0,
                    &[12.0, 11.0, 9.5],
                    false,
                );
                painter.text(
                    Pos2::new(rect.left() + 36.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    &name,
                    font,
                    TEXT,
                );
                if !model_name.is_empty() {
                    painter.text(
                        Pos2::new(rect.right() - 6.0, rect.center().y),
                        Align2::RIGHT_CENTER,
                        &model_name,
                        m.hud(9.0),
                        TEXT_DIM,
                    );
                }
                if hovered {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    choice = Some(Some(match entry {
                        crate::stratagems::StratagemRef::Base(s) => {
                            let idx = crate::stratagems::STRATAGEMS
                                .iter()
                                .position(|x| x.name == s.name && x.model == s.model)
                                .unwrap_or(0);
                            crate::compact_mode::preset::PresetEntry::from_base(idx)
                                .expect("内置战备索引")
                        }
                        crate::stratagems::StratagemRef::Plugin(p) => {
                            crate::compact_mode::preset::PresetEntry::from_plugin(p)
                        }
                    }));
                }
            }
        });

    area.horizontal(|ui| {
        if hud_button(ui, "清 除", Vec2::new(70.0, 24.0), m, DANGER, true).clicked() {
            choice = Some(None);
        }
        if hud_button(ui, "取 消", Vec2::new(70.0, 24.0), m, TEXT_SUB, false).clicked() {
            app.cancel_compact_choice();
        }
        if ui
            .add(egui::Label::new(
                egui::RichText::new("ESC 取消")
                    .font(m.hud(9.0))
                    .color(TEXT_DIM),
            ))
            .clicked()
        {
            app.cancel_compact_choice();
        }
    });

    if let Some(entry) = choice {
        app.apply_compact_choice(entry);
        app.log(LogKind::Info, "[Compact] 槽位已更新");
        let _ = PRESET_SLOTS;
    }
}
