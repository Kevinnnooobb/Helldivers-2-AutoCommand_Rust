use eframe::egui::{Context, Key, Pos2, Rect, Vec2};
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
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(BG_DEEP).inner_margin(0.0))
        .show(ctx, |ui| {
            let full = ui.available_rect_before_wrap();
            ui.advance_cursor_after_rect(full);
            scanlines(ui.painter(), full);
            let top = Rect::from_min_size(full.min, Vec2::new(full.width(), 48.0));
            let bottom = Rect::from_min_max(Pos2::new(full.left(), full.bottom() - 52.0), full.max);
            let content = Rect::from_min_max(Pos2::new(full.left(), top.bottom()), Pos2::new(full.right(), bottom.top()));
            render_topbar(app, ui, top, ctx);
            render_bottombar(app, ui, bottom);
            let inner = content.shrink2(Vec2::new(12.0, 8.0));
            let left = Rect::from_min_size(inner.min, Vec2::new(664.0, inner.height()));
            let right = Rect::from_min_max(Pos2::new(left.right() + 8.0, inner.top()), inner.max);
            render_left_column(app, ui, left);
            render_library(app, ui, right);
        });
    render_context_menu(app, ctx);
    render_library_context_menu(app, ctx);
    render_capture_modal(app, ctx);
    render_settings_modal(app, ctx);
    render_stratagem_settings(app, ctx);
    render_plugin_creator(app, ctx);
    if ctx.input(|i| i.key_pressed(Key::Escape)) && app.model.armed.is_some() { app.model.armed = None; }
}
