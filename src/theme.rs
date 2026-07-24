// HD2 HUD 设计系统 — 配色 / 字体 / 全局样式 / UiMetrics
use eframe::egui::{
    self, Color32, Context, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke,
};
use crate::stratagems::{
    CAT_BACKPACKS, CAT_EAGLE, CAT_EMPLACEMENTS, CAT_MISSION, CAT_ORBITAL, CAT_SENTRIES, CAT_SUPPORT,
    CAT_VEHICLES,
};

// ─── 背景层级 ───
pub const BG_DEEP: Color32 = Color32::from_rgb(0x05, 0x07, 0x0C);
pub const BG_PANEL: Color32 = Color32::from_rgb(0x0B, 0x0E, 0x15);
pub const BG_RAISED: Color32 = Color32::from_rgb(0x11, 0x15, 0x1F);
pub const BG_HOVER: Color32 = Color32::from_rgb(0x1A, 0x20, 0x2E);
pub const BG_INPUT: Color32 = Color32::from_rgb(0x08, 0x0A, 0x10);

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
pub const CAT_FIRE: Color32 = Color32::from_rgb(0xDE, 0x7B, 0x6C);
pub const CAT_EQUIP: Color32 = Color32::from_rgb(0x49, 0xAD, 0xC9);
pub const CAT_DEF: Color32 = Color32::from_rgb(0x67, 0x95, 0x52);

// ─── 状态 ───
pub const OK: Color32 = Color32::from_rgb(0x4B, 0xE8, 0x5C);
pub const DANGER: Color32 = Color32::from_rgb(0xE8, 0x5C, 0x5C);
pub const LINE: Color32 = Color32::from_rgb(0x24, 0x29, 0x36);

// ─── 设计基准布局（unscaled, scale=1.0 时即为像素值） ───
pub const DESIGN_W: f32 = 1100.0;
pub const DESIGN_H: f32 = 640.0;
pub const COMPACT_DESIGN_W: f32 = 554.0;
pub const COMPACT_DESIGN_H: f32 = 56.0;

/// 分类对应强调色
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

pub const FAM_HUD: &str = "hud";
pub const FAM_HUD_B: &str = "hud_bold";

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
    let cjk = ["C:/Windows/Fonts/msyh.ttc", "C:/Windows/Fonts/simhei.ttf"]
        .iter()
        .find_map(|p| std::fs::read(p).ok())
        .map(|d| FontData::from_owned(d).tweak(egui::FontTweak { scale: 0.95, ..Default::default() }));
    if let Some(data) = cjk { fonts.font_data.insert("cjk".into(), data.into()); }
    let mut hud_chain: Vec<String> = Vec::new();
    for n in ["saira_md", "cjk"] {
        if fonts.font_data.contains_key(n) { hud_chain.push(n.to_string()); }
    }
    let mut hud_b_chain: Vec<String> = Vec::new();
    for n in ["saira_bd", "cjk"] {
        if fonts.font_data.contains_key(n) { hud_b_chain.push(n.to_string()); }
    }
    fonts.families.insert(FontFamily::Name(FAM_HUD.into()), hud_chain.clone());
    fonts.families.insert(FontFamily::Name(FAM_HUD_B.into()), hud_b_chain);
    fonts.families.insert(FontFamily::Proportional, hud_chain);
    ctx.set_fonts(fonts);
}

// ─── UiMetrics: 缩放后的所有尺寸，单例存取 ───

#[derive(Clone, Copy)]
pub struct UiMetrics {
    pub scale: f32,
}

impl UiMetrics {
    pub fn new(scale: f32) -> Self { Self { scale } }

    pub fn hud(&self, size: f32) -> FontId {
        FontId::new(size * self.scale, FontFamily::Name(FAM_HUD.into()))
    }
    pub fn hud_b(&self, size: f32) -> FontId {
        FontId::new(size * self.scale, FontFamily::Name(FAM_HUD_B.into()))
    }

    // ─── 常用尺寸 (scaled) ───
    pub fn tile_w(&self) -> f32 { 124.0 * self.scale }
    pub fn tile_h(&self) -> f32 { 150.0 * self.scale }
    pub fn tile_gap(&self) -> f32 { 10.0 * self.scale }
    pub fn chamfer(&self) -> f32 { 10.0 * self.scale }
    pub fn topbar_h(&self) -> f32 { 48.0 * self.scale }
    pub fn bottombar_h(&self) -> f32 { 52.0 * self.scale }
    pub fn content_margin_x(&self) -> f32 { 12.0 * self.scale }
    pub fn content_margin_y(&self) -> f32 { 8.0 * self.scale }
    pub fn left_panel_w(&self) -> f32 { 664.0 * self.scale }
    pub fn panel_gap(&self) -> f32 { 8.0 * self.scale }

    // 紧凑模式
    pub fn compact_w(&self) -> f32 { COMPACT_DESIGN_W * self.scale }
    pub fn compact_h(&self) -> f32 { COMPACT_DESIGN_H * self.scale }
    pub fn compact_tile(&self) -> f32 { 40.0 * self.scale }
    pub fn compact_gap(&self) -> f32 { 6.0 * self.scale }
    pub fn compact_ctrl(&self) -> f32 { 30.0 * self.scale }
    pub fn compact_inner_margin(&self) -> f32 { 12.0 * self.scale }

    // 分类 Rail
    pub fn cat_rail_w(&self) -> f32 { 64.0 * self.scale }
    pub fn cat_row_w(&self) -> f32 { 60.0 * self.scale }
    pub fn cat_row_h(&self) -> f32 { 34.0 * self.scale }
    pub fn cat_row_spacing(&self) -> f32 { 38.0 * self.scale }
    pub fn cat_accent_bar_w(&self) -> f32 { 3.0 * self.scale }

    // 库行
    pub fn lib_row_h(&self) -> f32 { 36.0 * self.scale }
    pub fn lib_icon_size(&self) -> f32 { 26.0 * self.scale }
    pub fn lib_row_icon_x(&self) -> f32 { 20.0 * self.scale }
    pub fn lib_row_text_x(&self) -> f32 { 40.0 * self.scale }

    // 详情面板
    pub fn detail_icon_size(&self) -> f32 { 80.0 * self.scale }
    pub fn detail_icon_area_w(&self) -> f32 { 96.0 * self.scale }
    pub fn detail_text_area_w(&self) -> f32 { 190.0 * self.scale }
    pub fn detail_btn_area_w(&self) -> f32 { 88.0 * self.scale }

    // 字形按钮
    pub fn glyph_btn(&self) -> f32 { 30.0 * self.scale }
    pub fn glyph_btn_sm(&self) -> f32 { 24.0 * self.scale }
    pub fn glyph_btn_md(&self) -> f32 { 26.0 * self.scale }
    pub fn glyph_btn_hover_shrink(&self) -> f32 { 1.0 * self.scale }

    // HUD 按钮
    pub fn status_lamp_r(&self) -> f32 { 4.5 * self.scale }

    // 槽位 tile
    pub fn slot_tile_num_x(&self) -> f32 { 8.0 * self.scale }
    pub fn slot_tile_num_y(&self) -> f32 { 6.0 * self.scale }
    pub fn slot_icon_size(&self) -> f32 { 56.0 * self.scale }
    pub fn slot_icon_y(&self) -> f32 { 48.0 * self.scale }
    pub fn slot_name_y(&self) -> f32 { 84.0 * self.scale }
    pub fn slot_arrow_y(&self) -> f32 { 112.0 * self.scale }
    pub fn slot_arrow_size(&self) -> f32 { 10.0 * self.scale }
    pub fn slot_arrow_gap(&self) -> f32 { 3.0 * self.scale }
    pub fn slot_cat_bar_h(&self) -> f32 { 3.0 * self.scale }
    pub fn slot_cat_bar_y_offset(&self) -> f32 { 5.0 * self.scale }
    pub fn slot_hotkey_badge_w(&self) -> f32 { 21.0 * self.scale }
    pub fn slot_hotkey_badge_h(&self) -> f32 { 15.0 * self.scale }
    pub fn slot_hotkey_badge_x_offset(&self) -> f32 { 26.0 * self.scale }
    pub fn slot_hotkey_badge_y_offset(&self) -> f32 { 5.0 * self.scale }

    // 详情面板大箭头
    pub fn detail_arrow_size(&self) -> f32 { 18.0 * self.scale }
    pub fn detail_arrow_gap(&self) -> f32 { 6.0 * self.scale }

    // 库行小箭头
    pub fn lib_arrow_size(&self) -> f32 { 8.0 * self.scale }
    pub fn lib_arrow_gap(&self) -> f32 { 2.0 * self.scale }

    // 搜索框
    pub fn search_w(&self) -> f32 { 120.0 * self.scale }

    // 顶部监听灯区域
    pub fn listening_btn_w(&self) -> f32 { 110.0 * self.scale }
    pub fn listening_btn_h(&self) -> f32 { 32.0 * self.scale }

    // Profile 输入框/ComboBox
    pub fn profile_input_w(&self) -> f32 { 90.0 * self.scale }
    pub fn profile_combo_w(&self) -> f32 { 120.0 * self.scale }

    // 弹窗默认宽度
    pub fn modal_settings_w(&self) -> f32 { 360.0 * self.scale }
    pub fn modal_settings_h(&self) -> f32 { 330.0 * self.scale }
    pub fn modal_stratagem_w(&self) -> f32 { 420.0 * self.scale }
    pub fn modal_stratagem_h(&self) -> f32 { 400.0 * self.scale }
    pub fn modal_keycap_w(&self) -> f32 { 300.0 * self.scale }
    pub fn modal_keycap_h(&self) -> f32 { 150.0 * self.scale }
    pub fn modal_ctx_menu_w(&self) -> f32 { 150.0 * self.scale }
    pub fn modal_lib_ctx_w(&self) -> f32 { 180.0 * self.scale }
    pub fn modal_row_h(&self) -> f32 { 30.0 * self.scale }
    pub fn modal_pad_y(&self) -> f32 { 18.0 * self.scale }

    // 插件创建器
    pub fn plugin_creator_w(&self) -> f32 { 560.0 * self.scale }
    pub fn plugin_creator_h(&self) -> f32 { 480.0 * self.scale }

    // ─── 适配字体 ───
    pub fn fit_font(&self, p: &egui::Painter, text: &str, max_w: f32, sizes: &[f32], bold: bool) -> FontId {
        for &s in sizes {
            let f = if bold { self.hud_b(s) } else { self.hud(s) };
            if p.layout_no_wrap(text.to_string(), f.clone(), TEXT).size().x <= max_w {
                return f;
            }
        }
        if bold { self.hud_b(*sizes.last().unwrap()) } else { self.hud(*sizes.last().unwrap()) }
    }
}

// ─── 全局样式 ───

fn base_visuals() -> egui::Style {
    let mut s = egui::Style::default();
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
    s
}

/// 启动时调用一次：设置基础配色
pub fn apply_style(ctx: &Context) {
    ctx.set_style(base_visuals());
}

/// scale 变化时调用：在基础配色上叠加比例缩放
pub fn apply_scaled(metrics: &UiMetrics) -> egui::Style {
    let mut s = base_visuals();
    let sc = metrics.scale;

    s.spacing.item_spacing = egui::vec2(8.0 * sc, 4.0 * sc);
    s.spacing.button_padding = egui::vec2(6.0 * sc, 2.0 * sc);
    s.spacing.indent = 14.0 * sc;
    s.spacing.interact_size = egui::vec2(40.0 * sc, 20.0 * sc);
    s.spacing.text_edit_width = 160.0 * sc;
    s.spacing.icon_width = 16.0 * sc;
    s.spacing.icon_spacing = 4.0 * sc;
    s.spacing.combo_width = 120.0 * sc;
    s.spacing.scroll = egui::style::ScrollStyle {
        bar_width: (8.0 * sc) as u8 as f32,
        handle_min_length: 30.0 * sc,
        ..s.spacing.scroll
    };

    let cr = CornerRadius::same((4.0 * sc) as u8);
    let ws = &mut s.visuals.widgets;
    ws.noninteractive.corner_radius = cr;
    ws.inactive.corner_radius = cr;
    ws.hovered.corner_radius = cr;
    ws.active.corner_radius = cr;
    ws.open.corner_radius = cr;
    ws.noninteractive.expansion = 2.0 * sc;
    ws.inactive.expansion = 2.0 * sc;
    ws.hovered.expansion = 2.0 * sc;
    ws.active.expansion = 2.0 * sc;
    ws.open.expansion = 2.0 * sc;

    s.visuals.window_corner_radius = CornerRadius::same((8.0 * sc) as u8);
    s.visuals.menu_corner_radius = CornerRadius::same((4.0 * sc) as u8);
    s.visuals.resize_corner_size = 16.0 * sc;

    s
}
