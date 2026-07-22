// UI 共享辅助：常量、分类短标签、自适应字号
use eframe::egui::{Color32, FontId, Painter};
use crate::theme::*;

pub const TILE_W: f32 = 124.0;
pub const TILE_H: f32 = 150.0;
pub const TILE_GAP: f32 = 10.0;

/// 分类短标签（终端风格）
pub fn cat_short(cat: &str) -> &'static str {
    match cat {
        "Mission Stratagems" => "Mission",
        "Orbital Strikes" => "Orbital",
        "Eagle Strikes" => "Eagle",
        "Support Weapons" => "Support",
        "Sentries" => "Sentries",
        "Emplacements" => "Emplace",
        "Backpacks" => "Backpacks",
        "Vehicles" => "Vehicles",
        "NEW (Wiki)" => "New",
        _ => "?",
    }
}

/// 宽度自适应字号：超宽则降级
pub fn fit_font(p: &Painter, text: &str, max_w: f32, sizes: &[f32], bold: bool) -> FontId {
    for &s in sizes {
        let f = if bold { hud_b(s) } else { hud(s) };
        if p.layout_no_wrap(text.to_string(), f.clone(), crate::theme::TEXT).size().x <= max_w {
            return f;
        }
    }
    if bold { hud_b(*sizes.last().unwrap()) } else { hud(*sizes.last().unwrap()) }
}
