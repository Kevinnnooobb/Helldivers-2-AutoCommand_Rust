use eframe::egui::{
    self, Align2, Context, CornerRadius, Pos2, Rect, Sense, Stroke, Ui, Vec2,
};
use crate::theme::*;
use crate::widgets::*;
use crate::H2ACApp;

pub fn render_topbar(app: &mut H2ACApp, ui: &mut Ui, rect: Rect, ctx: &Context) {
    let drag = ui.interact(rect, ui.id().with("drag"), Sense::drag());
    if drag.drag_started_by(egui::PointerButton::Primary) {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }

    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, BG_PANEL);
    p.hline(rect.left()..=rect.right(), rect.bottom() - 0.5, Stroke::new(1.0, LINE));
    p.add(egui::Shape::convex_polygon(
        chamfer_points(Rect::from_min_size(rect.min, Vec2::new(6.0, 48.0)), 4.0),
        GOLD,
        Stroke::NONE,
    ));

    p.text(
        Pos2::new(rect.left() + 22.0, rect.top() + 13.0),
        Align2::LEFT_CENTER,
        "H2AC-RS",
        hud_b(20.0),
        GOLD,
    );
    p.text(
        Pos2::new(rect.left() + 22.0, rect.top() + 34.0),
        Align2::LEFT_CENTER,
        "SUPER DESTROYER TERMINAL",
        hud(9.0),
        TEXT_DIM,
    );

    let mut bar = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(16.0, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    bar.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if glyph_button(ui, Glyph::Close, 30.0, "关闭").clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if glyph_button(ui, Glyph::Minimize, 30.0, "最小化").clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
            if glyph_button(ui, Glyph::Compact, 30.0, "紧凑模式").clicked() {
                app.set_compact(ctx, true);
            }
            if glyph_button(ui, Glyph::Gear, 30.0, "按键设置").clicked() {
                app.open_settings();
            }

            ui.add_space(10.0);

            let (resp, lp) = ui.allocate_painter(Vec2::new(110.0, 32.0), Sense::click());
            let hovered = resp.hovered();
            if hovered {
                paint_chamfer(&lp, resp.rect.shrink(1.0), 5.0, BG_HOVER, Stroke::NONE);
            }
            let t = ui.ctx().input(|i| i.time);
            status_lamp(&lp, Pos2::new(resp.rect.left() + 16.0, resp.rect.center().y), 4.5, app.model.listening, t);
            lp.text(
                Pos2::new(resp.rect.left() + 28.0, resp.rect.center().y),
                Align2::LEFT_CENTER,
                if app.model.listening { "监听中" } else { "已静音" },
                hud(13.0),
                if app.model.listening { OK } else { DANGER },
            );
            if resp.clicked() {
                app.toggle_listening();
            }
            if app.model.listening {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(66));
            }
        });
    });
}
