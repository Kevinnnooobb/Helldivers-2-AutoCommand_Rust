// 紧凑模式 — 横向迷你条（置顶，点击执行）
use eframe::egui::{
    self, Align2, Color32, Context, CursorIcon, Pos2, Rect, Sense, Stroke, Ui, Vec2,
};
use crate::config;
use crate::stratagems::STRATAGEMS;
use crate::theme::*;
use crate::widgets::*;

use crate::H2ACApp;

const TILE: f32 = 40.0;
const GAP: f32 = 6.0;
const CTRL: f32 = 30.0; // 右侧控制区单元宽

/// 紧凑条窗口宽度
pub fn compact_width() -> f32 {
    12.0 * 2.0 + TILE * 10.0 + GAP * 9.0 + 10.0 + CTRL * 2.0 + 6.0
}

impl H2ACApp {
    pub fn show_compact(&mut self, ctx: &Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG_DEEP).inner_margin(0.0))
            .show(ctx, |ui| {
                let full = ui.available_rect_before_wrap();
                ui.advance_cursor_after_rect(full);

                // 整体可拖拽（控件命中优先）
                let drag = ui.interact(full, ui.id().with("cdrag"), Sense::drag());
                if drag.drag_started_by(egui::PointerButton::Primary) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }

                let p = ui.painter();
                paint_chamfer(p, full, 8.0, BG_PANEL, Stroke::new(1.0, LINE));
                corner_brackets(p, full.shrink(2.0), 6.0, GOLD_DIM);

                let mut bar = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(full.shrink2(Vec2::new(12.0, 0.0)))
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );

                bar.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for idx in 0..config::SLOT_COUNT {
                        self.render_compact_tile(ui, idx);
                        if idx < 9 {
                            ui.add_space(GAP);
                        }
                    }
                    ui.add_space(10.0);
                    self.render_compact_controls(ui, ctx);
                });
            });

        // 监听脉冲动画
        if self.listening {
            ctx.request_repaint_after(std::time::Duration::from_millis(66));
        }
    }

    fn render_compact_tile(&mut self, ui: &mut Ui, idx: usize) {
        let (resp, p) = ui.allocate_painter(Vec2::splat(TILE), Sense::click());
        let rect = resp.rect;
        let filled = self.slots[idx].is_some();

        let border = if resp.hovered() {
            Stroke::new(1.0, GOLD)
        } else {
            Stroke::new(1.0, LINE)
        };
        paint_chamfer(&p, rect, 6.0, if filled { BG_RAISED } else { BG_PANEL }, border);

        if let Some(si) = self.slots[idx] {
            let s = &STRATAGEMS[si];
            if let Some(tex) = self.icons.get(s.icon) {
                p.image(
                    tex.id(),
                    rect.shrink(6.0),
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            } else {
                p.text(rect.center(), Align2::CENTER_CENTER, "?", hud(12.0), TEXT_DIM);
            }
            // 分类色点
            p.circle_filled(
                Pos2::new(rect.right() - 5.0, rect.bottom() - 5.0),
                2.0,
                category_color(s.category),
            );
        } else {
            p.text(
                rect.center(),
                Align2::CENTER_CENTER,
                format!("{}", idx + 1),
                hud(11.0),
                TEXT_DIM,
            );
        }

        // 热键角标
        if let Some(hk) = self.config.slot_hotkeys.get(&idx.to_string()) {
            p.text(
                Pos2::new(rect.left() + 3.0, rect.top() + 1.0),
                Align2::LEFT_TOP,
                hk.to_uppercase(),
                hud_b(8.0),
                OK,
            );
        }

        // 执行闪光
        let now = ui.ctx().input(|i| i.time);
        if let Some(&t0) = self.flash.get(&idx) {
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
            if let Some(si) = self.slots[idx] {
                let s = &STRATAGEMS[si];
                resp.clone().on_hover_text(format!("{} {}", s.name, crate::stratagems::command_to_string(&s.command)));
            }
        }
        if resp.clicked() {
            self.execute_slot(idx);
        }
    }

    fn render_compact_controls(&mut self, ui: &mut Ui, ctx: &Context) {
        // 监听状态灯
        let (resp, p) = ui.allocate_painter(Vec2::splat(CTRL), Sense::click());
        let t = ui.ctx().input(|i| i.time);
        if resp.hovered() {
            paint_chamfer(&p, resp.rect.shrink(1.0), 4.0, BG_HOVER, Stroke::NONE);
        }
        status_lamp(&p, resp.rect.center(), 4.5, self.listening, t);
        resp.clone().on_hover_text(if self.listening { "监听中 — 点击静音" } else { "已静音 — 点击开启" });
        if resp.clicked() {
            self.toggle_listening();
        }

        // 还原主界面
        if glyph_button(ui, Glyph::Restore, CTRL, "返回主界面").clicked() {
            self.set_compact(ctx, false);
        }
    }
}
