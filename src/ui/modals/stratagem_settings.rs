use eframe::egui::{self, Align2, Context, Key, Vec2};
use crate::H2ACApp;
use crate::theme::*;
use crate::widgets::*;
use crate::LogKind;

/// 解析用户输入的指令序列："↑, ↓, 左" → ["up","down","left"]
fn parse_command_text(text: &str) -> Vec<String> {
    text.split(',')
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
        .collect()
}

pub fn render_stratagem_settings(app: &mut H2ACApp, ctx: &Context, m: &UiMetrics) {
    if !app.stratagem_settings.visible {
        return;
    }

    let mut close = false;
    egui::Area::new(egui::Id::new("stratagem_settings"))
        .order(egui::Order::Foreground)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            hud_panel(ui, Vec2::new(m.modal_stratagem_w(), m.modal_stratagem_h()), m, GOLD_DIM, |ui| {
                ui.label(egui::RichText::new("战备设置").font(m.hud_b(17.0)).color(GOLD));
                ui.label(egui::RichText::new("STRATAGEM SETTINGS").font(m.hud(10.0)).color(TEXT_DIM));
                ui.add_space(8.0);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("名称:").font(m.hud(13.0)).color(TEXT));
                    ui.add(
                        egui::TextEdit::singleline(&mut app.stratagem_settings.name)
                            .font(m.hud(13.0))
                            .desired_width(280.0)
                            .interactive(app.stratagem_settings.is_plugin),
                    );
                });
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("图标 key:").font(m.hud(13.0)).color(TEXT));
                    ui.add(
                        egui::TextEdit::singleline(&mut app.stratagem_settings.icon_key)
                            .font(m.hud(13.0))
                            .desired_width(280.0),
                    );
                });
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("描述:").font(m.hud(13.0)).color(TEXT));
                    ui.add(
                        egui::TextEdit::multiline(&mut app.stratagem_settings.description)
                            .font(m.hud(13.0))
                            .desired_width(280.0)
                            .desired_rows(3),
                    );
                });
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("指令序列:").font(m.hud(13.0)).color(TEXT));
                    ui.add(
                        egui::TextEdit::singleline(&mut app.stratagem_settings.command_text)
                            .font(m.hud(13.0))
                            .desired_width(280.0)
                            .hint_text("↑, ↓, ←, →"),
                    );
                });
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("分类:").font(m.hud(13.0)).color(TEXT));
                    egui::ComboBox::from_id_salt("stratagem_set_cat")
                        .width(280.0)
                        .selected_text(egui::RichText::new(super::cat_label(&app.stratagem_settings.category)).font(m.hud(13.0)).color(category_color(&app.stratagem_settings.category)))
                        .show_ui(ui, |ui| {
                            for cat in &app.lib_categories() {
                                if ui.selectable_label(false, egui::RichText::new(super::cat_label(cat)).font(m.hud(12.0))).clicked() {
                                    app.stratagem_settings.category = cat.clone();
                                }
                            }
                        });
                });
                ui.add_space(14.0);

                ui.horizontal(|ui| {
                    if hud_button(ui, "保 存", Vec2::new(100.0, 30.0), m, GOLD, false).clicked() {
                        let name = app.stratagem_settings.name.clone();
                        let icon = app.stratagem_settings.icon_key.clone();
                        let desc = app.stratagem_settings.description.clone();
                        let cat = app.stratagem_settings.category.clone();
                        let orig = app.stratagem_settings.original_name.clone();
                        let cmd = parse_command_text(&app.stratagem_settings.command_text);

                        if app.stratagem_settings.is_plugin {
                            // 运行时更新
                            for p in &mut app.plugins.stratagems {
                                if p.name == orig {
                                    p.name = name.clone();
                                    p.icon = icon.clone();
                                    p.description = desc.clone();
                                    p.category = cat.clone();
                                    p.command = cmd.clone();
                                }
                            }
                            // 磁盘持久化：结构化精确匹配（不再用 contains 字符串猜测）
                            let (f_name, f_icon, f_desc, f_cat, f_cmd, f_orig) =
                                (name.clone(), icon.clone(), desc.clone(), cat.clone(), cmd.clone(), orig.clone());
                            let _ = crate::plugin::rewrite_stratagems(&mut |strats| {
                                let mut changed = false;
                                for s in strats.iter_mut() {
                                    if s.name == f_orig {
                                        s.name = f_name.clone();
                                        s.icon = f_icon.clone();
                                        s.description = f_desc.clone();
                                        s.category = f_cat.clone();
                                        s.command = f_cmd.clone();
                                        changed = true;
                                    }
                                }
                                changed
                            });
                        } else {
                            app.set_category_override(&orig, &cat);
                        }
                        app.stratagem_settings.visible = false;
                        app.log(LogKind::Info, format!("战备设置已保存: {}", name));
                        close = true;
                    }
                    if hud_button(ui, "取 消", Vec2::new(100.0, 30.0), m, TEXT_SUB, false).clicked() {
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
