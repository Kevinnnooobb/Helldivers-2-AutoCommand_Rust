use eframe::egui::{self, Context, Key, Vec2};
use crate::theme::*;
use crate::widgets::*;
use crate::H2ACApp;
use crate::LogKind;

pub fn render_context_menu(app: &mut H2ACApp, ctx: &Context, m: &UiMetrics) {
    let Some(state) = app.context.clone() else { return };
    let slot = state.slot;
    let filled = app.slot_filled(slot);
    let rows = if filled { 3 } else { 1 };
    let size = Vec2::new(m.modal_ctx_menu_w(), rows as f32 * m.modal_row_h() + m.modal_pad_y());

    let mut close = false;
    let area = egui::Area::new(egui::Id::new("ctx_menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(state.pos)
        .show(ctx, |ui| {
            hud_panel(ui, size, m, GOLD_DIM, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                if filled {
                    if hud_button(ui, "执 行", Vec2::new(ui.available_width(), 26.0), m, GOLD, false).clicked() {
                        app.execute_slot(slot);
                        close = true;
                    }
                    if hud_button(ui, "清 除", Vec2::new(ui.available_width(), 26.0), m, DANGER, true).clicked() {
                        app.clear_slot(slot);
                        app.log(LogKind::Warn, format!("槽位 {} 已清除", slot + 1));
                        close = true;
                    }
                }
                if hud_button(ui, "设热键", Vec2::new(ui.available_width(), 26.0), m, CAT_EQUIP, false).clicked() {
                    app.capture.capturing = Some(slot);
                    app.capture.captured.clear();
                    app.capture.settings_capture = None;
                    close = true;
                }
            });
        });

    let rect = area.response.rect;
    if ctx.input(|i| i.pointer.primary_pressed()) {
        if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
            if !rect.contains(pos) {
                close = true;
            }
        }
    }
    if close || ctx.input(|i| i.key_pressed(Key::Escape)) {
        app.context = None;
    }
}
