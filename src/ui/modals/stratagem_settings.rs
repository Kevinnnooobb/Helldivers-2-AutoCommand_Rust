use eframe::egui::{self, Align2, Context, Key, Vec2};
use crate::H2ACApp;
use crate::theme::*;
use crate::widgets::*;
use crate::LogKind;

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
                        .selected_text(egui::RichText::new(super::cat_label(&app.stratagem_settings.category)).font(hud(13.0)).color(category_color(&app.stratagem_settings.category)))
                        .show_ui(ui, |ui| {
                            for cat in &app.lib_categories() {
                                if ui.selectable_label(false, egui::RichText::new(super::cat_label(cat)).font(hud(12.0))).clicked() {
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
