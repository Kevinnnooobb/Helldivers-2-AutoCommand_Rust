use eframe::egui::{self, Align2, Context, Key, Vec2};
use crate::H2ACApp;
use crate::theme::*;
use crate::widgets::*;
use crate::config;
use crate::LogKind;

pub fn render_settings_modal(app: &mut H2ACApp, ctx: &Context, m: &UiMetrics) {
    if !app.show_settings {
        return;
    }

    if let Some(ref field) = app.capture.settings_capture.clone() {
        let title = match field.as_str() {
            "↑" => "请按下 ↑ 键",
            "↓" => "请按下 ↓ 键",
            "←" => "请按下 ← 键",
            "→" => "请按下 → 键",
            "stratagem" => "请按下激活键",
            _ => "按下目标按键",
        };
        let just = key_capture_modal(ctx, "settings_capture", title, m, &mut app.capture.captured, |_, _| {});
        if just {
            match field.as_str() {
                "stratagem" => app.settings_key = app.capture.captured.clone(),
                dir => { app.settings_bindings.insert(dir.to_string(), app.capture.captured.clone()); }
            }
            app.capture.settings_capture = None;
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            app.capture.settings_capture = None;
        }
        return;
    }

    egui::Area::new(egui::Id::new("settings"))
        .order(egui::Order::Foreground)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            hud_panel(ui, Vec2::new(m.modal_settings_w(), m.modal_settings_h()), m, GOLD_DIM, |ui| {
                ui.label(egui::RichText::new("按键设置").font(m.hud_b(17.0)).color(GOLD));
                ui.label(egui::RichText::new("KEY BINDINGS").font(m.hud(10.0)).color(TEXT_DIM));
                ui.add_space(10.0);

                ui.label(egui::RichText::new("方向键绑定").font(m.hud(13.0)).color(TEXT));
                for dir in &["↑", "↓", "←", "→"] {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(*dir).font(m.hud(14.0)).color(GOLD_MID));
                        let v = app.settings_bindings.entry(dir.to_string()).or_default();
                        if hud_button(ui, "🎬", Vec2::new(32.0, 24.0), m, GOLD_MID, false).clicked() {
                            app.capture.settings_capture = Some(dir.to_string());
                            app.capture.captured.clear();
                            app.capture.capturing = None;
                        }
                        ui.add(egui::TextEdit::singleline(v).font(m.hud(13.0)).desired_width(64.0));
                    });
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("激活键:").font(m.hud(13.0)));
                    if hud_button(ui, "🎬", Vec2::new(32.0, 24.0), m, GOLD_MID, false).clicked() {
                        app.capture.settings_capture = Some("stratagem".into());
                        app.capture.captured.clear();
                        app.capture.capturing = None;
                    }
                    ui.add(egui::TextEdit::singleline(&mut app.settings_key).font(m.hud(13.0)).desired_width(80.0));
                });
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("按键延迟(秒):").font(m.hud(13.0)));
                    ui.add(egui::DragValue::new(&mut app.settings_delay).speed(0.01).range(0.01..=0.5));
                });
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("预延迟(秒):").font(m.hud(13.0)));
                    ui.label(egui::RichText::new("Ctrl→面板就绪").font(m.hud(9.0)).color(TEXT_DIM));
                    ui.add(egui::DragValue::new(&mut app.settings_pre_delay).speed(0.01).range(0.02..=0.5));
                });
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if hud_button(ui, "保 存", Vec2::new(100.0, 30.0), m, GOLD, false).clicked() {
                        app.model.config.key_bindings = app.settings_bindings.clone();
                        app.model.config.stratagem_key = app.settings_key.clone();
                        app.model.config.key_delay = app.settings_delay;
                        app.model.config.pre_delay = app.settings_pre_delay;
                        config::save_config(&app.model.config);
                        app.show_settings = false;
                        app.log(LogKind::Info, "设置已保存");
                    }
                    if hud_button(ui, "取 消", Vec2::new(100.0, 30.0), m, TEXT_SUB, false).clicked() {
                        app.show_settings = false;
                    }
                });
            });
        });
    if ctx.input(|i| i.key_pressed(Key::Escape)) && app.show_settings {
        app.show_settings = false;
    }
}
