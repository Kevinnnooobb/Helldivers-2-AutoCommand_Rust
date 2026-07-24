use eframe::egui::{self, Align2, Color32, CornerRadius, CursorIcon, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use crate::stratagems::{OwnedStratagem, StratagemRef};
use crate::theme::*;
use crate::widgets::*;
use crate::H2ACApp;
use crate::LibraryContext;
use crate::ui::common::cat_short;

pub fn render_library(app: &mut H2ACApp, ui: &mut Ui, rect: Rect, m: &UiMetrics) {
    let mut region = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    hud_panel(&mut region, rect.size(), m, LINE, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("战备库").font(m.hud_b(16.0)).color(TEXT));
            ui.label(egui::RichText::new("LIBRARY").font(m.hud(10.0)).color(TEXT_DIM));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if glyph_button(ui, Glyph::Save, 24.0, "创建/管理插件").clicked() {
                    app.creator.open = !app.creator.open;
                }
                ui.add_space(4.0);
                let fetching = app.wiki.fetch_rx.is_some();
                if glyph_button(ui, if fetching { Glyph::Restore } else { Glyph::Search }, 24.0,
                    if fetching { "正在拉取…" } else if app.wiki.cache_exists { "刷新 Wiki 数据" } else { "从 Wiki 拉取战备数据" }
                ).clicked() && !fetching {
                    app.creator.open = true;
                    app.creator.tab = crate::state::CreatorTab::Fetch;
                    app.start_wiki_fetch();
                }
                ui.add_space(4.0);
                let search = egui::TextEdit::singleline(&mut app.library.lib_search)
                    .hint_text(egui::RichText::new("搜索…").font(m.hud(12.0)).color(TEXT_DIM))
                    .font(m.hud(13.0))
                    .desired_width(120.0)
                    .frame(true);
                let sresp = ui.add(search);
                paint_glyph(ui.painter(), Rect::from_center_size(
                    Pos2::new(sresp.rect.right() - 14.0, sresp.rect.center().y),
                    Vec2::splat(12.0),
                ), Glyph::Search, TEXT_DIM);
            });
        });
        ui.add_space(8.0);

        let body = ui.available_rect_before_wrap();
        let rail = Rect::from_min_size(body.min, Vec2::new(m.cat_rail_w(), body.height()));
        let list = Rect::from_min_max(
            Pos2::new(rail.right() + 8.0, body.top()),
            body.max,
        );

        let cats = app.lib_categories();
        for (i, cat) in cats.iter().enumerate() {
            let row = Rect::from_min_size(
                Pos2::new(rail.left(), rail.top() + i as f32 * m.cat_row_spacing()),
                Vec2::new(m.cat_row_w(), m.cat_row_h()),
            );
            let resp = ui.interact(row, ui.id().with(("cat", i)), Sense::click());
            let p = ui.painter_at(row);
            let sel = app.library.lib_search.is_empty() && app.library.lib_category == *cat;
            let accent = category_color(cat);

            if resp.hovered() || sel {
                paint_chamfer(&p, row, 5.0, if sel { BG_HOVER } else { BG_RAISED }, Stroke::NONE);
            }

            p.rect_filled(
                Rect::from_min_size(row.min, Vec2::new(m.cat_accent_bar_w(), m.cat_row_h())),
                CornerRadius::ZERO,
                if sel { accent } else { accent.gamma_multiply(0.35) },
            );
            p.text(
                Pos2::new(row.left() + 9.0, row.center().y),
                Align2::LEFT_CENTER,
                cat_short(cat),
                m.hud(13.0),
                if sel { TEXT } else { TEXT_SUB },
            );
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            if resp.clicked() {
                app.library.lib_category = cat.to_string();
                app.library.lib_search.clear();
            }
        }

        let items: Vec<OwnedStratagem> = if app.library.lib_search.is_empty() {
            app.lib_by_category_owned(&app.library.lib_category)
        } else {
            app.lib_search_owned(&app.library.lib_search)
        };
        let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(list));
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(&mut list_ui, |ui| {
                for s in &items {
                    render_library_row(app, ui, &s.as_ref(), m);
                }
                if app.library.lib_search.is_empty() {
                    ui.label(
                        egui::RichText::new("— 列表结束 —")
                            .font(m.hud(10.0))
                            .color(TEXT_DIM),
                    );
                }
            });
    });
}

fn render_library_row(app: &mut H2ACApp, ui: &mut Ui, s: &StratagemRef, m: &UiMetrics) {
    let width = ui.available_rect_before_wrap().width();
    let (resp, p) = ui.allocate_painter(Vec2::new(width, m.lib_row_h()), Sense::click());
    let rect = resp.rect;
    let acc = app.effective_category(s.name(), s.category());
    let _accent = category_color(&acc);

    let in_armed = app
        .model.armed
        .and_then(|a| app.slot_name(a))
        .map_or(false, |n| n == s.name());

    if resp.hovered() {
        paint_chamfer(&p, rect, 5.0, BG_HOVER, Stroke::NONE);
    } else if in_armed {
        paint_chamfer(&p, rect, 5.0, GOLD_GHOST, Stroke::new(1.0, GOLD_DIM));
    }

    let icon_rect = Rect::from_center_size(
        Pos2::new(rect.left() + m.lib_row_icon_x(), rect.center().y),
        Vec2::splat(m.lib_icon_size()),
    );
    if let Some(tex) = app.model.icons.get(s.icon()) {
        p.image(
            tex.id(),
            icon_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    }

    let label = if app.library.lib_search.is_empty() {
        s.name().to_string()
    } else {
        format!("{} · {}", s.name(), cat_short(s.category()))
    };
    let max_w = width - 160.0;
    let font = m.fit_font(&p, &label, max_w, &[13.0, 11.0], false);
    let display: std::borrow::Cow<'_, str> = {
        let mut s = String::new();
        let mut fits = true;
        for ch in label.chars() {
            let test = format!("{s}{ch}…");
            if p.layout_no_wrap(test.clone(), font.clone(), TEXT).size().x <= max_w {
                s.push(ch);
            } else {
                fits = false;
                break;
            }
        }
        if fits && !label.is_empty() {
            std::borrow::Cow::Owned(label)
        } else if s.is_empty() {
            std::borrow::Cow::Borrowed("…")
        } else {
            std::borrow::Cow::Owned(format!("{s}…"))
        }
    };
    p.text(
        Pos2::new(rect.left() + m.lib_row_text_x(), rect.center().y),
        Align2::LEFT_CENTER,
        &display,
        font,
        if in_armed { GOLD } else { TEXT },
    );

    let cmd = s.command();
    let cmd_refs: Vec<&str> = cmd.iter().map(|c| *c).collect();
    let arrow_size = m.lib_arrow_size();
    let arrow_gap = m.lib_arrow_gap();
    let aw = arrow_strip_w(&cmd_refs, arrow_size, arrow_gap);
    arrow_strip(
        &p,
        Pos2::new(rect.right() - 10.0 - aw, rect.center().y - 4.0),
        &cmd_refs,
        arrow_size,
        arrow_gap,
        TEXT_DIM,
    );

    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        resp.clone().on_hover_text(format!("{}\n{}", s.model(), s.description()));
    }
    if resp.clicked() {
        app.assign_stratagem_ref(s);
    }
    if resp.secondary_clicked() {
        app.library_context = Some(LibraryContext {
            name: s.name().to_string(),
            category: s.category().to_string(),
            icon_key: s.icon().to_string(),
            command: s.command().iter().map(|c| c.to_string()).collect(),
            description: s.description().to_string(),
            is_plugin: matches!(s, StratagemRef::Plugin(_)),
            pos: resp.rect.left_bottom(),
        });
    }
}
