use crate::theme::*;
use crate::widgets::*;
use crate::H2ACApp;
use eframe::egui::{self, Align2, Context, CornerRadius, Pos2, Rect, Sense, Stroke, Ui, Vec2};

pub fn render_topbar(app: &mut H2ACApp, ui: &mut Ui, rect: Rect, ctx: &Context, m: &UiMetrics) {
    let drag = ui.interact(rect, ui.id().with("drag"), Sense::drag());
    if drag.drag_started_by(egui::PointerButton::Primary) {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }

    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::ZERO, BG_PANEL);
    p.hline(
        rect.left()..=rect.right(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, LINE),
    );
    p.add(egui::Shape::convex_polygon(
        chamfer_points(Rect::from_min_size(rect.min, Vec2::new(6.0, 48.0)), 4.0),
        GOLD,
        Stroke::NONE,
    ));

    p.text(
        Pos2::new(rect.left() + 22.0, rect.top() + 13.0),
        Align2::LEFT_CENTER,
        "H2AC-RS",
        m.hud_b(20.0),
        GOLD,
    );
    p.text(
        Pos2::new(rect.left() + 22.0, rect.top() + 34.0),
        Align2::LEFT_CENTER,
        "SUPER DESTROYER TERMINAL",
        m.hud(9.0),
        TEXT_DIM,
    );

    let mut bar = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(16.0, 0.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    bar.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if glyph_button(ui, Glyph::Close, m.glyph_btn(), "关闭").clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if glyph_button(ui, Glyph::Minimize, m.glyph_btn(), "最小化").clicked() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
            if glyph_button(
                ui,
                Glyph::Compact,
                m.glyph_btn(),
                "紧凑模式（配装预设浮窗）",
            )
            .clicked()
            {
                app.toggle_compact_overlay(ctx);
            }
            if glyph_button(ui, Glyph::Gear, m.glyph_btn(), "按键设置").clicked() {
                app.open_settings();
            }
            if glyph_button(
                ui,
                if app.model.debug_mode {
                    Glyph::Keyboard
                } else {
                    Glyph::Search
                },
                m.glyph_btn(),
                if app.model.debug_mode {
                    "调试:开"
                } else {
                    "调试:关"
                },
            )
            .clicked()
            {
                app.model.debug_mode = !app.model.debug_mode;
                app.debug(if app.model.debug_mode {
                    "DBG ON"
                } else {
                    "DBG OFF"
                });
            }

            ui.add_space(10.0);

            let (resp, lp) = ui.allocate_painter(
                Vec2::new(m.listening_btn_w(), m.listening_btn_h()),
                Sense::click(),
            );
            let hovered = resp.hovered();
            if hovered {
                paint_chamfer(&lp, resp.rect.shrink(1.0), 5.0, BG_HOVER, Stroke::NONE);
            }
            let t = ui.ctx().input(|i| i.time);
            status_lamp(
                &lp,
                Pos2::new(resp.rect.left() + 16.0, resp.rect.center().y),
                4.5,
                app.model.listening,
                t,
            );
            lp.text(
                Pos2::new(resp.rect.left() + 28.0, resp.rect.center().y),
                Align2::LEFT_CENTER,
                if app.model.listening {
                    "监听中"
                } else {
                    "已静音"
                },
                m.hud(13.0),
                if app.model.listening { OK } else { DANGER },
            );
            let hk = app.model.config.listen_hotkey.clone();
            resp.clone().on_hover_text(if hk.is_empty() {
                "点击开关监听 · 右键绑定全局快捷键".to_string()
            } else {
                format!(
                    "点击开关监听 · 按 {} 快速开关 · 右键改绑",
                    hk.to_uppercase()
                )
            });
            if resp.secondary_clicked() {
                app.capture.capturing = None;
                app.capture.settings_capture = None;
                app.capture.capturing_listen = true;
                app.capture.captured.clear();
            }
            if resp.clicked() {
                app.toggle_listening();
            }
            if app.model.listening {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(66));
            }

            // ─── Loadout Sync 状态胶囊（点击 = 自动装配 / 取消）───
            ui.add_space(6.0);
            let snap = app.model.loadout_sync.snapshot();
            let (text, color) = match &snap.status {
                crate::loadout_sync::state::SyncStatus::Idle => ("自动装配".to_string(), TEXT_DIM),
                crate::loadout_sync::state::SyncStatus::Running => {
                    (format!("装配中 {}/{}", snap.step.max(1), snap.total), GOLD)
                }
                crate::loadout_sync::state::SyncStatus::Succeeded => ("装配完成".to_string(), OK),
                crate::loadout_sync::state::SyncStatus::Failed { .. } => {
                    ("装配失败".to_string(), DANGER)
                }
                crate::loadout_sync::state::SyncStatus::Cancelled => {
                    ("已取消".to_string(), TEXT_SUB)
                }
            };
            let (resp, sp) = ui.allocate_painter(
                Vec2::new(m.sync_btn_w(), m.listening_btn_h()),
                Sense::click(),
            );
            let hovered = resp.hovered();
            if hovered {
                paint_chamfer(&sp, resp.rect.shrink(1.0), 5.0, BG_HOVER, Stroke::NONE);
            }
            sp.text(
                resp.rect.center(),
                Align2::CENTER_CENTER,
                &text,
                m.hud(12.0),
                color,
            );
            let tip = match &snap.status {
                crate::loadout_sync::state::SyncStatus::Failed { code, message } => {
                    format!("{code}: {message}")
                }
                crate::loadout_sync::state::SyncStatus::Running => {
                    format!("{} · {}（再次触发即取消）", snap.stage, snap.target)
                }
                _ => {
                    let hk = app.model.config.loadout_sync_hotkey.clone();
                    if hk.is_empty() {
                        "把 Slot06~10 同步到游戏 Loadout".to_string()
                    } else {
                        format!(
                            "把 Slot06~10 同步到游戏 Loadout · 快捷键 {}",
                            hk.to_uppercase()
                        )
                    }
                }
            };
            resp.clone().on_hover_text(tip);
            if resp.clicked() {
                app.toggle_loadout_sync();
            }
        });
    });
}
