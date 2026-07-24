use eframe::egui::{Context, CursorIcon, Key, Pos2, Rect, Sense, Vec2};
use crate::theme::*;
use crate::widgets::*;
use crate::H2ACApp;
use crate::ui::topbar::render_topbar;
use crate::ui::bottombar::render_bottombar;
use crate::ui::grid::render_left_column;
use crate::ui::library::render_library;
use crate::ui::modals::{render_context_menu, render_capture_modal, render_settings_modal, render_library_context_menu, render_stratagem_settings};
use crate::ui::plugin_creator::render_plugin_creator;

pub fn show_main(app: &mut H2ACApp, ctx: &Context) {
    let m = app.model.metrics;
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(BG_DEEP).inner_margin(0.0))
        .show(ctx, |ui| {
            let full = ui.available_rect_before_wrap();
            ui.advance_cursor_after_rect(full);
            scanlines(ui.painter(), full, &m);

            let top = Rect::from_min_size(full.min, Vec2::new(full.width(), m.topbar_h()));
            let bottom = Rect::from_min_max(Pos2::new(full.left(), full.bottom() - m.bottombar_h()), full.max);
            let content = Rect::from_min_max(Pos2::new(full.left(), top.bottom()), Pos2::new(full.right(), bottom.top()));
            render_topbar(app, ui, top, ctx, &m);
            render_bottombar(app, ui, bottom, &m);
            let inner = content.shrink2(Vec2::new(m.content_margin_x(), m.content_margin_y()));
            let left = Rect::from_min_size(inner.min, Vec2::new(m.left_panel_w(), inner.height()));
            let right = Rect::from_min_max(Pos2::new(left.right() + m.panel_gap(), inner.top()), inner.max);
            render_left_column(app, ui, left, &m);
            render_library(app, ui, right, &m);
        });
    render_context_menu(app, ctx, &m);
    render_library_context_menu(app, ctx, &m);
    render_capture_modal(app, ctx, &m);
    render_settings_modal(app, ctx, &m);
    render_stratagem_settings(app, ctx, &m);
    render_plugin_creator(app, ctx, &m);
    if ctx.input(|i| i.key_pressed(Key::Escape)) && app.model.armed.is_some() { app.model.armed = None; }

    let _grip = egui::Area::new(egui::Id::new("resize_grip"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::RIGHT_BOTTOM, [0.0, 0.0])
        .show(ctx, |ui| {
            let (resp, _) = ui.allocate_painter(Vec2::new(20.0, 20.0), Sense::drag());
            if resp.hovered() { ctx.set_cursor_icon(CursorIcon::ResizeNwSe); }
            let delta = resp.drag_delta();
            if delta != Vec2::ZERO {
                let new = ctx.screen_rect().size() + delta;
                let min_w = 440.0;
                let min_h = 256.0;
                if new.x >= min_w && new.y >= min_h {
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(
                        egui::Vec2::new(new.x.round(), new.y.round()),
                    ));
                }
            }
        });
}
