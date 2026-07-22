// 主界面 — 顶栏 / 战备网格 / 详情条 / 战备库 / 日志底栏
use eframe::egui::{
    self, Align2, Color32, Context, CornerRadius, CursorIcon, Key, Painter, Pos2, Rect, Sense,
    Stroke, Ui, Vec2,
};
use crate::stratagems::{get_by_category, search, Stratagem, STRATAGEMS};
use crate::theme::*;
use crate::widgets::*;
use crate::{config, stratagems};

use crate::{H2ACApp, LogKind};

const TILE_W: f32 = 124.0;
const TILE_H: f32 = 150.0;
const TILE_GAP: f32 = 10.0;

/// 分类短标签（终端风格）
fn cat_short(cat: &str) -> &'static str {
    match cat {
        stratagems::CAT_MISSION => "任务",
        stratagems::CAT_ORBITAL => "轨道",
        stratagems::CAT_EAGLE => "飞鹰",
        stratagems::CAT_SUPPORT => "武器",
        stratagems::CAT_SENTRIES => "哨戒",
        stratagems::CAT_MINES => "雷盾",
        stratagems::CAT_BACKPACKS => "背包",
        stratagems::CAT_VEHICLES => "载具",
        _ => "??",
    }
}

/// 宽度自适应字号：超宽则降级
fn fit_font(p: &Painter, text: &str, max_w: f32, sizes: &[f32], bold: bool) -> egui::FontId {
    for &s in sizes {
        let f = if bold { hud_b(s) } else { hud(s) };
        if p.layout_no_wrap(text.to_string(), f.clone(), TEXT).size().x <= max_w {
            return f;
        }
    }
    if bold { hud_b(*sizes.last().unwrap()) } else { hud(*sizes.last().unwrap()) }
}

impl H2ACApp {
    pub fn show_main(&mut self, ctx: &Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG_DEEP).inner_margin(0.0))
            .show(ctx, |ui| {
                let full = ui.available_rect_before_wrap();
                ui.advance_cursor_after_rect(full);
                scanlines(ui.painter(), full);

                let top = Rect::from_min_size(full.min, Vec2::new(full.width(), 48.0));
                let bottom = Rect::from_min_max(
                    Pos2::new(full.left(), full.bottom() - 52.0),
                    full.max,
                );
                let content = Rect::from_min_max(
                    Pos2::new(full.left(), top.bottom()),
                    Pos2::new(full.right(), bottom.top()),
                );

                self.render_topbar(ui, top, ctx);
                self.render_bottombar(ui, bottom);

                // 内容区：左 664 + 间隔 8 + 右库
                let inner = content.shrink2(Vec2::new(12.0, 8.0));
                let left = Rect::from_min_size(inner.min, Vec2::new(664.0, inner.height()));
                let right = Rect::from_min_max(
                    Pos2::new(left.right() + 8.0, inner.top()),
                    inner.max,
                );

                self.render_left_column(ui, left);
                self.render_library(ui, right);
            });

        self.render_context_menu(ctx);
        self.render_capture_modal(ctx);
        self.render_settings_modal(ctx);

        // ESC：解除待命
        if ctx.input(|i| i.key_pressed(Key::Escape)) && self.armed.is_some() {
            self.armed = None;
        }
    }

    // ─── 顶栏（自定义标题栏） ───

    fn render_topbar(&mut self, ui: &mut Ui, rect: Rect, ctx: &Context) {
        // 拖拽区先注册，让按钮命中优先
        let drag = ui.interact(rect, ui.id().with("drag"), Sense::drag());
        if drag.drag_started_by(egui::PointerButton::Primary) {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }

        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::ZERO, BG_PANEL);
        p.hline(rect.left()..=rect.right(), rect.bottom() - 0.5, Stroke::new(1.0, LINE));
        // 金色装饰角
        p.add(egui::Shape::convex_polygon(
            chamfer_points(Rect::from_min_size(rect.min, Vec2::new(6.0, 48.0)), 4.0),
            GOLD,
            Stroke::NONE,
        ));

        // 标题文字直接绘制（避免嵌套布局溢出）
        p.text(
            Pos2::new(rect.left() + 22.0, rect.top() + 13.0),
            Align2::LEFT_CENTER,
            "H2AC-RS",
            hud_b(20.0),
            GOLD,
        );
        p.text(
            Pos2::new(rect.left() + 22.0, rect.top() + 34.0),
            Align2::LEFT_CENTER,
            "SUPER DESTROYER TERMINAL",
            hud(9.0),
            TEXT_DIM,
        );

        let mut bar = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(Vec2::new(16.0, 0.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        bar.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if glyph_button(ui, Glyph::Close, 30.0, "关闭").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if glyph_button(ui, Glyph::Minimize, 30.0, "最小化").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
                if glyph_button(ui, Glyph::Compact, 30.0, "紧凑模式").clicked() {
                    self.set_compact(ctx, true);
                }
                if glyph_button(ui, Glyph::Gear, 30.0, "按键设置").clicked() {
                    self.open_settings();
                }

                ui.add_space(10.0);

                // 监听开关：状态灯 + 文字
                let (resp, lp) = ui.allocate_painter(Vec2::new(110.0, 32.0), Sense::click());
                let hovered = resp.hovered();
                if hovered {
                    paint_chamfer(&lp, resp.rect.shrink(1.0), 5.0, BG_HOVER, Stroke::NONE);
                }
                let t = ui.ctx().input(|i| i.time);
                status_lamp(&lp, Pos2::new(resp.rect.left() + 16.0, resp.rect.center().y), 4.5, self.listening, t);
                lp.text(
                    Pos2::new(resp.rect.left() + 28.0, resp.rect.center().y),
                    Align2::LEFT_CENTER,
                    if self.listening { "监听中" } else { "已静音" },
                    hud(13.0),
                    if self.listening { OK } else { DANGER },
                );
                if resp.clicked() {
                    self.toggle_listening();
                }
                if self.listening {
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(66));
                }
            });
        });
    }

    // ─── 左列：网格头 + 槽位网格 + 详情条 ───

    fn render_left_column(&mut self, ui: &mut Ui, rect: Rect) {
        // 网格头
        let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 28.0));
        let mut h = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(header)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        h.horizontal(|ui| {
            ui.label(egui::RichText::new("战备配置").font(hud_b(16.0)).color(TEXT));
            ui.label(egui::RichText::new("LOADOUT").font(hud(11.0)).color(TEXT_DIM));
            let hint = if self.armed.is_some() {
                "待命装填中 — 点击右侧战备库装入，ESC 取消"
            } else {
                "点选槽位待命 · 双击执行 · 右键快捷操作"
            };
            ui.label(egui::RichText::new(hint).font(hud(11.0)).color(if self.armed.is_some() { GOLD } else { TEXT_DIM }));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if hud_button(ui, "清空全部", Vec2::new(76.0, 24.0), DANGER, true).clicked() {
                    for i in 0..config::SLOT_COUNT {
                        self.clear_slot(i);
                    }
                    self.armed = None;
                    self.log(LogKind::Warn, "全部槽位已清空");
                }
            });
        });

        // 网格
        let grid_top = header.bottom() + 8.0;
        let grid_rect = Rect::from_min_size(
            Pos2::new(rect.left(), grid_top),
            Vec2::new(TILE_W * 5.0 + TILE_GAP * 4.0, TILE_H * 2.0 + TILE_GAP),
        );
        self.render_grid(ui, grid_rect);

        // 详情条
        let detail = Rect::from_min_max(
            Pos2::new(rect.left(), grid_rect.bottom() + 8.0),
            rect.max,
        );
        self.render_detail(ui, detail);
    }

    fn render_grid(&mut self, ui: &mut Ui, rect: Rect) {
        for idx in 0..config::SLOT_COUNT {
            let row = idx / 5;
            let col = idx % 5;
            let tile = Rect::from_min_size(
                Pos2::new(
                    rect.left() + col as f32 * (TILE_W + TILE_GAP),
                    rect.top() + row as f32 * (TILE_H + TILE_GAP),
                ),
                Vec2::new(TILE_W, TILE_H),
            );
            self.render_slot_tile(ui, tile, idx);
        }
    }

    fn render_slot_tile(&mut self, ui: &mut Ui, rect: Rect, idx: usize) {
        let resp = ui.interact(rect, ui.id().with(("slot", idx)), Sense::click());
        let p = ui.painter_at(rect);
        let filled = self.slots[idx].is_some();
        let armed = self.armed == Some(idx);
        let hovered = resp.hovered();

        // 底
        let bg = if filled { BG_RAISED } else { BG_PANEL };
        let border = if armed {
            Stroke::new(1.5, GOLD)
        } else if hovered {
            Stroke::new(1.0, GOLD_DIM)
        } else {
            Stroke::new(1.0, LINE)
        };
        paint_chamfer(&p, rect, 8.0, bg, border);
        if armed {
            corner_brackets(&p, rect.shrink(3.0), 8.0, GOLD);
            p.add(egui::Shape::convex_polygon(
                chamfer_points(rect.shrink(1.5), 8.0),
                GOLD_GHOST,
                Stroke::NONE,
            ));
        }

        // 槽位编号
        p.text(
            Pos2::new(rect.left() + 8.0, rect.top() + 6.0),
            Align2::LEFT_TOP,
            format!("{:02}", idx + 1),
            hud_b(10.0),
            TEXT_DIM,
        );

        // 执行闪光
        let now = ui.ctx().input(|i| i.time);
        if let Some(&t0) = self.flash.get(&idx) {
            let k = (now - t0) as f32 / 0.7;
            if k < 1.0 {
                let a = ((1.0 - k) * 140.0) as u8;
                p.add(egui::Shape::convex_polygon(
                    chamfer_points(rect, 8.0),
                    Color32::from_rgba_unmultiplied(0xF5, 0xC8, 0x42, a / 3),
                    Stroke::NONE,
                ));
                corner_brackets(&p, rect.shrink(3.0), 8.0, Color32::from_rgba_unmultiplied(0xF5, 0xC8, 0x42, a));
                ui.ctx().request_repaint();
            }
        }

        if let Some(si) = self.slots[idx] {
            let s = &STRATAGEMS[si];
            let accent = category_color(s.category);

            // 图标
            let icon_rect = Rect::from_center_size(
                Pos2::new(rect.center().x, rect.top() + 48.0),
                Vec2::splat(56.0),
            );
            if let Some(tex) = self.icons.get(s.icon) {
                p.image(
                    tex.id(),
                    icon_rect,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }

            // 名称
            let font = fit_font(&p, s.name, rect.width() - 14.0, &[13.0, 11.5, 10.0], false);
            p.text(
                Pos2::new(rect.center().x, rect.top() + 84.0),
                Align2::CENTER_TOP,
                s.name,
                font,
                TEXT,
            );

            // 指令箭头
            let aw = arrow_strip_w(&s.command, 10.0, 3.0);
            arrow_strip(
                &p,
                Pos2::new(rect.center().x - aw / 2.0, rect.top() + 112.0),
                &s.command,
                10.0,
                3.0,
                GOLD_MID,
            );

            // 底部分类色条
            let strip = Rect::from_min_max(
                Pos2::new(rect.left() + 10.0, rect.bottom() - 5.0),
                Pos2::new(rect.right() - 10.0, rect.bottom() - 2.0),
            );
            p.rect_filled(strip, CornerRadius::ZERO, accent);
        } else {
            p.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "EMPTY",
                hud(12.0),
                TEXT_DIM,
            );
        }

        // 热键角标
        if let Some(hk) = self.config.slot_hotkeys.get(&idx.to_string()) {
            let badge = Rect::from_min_size(
                Pos2::new(rect.right() - 26.0, rect.top() + 5.0),
                Vec2::new(21.0, 15.0),
            );
            paint_chamfer(&p, badge, 3.0, BG_DEEP, Stroke::new(1.0, OK.gamma_multiply(0.6)));
            let hf = fit_font(&p, &hk.to_uppercase(), 17.0, &[9.5, 8.0], true);
            p.text(badge.center(), Align2::CENTER_CENTER, hk.to_uppercase(), hf, OK);
        }

        // ─── 交互 ───
        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if resp.clicked() {
            self.detail_slot = Some(idx);
            self.armed = if self.armed == Some(idx) { None } else { Some(idx) };
            self.context = None;
        }
        if resp.double_clicked() && filled {
            self.execute_slot(idx);
        }
        if resp.secondary_clicked() {
            self.context = Some(crate::ContextState { slot: idx, pos: rect.min });
            self.armed = None;
        }
    }

    // ─── 选中槽位详情条 ───

    fn render_detail(&mut self, ui: &mut Ui, rect: Rect) {
        let mut region = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        hud_panel(&mut region, rect.size(), LINE, |ui| {
            let Some(idx) = self.detail_slot.filter(|&i| self.slots[i].is_some()) else {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        egui::RichText::new("— 点选槽位以查看详情 —")
                            .font(hud(13.0))
                            .color(TEXT_DIM),
                    );
                });
                return;
            };
            let s = &STRATAGEMS[self.slots[idx].unwrap()];
            let accent = category_color(s.category);

            let content = ui.available_rect_before_wrap();
            let h = content.height();

            let icon_area = Rect::from_min_size(content.min, Vec2::new(96.0, h));
            let text_area = Rect::from_min_size(
                Pos2::new(icon_area.right(), content.top()),
                Vec2::new(190.0, h),
            );
            let btn_area = Rect::from_min_size(
                Pos2::new(content.right() - 88.0, content.top()),
                Vec2::new(88.0, h),
            );
            let arrow_area = Rect::from_min_max(
                Pos2::new(text_area.right(), content.top()),
                Pos2::new(btn_area.left(), content.bottom()),
            );

            // ---- 图标 ----
            let ir = Rect::from_center_size(icon_area.center(), Vec2::splat(80.0));
            paint_chamfer(ui.painter(), ir, 8.0, BG_DEEP, Stroke::new(1.0, accent.gamma_multiply(0.5)));
            if let Some(tex) = self.icons.get(s.icon) {
                ui.painter().image(
                    tex.id(),
                    ir.shrink(10.0),
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }

            // ---- 文字列 ----
            let mut text_ui = ui.new_child(egui::UiBuilder::new().max_rect(text_area));
            text_ui.vertical(|ui| {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(s.name).font(hud_b(22.0)).color(TEXT));
                ui.label(
                    egui::RichText::new(format!("{} · {}", s.model, s.category))
                        .font(hud(12.0))
                        .color(accent),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(s.description)
                        .font(hud(12.0))
                        .color(TEXT_SUB),
                );
                let hk = self
                    .config
                    .slot_hotkeys
                    .get(&idx.to_string())
                    .map(|h| h.to_uppercase())
                    .unwrap_or_else(|| "未绑定".into());
                ui.label(
                    egui::RichText::new(format!("快捷键 {hk}"))
                        .font(hud(11.0))
                        .color(TEXT_DIM),
                );
            });

            // ---- 按钮列 ----
            let mut btn_ui = ui.new_child(egui::UiBuilder::new().max_rect(btn_area));
            btn_ui.vertical_centered(|ui| {
                ui.add_space(6.0);
                if hud_button(ui, "执 行", Vec2::new(88.0, 34.0), GOLD, false).clicked() {
                    self.execute_slot(idx);
                }
                ui.add_space(4.0);
                if hud_button(ui, "设热键", Vec2::new(88.0, 26.0), CAT_EQUIP, false).clicked() {
                    self.capturing = Some(idx);
                    self.captured.clear();
                }
                ui.add_space(4.0);
                if hud_button(ui, "清 除", Vec2::new(88.0, 26.0), DANGER, true).clicked() {
                    self.clear_slot(idx);
                    if self.armed == Some(idx) {
                        self.armed = None;
                    }
                    self.log(LogKind::Warn, format!("槽位 {} 已清除", idx + 1));
                }
            });

            // ---- 大箭头（居中于箭头区） ----
            let aw = arrow_strip_w(&s.command, 18.0, 6.0);
            let start = Pos2::new(
                arrow_area.center().x - aw / 2.0,
                arrow_area.center().y - 9.0,
            );
            arrow_strip(ui.painter(), start, &s.command, 18.0, 6.0, GOLD);
        });
    }

    // ─── 战备库 ───

    fn render_library(&mut self, ui: &mut Ui, rect: Rect) {
        let mut region = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        hud_panel(&mut region, rect.size(), LINE, |ui| {
            // 头部
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("战备库").font(hud_b(16.0)).color(TEXT));
                ui.label(egui::RichText::new("LIBRARY").font(hud(10.0)).color(TEXT_DIM));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let search = egui::TextEdit::singleline(&mut self.lib_search)
                        .hint_text(egui::RichText::new("搜索…").font(hud(12.0)).color(TEXT_DIM))
                        .font(hud(13.0))
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

            // 主体：分类栏 + 列表
            let body = ui.available_rect_before_wrap();
            let rail = Rect::from_min_size(body.min, Vec2::new(64.0, body.height()));
            let list = Rect::from_min_max(
                Pos2::new(rail.right() + 8.0, body.top()),
                body.max,
            );

            // 分类栏（绝对坐标 + ui.interact，与槽位同款可靠模式）
            let cats = self.categories.clone();
            for (i, cat) in cats.iter().enumerate() {
                let row = Rect::from_min_size(
                    Pos2::new(rail.left(), rail.top() + i as f32 * 38.0),
                    Vec2::new(60.0, 34.0),
                );
                let resp = ui.interact(row, ui.id().with(("cat", i)), Sense::click());
                let p = ui.painter_at(row);
                let sel = self.lib_search.is_empty() && self.lib_category == *cat;
                let accent = category_color(cat);
                if resp.hovered() || sel {
                    paint_chamfer(&p, row, 5.0, if sel { BG_HOVER } else { BG_RAISED }, Stroke::NONE);
                }
                // 色条
                p.rect_filled(
                    Rect::from_min_size(row.min, Vec2::new(3.0, 34.0)),
                    CornerRadius::ZERO,
                    if sel { accent } else { accent.gamma_multiply(0.35) },
                );
                p.text(
                    Pos2::new(row.left() + 9.0, row.center().y),
                    Align2::LEFT_CENTER,
                    cat_short(cat),
                    hud(13.0),
                    if sel { TEXT } else { TEXT_SUB },
                );
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                if resp.clicked() {
                    self.lib_category = cat.to_string();
                    self.lib_search.clear();
                }
            }

            // 战备列表
            let items: Vec<&'static Stratagem> = if self.lib_search.is_empty() {
                get_by_category(&self.lib_category)
            } else {
                search(&self.lib_search)
            };
            let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(list));
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(&mut list_ui, |ui| {
                    for s in items {
                        self.render_library_row(ui, s, list.width());
                    }
                    if self.lib_search.is_empty() {
                        ui.label(
                            egui::RichText::new("— 列表结束 —")
                                .font(hud(10.0))
                                .color(TEXT_DIM),
                        );
                    }
                });
        });
    }

    fn render_library_row(&mut self, ui: &mut Ui, s: &'static Stratagem, width: f32) {
        let (resp, p) = ui.allocate_painter(Vec2::new(width, 36.0), Sense::click());
        let rect = resp.rect;
        let _accent = category_color(s.category);

        // 是否已装入待命槽位
        let in_armed = self
            .armed
            .and_then(|a| self.slots[a])
            .map_or(false, |si| STRATAGEMS[si].name == s.name);

        if resp.hovered() {
            paint_chamfer(&p, rect, 5.0, BG_HOVER, Stroke::NONE);
        } else if in_armed {
            paint_chamfer(&p, rect, 5.0, GOLD_GHOST, Stroke::new(1.0, GOLD_DIM));
        }

        // 图标
        let icon_rect = Rect::from_center_size(
            Pos2::new(rect.left() + 20.0, rect.center().y),
            Vec2::splat(26.0),
        );
        if let Some(tex) = self.icons.get(s.icon) {
            p.image(
                tex.id(),
                icon_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        }

        // 名称（搜索态附带分类）
        let label = if self.lib_search.is_empty() {
            s.name.to_string()
        } else {
            format!("{} · {}", s.name, cat_short(s.category))
        };
        let font = fit_font(&p, &label, width - 160.0, &[13.0, 11.0], false);
        let max_w = width - 160.0;
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
            Pos2::new(rect.left() + 40.0, rect.center().y),
            Align2::LEFT_CENTER,
            &display,
            font,
            if in_armed { GOLD } else { TEXT },
        );

        // 右侧箭头
        let aw = arrow_strip_w(&s.command, 8.0, 2.0);
        arrow_strip(
            &p,
            Pos2::new(rect.right() - 10.0 - aw, rect.center().y - 4.0),
            &s.command,
            8.0,
            2.0,
            TEXT_DIM,
        );

        if resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            resp.clone().on_hover_text(format!("{}\n{}", s.model, s.description));
        }
        if resp.clicked() {
            self.assign_stratagem(s);
        }
    }

    // ─── 底部日志 + Profile 栏 ───

    fn render_bottombar(&mut self, ui: &mut Ui, rect: Rect) {
        let p = ui.painter().clone();
        p.rect_filled(rect, CornerRadius::ZERO, BG_PANEL);
        p.hline(rect.left()..=rect.right(), rect.top() + 0.5, Stroke::new(1.0, LINE));

        let mut bar = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(Vec2::new(16.0, 4.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );

        // 日志（最新 3 条，新→旧颜色渐暗）
        let logs: Vec<_> = self.logs.iter().rev().take(3).collect();
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

        // Profile 管理
        bar.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if glyph_button(ui, Glyph::Trash, 26.0, "删除当前 Profile").clicked()
                && !self.current_profile.is_empty()
            {
                config::delete_profile(&self.current_profile);
                self.refresh_profiles();
                self.log(LogKind::Warn, format!("Profile 已删除: {}", self.current_profile));
                self.current_profile.clear();
            }
            if glyph_button(ui, Glyph::Save, 26.0, "保存为 Profile").clicked()
                && !self.save_profile_name.is_empty()
            {
                config::save_profile(
                    &self.save_profile_name,
                    &self.slots,
                    &self.config.slot_hotkeys,
                );
                self.current_profile = self.save_profile_name.clone();
                self.config.last_profile = self.current_profile.clone();
                config::save_config(&self.config);
                self.refresh_profiles();
                self.log(LogKind::Info, format!("已保存 Profile: {}", self.save_profile_name));
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.save_profile_name)
                    .hint_text(egui::RichText::new("新Profile名").font(hud(12.0)).color(TEXT_DIM))
                    .font(hud(12.0))
                    .desired_width(90.0),
            );

            let mut sel = self.current_profile.clone();
            egui::ComboBox::from_id_salt("profile_combo")
                .width(120.0)
                .selected_text(
                    egui::RichText::new(if sel.is_empty() { "选择 Profile" } else { &sel })
                        .font(hud(12.0)),
                )
                .show_ui(ui, |ui| {
                    for n in &self.profile_names.clone() {
                        ui.selectable_value(&mut sel, n.clone(), n.as_str());
                    }
                });
            if sel != self.current_profile && !sel.is_empty() {
                if let Some(pr) = config::load_profile(&sel) {
                    self.slots = pr.loadout;
                    self.config.slot_hotkeys = pr.slot_hotkeys;
                    self.config.last_profile = sel.clone();
                    config::save_config(&self.config);
                    self.current_profile = sel.clone();
                    self.log(LogKind::Info, format!("已加载 Profile: {sel}"));
                }
            }
            ui.label(egui::RichText::new("PROFILE").font(hud(10.0)).color(TEXT_DIM));
        });
    }

    // ─── 右键菜单 ───

    fn render_context_menu(&mut self, ctx: &Context) {
        let Some(state) = self.context.clone() else { return };
        let slot = state.slot;
        let filled = self.slots[slot].is_some();
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
                            self.execute_slot(slot);
                            close = true;
                        }
                        if hud_button(ui, "清 除", Vec2::new(ui.available_width(), 26.0), DANGER, true).clicked() {
                            self.clear_slot(slot);
                            self.log(LogKind::Warn, format!("槽位 {} 已清除", slot + 1));
                            close = true;
                        }
                    }
                    if hud_button(ui, "设热键", Vec2::new(ui.available_width(), 26.0), CAT_EQUIP, false).clicked() {
                        self.capturing = Some(slot);
                        self.captured.clear();
                        close = true;
                    }
                });
            });

        // 点击外部关闭
        let rect = area.response.rect;
        if ctx.input(|i| i.pointer.primary_pressed()) {
            if let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) {
                if !rect.contains(pos) {
                    close = true;
                }
            }
        }
        if close || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.context = None;
        }
    }

    // ─── 热键捕获 ───

    fn render_capture_modal(&mut self, ctx: &Context) {
        let Some(slot) = self.capturing else { return };

        ctx.input(|i| {
            for ev in &i.events {
                if let egui::Event::Key { key, pressed: true, modifiers, .. } = ev {
                    if modifiers.ctrl || modifiers.alt || modifiers.mac_cmd {
                        return;
                    }
                    let n: &str = match key {
                        Key::F1 => "f1", Key::F2 => "f2", Key::F3 => "f3", Key::F4 => "f4",
                        Key::F5 => "f5", Key::F6 => "f6", Key::F7 => "f7", Key::F8 => "f8",
                        Key::F9 => "f9", Key::F10 => "f10", Key::F11 => "f11", Key::F12 => "f12",
                        Key::Space => "space", Key::Enter => "enter", Key::Tab => "tab",
                        Key::Backspace => "backspace",
                        Key::Num0 => "0", Key::Num1 => "1", Key::Num2 => "2", Key::Num3 => "3",
                        Key::Num4 => "4", Key::Num5 => "5", Key::Num6 => "6", Key::Num7 => "7",
                        Key::Num8 => "8", Key::Num9 => "9",
                        Key::A => "a", Key::B => "b", Key::C => "c", Key::D => "d", Key::E => "e",
                        Key::F => "f", Key::G => "g", Key::H => "h", Key::I => "i", Key::J => "j",
                        Key::K => "k", Key::L => "l", Key::M => "m", Key::N => "n", Key::O => "o",
                        Key::P => "p", Key::Q => "q", Key::R => "r", Key::S => "s", Key::T => "t",
                        Key::U => "u", Key::V => "v", Key::W => "w", Key::X => "x", Key::Y => "y",
                        Key::Z => "z",
                        _ => return,
                    };
                    self.captured = n.to_string();
                }
            }
        });

        egui::Area::new(egui::Id::new("capture"))
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                hud_panel(ui, Vec2::new(300.0, 150.0), GOLD_DIM, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new(format!("槽位 {} — 设置快捷键", slot + 1))
                                .font(hud_b(15.0))
                                .color(GOLD),
                        );
                        ui.add_space(10.0);
                        if self.captured.is_empty() {
                            paint_glyph(ui.painter(), Rect::from_center_size(
                                Pos2::new(ui.cursor().center().x, ui.cursor().min.y + 14.0),
                                Vec2::splat(28.0),
                            ), Glyph::Keyboard, TEXT_SUB);
                            ui.add_space(30.0);
                            ui.label(egui::RichText::new("按下目标按键…").font(hud(13.0)).color(TEXT_SUB));
                        } else {
                            ui.label(
                                egui::RichText::new(self.captured.to_uppercase())
                                    .font(hud_b(24.0))
                                    .color(OK),
                            );
                            ui.add_space(10.0);
                            ui.horizontal(|ui| {
                                if hud_button(ui, "确 认", Vec2::new(90.0, 28.0), GOLD, false).clicked() {
                                    self.config
                                        .slot_hotkeys
                                        .insert(slot.to_string(), self.captured.clone());
                                    config::save_config(&self.config);
                                    self.log(LogKind::Info, format!("槽位 {} 快捷键: {}", slot + 1, self.captured));
                                    self.capturing = None;
                                }
                                if hud_button(ui, "取 消", Vec2::new(90.0, 28.0), TEXT_SUB, false).clicked() {
                                    self.capturing = None;
                                }
                            });
                        }
                    });
                });
            });

        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.capturing = None;
        }
    }

    // ─── 设置 ───

    fn render_settings_modal(&mut self, ctx: &Context) {
        if !self.show_settings {
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
                            let v = self.settings_bindings.entry(dir.to_string()).or_default();
                            ui.add(egui::TextEdit::singleline(v).font(hud(13.0)).desired_width(80.0));
                        });
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("激活键:").font(hud(13.0)));
                        ui.add(egui::TextEdit::singleline(&mut self.settings_key).font(hud(13.0)).desired_width(80.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("按键延迟(秒):").font(hud(13.0)));
                        ui.add(egui::DragValue::new(&mut self.settings_delay).speed(0.01).range(0.01..=0.5));
                    });
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        if hud_button(ui, "保 存", Vec2::new(100.0, 30.0), GOLD, false).clicked() {
                            self.config.key_bindings = self.settings_bindings.clone();
                            self.config.stratagem_key = self.settings_key.clone();
                            self.config.key_delay = self.settings_delay;
                            config::save_config(&self.config);
                            self.show_settings = false;
                            self.log(LogKind::Info, "设置已保存");
                        }
                        if hud_button(ui, "取 消", Vec2::new(100.0, 30.0), TEXT_SUB, false).clicked() {
                            self.show_settings = false;
                        }
                    });
                });
            });
        if ctx.input(|i| i.key_pressed(Key::Escape)) && self.show_settings {
            self.show_settings = false;
        }
    }
}
