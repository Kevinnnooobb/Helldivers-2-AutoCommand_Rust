use eframe::egui::{self, Align2, Context, Key, Vec2};
use crate::config;
use crate::stratagems::{STRATAGEMS, StratagemRef};
use crate::theme::*;
use crate::widgets::*;
use crate::H2ACApp;
use crate::LogKind;

pub fn render_context_menu(app: &mut H2ACApp, ctx: &Context) {
    let Some(state) = app.context.clone() else { return };
    let slot = state.slot;
    let filled = app.slot_filled(slot);
    let rows = if filled { 3 } else { 1 };
    let size = Vec2::new(150.0, rows as f32 * 30.0 + 18.0);

    let mut close = false;
    let area = egui::Area::new(egui::Id::new("ctx_menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(state.pos)
        .show(ctx, |ui| {
            hud_panel(ui, size, GOLD_DIM, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                if filled {
                    if hud_button(ui, "执 行", Vec2::new(ui.available_width(), 26.0), GOLD, false).clicked() {
                        app.execute_slot(slot);
                        close = true;
                    }
                    if hud_button(ui, "清 除", Vec2::new(ui.available_width(), 26.0), DANGER, true).clicked() {
                        app.clear_slot(slot);
                        app.log(LogKind::Warn, format!("槽位 {} 已清除", slot + 1));
                        close = true;
                    }
                }
                if hud_button(ui, "设热键", Vec2::new(ui.available_width(), 26.0), CAT_EQUIP, false).clicked() {
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

pub fn render_settings_modal(app: &mut H2ACApp, ctx: &Context) {
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
        let just = key_capture_modal(ctx, "settings_capture", title, &mut app.capture.captured, |_, _| {});
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
            hud_panel(ui, Vec2::new(360.0, 330.0), GOLD_DIM, |ui| {
                ui.label(egui::RichText::new("按键设置").font(hud_b(17.0)).color(GOLD));
                ui.label(egui::RichText::new("KEY BINDINGS").font(hud(10.0)).color(TEXT_DIM));
                ui.add_space(10.0);

                ui.label(egui::RichText::new("方向键绑定").font(hud(13.0)).color(TEXT));
                for dir in &["↑", "↓", "←", "→"] {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(*dir).font(hud(14.0)).color(GOLD_MID));
                        let v = app.settings_bindings.entry(dir.to_string()).or_default();
                        let display = format_key_name(v);
                        if hud_button(ui, &display, Vec2::new(80.0, 24.0), GOLD_MID, false).clicked() {
                            app.capture.settings_capture = Some(dir.to_string());
                            app.capture.captured.clear();
                            app.capture.capturing = None;
                        }
                    });
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("激活键:").font(hud(13.0)));
                    let display = format_key_name(&app.settings_key);
                    if hud_button(ui, &display, Vec2::new(80.0, 24.0), GOLD_MID, false).clicked() {
                        app.capture.settings_capture = Some("stratagem".into());
                        app.capture.captured.clear();
                        app.capture.capturing = None;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("按键延迟(秒):").font(hud(13.0)));
                    ui.add(egui::DragValue::new(&mut app.settings_delay).speed(0.01).range(0.01..=0.5));
                });
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("预延迟(秒):").font(hud(13.0)));
                    ui.label(egui::RichText::new("Ctrl→面板就绪").font(hud(9.0)).color(TEXT_DIM));
                    ui.add(egui::DragValue::new(&mut app.settings_pre_delay).speed(0.01).range(0.02..=0.5));
                });
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if hud_button(ui, "保 存", Vec2::new(100.0, 30.0), GOLD, false).clicked() {
                        app.model.config.key_bindings = app.settings_bindings.clone();
                        app.model.config.stratagem_key = app.settings_key.clone();
                        app.model.config.key_delay = app.settings_delay;
                        app.model.config.pre_delay = app.settings_pre_delay;
                        config::save_config(&app.model.config);
                        app.show_settings = false;
                        app.log(LogKind::Info, "设置已保存");
                    }
                    if hud_button(ui, "取 消", Vec2::new(100.0, 30.0), TEXT_SUB, false).clicked() {
                        app.show_settings = false;
                    }
                });
            });
        });
    if ctx.input(|i| i.key_pressed(Key::Escape)) && app.show_settings {
        app.show_settings = false;
    }
}

pub fn render_library_context_menu(app: &mut H2ACApp, ctx: &Context) {
    let Some(ref lib_ctx) = app.library_context.clone() else { return };

    let num_rows = 1 // category ComboBox
        + if app.model.armed.is_some() { 1 } else { 0 }
        + 1 // settings
        + if lib_ctx.is_plugin { 1 } else { 0 }; // delete
    let size = Vec2::new(160.0, num_rows as f32 * 30.0 + 18.0);

    let mut close = false;
    let area = egui::Area::new(egui::Id::new("lib_ctx_menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(lib_ctx.pos)
        .show(ctx, |ui| {
            hud_panel(ui, size, GOLD_DIM, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;

                if app.model.armed.is_some() {
                    if hud_button(ui, "装入槽位", Vec2::new(ui.available_width(), 26.0), GOLD, false).clicked() {
                        close = true;
                        let name = lib_ctx.name.clone();
                        if lib_ctx.is_plugin {
                            if let Some(clone) = app.plugins.stratagems.iter()
                                .find(|p| p.name == name).cloned()
                            {
                                let sref = StratagemRef::Plugin(&clone);
                                app.assign_stratagem_ref(&sref);
                            }
                        } else if let Some(s) = STRATAGEMS.iter().find(|s| s.name == name) {
                            let sref = StratagemRef::Base(s);
                            app.assign_stratagem_ref(&sref);
                        }
                    }
                }

                let mut cat_sel = lib_ctx.category.clone();
                egui::ComboBox::from_id_salt("lib_ctx_cat")
                    .width(140.0)
                    .selected_text(egui::RichText::new(cat_label(&cat_sel)).font(hud(12.0)).color(category_color(&cat_sel)))
                    .show_ui(ui, |ui| {
                        for cat in &app.lib_categories() {
                            if ui.selectable_label(false, egui::RichText::new(cat_label(cat)).font(hud(12.0))).clicked() {
                                cat_sel = cat.clone();
                            }
                        }
                    });
                if cat_sel != lib_ctx.category {
                    app.set_category_override(&lib_ctx.name, &cat_sel);
                    app.log(LogKind::Info, format!("分类修改: {} → {}", lib_ctx.name, cat_sel));
                }

                if hud_button(ui, "设 置", Vec2::new(ui.available_width(), 26.0), CAT_EQUIP, false).clicked() {
                    close = true;
                    app.stratagem_settings.visible = true;
                    app.stratagem_settings.name = lib_ctx.name.clone();
                    app.stratagem_settings.icon_key = lib_ctx.icon_key.clone();
                    app.stratagem_settings.command_text = lib_ctx.command.join(", ");
                    app.stratagem_settings.description = lib_ctx.description.clone();
                    app.stratagem_settings.category = lib_ctx.category.clone();
                    app.stratagem_settings.is_plugin = lib_ctx.is_plugin;
                    app.stratagem_settings.original_name = lib_ctx.name.clone();
                }

                if lib_ctx.is_plugin {
                    if hud_button(ui, "删 除", Vec2::new(ui.available_width(), 26.0), DANGER, true).clicked() {
                        close = true;
                        app.delete_plugin_stratagem(&lib_ctx.name);
                    }
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
        app.library_context = None;
    }
}

pub fn render_stratagem_settings(app: &mut H2ACApp, ctx: &Context) {
    if !app.stratagem_settings.visible {
        return;
    }

    let mut close = false;
    egui::Area::new(egui::Id::new("stratagem_settings"))
        .order(egui::Order::Foreground)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            hud_panel(ui, Vec2::new(420.0, 400.0), GOLD_DIM, |ui| {
                ui.label(egui::RichText::new("战备设置").font(hud_b(17.0)).color(GOLD));
                ui.label(egui::RichText::new("STRATAGEM SETTINGS").font(hud(10.0)).color(TEXT_DIM));
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("名称:").font(hud(13.0)).color(TEXT));
                    ui.add(
                        egui::TextEdit::singleline(&mut app.stratagem_settings.name)
                            .font(hud(13.0))
                            .desired_width(280.0)
                            .interactive(app.stratagem_settings.is_plugin),
                    );
                });
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("图标 key:").font(hud(13.0)).color(TEXT));
                    ui.add(
                        egui::TextEdit::singleline(&mut app.stratagem_settings.icon_key)
                            .font(hud(13.0))
                            .desired_width(280.0),
                    );
                });
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("描述:").font(hud(13.0)).color(TEXT));
                    ui.add(
                        egui::TextEdit::multiline(&mut app.stratagem_settings.description)
                            .font(hud(13.0))
                            .desired_width(280.0)
                            .desired_rows(3),
                    );
                });
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("指令序列:").font(hud(13.0)).color(TEXT));
                    ui.add(
                        egui::TextEdit::singleline(&mut app.stratagem_settings.command_text)
                            .font(hud(13.0))
                            .desired_width(280.0)
                            .hint_text("↑, ↓, ←, →"),
                    );
                });
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("分类:").font(hud(13.0)).color(TEXT));
                    egui::ComboBox::from_id_salt("stratagem_set_cat")
                        .width(280.0)
                        .selected_text(egui::RichText::new(cat_label(&app.stratagem_settings.category)).font(hud(13.0)).color(category_color(&app.stratagem_settings.category)))
                        .show_ui(ui, |ui| {
                            for cat in &app.lib_categories() {
                                if ui.selectable_label(false, egui::RichText::new(cat_label(cat)).font(hud(12.0))).clicked() {
                                    app.stratagem_settings.category = cat.clone();
                                }
                            }
                        });
                });
                ui.add_space(14.0);

                ui.horizontal(|ui| {
                    if hud_button(ui, "保 存", Vec2::new(100.0, 30.0), GOLD, false).clicked() {
                        let name = app.stratagem_settings.name.clone();
                        let icon = app.stratagem_settings.icon_key.clone();
                        let desc = app.stratagem_settings.description.clone();
                        let cat = app.stratagem_settings.category.clone();
                        let orig = app.stratagem_settings.original_name.clone();
                        let cmd: Vec<String> = app.stratagem_settings.command_text
                            .split(',')
                            .map(|s| {
                                let t = s.trim();
                                match t {
                                    "↑" | "上" => "up".to_string(),
                                    "↓" | "下" => "down".to_string(),
                                    "←" | "左" => "left".to_string(),
                                    "→" | "右" => "right".to_string(),
                                    _ => t.to_string(),
                                }
                            })
                            .filter(|s| !s.is_empty())
                            .collect();

                        if app.stratagem_settings.is_plugin {
                            for p in &mut app.plugins.stratagems {
                                if p.name == orig {
                                    p.name = name.clone();
                                    p.icon = icon.clone();
                                    p.description = desc.clone();
                                    p.category = cat.clone();
                                    p.command = cmd.clone();
                                }
                            }
                            let dir = crate::plugin::plugins_dir();
                            let _ = std::fs::create_dir_all(&dir);
                            if let Ok(entries) = std::fs::read_dir(&dir) {
                                for entry in entries.flatten() {
                                    let path = entry.path();
                                    if path.extension().map_or(true, |e| e != "json") { continue; }
                                    if let Ok(data) = std::fs::read_to_string(&path) {
                                        if data.contains(&format!("\"name\": \"{}\"", orig)) {
                                            if let Ok(mut m) = serde_json::from_str::<crate::stratagems::PluginManifest>(&data) {
                                                for s in &mut m.stratagems {
                                                    if s.name == orig {
                                                        s.name = name.clone();
                                                        s.icon = icon.clone();
                                                        s.description = desc.clone();
                                                        s.category = cat.clone();
                                                        s.command = cmd.clone();
                                                    }
                                                }
                                                let _ = std::fs::write(&path, serde_json::to_string_pretty(&m).unwrap_or_default());
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            app.set_category_override(&orig, &cat);
                        }
                        app.stratagem_settings.visible = false;
                        app.log(LogKind::Info, format!("战备设置已保存: {}", name));
                        close = true;
                    }
                    if hud_button(ui, "取 消", Vec2::new(100.0, 30.0), TEXT_SUB, false).clicked() {
                        app.stratagem_settings.visible = false;
                        close = true;
                    }
                });
            });
        });

    if ctx.input(|i| i.key_pressed(Key::Escape)) && app.stratagem_settings.visible {
        app.stratagem_settings.visible = false;
    }
}

fn cat_label(cat: &str) -> String {
    let short = crate::ui::common::cat_short(cat);
    if short == "??" { cat.to_string() } else { format!("{short} ({cat})") }
}
