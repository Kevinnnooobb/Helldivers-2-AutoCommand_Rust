// HUD 自绘组件库 — 切角面板 / 角括号 / 箭头 / 字形 / 状态灯
use eframe::egui::{
    self, Align2, Color32, Context, CornerRadius, Key, Painter, Pos2, Rect,
    Response, Sense, Shape, Stroke, Ui, Vec2,
};
use crate::stratagems::{DOWN, LEFT, RIGHT, UP};
use crate::theme::*;

// ─── 切角矩形（左上 + 右下斜切，HD2 签名形状） ───

pub fn chamfer_points(r: Rect, c: f32) -> Vec<Pos2> {
    vec![
        Pos2::new(r.left() + c, r.top()),
        Pos2::new(r.right(), r.top()),
        Pos2::new(r.right(), r.bottom() - c),
        Pos2::new(r.right() - c, r.bottom()),
        Pos2::new(r.left(), r.bottom()),
        Pos2::new(r.left(), r.top() + c),
    ]
}

pub fn paint_chamfer(p: &Painter, r: Rect, c: f32, fill: Color32, stroke: Stroke) {
    p.add(Shape::convex_polygon(chamfer_points(r, c), fill, stroke));
}

/// 四角 L 形括号装饰
pub fn corner_brackets(p: &Painter, r: Rect, arm: f32, color: Color32) {
    let s = Stroke::new(1.5, color);
    let (l, t, rr, b) = (r.left(), r.top(), r.right(), r.bottom());
    for (cx, cy, dx, dy) in [
        (l, t, 1.0f32, 1.0f32),
        (rr, t, -1.0, 1.0),
        (rr, b, -1.0, -1.0),
        (l, b, 1.0, -1.0),
    ] {
        p.add(Shape::line(
            vec![
                Pos2::new(cx + dx * arm, cy),
                Pos2::new(cx, cy),
                Pos2::new(cx, cy + dy * arm),
            ],
            s,
        ));
    }
}

/// 低透明度扫描线背景
pub fn scanlines(p: &Painter, r: Rect) {
    let line = Color32::from_rgba_unmultiplied(0xFF, 0xFF, 0xFF, 2);
    let mut y = r.top() + 2.0;
    while y < r.bottom() {
        p.hline(r.left()..=r.right(), y, Stroke::new(1.0, line));
        y += 4.0;
    }
}

// ─── 方向箭头（实心三角） ───

pub fn paint_arrow(p: &Painter, c: Pos2, dir: &str, size: f32, color: Color32) {
    let h = size / 2.0;
    let pts = if dir == UP {
        vec![
            Pos2::new(c.x, c.y - h),
            Pos2::new(c.x + h, c.y + h),
            Pos2::new(c.x - h, c.y + h),
        ]
    } else if dir == DOWN {
        vec![
            Pos2::new(c.x, c.y + h),
            Pos2::new(c.x + h, c.y - h),
            Pos2::new(c.x - h, c.y - h),
        ]
    } else if dir == LEFT {
        vec![
            Pos2::new(c.x - h, c.y),
            Pos2::new(c.x + h, c.y - h),
            Pos2::new(c.x + h, c.y + h),
        ]
    } else if dir == RIGHT {
        vec![
            Pos2::new(c.x + h, c.y),
            Pos2::new(c.x - h, c.y - h),
            Pos2::new(c.x - h, c.y + h),
        ]
    } else {
        return;
    };
    p.add(Shape::convex_polygon(pts, color, Stroke::NONE));
}

/// 指令箭头串，返回总宽度
pub fn arrow_strip(p: &Painter, origin: Pos2, cmd: &[&str], size: f32, gap: f32, color: Color32) -> f32 {
    let step = size + gap;
    for (i, d) in cmd.iter().enumerate() {
        let cx = origin.x + step * i as f32 + size / 2.0;
        paint_arrow(p, Pos2::new(cx, origin.y + size / 2.0), d, size, color);
    }
    if cmd.is_empty() {
        0.0
    } else {
        step * cmd.len() as f32 - gap
    }
}

/// 箭头串总宽度（先量后画，用于居中）
pub fn arrow_strip_w(cmd: &[&str], size: f32, gap: f32) -> f32 {
    if cmd.is_empty() {
        0.0
    } else {
        (size + gap) * cmd.len() as f32 - gap
    }
}

// ─── 状态灯（脉冲） ───

pub fn status_lamp(p: &Painter, c: Pos2, r: f32, on: bool, time: f64) {
    let color = if on { OK } else { DANGER };
    if on {
        // 呼吸光环
        let pulse = ((time * 1.6).sin() * 0.5 + 0.5) as f32;
        let glow_r = r + 2.0 + pulse * 2.5;
        let alpha = (14.0 + pulse * 16.0) as u8;
        p.circle_filled(c, glow_r, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha));
    }
    p.circle_filled(c, r, color);
    p.circle_stroke(c, r + 1.5, Stroke::new(1.0, color.gamma_multiply(0.5)));
}

// ─── 铬件字形 ───

#[derive(Clone, Copy, PartialEq)]
pub enum Glyph {
    Close,
    Minimize,
    Gear,
    Trash,
    Save,
    Compact,
    Restore,
    Keyboard,
    Play,
    Search,
}

pub fn paint_glyph(p: &Painter, r: Rect, g: Glyph, color: Color32) {
    let c = r.center();
    let u = r.width().min(r.height()) / 2.0; // 半尺寸单位
    let s = Stroke::new(1.5, color);
    match g {
        Glyph::Close => {
            let d = u * 0.55;
            p.add(Shape::line(
                vec![
                    Pos2::new(c.x - d, c.y - d),
                    Pos2::new(c.x + d, c.y + d),
                ],
                s,
            ));
            p.add(Shape::line(
                vec![
                    Pos2::new(c.x + d, c.y - d),
                    Pos2::new(c.x - d, c.y + d),
                ],
                s,
            ));
        }
        Glyph::Minimize => {
            let d = u * 0.55;
            p.add(Shape::line(
                vec![Pos2::new(c.x - d, c.y), Pos2::new(c.x + d, c.y)],
                s,
            ));
        }
        Glyph::Gear => {
            p.circle_stroke(c, u * 0.42, s);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let (sx, sy) = (a.cos(), a.sin());
                p.add(Shape::line(
                    vec![
                        Pos2::new(c.x + sx * u * 0.58, c.y + sy * u * 0.58),
                        Pos2::new(c.x + sx * u * 0.85, c.y + sy * u * 0.85),
                    ],
                    s,
                ));
            }
        }
        Glyph::Trash => {
            let (w, h) = (u * 0.62, u * 0.62);
            let top = c.y - h * 0.55;
            // 盖
            p.add(Shape::line(
                vec![
                    Pos2::new(c.x - w, top),
                    Pos2::new(c.x + w, top),
                ],
                s,
            ));
            p.add(Shape::line(
                vec![
                    Pos2::new(c.x - w * 0.35, top),
                    Pos2::new(c.x - w * 0.35, top - h * 0.3),
                    Pos2::new(c.x + w * 0.35, top - h * 0.3),
                    Pos2::new(c.x + w * 0.35, top),
                ],
                s,
            ));
            // 桶身
            p.add(Shape::line(
                vec![
                    Pos2::new(c.x - w * 0.7, top),
                    Pos2::new(c.x - w * 0.5, c.y + h * 0.7),
                    Pos2::new(c.x + w * 0.5, c.y + h * 0.7),
                    Pos2::new(c.x + w * 0.7, top),
                ],
                s,
            ));
        }
        Glyph::Save => {
            let d = u * 0.7;
            let rr = Rect::from_center_size(c, Vec2::splat(d * 2.0));
            p.rect_stroke(rr, CornerRadius::ZERO, s, egui::StrokeKind::Inside);
            let inner = Rect::from_min_size(
                Pos2::new(rr.left() + d * 0.35, rr.top()),
                Vec2::new(d * 0.7, d * 0.55),
            );
            p.rect_stroke(inner, CornerRadius::ZERO, s, egui::StrokeKind::Inside);
            p.hline(
                rr.left() + d * 0.4..=rr.right() - d * 0.4,
                rr.bottom() - d * 0.45,
                s,
            );
        }
        Glyph::Compact => {
            // 四角向内箭头 = 进入紧凑
            let d = u * 0.75;
            let a = u * 0.35;
            for (cx, cy, dx, dy) in [
                (c.x - d, c.y - d, 1.0f32, 1.0f32),
                (c.x + d, c.y - d, -1.0, 1.0),
                (c.x + d, c.y + d, -1.0, -1.0),
                (c.x - d, c.y + d, 1.0, -1.0),
            ] {
                p.add(Shape::line(
                    vec![
                        Pos2::new(cx + dx * a, cy),
                        Pos2::new(cx, cy),
                        Pos2::new(cx, cy + dy * a),
                    ],
                    s,
                ));
            }
        }
        Glyph::Restore => {
            // 两个交叠方框 = 还原
            let d = u * 0.45;
            let off = u * 0.3;
            let back = Rect::from_center_size(
                Pos2::new(c.x + off, c.y - off),
                Vec2::splat(d * 2.0),
            );
            let front = Rect::from_center_size(
                Pos2::new(c.x - off, c.y + off),
                Vec2::splat(d * 2.0),
            );
            p.rect_stroke(back, CornerRadius::ZERO, s, egui::StrokeKind::Inside);
            p.rect_filled(front, CornerRadius::ZERO, BG_PANEL);
            p.rect_stroke(front, CornerRadius::ZERO, s, egui::StrokeKind::Inside);
        }
        Glyph::Keyboard => {
            let rr = Rect::from_center_size(c, Vec2::new(u * 1.5, u * 0.95));
            paint_chamfer(p, rr, 3.0, Color32::TRANSPARENT, s);
            for i in -1..=1 {
                p.circle_filled(
                    Pos2::new(c.x + i as f32 * u * 0.4, c.y - u * 0.12),
                    1.2,
                    color,
                );
            }
            p.hline(
                c.x - u * 0.4..=c.x + u * 0.4,
                c.y + u * 0.22,
                Stroke::new(1.2, color),
            );
        }
        Glyph::Play => {
            let d = u * 0.6;
            p.add(Shape::convex_polygon(
                vec![
                    Pos2::new(c.x - d * 0.6, c.y - d),
                    Pos2::new(c.x + d, c.y),
                    Pos2::new(c.x - d * 0.6, c.y + d),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Glyph::Search => {
            let cr = u * 0.45;
            let cc = Pos2::new(c.x - u * 0.15, c.y - u * 0.15);
            p.circle_stroke(cc, cr, s);
            p.add(Shape::line(
                vec![
                    Pos2::new(cc.x + cr * 0.75, cc.y + cr * 0.75),
                    Pos2::new(c.x + u * 0.6, c.y + u * 0.6),
                ],
                s,
            ));
        }
    }
}

// ─── 铬件按钮（字形 + 悬停反馈） ───

pub fn glyph_button(ui: &mut Ui, glyph: Glyph, size: f32, tip: &str) -> Response {
    let (resp, p) = ui.allocate_painter(Vec2::splat(size), Sense::click());
    let rect = resp.rect;
    let hovered = resp.hovered();
    if hovered {
        paint_chamfer(&p, rect.shrink(1.0), 4.0, BG_HOVER, Stroke::NONE);
    }
    let color = if hovered { GOLD } else { TEXT_SUB };
    paint_glyph(&p, rect.shrink(size * 0.22), glyph, color);
    if !tip.is_empty() {
        resp.clone().on_hover_text(tip);
    }
    resp
}

// ─── HUD 文字按钮（切角） ───

pub fn hud_button(
    ui: &mut Ui,
    label: &str,
    size: Vec2,
    accent: Color32,
    danger: bool,
) -> Response {
    let (resp, p) = ui.allocate_painter(size, Sense::click());
    let rect = resp.rect;
    let hovered = resp.hovered();
    let base = if danger { DANGER } else { accent };
    let fill = if hovered {
        base.gamma_multiply(0.22)
    } else {
        base.gamma_multiply(0.08)
    };
    paint_chamfer(&p, rect, 6.0, fill, Stroke::new(1.0, base.gamma_multiply(0.7)));
    if hovered {
        corner_brackets(&p, rect.shrink(2.0), 4.0, base);
    }
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        hud(13.0),
        if hovered { TEXT } else { base },
    );
    resp
}

// ─── 固定尺寸切角面板（内含子 UI） ───

pub fn hud_panel(
    ui: &mut Ui,
    size: Vec2,
    border: Color32,
    add: impl FnOnce(&mut Ui),
) -> Rect {
    let rect = Rect::from_min_size(ui.cursor().min, size);
    ui.advance_cursor_after_rect(rect);
    paint_chamfer(ui.painter(), rect, CHAMFER, BG_PANEL, Stroke::new(1.0, border));
    corner_brackets(ui.painter(), rect.shrink(3.0), 7.0, GOLD_DIM);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(14.0, 10.0)))
            .layout(*ui.layout()),
    );
        add(&mut child);
    rect
}

pub fn egui_key_to_name(key: &Key) -> Option<&'static str> {
    match key {
        Key::F1 => Some("f1"), Key::F2 => Some("f2"), Key::F3 => Some("f3"), Key::F4 => Some("f4"),
        Key::F5 => Some("f5"), Key::F6 => Some("f6"), Key::F7 => Some("f7"), Key::F8 => Some("f8"),
        Key::F9 => Some("f9"), Key::F10 => Some("f10"), Key::F11 => Some("f11"), Key::F12 => Some("f12"),
        Key::Space => Some("space"), Key::Enter => Some("enter"), Key::Tab => Some("tab"),
        Key::Backspace => Some("backspace"), Key::Escape => Some("esc"),
        Key::Num0 => Some("0"), Key::Num1 => Some("1"), Key::Num2 => Some("2"), Key::Num3 => Some("3"),
        Key::Num4 => Some("4"), Key::Num5 => Some("5"), Key::Num6 => Some("6"), Key::Num7 => Some("7"),
        Key::Num8 => Some("8"), Key::Num9 => Some("9"),
        Key::A => Some("a"), Key::B => Some("b"), Key::C => Some("c"), Key::D => Some("d"),
        Key::E => Some("e"), Key::F => Some("f"), Key::G => Some("g"), Key::H => Some("h"),
        Key::I => Some("i"), Key::J => Some("j"), Key::K => Some("k"), Key::L => Some("l"),
        Key::M => Some("m"), Key::N => Some("n"), Key::O => Some("o"), Key::P => Some("p"),
        Key::Q => Some("q"), Key::R => Some("r"), Key::S => Some("s"), Key::T => Some("t"),
        Key::U => Some("u"), Key::V => Some("v"), Key::W => Some("w"), Key::X => Some("x"),
        Key::Y => Some("y"), Key::Z => Some("z"),
        Key::ArrowUp => Some("up"), Key::ArrowDown => Some("down"),
        Key::ArrowLeft => Some("left"), Key::ArrowRight => Some("right"),
        Key::Insert => Some("insert"), Key::Delete => Some("delete"),
        Key::Home => Some("home"), Key::End => Some("end"),
        Key::PageUp => Some("pageup"), Key::PageDown => Some("pagedown"),
        _ => None,
    }
}

pub fn format_key_name(name: &str) -> String {
    match name {
        "lctrl" => "L Ctrl".into(), "rctrl" => "R Ctrl".into(),
        "lalt" => "L Alt".into(), "ralt" => "R Alt".into(),
        "lshift" => "L Shift".into(), "rshift" => "R Shift".into(),
        "capslock" => "Caps".into(), "backspace" => "Bksp".into(),
        "pageup" => "PgUp".into(), "pagedown" => "PgDn".into(),
        "numlock" => "NumLk".into(), "scrolllock" => "ScrLk".into(),
        "insert" => "Ins".into(), "delete" => "Del".into(),
        _ => name.to_uppercase(),
    }
}

pub fn key_capture_modal(
    ctx: &Context,
    area_id: &str,
    title: &str,
    captured_value: &mut String,
    buttons: impl FnOnce(&mut Ui, &str),
) -> bool {
    let mut just_captured = false;
    ctx.input(|i| {
        for ev in &i.events {
            if let egui::Event::Key { key, pressed: true, modifiers, .. } = ev {
                if modifiers.ctrl || modifiers.alt || modifiers.mac_cmd { continue; }
                if let Some(name) = egui_key_to_name(key) {
                    *captured_value = name.to_string();
                    just_captured = true;
                }
            }
        }
    });

    // egui 0.31 does not emit Key events for Ctrl/Alt/Shift themselves.
    // Detect modifier-key presses by tracking modifier state changes between frames.
    let mods = ctx.input(|i| i.modifiers);
    let id_ctrl = egui::Id::new("__keycap_ctrl");
    let id_alt = egui::Id::new("__keycap_alt");
    let id_shift = egui::Id::new("__keycap_shift");
    let prev_ctrl = ctx.data_mut(|d| {
        let v = d.get_temp::<bool>(id_ctrl).unwrap_or(false);
        d.insert_temp(id_ctrl, mods.ctrl);
        v
    });
    let prev_alt = ctx.data_mut(|d| {
        let v = d.get_temp::<bool>(id_alt).unwrap_or(false);
        d.insert_temp(id_alt, mods.alt);
        v
    });
    let prev_shift = ctx.data_mut(|d| {
        let v = d.get_temp::<bool>(id_shift).unwrap_or(false);
        d.insert_temp(id_shift, mods.shift);
        v
    });
    if mods.ctrl && !prev_ctrl {
        *captured_value = "ctrl".to_string();
        just_captured = true;
    }
    if mods.alt && !prev_alt {
        *captured_value = "alt".to_string();
        just_captured = true;
    }
    if mods.shift && !prev_shift {
        *captured_value = "shift".to_string();
        just_captured = true;
    }

    egui::Area::new(egui::Id::new(area_id))
        .order(egui::Order::Foreground)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            hud_panel(ui, Vec2::new(300.0, 150.0), GOLD_DIM, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(title).font(hud_b(15.0)).color(GOLD));
                    ui.add_space(10.0);
                    if captured_value.is_empty() {
                        paint_glyph(ui.painter(), Rect::from_center_size(
                            Pos2::new(ui.cursor().center().x, ui.cursor().min.y + 14.0),
                            Vec2::splat(28.0),
                        ), Glyph::Keyboard, TEXT_SUB);
                        ui.add_space(30.0);
                        ui.label(egui::RichText::new("按下目标按键…").font(hud(13.0)).color(TEXT_SUB));
                    } else {
                        let key_name = captured_value.clone();
                        ui.label(
                            egui::RichText::new(key_name.to_uppercase())
                                .font(hud_b(24.0))
                                .color(OK),
                        );
                        ui.add_space(10.0);
                        buttons(ui, &key_name);
                    }
                });
            });
        });

    just_captured
}
