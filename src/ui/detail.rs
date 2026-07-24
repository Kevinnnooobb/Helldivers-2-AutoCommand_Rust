use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Ui, Vec2};
use crate::stratagems::{STRATAGEMS, dir_to_arrow};
use crate::theme::*;
use crate::widgets::*;
use crate::H2ACApp;
use crate::LogKind;

pub fn render_detail(app: &mut H2ACApp, ui: &mut Ui, rect: Rect, m: &UiMetrics) {
    let mut region = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    hud_panel(&mut region, rect.size(), m, LINE, |ui| {
        let Some(idx) = app.model.detail_slot.filter(|&i| app.slot_filled(i)) else {
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new("— 点选槽位以查看详情 —").font(m.hud(13.0)).color(TEXT_DIM));
            });
            return;
        };
        let (name, icon_key, cmd, def_cat, model_name): (String, String, Vec<&str>, String, String);
        if let Some(p) = app.model.plugin_slots.get(&idx) {
            name = p.name.clone();
            icon_key = p.icon.clone();
            cmd = p.command.iter().map(|c| dir_to_arrow(c.as_str())).collect();
            def_cat = p.category.clone();
            model_name = p.model.clone();
        } else if let Some(si) = app.model.slots[idx] {
            let s = &STRATAGEMS[si];
            name = s.name.to_string();
            icon_key = s.icon.to_string();
            cmd = s.command.to_vec();
            def_cat = s.category.to_string();
            model_name = s.model.to_string();
        } else {
            return;
        };
        let eff_cat = app.effective_category(&name, &def_cat);
        let accent = category_color(&eff_cat);

        let content = ui.available_rect_before_wrap();
        let h = content.height();
        let icon_area = Rect::from_min_size(content.min, Vec2::new(m.detail_icon_area_w(), h));
        let text_area = Rect::from_min_size(Pos2::new(icon_area.right(), content.top()), Vec2::new(m.detail_text_area_w(), h));
        let btn_area = Rect::from_min_size(Pos2::new(content.right() - m.detail_btn_area_w(), content.top()), Vec2::new(m.detail_btn_area_w(), h));
        let arrow_area = Rect::from_min_max(Pos2::new(text_area.right(), content.top()), Pos2::new(btn_area.left(), content.bottom()));

        let ir = Rect::from_center_size(icon_area.center(), Vec2::splat(m.detail_icon_size()));
        paint_chamfer(ui.painter(), ir, 8.0, BG_DEEP, Stroke::new(1.0, accent.gamma_multiply(0.5)));
        if let Some(tex) = app.model.icons.get(&icon_key) {
            ui.painter().image(tex.id(), ir.shrink(10.0), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }

        let mut text_ui = ui.new_child(egui::UiBuilder::new().max_rect(text_area));
        text_ui.vertical(|ui| {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(&name).font(m.hud_b(22.0)).color(TEXT));

            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("{} ·", model_name)).font(m.hud(12.0)).color(TEXT_SUB));
                let mut cat_sel = eff_cat.clone();
                egui::ComboBox::from_id_salt("detail_cat")
                    .width(140.0)
                    .selected_text(egui::RichText::new(&cat_sel).font(m.hud(12.0)).color(accent))
                    .show_ui(ui, |ui| {
                        let cats = app.lib_categories();
                        for cat in &cats {
                            if ui.selectable_label(false, egui::RichText::new(cat).font(m.hud(12.0))).clicked() {
                                cat_sel = cat.clone();
                            }
                        }
                        if ui.button(egui::RichText::new("✚ 新建分类").font(m.hud(12.0))).clicked() {
                            cat_sel = String::new();
                        }
                    });
                if cat_sel != eff_cat && !cat_sel.is_empty() {
                    app.set_category_override(&name, &cat_sel);
                    app.log(LogKind::Info, format!("分类修改: {} → {}", name, cat_sel));
                } else if cat_sel.is_empty() && cat_sel != eff_cat {
                    let mut new_cat = String::new();
                    ui.add(egui::TextEdit::singleline(&mut new_cat).hint_text("输入新分类名…").font(m.hud(12.0)).desired_width(110.0));
                    if ui.button("确定").clicked() && !new_cat.trim().is_empty() {
                        app.set_category_override(&name, new_cat.trim());
                        app.log(LogKind::Info, format!("分类修改: {} → {}", name, new_cat.trim()));
                    }
                }
                if eff_cat != def_cat {
                    ui.label(egui::RichText::new("✎").font(m.hud(9.0)).color(GOLD_DIM));
                }
            });

        });

        let mut btn_ui = ui.new_child(egui::UiBuilder::new().max_rect(btn_area));
        btn_ui.vertical_centered(|ui| {
            ui.add_space(6.0);
            if hud_button(ui, "执 行", Vec2::new(88.0, 34.0), m, GOLD, false).clicked() { app.execute_slot(idx); }
            ui.add_space(4.0);
            if hud_button(ui, "设热键", Vec2::new(88.0, 26.0), m, CAT_EQUIP, false).clicked() {
                app.capture.capturing = Some(idx);
                app.capture.captured.clear();
            }
            ui.add_space(4.0);
            if hud_button(ui, "清 除", Vec2::new(88.0, 26.0), m, DANGER, true).clicked() {
                app.clear_slot(idx);
                if app.model.armed == Some(idx) { app.model.armed = None; }
                app.log(LogKind::Warn, format!("槽位 {} 已清除", idx + 1));
            }
            if eff_cat != def_cat {
                ui.add_space(4.0);
                if hud_button(ui, "重置分类", Vec2::new(88.0, 22.0), m, TEXT_SUB, false).clicked() {
                    app.clear_category_override(&name);
                    app.log(LogKind::Info, format!("分类已重置: {}", name));
                }
            }
        });

        let arrow_size = m.detail_arrow_size();
        let arrow_gap = m.detail_arrow_gap();
        let aw = arrow_strip_w(&cmd, arrow_size, arrow_gap);
        arrow_strip(ui.painter(), Pos2::new(arrow_area.center().x - aw / 2.0, arrow_area.center().y - 9.0),
            &cmd, arrow_size, arrow_gap, GOLD);
    });
}
