use eframe::egui::{self, Context, Key, Vec2};
use crate::H2ACApp;
use crate::theme::*;
use crate::widgets::*;
use crate::stratagems::{STRATAGEMS, StratagemRef};
use crate::LogKind;

pub fn render_library_context_menu(app: &mut H2ACApp, ctx: &Context, m: &UiMetrics) {
    let Some(ref lib_ctx) = app.library_context.clone() else { return };

    let num_rows = 1
        + if app.model.armed.is_some() { 1 } else { 0 }
        + 1
        + if lib_ctx.is_plugin { 1 } else { 0 };
    let size = Vec2::new(m.modal_lib_ctx_w(), num_rows as f32 * m.modal_row_h() + m.modal_pad_y());

    let mut close = false;
    let area = egui::Area::new(egui::Id::new("lib_ctx_menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(lib_ctx.pos)
        .show(ctx, |ui| {
            hud_panel(ui, size, m, GOLD_DIM, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;

                if app.model.armed.is_some() {
                    if hud_button(ui, "装入槽位", Vec2::new(ui.available_width(), 26.0), m, GOLD, false).clicked() {
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
                    .width(ui.available_width())
                    .selected_text(egui::RichText::new(super::cat_label(&cat_sel)).font(m.hud(12.0)).color(category_color(&cat_sel)))
                    .show_ui(ui, |ui| {
                        for cat in &app.lib_categories() {
                            if ui.selectable_label(false, egui::RichText::new(super::cat_label(cat)).font(m.hud(12.0))).clicked() {
                                cat_sel = cat.clone();
                                app.set_category_override(&lib_ctx.name, cat);
                                app.log(LogKind::Info, format!("分类: {} → {}", lib_ctx.name, cat));
                                close = true;
                            }
                        }
                    });

                if hud_button(ui, "设 置", Vec2::new(ui.available_width(), 26.0), m, CAT_EQUIP, false).clicked() {
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
                    if hud_button(ui, "删 除", Vec2::new(ui.available_width(), 26.0), m, DANGER, true).clicked() {
                        close = true;
                        app.delete_plugin_stratagem(&lib_ctx.name);
                    }
                }
            });
        });

    let rect = area.response.rect;
    if ctx.input(|i| i.pointer.primary_pressed()) {
        if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
            if !rect.contains(pos) && !ctx.memory(|mem| mem.any_popup_open()) {
                close = true;
            }
        }
    }
    if close || ctx.input(|i| i.key_pressed(Key::Escape)) {
        app.library_context = None;
    }
}
