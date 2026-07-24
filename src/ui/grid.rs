use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, Pos2, Rect, Sense, Stroke, Ui, Vec2,
};
use crate::config;
use crate::theme::*;
use crate::widgets::*;
use crate::H2ACApp;
use crate::LogKind;
use crate::ui::detail::render_detail;

pub fn render_left_column(app: &mut H2ACApp, ui: &mut Ui, rect: Rect, m: &UiMetrics) {
    let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 28.0));
    let mut h = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(header)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    h.horizontal(|ui| {
        ui.label(egui::RichText::new("战备配置").font(m.hud_b(16.0)).color(TEXT));
        ui.label(egui::RichText::new("LOADOUT").font(m.hud(11.0)).color(TEXT_DIM));
        let hint = if app.model.armed.is_some() {
            "待命装填中 — 点击右侧战备库装入，ESC 取消"
        } else {
            "点选槽位待命 · 双击执行 · 右键快捷操作"
        };
        ui.label(egui::RichText::new(hint).font(m.hud(11.0)).color(if app.model.armed.is_some() { GOLD } else { TEXT_DIM }));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if hud_button(ui, "清空全部", Vec2::new(76.0, 24.0), m, DANGER, true).clicked() {
                for i in 0..config::SLOT_COUNT {
                    app.clear_slot(i);
                }
                app.model.armed = None;
                app.log(LogKind::Warn, "全部槽位已清空");
            }
        });
    });

    let grid_top = header.bottom() + 8.0;
    let tw = m.tile_w();
    let th = m.tile_h();
    let tg = m.tile_gap();
    let grid_rect = Rect::from_min_size(
        Pos2::new(rect.left(), grid_top),
        Vec2::new(tw * 5.0 + tg * 4.0, th * 2.0 + tg),
    );
    render_grid(app, ui, grid_rect, m);

    let detail = Rect::from_min_max(
        Pos2::new(rect.left(), grid_rect.bottom() + 8.0),
        rect.max,
    );
    render_detail(app, ui, detail, m);
}

pub fn render_grid(app: &mut H2ACApp, ui: &mut Ui, rect: Rect, m: &UiMetrics) {
    let tw = m.tile_w();
    let th = m.tile_h();
    let tg = m.tile_gap();
    for idx in 0..config::SLOT_COUNT {
        let row = idx / 5;
        let col = idx % 5;
        let tile = Rect::from_min_size(
            Pos2::new(
                rect.left() + col as f32 * (tw + tg),
                rect.top() + row as f32 * (th + tg),
            ),
            Vec2::new(tw, th),
        );
        render_slot_tile(app, ui, tile, idx, m);
    }
}

pub fn render_slot_tile(app: &mut H2ACApp, ui: &mut Ui, rect: Rect, idx: usize, m: &UiMetrics) {
    let resp = ui.interact(rect, ui.id().with(("slot", idx)), Sense::click());
    let p = ui.painter_at(rect);
    let filled = app.slot_filled(idx);
    let armed = app.model.armed == Some(idx);
    let hovered = resp.hovered();

    let bg = if filled { BG_RAISED } else { BG_PANEL };
    let border = if armed {
        Stroke::new(1.5, GOLD)
    } else if hovered {
        Stroke::new(1.0, GOLD_DIM)
    } else {
        Stroke::new(1.0, LINE)
    };
    let c = m.chamfer();
    paint_chamfer(&p, rect, c, bg, border);
    if armed {
        corner_brackets(&p, rect.shrink(3.0), 8.0, GOLD);
        p.add(egui::Shape::convex_polygon(
            chamfer_points(rect.shrink(1.5), c),
            GOLD_GHOST,
            Stroke::NONE,
        ));
    }

    p.text(
        Pos2::new(rect.left() + m.slot_tile_num_x(), rect.top() + m.slot_tile_num_y()),
        Align2::LEFT_TOP,
        format!("{:02}", idx + 1),
        m.hud_b(10.0),
        TEXT_DIM,
    );

    let now = ui.ctx().input(|i| i.time);
    if let Some(&t0) = app.model.flash.get(&idx) {
        let k = (now - t0) as f32 / 0.7;
        if k < 1.0 {
            let a = ((1.0 - k) * 140.0) as u8;
            p.add(egui::Shape::convex_polygon(
                chamfer_points(rect, c),
                Color32::from_rgba_unmultiplied(0xF5, 0xC8, 0x42, a / 3),
                Stroke::NONE,
            ));
            corner_brackets(&p, rect.shrink(3.0), 8.0, Color32::from_rgba_unmultiplied(0xF5, 0xC8, 0x42, a));
            ui.ctx().request_repaint();
        }
    }

    if filled {
        let name = app.slot_name(idx).unwrap_or_default();
        let icon_key = app.slot_icon(idx).unwrap_or("");
        let cmd = app.slot_command(idx);
        let eff_cat = app.effective_category(&name, &app.slot_category(idx).unwrap_or_default());
        let accent = category_color(&eff_cat);

        let icon_rect = Rect::from_center_size(
            Pos2::new(rect.center().x, rect.top() + m.slot_icon_y()),
            Vec2::splat(m.slot_icon_size()),
        );
        if let Some(tex) = app.model.icons.get(icon_key) {
            p.image(
                tex.id(),
                icon_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }

        let font = m.fit_font(&p, &name, rect.width() - 14.0, &[13.0, 11.5, 10.0], false);
        p.text(
            Pos2::new(rect.center().x, rect.top() + m.slot_name_y()),
            Align2::CENTER_TOP,
            &name,
            font,
            TEXT,
        );

        let arrow_size = m.slot_arrow_size();
        let arrow_gap = m.slot_arrow_gap();
        let aw = arrow_strip_w(&cmd, arrow_size, arrow_gap);
        arrow_strip(
            &p,
            Pos2::new(rect.center().x - aw / 2.0, rect.top() + m.slot_arrow_y()),
            &cmd,
            arrow_size,
            arrow_gap,
            GOLD_MID,
        );

        let strip = Rect::from_min_max(
            Pos2::new(rect.left() + 10.0, rect.bottom() - m.slot_cat_bar_y_offset()),
            Pos2::new(rect.right() - 10.0, rect.bottom() - m.slot_cat_bar_y_offset() + m.slot_cat_bar_h()),
        );
        p.rect_filled(strip, CornerRadius::ZERO, accent);
    } else {
        p.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "EMPTY",
            m.hud(12.0),
            TEXT_DIM,
        );
    }

    if let Some(hk) = app.model.config.slot_hotkeys.get(&idx.to_string()) {
        let badge = Rect::from_min_size(
            Pos2::new(rect.right() - m.slot_hotkey_badge_x_offset(), rect.top() + m.slot_hotkey_badge_y_offset()),
            Vec2::new(m.slot_hotkey_badge_w(), m.slot_hotkey_badge_h()),
        );
        paint_chamfer(&p, badge, 3.0, BG_DEEP, Stroke::new(1.0, OK.gamma_multiply(0.6)));
        let hf = m.fit_font(&p, &hk.to_uppercase(), 17.0, &[9.5, 8.0], true);
        p.text(badge.center(), Align2::CENTER_CENTER, hk.to_uppercase(), hf, OK);
    }

    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if resp.clicked() {
        app.model.detail_slot = Some(idx);
        app.model.armed = if app.model.armed == Some(idx) { None } else { Some(idx) };
        app.context = None;
    }
    if resp.double_clicked() && filled {
        app.execute_slot(idx);
    }
    if resp.secondary_clicked() {
        app.context = Some(crate::ContextState { slot: idx, pos: rect.min });
        app.model.armed = None;
    }
}
