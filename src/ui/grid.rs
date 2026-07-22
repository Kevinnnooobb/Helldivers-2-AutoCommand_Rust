use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, Pos2, Rect, Sense, Stroke, Ui, Vec2,
};
use crate::config;
use crate::theme::*;
use crate::widgets::*;
use crate::H2ACApp;
use crate::LogKind;
use crate::ui::common::{fit_font, TILE_GAP, TILE_H, TILE_W};
use crate::ui::detail::render_detail;

pub fn render_left_column(app: &mut H2ACApp, ui: &mut Ui, rect: Rect) {
    let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 28.0));
    let mut h = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(header)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    h.horizontal(|ui| {
        ui.label(egui::RichText::new("战备配置").font(hud_b(16.0)).color(TEXT));
        ui.label(egui::RichText::new("LOADOUT").font(hud(11.0)).color(TEXT_DIM));
        let hint = if app.model.armed.is_some() {
            "待命装填中 — 点击右侧战备库装入，ESC 取消"
        } else {
            "点选槽位待命 · 双击执行 · 右键快捷操作"
        };
        ui.label(egui::RichText::new(hint).font(hud(11.0)).color(if app.model.armed.is_some() { GOLD } else { TEXT_DIM }));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if hud_button(ui, "清空全部", Vec2::new(76.0, 24.0), DANGER, true).clicked() {
                for i in 0..config::SLOT_COUNT {
                    app.clear_slot(i);
                }
                app.model.armed = None;
                app.log(LogKind::Warn, "全部槽位已清空");
            }
        });
    });

    let grid_top = header.bottom() + 8.0;
    let grid_rect = Rect::from_min_size(
        Pos2::new(rect.left(), grid_top),
        Vec2::new(TILE_W * 5.0 + TILE_GAP * 4.0, TILE_H * 2.0 + TILE_GAP),
    );
    render_grid(app, ui, grid_rect);

    let detail = Rect::from_min_max(
        Pos2::new(rect.left(), grid_rect.bottom() + 8.0),
        rect.max,
    );
    render_detail(app, ui, detail);
}

pub fn render_grid(app: &mut H2ACApp, ui: &mut Ui, rect: Rect) {
    for idx in 0..config::SLOT_COUNT {
        let row = idx / 5;
        let col = idx % 5;
        let tile = Rect::from_min_size(
            Pos2::new(
                rect.left() + col as f32 * (TILE_W + TILE_GAP),
                rect.top() + row as f32 * (TILE_H + TILE_GAP),
            ),
            Vec2::new(TILE_W, TILE_H),
        );
        render_slot_tile(app, ui, tile, idx);
    }
}

pub fn render_slot_tile(app: &mut H2ACApp, ui: &mut Ui, rect: Rect, idx: usize) {
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
    paint_chamfer(&p, rect, 8.0, bg, border);
    if armed {
        corner_brackets(&p, rect.shrink(3.0), 8.0, GOLD);
        p.add(egui::Shape::convex_polygon(
            chamfer_points(rect.shrink(1.5), 8.0),
            GOLD_GHOST,
            Stroke::NONE,
        ));
    }

    p.text(
        Pos2::new(rect.left() + 8.0, rect.top() + 6.0),
        Align2::LEFT_TOP,
        format!("{:02}", idx + 1),
        hud_b(10.0),
        TEXT_DIM,
    );

    let now = ui.ctx().input(|i| i.time);
    if let Some(&t0) = app.model.flash.get(&idx) {
        let k = (now - t0) as f32 / 0.7;
        if k < 1.0 {
            let a = ((1.0 - k) * 140.0) as u8;
            p.add(egui::Shape::convex_polygon(
                chamfer_points(rect, 8.0),
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
            Pos2::new(rect.center().x, rect.top() + 48.0),
            Vec2::splat(56.0),
        );
        if let Some(tex) = app.model.icons.get(icon_key) {
            p.image(
                tex.id(),
                icon_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }

        let font = fit_font(&p, &name, rect.width() - 14.0, &[13.0, 11.5, 10.0], false);
        p.text(
            Pos2::new(rect.center().x, rect.top() + 84.0),
            Align2::CENTER_TOP,
            &name,
            font,
            TEXT,
        );

        let aw = arrow_strip_w(&cmd, 10.0, 3.0);
        arrow_strip(
            &p,
            Pos2::new(rect.center().x - aw / 2.0, rect.top() + 112.0),
            &cmd,
            10.0,
            3.0,
            GOLD_MID,
        );

        let strip = Rect::from_min_max(
            Pos2::new(rect.left() + 10.0, rect.bottom() - 5.0),
            Pos2::new(rect.right() - 10.0, rect.bottom() - 2.0),
        );
        p.rect_filled(strip, CornerRadius::ZERO, accent);
    } else {
        p.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "EMPTY",
            hud(12.0),
            TEXT_DIM,
        );
    }

    if let Some(hk) = app.model.config.slot_hotkeys.get(&idx.to_string()) {
        let badge = Rect::from_min_size(
            Pos2::new(rect.right() - 26.0, rect.top() + 5.0),
            Vec2::new(21.0, 15.0),
        );
        paint_chamfer(&p, badge, 3.0, BG_DEEP, Stroke::new(1.0, OK.gamma_multiply(0.6)));
        let hf = fit_font(&p, &hk.to_uppercase(), 17.0, &[9.5, 8.0], true);
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
