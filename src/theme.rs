// HD2 HUD 设计系统 — 配色 / 字体 / 全局样式
use eframe::egui::{
    self, Color32, Context, FontData, FontDefinitions, FontFamily, FontId, Stroke,
};
use crate::stratagems::{
    CAT_BACKPACKS, CAT_EAGLE, CAT_EMPLACEMENTS, CAT_MISSION, CAT_ORBITAL, CAT_SENTRIES, CAT_SUPPORT,
    CAT_VEHICLES,
};

// ─── 背景层级 ───
pub const BG_DEEP: Color32 = Color32::from_rgb(0x05, 0x07, 0x0C); // 最深背景
pub const BG_PANEL: Color32 = Color32::from_rgb(0x0B, 0x0E, 0x15); // 面板
pub const BG_RAISED: Color32 = Color32::from_rgb(0x11, 0x15, 0x1F); // 抬升元素
pub const BG_HOVER: Color32 = Color32::from_rgb(0x1A, 0x20, 0x2E); // 悬停
pub const BG_INPUT: Color32 = Color32::from_rgb(0x08, 0x0A, 0x10); // 输入框

// ─── 主色（超级地球金） ───
pub const GOLD: Color32 = Color32::from_rgb(0xF5, 0xC8, 0x42);
pub const GOLD_MID: Color32 = Color32::from_rgb(0xC9, 0xB2, 0x69);
pub const GOLD_DIM: Color32 = Color32::from_rgb(0x6E, 0x60, 0x2E);
pub const GOLD_GHOST: Color32 = Color32::from_rgba_premultiplied(17, 14, 5, 18);

// ─── 文字 ───
pub const TEXT: Color32 = Color32::from_rgb(0xE8, 0xE6, 0xDC);
pub const TEXT_SUB: Color32 = Color32::from_rgb(0x9A, 0x9E, 0xA8);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x5A, 0x5E, 0x6A);

// ─── 分类色（采样自游戏图标） ───
pub const CAT_FIRE: Color32 = Color32::from_rgb(0xDE, 0x7B, 0x6C); // 轨道/飞鹰
pub const CAT_EQUIP: Color32 = Color32::from_rgb(0x49, 0xAD, 0xC9); // 装备/载具
pub const CAT_DEF: Color32 = Color32::from_rgb(0x67, 0x95, 0x52); // 哨戒/地雷

// ─── 状态 ───
pub const OK: Color32 = Color32::from_rgb(0x4B, 0xE8, 0x5C);
pub const DANGER: Color32 = Color32::from_rgb(0xE8, 0x5C, 0x5C);
pub const LINE: Color32 = Color32::from_rgb(0x24, 0x29, 0x36); // 分隔线

// ─── 布局尺寸 ───
pub const MAIN_W: f32 = 1100.0;
pub const MAIN_H: f32 = 640.0;
pub const COMPACT_H: f32 = 56.0;
pub const CHAMFER: f32 = 10.0; // 切角大小

/// 分类对应强调色（Wiki 色系：Red=Offensive, Blue=Supply, Green=Defensive, Yellow=Mission）
pub fn category_color(cat: &str) -> Color32 {
    match cat {
        CAT_MISSION => GOLD_MID,
        CAT_ORBITAL | CAT_EAGLE => CAT_FIRE,
        CAT_SUPPORT | CAT_BACKPACKS | CAT_VEHICLES => CAT_EQUIP,
        CAT_SENTRIES | CAT_EMPLACEMENTS => CAT_DEF,
        _ => TEXT_SUB,
    }
}

// ─── 字体 ───

pub const FAM_HUD: &str = "hud"; // Saira Condensed Medium + CJK 兜底
pub const FAM_HUD_B: &str = "hud_bold"; // Saira Condensed Bold + CJK 兜底

pub fn hud(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FAM_HUD.into()))
}

pub fn hud_b(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FAM_HUD_B.into()))
}

pub fn install_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        "saira_md".into(),
        FontData::from_static(include_bytes!("../assets/fonts/SairaCondensed-Medium.ttf")).into(),
    );
    fonts.font_data.insert(
        "saira_bd".into(),
        FontData::from_static(include_bytes!("../assets/fonts/SairaCondensed-Bold.ttf")).into(),
    );

    // 系统 CJK 兜底
    let cjk = ["C:/Windows/Fonts/msyh.ttc", "C:/Windows/Fonts/simhei.ttf"]
        .iter()
        .find_map(|p| std::fs::read(p).ok())
        .map(|d| {
            FontData::from_owned(d).tweak(egui::FontTweak {
                scale: 0.95,
                ..Default::default()
            })
        });
    if let Some(data) = cjk {
        fonts.font_data.insert("cjk".into(), data.into());
    }

    // HUD 正文族：Saira → CJK → egui 内置
    let mut hud_chain: Vec<String> = Vec::new();
    for n in ["saira_md", "cjk"] {
        if fonts.font_data.contains_key(n) {
            hud_chain.push(n.to_string());
        }
    }
    let mut hud_b_chain: Vec<String> = Vec::new();
    for n in ["saira_bd", "cjk"] {
        if fonts.font_data.contains_key(n) {
            hud_b_chain.push(n.to_string());
        }
    }

    fonts
        .families
        .insert(FontFamily::Name(FAM_HUD.into()), hud_chain.clone());
    fonts
        .families
        .insert(FontFamily::Name(FAM_HUD_B.into()), hud_b_chain);
    // 默认 Proportional 也走 HUD 链，保证全局一致
    fonts.families.insert(FontFamily::Proportional, hud_chain);

    ctx.set_fonts(fonts);
}

// ─── 全局样式 ───

pub fn apply_style(ctx: &Context) {
    let mut s = (*ctx.style()).clone();
    let v = &mut s.visuals;

    v.dark_mode = true;
    v.panel_fill = BG_DEEP;
    v.window_fill = BG_PANEL;
    v.extreme_bg_color = BG_INPUT;
    v.override_text_color = Some(TEXT);

    v.widgets.noninteractive.bg_fill = BG_PANEL;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.inactive.bg_fill = BG_RAISED;
    v.widgets.inactive.weak_bg_fill = BG_RAISED;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.hovered.bg_fill = BG_HOVER;
    v.widgets.hovered.weak_bg_fill = BG_HOVER;
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, GOLD);
    v.widgets.active.bg_fill = GOLD;
    v.widgets.active.weak_bg_fill = GOLD_MID;
    v.widgets.active.fg_stroke = Stroke::new(1.0, BG_DEEP);

    v.selection.bg_fill = GOLD_GHOST;
    v.selection.stroke = Stroke::new(1.0, GOLD);

    v.window_stroke = Stroke::new(1.0, LINE);

    ctx.set_style(s);
}
