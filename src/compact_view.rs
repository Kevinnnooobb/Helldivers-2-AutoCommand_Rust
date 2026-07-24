// 紧凑模式 — 横向迷你条（置顶，点击执行）
use eframe::egui::{
    self, Align2, Color32, Context, CursorIcon, Pos2, Rect, Sense, Stroke, Ui, Vec2,
};
use crate::config;
use crate::theme::*;
use crate::widgets::*;

use crate::H2ACApp;

impl H2ACApp {
    pub fn show_compact(&mut self, ctx: &Context) {
        let m = self.model.metrics;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG_DEEP).inner_margin(0.0))
            .show(ctx, |ui| {
                let full = ui.available_rect_before_wrap();
                ui.advance_cursor_after_rect(full);

                let lw = m.compact_w();
                let lh = m.compact_h();
                let ox = ((full.width() - lw) / 2.0).max(0.0);
                let oy = ((full.height() - lh) / 2.0).max(0.0);
                let cr = Rect::from_min_size(Pos2::new(full.left() + ox, full.top() + oy), Vec2::new(lw, lh));

                let drag = ui.interact(cr, ui.id().with("cdrag"), Sense::drag());
                if drag.drag_started_by(egui::PointerButton::Primary) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }

                let p = ui.painter();
                paint_chamfer(p, cr, m.chamfer(), BG_PANEL, Stroke::new(1.0, LINE));
                corner_brackets(p, cr.shrink(2.0), 6.0, GOLD_DIM);

                let inner_margin = m.compact_inner_margin();
                let mut bar = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(cr.shrink2(Vec2::new(inner_margin, 0.0)))
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );

                let gap = m.compact_gap();
                bar.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for idx in 0..config::SLOT_COUNT {
                        self.render_compact_tile(ui, idx, &m);
                        if idx < 9 {
                            ui.add_space(gap);
                        }
                    }
                    ui.add_space(10.0);
                    self.render_compact_controls(ui, ctx, &m);
                });
            });

        if self.model.listening {
            ctx.request_repaint_after(std::time::Duration::from_millis(66));
        }

         let _grip = egui::Area::new(egui::Id::new("compact_resize_grip"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::RIGHT_BOTTOM, [0.0, 0.0])
            .show(ctx, |ui| {
                let (resp, _) = ui.allocate_painter(Vec2::new(16.0, 16.0), Sense::drag());
                if resp.hovered() { ctx.set_cursor_icon(CursorIcon::ResizeNwSe); }
                let delta = resp.drag_delta();
                if delta != Vec2::ZERO {
                    let new = ctx.screen_rect().size() + delta;
                    let min_w = 277.0;
                    let min_h = 28.0;
                    if new.x >= min_w && new.y >= min_h {
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(
                            egui::Vec2::new(new.x.round(), new.y.round()),
                        ));
                    }
                }
            });
    }

    fn render_compact_tile(&mut self, ui: &mut Ui, idx: usize, m: &UiMetrics) {
        let tile = m.compact_tile();
        let (resp, p) = ui.allocate_painter(Vec2::splat(tile), Sense::click());
        let rect = resp.rect;
        let filled = self.slot_filled(idx);

        let border = if resp.hovered() {
            Stroke::new(1.0, GOLD)
        } else {
            Stroke::new(1.0, LINE)
        };
        paint_chamfer(&p, rect, 6.0, if filled { BG_RAISED } else { BG_PANEL }, border);

        if filled {
            let icon_key = self.slot_icon(idx).unwrap_or("");
            if let Some(tex) = self.model.icons.get(icon_key) {
                p.image(
                    tex.id(),
                    rect.shrink(6.0),
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            } else {
                p.text(rect.center(), Align2::CENTER_CENTER, "?", m.hud(12.0), TEXT_DIM);
            }
            p.circle_filled(
                Pos2::new(rect.right() - 5.0, rect.bottom() - 5.0),
                2.0,
                category_color(&self.slot_category(idx).unwrap_or_default()),
            );
        } else {
            p.text(
                rect.center(),
                Align2::CENTER_CENTER,
                format!("{}", idx + 1),
                m.hud(11.0),
                TEXT_DIM,
            );
        }

        if let Some(hk) = self.model.config.slot_hotkeys.get(&idx.to_string()) {
            p.text(
                Pos2::new(rect.left() + 3.0, rect.top() + 1.0),
                Align2::LEFT_TOP,
                hk.to_uppercase(),
                m.hud_b(8.0),
                OK,
            );
        }

        let now = ui.ctx().input(|i| i.time);
        if let Some(&t0) = self.model.flash.get(&idx) {
            let k = (now - t0) as f32 / 0.7;
            if k < 1.0 {
                let a = ((1.0 - k) * 160.0) as u8;
                p.add(egui::Shape::convex_polygon(
                    chamfer_points(rect, 6.0),
                    Color32::from_rgba_unmultiplied(0xF5, 0xC8, 0x42, a / 3),
                    Stroke::NONE,
                ));
                ui.ctx().request_repaint();
            }
        }

        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            if filled {
                let name = self.slot_name(idx).unwrap_or_default();
                let cmd = self.slot_command(idx);
                resp.clone().on_hover_text(format!("{} {}", name, crate::stratagems::command_to_string(&cmd)));
            }
        }
        if resp.clicked() {
            self.execute_slot(idx);
        }
    }

    fn render_compact_controls(&mut self, ui: &mut Ui, ctx: &Context, m: &UiMetrics) {
        let ctrl = m.compact_ctrl();
        let (resp, p) = ui.allocate_painter(Vec2::splat(ctrl), Sense::click());
        let t = ui.ctx().input(|i| i.time);
        if resp.hovered() {
            paint_chamfer(&p, resp.rect.shrink(1.0), 4.0, BG_HOVER, Stroke::NONE);
        }
        status_lamp(&p, resp.rect.center(), 4.5, self.model.listening, t);
        resp.clone().on_hover_text(if self.model.listening { "监听中 — 点击静音" } else { "已静音 — 点击开启" });
        if resp.clicked() {
            self.toggle_listening();
        }

        if glyph_button(ui, Glyph::Restore, ctrl, "返回主界面").clicked() {
            self.set_compact(ctx, false);
        }
    }
}
