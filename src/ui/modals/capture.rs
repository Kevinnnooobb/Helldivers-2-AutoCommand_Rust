use eframe::egui::{Context, Key, Vec2};
use crate::H2ACApp;
use crate::theme::*;
use crate::widgets::*;
use crate::config;
use crate::LogKind;

pub fn render_capture_modal(app: &mut H2ACApp, ctx: &Context, m: &UiMetrics) {
    if let Some(slot) = app.capture.capturing {
        let title = format!("槽位 {} — 设置快捷键", slot + 1);
        let mut captured = std::mem::take(&mut app.capture.captured);
        let app_ref = &mut *app;
        let _just = key_capture_modal(ctx, "capture", &title, m, &mut captured, |ui, key_name| {
            ui.horizontal(|ui| {
                if hud_button(ui, "确 认", Vec2::new(90.0, 28.0), m, GOLD, false).clicked() {
                    app_ref.model.config
                        .slot_hotkeys
                        .insert(slot.to_string(), key_name.to_string());
                    config::save_config(&app_ref.model.config);
                    app_ref.sync_hotkey_map();
                    app_ref.log(LogKind::Info, format!("槽位 {} 快捷键: {}", slot + 1, key_name));
                    if !app_ref.model.config.listen_hotkey.is_empty()
                        && key_name == app_ref.model.config.listen_hotkey
                    {
                        app_ref.log(LogKind::Warn, "该键与监听开关热键相同，将优先开关监听");
                    }
                    app_ref.capture.capturing = None;
                }
                if hud_button(ui, "取 消", Vec2::new(90.0, 28.0), m, TEXT_SUB, false).clicked() {
                    app_ref.capture.capturing = None;
                }
            });
        });
        app.capture.captured = captured;

        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            app.capture.capturing = None;
        }
        return;
    }

    if !app.capture.capturing_listen {
        return;
    }

    let title = "监听开关 — 设置全局快捷键";
    let mut captured = std::mem::take(&mut app.capture.captured);
    let app_ref = &mut *app;
    let _just = key_capture_modal(ctx, "listen_capture", title, m, &mut captured, |ui, key_name| {
        ui.horizontal(|ui| {
            if hud_button(ui, "确 认", Vec2::new(90.0, 28.0), m, GOLD, false).clicked() {
                app_ref.model.config.listen_hotkey = key_name.to_string();
                config::save_config(&app_ref.model.config);
                if app_ref.model.listening || !app_ref.model.config.listen_hotkey.is_empty() {
                    if app_ref.hotkey.is_none() {
                        app_ref.start_hotkeys();
                    } else {
                        app_ref.sync_hotkey_map();
                    }
                } else {
                    app_ref.stop_hotkeys();
                }
                app_ref.log(LogKind::Info, format!("监听开关快捷键: {}", key_name));
                app_ref.capture.capturing_listen = false;
            }
            if hud_button(ui, "取 消", Vec2::new(90.0, 28.0), m, TEXT_SUB, false).clicked() {
                app_ref.capture.capturing_listen = false;
            }
        });
    });
    app.capture.captured = captured;

    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        app.capture.capturing_listen = false;
    }
}
