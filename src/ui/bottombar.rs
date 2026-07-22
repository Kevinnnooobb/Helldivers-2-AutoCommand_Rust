use eframe::egui::{self, Align2, CornerRadius, Pos2, Rect, Stroke, Ui, Vec2};
use crate::config;
use crate::theme::*;
use crate::widgets::*;
use crate::H2ACApp;
use crate::LogKind;

pub fn render_bottombar(app: &mut H2ACApp, ui: &mut Ui, rect: Rect) {
    let p = ui.painter().clone();
    p.rect_filled(rect, CornerRadius::ZERO, BG_PANEL);
    p.hline(rect.left()..=rect.right(), rect.top() + 0.5, Stroke::new(1.0, LINE));

    let mut bar = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(16.0, 4.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );

    let logs: Vec<_> = app.logs.iter().rev().take(3).collect();
    let mut y = rect.bottom() - 8.0;
    for (i, e) in logs.iter().enumerate() {
        let color = match e.kind {
            LogKind::Exec => GOLD.gamma_multiply(1.0 - i as f32 * 0.35),
            LogKind::Warn => DANGER.gamma_multiply(1.0 - i as f32 * 0.35),
            LogKind::Info => TEXT_SUB.gamma_multiply(1.0 - i as f32 * 0.35),
        };
        p.text(
            Pos2::new(rect.left() + 16.0, y),
            Align2::LEFT_BOTTOM,
            format!("> {}  {}", e.time, e.text),
            hud(11.0),
            color,
        );
        y -= 15.0;
    }

    bar.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if glyph_button(ui, Glyph::Trash, 26.0, "删除当前 Profile").clicked()
            && !app.model.current_profile.is_empty()
        {
            config::delete_profile(&app.model.current_profile);
            app.refresh_profiles();
            app.log(LogKind::Warn, format!("Profile 已删除: {}", app.model.current_profile));
            app.model.current_profile.clear();
        }
        if glyph_button(ui, Glyph::Save, 26.0, "保存为 Profile").clicked()
            && !app.model.save_profile_name.is_empty()
        {
            let ps: std::collections::HashMap<String, _> = app.model.plugin_slots.iter().map(|(k,v)| (k.to_string(), v.clone())).collect();
            config::save_profile(
                &app.model.save_profile_name,
                &app.model.slots,
                &app.model.config.slot_hotkeys,
                &ps,
            );
            app.model.current_profile = app.model.save_profile_name.clone();
            app.model.config.last_profile = app.model.current_profile.clone();
            config::save_config(&app.model.config);
            app.refresh_profiles();
            app.log(LogKind::Info, format!("已保存 Profile: {}", app.model.save_profile_name));
        }
        // ▶ 加载按钮
        if glyph_button(ui, Glyph::Play, 26.0, "加载所选 Profile").clicked()
            && !app.model.current_profile.is_empty()
        {
            app.load_profile_data(&app.model.current_profile.clone());
        }
        ui.add(
            egui::TextEdit::singleline(&mut app.model.save_profile_name)
                .hint_text(egui::RichText::new("新Profile名").font(hud(12.0)).color(TEXT_DIM))
                .font(hud(12.0))
                .desired_width(90.0),
        );

        let mut sel = app.model.current_profile.clone();
        egui::ComboBox::from_id_salt("profile_combo")
            .width(120.0)
            .selected_text(
                egui::RichText::new(if sel.is_empty() { "选择 Profile" } else { &sel })
                    .font(hud(12.0)),
            )
            .show_ui(ui, |ui| {
                for n in &app.model.profile_names.clone() {
                    ui.selectable_value(&mut sel, n.clone(), n.as_str());
                }
            });
        if sel != app.model.current_profile && !sel.is_empty() {
            app.model.current_profile = sel;
        }
        ui.label(egui::RichText::new("PROFILE").font(hud(10.0)).color(TEXT_DIM));
    });
}
