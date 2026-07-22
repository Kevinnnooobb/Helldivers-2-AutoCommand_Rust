use eframe::egui::{Context, Key, Vec2};
use crate::H2ACApp;
use crate::theme::*;
use crate::widgets::*;
use crate::config;
use crate::LogKind;

pub fn render_capture_modal(app: &mut H2ACApp, ctx: &Context) {
    let Some(slot) = app.capture.capturing else { return };

    let title = format!("槽位 {} — 设置快捷键", slot + 1);
    let mut captured = std::mem::take(&mut app.capture.captured);
    let app_ref = &mut *app;
    let _just = key_capture_modal(ctx, "capture", &title, &mut captured, |ui, key_name| {
        ui.horizontal(|ui| {
            if hud_button(ui, "确 认", Vec2::new(90.0, 28.0), GOLD, false).clicked() {
                app_ref.model.config
                    .slot_hotkeys
                    .insert(slot.to_string(), key_name.to_string());
                config::save_config(&app_ref.model.config);
                app_ref.log(LogKind::Info, format!("槽位 {} 快捷键: {}", slot + 1, key_name));
                app_ref.capture.capturing = None;
            }
            if hud_button(ui, "取 消", Vec2::new(90.0, 28.0), TEXT_SUB, false).clicked() {
                app_ref.capture.capturing = None;
            }
        });
    });
    app.capture.captured = captured;

    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        app.capture.capturing = None;
    }
}
