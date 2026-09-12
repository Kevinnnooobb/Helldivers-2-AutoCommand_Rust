use crate::config;
use crate::theme::*;
use crate::widgets::*;
use crate::H2ACApp;
use crate::LogKind;
use eframe::egui::{self, Align2, Context, Key, Vec2};

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
            "listen" => "请按下监听开关快捷键",
            "loadout_sync" => "请按下自动装配快捷键",
            "compact_toggle" => "请按下浮窗显示/隐藏快捷键（可含 Ctrl/Shift）",
            "loadout_cancel" => "请按下取消装配快捷键（全局）",
            _ => "按下目标按键",
        };
        let just = key_capture_modal(
            ctx,
            "settings_capture",
            title,
            m,
            &mut app.capture.captured,
            |_, _| {},
        );
        if just {
            match field.as_str() {
                "stratagem" => app.settings_key = app.capture.captured.clone(),
                "listen" => app.settings_listen_hotkey = app.capture.captured.clone(),
                "loadout_sync" => app.settings_loadout_sync_hotkey = app.capture.captured.clone(),
                "compact_toggle" => app.settings_compact_toggle = app.capture.captured.clone(),
                "loadout_cancel" => app.settings_cancel_hotkey = app.capture.captured.clone(),
                dir => {
                    app.settings_bindings
                        .insert(dir.to_string(), app.capture.captured.clone());
                }
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
            hud_panel(
                ui,
                Vec2::new(m.modal_settings_w(), m.modal_settings_h()),
                m,
                GOLD_DIM,
                |ui| {
                    // ── 固定标题 ──
                    ui.label(
                        egui::RichText::new("按键设置")
                            .font(m.hud_b(17.0))
                            .color(GOLD),
                    );
                    ui.label(
                        egui::RichText::new("KEY BINDINGS")
                            .font(m.hud(10.0))
                            .color(TEXT_DIM),
                    );
                    ui.add_space(8.0);

                    // ── 可滚动的主体：设置项已经超过弹窗高度，必须能滚（滚轮/拖动条）──
                    let footer_h = 44.0;
                    let body_h = (ui.available_height() - footer_h).max(80.0);
                    egui::ScrollArea::vertical()
                        .id_salt("settings_scroll")
                        .max_height(body_h)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("方向键绑定")
                                    .font(m.hud(13.0))
                                    .color(TEXT),
                            );
                            for dir in &["↑", "↓", "←", "→"] {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(*dir).font(m.hud(14.0)).color(GOLD_MID),
                                    );
                                    let v =
                                        app.settings_bindings.entry(dir.to_string()).or_default();
                                    if hud_button(
                                        ui,
                                        "捕获",
                                        Vec2::new(40.0, 24.0),
                                        m,
                                        GOLD_MID,
                                        false,
                                    )
                                    .clicked()
                                    {
                                        app.capture.settings_capture = Some(dir.to_string());
                                        app.capture.captured.clear();
                                        app.capture.capturing = None;
                                    }
                                    ui.add(
                                        egui::TextEdit::singleline(v)
                                            .font(m.hud(13.0))
                                            .desired_width(64.0),
                                    );
                                });
                            }
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("激活键:").font(m.hud(13.0)));
                                if hud_button(ui, "捕获", Vec2::new(40.0, 24.0), m, GOLD_MID, false)
                                    .clicked()
                                {
                                    app.capture.settings_capture = Some("stratagem".into());
                                    app.capture.captured.clear();
                                    app.capture.capturing = None;
                                }
                                ui.add(
                                    egui::TextEdit::singleline(&mut app.settings_key)
                                        .font(m.hud(13.0))
                                        .desired_width(80.0),
                                );
                            });
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("监听开关:").font(m.hud(13.0)));
                                if hud_button(ui, "捕获", Vec2::new(40.0, 24.0), m, GOLD_MID, false)
                                    .clicked()
                                {
                                    app.capture.settings_capture = Some("listen".into());
                                    app.capture.captured.clear();
                                    app.capture.capturing = None;
                                    app.capture.capturing_listen = false;
                                }
                                ui.add(
                                    egui::TextEdit::singleline(&mut app.settings_listen_hotkey)
                                        .font(m.hud(13.0))
                                        .desired_width(80.0),
                                );
                                ui.label(
                                    egui::RichText::new("全局快速开关")
                                        .font(m.hud(9.0))
                                        .color(TEXT_DIM),
                                );
                            });
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("自动装配:").font(m.hud(13.0)));
                                if hud_button(ui, "捕获", Vec2::new(40.0, 24.0), m, GOLD_MID, false)
                                    .clicked()
                                {
                                    app.capture.settings_capture = Some("loadout_sync".into());
                                    app.capture.captured.clear();
                                    app.capture.capturing = None;
                                    app.capture.capturing_listen = false;
                                    app.capture.capturing_loadout_sync = false;
                                }
                                ui.add(
                                    egui::TextEdit::singleline(
                                        &mut app.settings_loadout_sync_hotkey,
                                    )
                                    .font(m.hud(13.0))
                                    .desired_width(80.0),
                                );
                                ui.label(
                                    egui::RichText::new("Slot06~10 → 游戏 Loadout")
                                        .font(m.hud(9.0))
                                        .color(TEXT_DIM),
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("浮窗热键:").font(m.hud(13.0)));
                                if hud_button(ui, "捕获", Vec2::new(40.0, 24.0), m, GOLD_MID, false)
                                    .clicked()
                                {
                                    app.capture.settings_capture = Some("compact_toggle".into());
                                    app.capture.captured.clear();
                                    app.capture.capturing = None;
                                    app.capture.capturing_listen = false;
                                    app.capture.capturing_loadout_sync = false;
                                }
                                ui.add(
                                    egui::TextEdit::singleline(&mut app.settings_compact_toggle)
                                        .font(m.hud(13.0))
                                        .desired_width(110.0),
                                );
                                ui.label(
                                    egui::RichText::new("默认 Ctrl+Shift+F7")
                                        .font(m.hud(9.0))
                                        .color(TEXT_DIM),
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("取消装配:").font(m.hud(13.0)));
                                if hud_button(ui, "捕获", Vec2::new(40.0, 24.0), m, GOLD_MID, false)
                                    .clicked()
                                {
                                    app.capture.settings_capture = Some("loadout_cancel".into());
                                    app.capture.captured.clear();
                                    app.capture.capturing = None;
                                    app.capture.capturing_listen = false;
                                    app.capture.capturing_loadout_sync = false;
                                }
                                ui.add(
                                    egui::TextEdit::singleline(&mut app.settings_cancel_hotkey)
                                        .font(m.hud(13.0))
                                        .desired_width(110.0),
                                );
                                ui.label(
                                    egui::RichText::new("全局，独立于浮窗")
                                        .font(m.hud(9.0))
                                        .color(TEXT_DIM),
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("浮窗透明度:").font(m.hud(13.0)));
                                ui.add(
                                    egui::DragValue::new(&mut app.settings_compact_opacity)
                                        .speed(0.01)
                                        .range(0.35..=1.0),
                                );
                                ui.checkbox(
                                    &mut app.settings_debug_shots,
                                    egui::RichText::new("保存调试图").font(m.hud(11.0)),
                                );
                            });
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("按键延迟(秒):").font(m.hud(13.0)));
                                ui.add(
                                    egui::DragValue::new(&mut app.settings_delay)
                                        .speed(0.01)
                                        .range(0.01..=0.5),
                                );
                            });
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("预延迟(秒):").font(m.hud(13.0)));
                                ui.label(
                                    egui::RichText::new("Ctrl→面板就绪")
                                        .font(m.hud(9.0))
                                        .color(TEXT_DIM),
                                );
                                ui.add(
                                    egui::DragValue::new(&mut app.settings_pre_delay)
                                        .speed(0.01)
                                        .range(0.02..=0.5),
                                );
                            });
                            ui.add_space(6.0);
                        }); // end ScrollArea

                    // ── 固定底部按钮（始终可见，不会被滚动带走）──
                    ui.horizontal(|ui| {
                        if hud_button(ui, "保 存", Vec2::new(100.0, 30.0), m, GOLD, false).clicked()
                        {
                            app.model.config.key_bindings = app.settings_bindings.clone();
                            app.model.config.stratagem_key = app.settings_key.clone();
                            app.model.config.key_delay = app.settings_delay;
                            app.model.config.pre_delay = app.settings_pre_delay;
                            app.model.config.listen_hotkey =
                                app.settings_listen_hotkey.trim().to_string();
                            app.model.config.loadout_sync_hotkey =
                                app.settings_loadout_sync_hotkey.trim().to_string();
                            app.model.config.compact_mode.toggle_hotkey =
                                crate::hotkey::normalize_hotkey(&app.settings_compact_toggle);
                            app.model.config.loadout_sync_cancel_hotkey =
                                crate::hotkey::normalize_hotkey(&app.settings_cancel_hotkey);
                            app.model.config.compact_mode.opacity =
                                app.settings_compact_opacity.clamp(0.35, 1.0);
                            app.model.config.loadout_sync.debug_screenshots =
                                app.settings_debug_shots;
                            config::save_config(&app.model.config);
                            for conflict in app.model.config.hotkey_conflicts() {
                                app.log(LogKind::Warn, format!("快捷键冲突：{conflict}"));
                            }
                            // 让新热键立即生效；静音且无监听热键时回收钩子
                            if app.model.listening || !app.model.config.listen_hotkey.is_empty() {
                                if app.hotkey.is_none() {
                                    app.start_hotkeys();
                                } else {
                                    app.sync_hotkey_map();
                                }
                            } else {
                                app.stop_hotkeys();
                            }
                            app.show_settings = false;
                            app.log(LogKind::Info, "设置已保存");
                        }
                        if hud_button(ui, "取 消", Vec2::new(100.0, 30.0), m, TEXT_SUB, false)
                            .clicked()
                        {
                            app.show_settings = false;
                        }
                    });
                },
            );
        });
    if ctx.input(|i| i.key_pressed(Key::Escape)) && app.show_settings {
        app.show_settings = false;
    }
}
