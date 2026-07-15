// 绝地潜兵2 自动呼叫战备 — egui 界面
// Helldivers 2 Auto Stratagem Caller — Rust + egui
// 布局：顶栏 → 2×5 槽位网格 → 两栏选择器弹窗
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod executor;
mod hotkey;
mod stratagems;

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use config::{
    delete_profile, list_profiles, load_config, load_profile, save_config, save_profile, Config,
    SLOT_COUNT,
};
use eframe::egui::{
    self, vec2, Align, Align2, Color32, Context, Frame, Id, Key, Layout, Margin,
    Order, Pos2, RichText, ScrollArea, Sense, Stroke, Vec2, Window,
};
use executor::execute_stratagem;
use stratagems::{command_to_string, get_by_category, get_categories, STRATAGEMS};

// ─── CJK 字体 ───

fn setup_cjk_font(ctx: &Context) {
    let mut fonts = egui::FontDefinitions::default();
    let candidates = [
        "C:/Windows/Fonts/msyh.ttc",
        "C:/Windows/Fonts/msyhbd.ttc",
        "C:/Windows/Fonts/simhei.ttf",
    ];
    for path in &candidates {
        if let Ok(data) = std::fs::read(path) {
            fonts.font_data.insert(
                "cjk".into(),
                egui::FontData::from_owned(data).tweak(egui::FontTweak {
                    scale: 0.95,
                    ..Default::default()
                }).into(),
            );
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "cjk".into());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push("cjk".into());
            ctx.set_fonts(fonts);
            return;
        }
    }
}

// ─── 颜色常量 ───

const BG: Color32 = Color32::from_rgb(0x0A, 0x0B, 0x10);
const PANEL: Color32 = Color32::from_rgb(0x0E, 0x0F, 0x16);
const TBAR: Color32 = Color32::from_rgb(0x10, 0x11, 0x1A);
const BTN: Color32 = Color32::from_rgb(0x1A, 0x1D, 0x28);
const BTN_H: Color32 = Color32::from_rgb(0x25, 0x28, 0x38);
const GOLD: Color32 = Color32::from_rgb(0xF5, 0xC8, 0x42);
const BORDER: Color32 = Color32::from_rgb(0x2E, 0x2C, 0x1E);
const BORDER_G: Color32 = Color32::from_rgb(0xD4, 0xA8, 0x25);
const TXT: Color32 = Color32::from_rgb(0xF0, 0xE4, 0xA8);
const TXT_DIM: Color32 = Color32::from_rgb(0x6A, 0x5E, 0x30);
const TXT_SUB: Color32 = Color32::from_rgb(0x9C, 0x8B, 0x48);
const SFILL: Color32 = Color32::from_rgb(0x1A, 0x18, 0x12);
const SEMPTY: Color32 = Color32::from_rgb(0x0F, 0x10, 0x18);
const STAT: Color32 = Color32::from_rgb(0x0C, 0x0D, 0x14);
const MENU: Color32 = Color32::from_rgb(0x14, 0x16, 0x20);

fn rn(n: u8) -> egui::CornerRadius { egui::CornerRadius::same(n) }
fn mg(x: i8, y: i8) -> Margin { Margin::symmetric(x, y) }

// ─── 应用状态 ───

struct SelectorState {
    slot: usize,
    pos: Pos2,
    category: String,
}

struct ContextState {
    slot: usize,
    pos: Pos2,
}

struct H2ACApp {
    config: Config,
    categories: Vec<&'static str>,
    slots: Vec<Option<usize>>,
    listening: bool,
    profile_names: Vec<String>,
    current_profile: String,
    save_profile_name: String,
    status: String,
    show_settings: bool,
    settings_bindings: HashMap<String, String>,
    settings_key: String,
    settings_delay: f64,
    // 战备选择器
    selector: Option<SelectorState>,
    // 右键菜单
    context: Option<ContextState>,
    // 热键捕获
    capturing: Option<usize>,
    captured: String,
    // 全局热键
    hotkey_rx: Option<mpsc::Receiver<usize>>,
    hotkey_run: Option<Arc<std::sync::atomic::AtomicBool>>,
}

impl H2ACApp {
    fn new() -> Self {
        let config = load_config();
        let slots = config.loadout.clone();
        let listening = config.listening_enabled;
        let categories = get_categories();
        let profile_names = list_profiles();

        let mut app = Self {
            config,
            categories,
            slots,
            listening,
            profile_names,
            current_profile: String::new(),
            save_profile_name: String::new(),
            status: "就绪 — 点击槽位选择战备，右键执行".into(),
            show_settings: false,
            settings_bindings: HashMap::new(),
            settings_key: String::new(),
            settings_delay: 0.05,
            selector: None,
            context: None,
            capturing: None,
            captured: String::new(),
            hotkey_rx: None,
            hotkey_run: None,
        };

        if app.listening { app.start_hotkeys(); }
        app
    }

    fn execute_slot(&mut self, slot: usize) {
        if let Some(idx) = self.slots[slot] {
            if let Some(s) = STRATAGEMS.get(idx) {
                let name = s.name;
                let cmd = command_to_string(&s.command);
                self.status = format!("执行: {name} ({cmd})");
                let sc = s.clone();
                let cg = self.config.clone();
                thread::spawn(move || execute_stratagem(&sc, &cg));
            }
        }
    }

    fn start_hotkeys(&mut self) {
        let map: HashMap<String, usize> = self.config.slot_hotkeys.iter()
            .filter_map(|(k, v)| Some((v.clone(), k.parse::<usize>().ok()?)))
            .filter(|(_, s)| *s < SLOT_COUNT)
            .collect();
        let map = Arc::new(Mutex::new(map));
        let (tx, rx) = mpsc::channel();
        let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
        hotkey::start(map, tx, running.clone());
        self.hotkey_rx = Some(rx);
        self.hotkey_run = Some(running);
    }

    fn stop_hotkeys(&mut self) {
        if let Some(ref r) = self.hotkey_run { r.store(false, std::sync::atomic::Ordering::Relaxed); }
        self.hotkey_rx = None;
        self.hotkey_run = None;
    }

    fn toggle_listening(&mut self) {
        self.listening = !self.listening;
        if self.listening { self.start_hotkeys(); self.status = "监听已开启".into(); }
        else { self.stop_hotkeys(); self.status = "监听已关闭".into(); }
        self.config.listening_enabled = self.listening;
        save_config(&self.config);
    }

    fn refresh_profiles(&mut self) { self.profile_names = list_profiles(); }

    fn set_slot(&mut self, slot: usize, idx: usize) {
        self.slots[slot] = Some(idx);
        self.config.loadout[slot] = Some(idx);
        save_config(&self.config);
    }

    fn clear_slot(&mut self, slot: usize) {
        self.slots[slot] = None;
        self.config.loadout[slot] = None;
        self.config.slot_hotkeys.remove(&slot.to_string());
        save_config(&self.config);
    }

    fn hotkey_label(&self, slot: usize) -> String {
        self.config.slot_hotkeys.get(&slot.to_string())
            .map(|h| format!("⌨{}", h))
            .unwrap_or_default()
    }
}

impl eframe::App for H2ACApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        if ctx.data(|d| d.get_temp::<bool>(Id::new("font_loaded")).is_none()) {
            setup_cjk_font(ctx);
            ctx.data_mut(|d| d.insert_temp(Id::new("font_loaded"), true));
        }

        if let Some(ref rx) = self.hotkey_rx {
            if let Ok(s) = rx.try_recv() { self.execute_slot(s); }
        }

        apply_style(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_toolbar(ui);
            ui.add_space(8.0);
            self.render_title(ui);
            ui.add_space(10.0);
            self.render_grid(ui);
            ui.add_space(6.0);
            self.render_status(ui);
        });

        self.render_selector(ctx);
        self.render_context(ctx);
        self.render_capture(ctx);
        self.render_settings(ctx);
    }
}

// ─── 渲染 ───

impl H2ACApp {
    fn render_toolbar(&mut self, ui: &mut egui::Ui) {
        Frame::default().fill(TBAR).stroke(Stroke::new(1.0, BORDER))
            .corner_radius(rn(8)).inner_margin(mg(12, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (lbl, fill) = if self.listening {
                        ("🟢 监听开启", Color32::from_rgb(0x1A, 0x3A, 0x1A))
                    } else {
                        ("🔴 监听关闭", Color32::from_rgb(0x3A, 0x1A, 0x1A))
                    };
                    if ui.add_sized(vec2(110.0, 28.0),
                        egui::Button::new(RichText::new(lbl).size(12.0)).fill(fill)).clicked() {
                        self.toggle_listening();
                    }

                    ui.separator();

                    if ui.button("⚙ 设置").clicked() {
                        self.settings_bindings = self.config.key_bindings.clone();
                        self.settings_key = self.config.stratagem_key.clone();
                        self.settings_delay = self.config.key_delay;
                        self.show_settings = true;
                    }

                    // 清空全部
                    if ui.add_sized(vec2(80.0, 26.0),
                        egui::Button::new(RichText::new("✕ 清空").size(12.0)
                            .color(Color32::from_rgb(0xE8, 0x5C, 0x5C)))).clicked() {
                        for i in 0..SLOT_COUNT { self.clear_slot(i); }
                        self.status = "全部槽位已清空".into();
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("💾").clicked() && !self.save_profile_name.is_empty() {
                            save_profile(&self.save_profile_name, &self.slots, &self.config.slot_hotkeys);
                            self.current_profile = self.save_profile_name.clone();
                            self.config.last_profile = self.current_profile.clone();
                            save_config(&self.config);
                            self.refresh_profiles();
                            self.status = format!("已保存: {}", self.save_profile_name);
                        }
                        ui.add(egui::TextEdit::singleline(&mut self.save_profile_name)
                            .hint_text("Profile名").desired_width(100.0));

                        let mut sel = self.current_profile.clone();
                        egui::ComboBox::from_id_salt("prof")
                            .width(130.0)
                            .selected_text(if sel.is_empty() { "选择 Profile" } else { &sel })
                            .show_ui(ui, |ui| {
                                for n in &self.profile_names.clone() {
                                    ui.selectable_value(&mut sel, n.clone(), n.as_str());
                                }
                            });
                        if sel != self.current_profile && !sel.is_empty() {
                            if let Some(p) = load_profile(&sel) {
                                self.slots = p.loadout;
                                self.config.slot_hotkeys = p.slot_hotkeys;
                                self.config.last_profile = sel.clone();
                                save_config(&self.config);
                                self.current_profile = sel;
                                self.status = format!("已加载: {}", self.current_profile);
                            }
                        }
                        if ui.button("🗑").clicked() && !self.current_profile.is_empty() {
                            delete_profile(&self.current_profile);
                            self.refresh_profiles();
                            self.current_profile.clear();
                            self.status = "Profile 已删除".into();
                        }
                    });
                });
            });
    }

    fn render_title(&self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("⚔ 绝地潜兵2 战备助手").size(20.0).color(GOLD).strong());
            ui.label(RichText::new("点击槽位选择战备 · 右键执行").size(11.0).color(TXT_SUB));
        });
    }

    fn render_grid(&mut self, ui: &mut egui::Ui) {
        let grid_w = ui.available_width();
        let gap = 10.0;
        let slot_w = ((grid_w - gap * 4.0) / 5.0).max(110.0);
        let slot_h = 100.0;

        egui::Grid::new("slot_grid")
            .num_columns(5)
            .spacing([gap, gap])
            .show(ui, |ui| {
                for idx in 0..SLOT_COUNT {
                    let filled = self.slots[idx].is_some();
                    let (label, hotkey) = self.slot_info(idx);

                    let bg = if filled { SFILL } else { SEMPTY };
                    let border_color = if filled { GOLD } else { BORDER };
                    let text_color = if filled { GOLD } else { TXT_DIM };

                    let (resp, painter) =
                        ui.allocate_painter(vec2(slot_w, slot_h), Sense::click());
                    let rect = resp.rect;

                    painter.rect_filled(rect, rn(10), bg);
                    painter.rect_stroke(rect, rn(10), Stroke::new(2.0, border_color), egui::StrokeKind::Inside);

                    let cx = rect.center().x;
                    let mut y = rect.top() + 8.0;

                    if !hotkey.is_empty() {
                        let hk_color = Color32::from_rgb(0x4B, 0xE8, 0x5C);
                        let hk_rect = painter.text(
                            egui::pos2(cx, y),
                            egui::Align2::CENTER_TOP,
                            &hotkey,
                            egui::FontId::proportional(10.0),
                            hk_color,
                        );
                        y += hk_rect.height() + 4.0;
                    }

                    let name_rect = painter.text(
                        egui::pos2(cx, y),
                        egui::Align2::CENTER_TOP,
                        &label,
                        egui::FontId::proportional(13.0),
                        text_color,
                    );
                    y += name_rect.height() + 2.0;

                    if filled {
                        if let Some(si) = self.slots[idx] {
                            if let Some(s) = STRATAGEMS.get(si) {
                                painter.text(
                                    egui::pos2(cx, y),
                                    egui::Align2::CENTER_TOP,
                                    s.model,
                                    egui::FontId::proportional(11.0),
                                    TXT_SUB,
                                );
                            }
                        }
                    }

                    if resp.clicked() {
                        let cat = if let Some(si) = self.slots[idx] {
                            STRATAGEMS.get(si).map(|s| s.category.to_string())
                        } else {
                            self.categories.first().map(|c| c.to_string())
                        };
                        self.selector = Some(SelectorState {
                            slot: idx,
                            pos: rect.left_top(),
                            category: cat.unwrap_or_default(),
                        });
                        self.context = None;
                    }

                    if resp.secondary_clicked() {
                        self.context = Some(ContextState {
                            slot: idx,
                            pos: rect.left_top(),
                        });
                        self.selector = None;
                    }

                    if (idx + 1) % 5 == 0 {
                        ui.end_row();
                    }
                }
            });
    }

    fn slot_info(&self, idx: usize) -> (String, String) {
        let hk = self.hotkey_label(idx);
        if let Some(si) = self.slots[idx] {
            if let Some(s) = STRATAGEMS.get(si) {
                return (s.name.to_string(), hk);
            }
        }
        (format!("槽位 {}", idx + 1), hk)
    }

    fn render_status(&self, ui: &mut egui::Ui) {
        Frame::default().fill(STAT).stroke(Stroke::new(1.0, BORDER))
            .inner_margin(mg(8, 4))
            .show(ui, |ui| {
                ui.label(RichText::new(&self.status).size(12.0)
                    .color(Color32::from_rgb(0xD4, 0xC6, 0x74)));
            });
    }

    // ─── 两栏选择器弹窗 ───

    fn render_selector(&mut self, ctx: &Context) {
        let Some(ref state) = self.selector else { return };
        let slot = state.slot;
        let pos = state.pos;
        let sel_cat = state.category.clone();

        let mut close = false;
        let mut choose_cat: Option<String> = None;

        Window::new("stratagem_selector")
            .title_bar(false).resizable(false).collapsible(false)
            .fixed_pos(pos).order(Order::Foreground)
            .show(ctx, |ui| {
                ui.set_min_size(vec2(400.0, 320.0));
                ui.set_max_height(420.0);

                // 标题行
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("槽位 {} — 选择战备", slot + 1))
                        .size(14.0).color(GOLD).strong());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("✕").clicked() { close = true; }
                    });
                });
                ui.separator();

                // 两栏布局
                ui.columns(2, |cols| {
                    // 左栏：分类列表
                    ScrollArea::vertical().show(&mut cols[0], |ui| {
                        ui.set_min_width(120.0);
                        for cat in &self.categories {
                            let is_sel = cat == &sel_cat;
                            let color = if is_sel { GOLD } else { TXT };
                            let btn = egui::Button::new(RichText::new(*cat).size(12.0).color(color))
                                .fill(if is_sel { BTN_H } else { BTN });
                            if ui.add_sized(vec2(130.0, 30.0), btn).clicked() {
                                if cat != &sel_cat {
                                    choose_cat = Some(cat.to_string());
                                }
                            }
                        }
                    });

                    // 右栏：战备列表
                    ScrollArea::vertical().show(&mut cols[1], |ui| {
                        let strats = get_by_category(&sel_cat);
                        for s in &strats {
                            let filled = self.slots[slot]
                                .and_then(|si| STRATAGEMS.get(si))
                                .map(|cur| cur.name == s.name)
                                .unwrap_or(false);
                            let bg = if filled { Color32::from_rgb(0x2A, 0x24, 0x10) } else { BTN };
                            let border = if filled { GOLD } else { BORDER };
                            let label = format!("[{}] {}", s.model, s.name);
                            let btn = egui::Button::new(RichText::new(&label).size(12.0))
                                .fill(bg);
                            if ui.add_sized(vec2(ui.available_width(), 28.0), btn).clicked() {
                                if let Some(idx) = STRATAGEMS.iter().position(|x| x.name == s.name) {
                                    self.set_slot(slot, idx);
                                    self.status = format!("槽位 {}: {} [{}]", slot + 1, s.name, s.model);
                                    close = true;
                                }
                            }
                        }
                    });
                });

                ui.separator();

                // 底部操作栏
                ui.horizontal(|ui| {
                    if ui.add_sized(vec2(80.0, 28.0),
                        egui::Button::new(RichText::new("✕ 清除").color(Color32::RED))).clicked() {
                        self.clear_slot(slot);
                        self.status = format!("槽位 {} 已清除", slot + 1);
                        close = true;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.slots[slot].is_some() {
                            if ui.button("▶ 执行").clicked() {
                                self.execute_slot(slot);
                                close = true;
                            }
                        }
                        if ui.button("⌨ 快捷键").clicked() {
                            self.capturing = Some(slot);
                            self.captured.clear();
                            close = true;
                        }
                    });
                });
            });

        if let Some(cat) = choose_cat {
            if let Some(ref mut s) = self.selector { s.category = cat; }
        }
        if close || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.selector = None;
        }
    }

    // ─── 右键菜单 ───

    fn render_context(&mut self, ctx: &Context) {
        let Some(ref ctx_state) = self.context else { return };
        let slot = ctx_state.slot;
        let pos = ctx_state.pos;
        let mut close = false;

        Window::new("context_menu")
            .title_bar(false).resizable(false).collapsible(false)
            .fixed_pos(pos).order(Order::Foreground)
            .show(ctx, |ui| {
                if self.slots[slot].is_some() {
                    if ui.button("▶ 执行").clicked() { self.execute_slot(slot); close = true; }
                    if ui.button("✕ 清除").clicked() { self.clear_slot(slot); close = true; }
                }
                if ui.button("⌨ 快捷键").clicked() {
                    self.capturing = Some(slot);
                    self.captured.clear();
                    close = true;
                }
            });

        if close || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.context = None;
        }
    }

    // ─── 热键捕获 ───

    fn render_capture(&mut self, ctx: &Context) {
        let Some(slot) = self.capturing else { return };

        ctx.input(|i| {
            for ev in &i.events {
                if let egui::Event::Key { key, pressed: true, modifiers, .. } = ev {
                    if modifiers.ctrl || modifiers.alt || modifiers.mac_cmd { return; }
                    let n: &str = match key {
                        Key::F1=>"f1",Key::F2=>"f2",Key::F3=>"f3",Key::F4=>"f4",
                        Key::F5=>"f5",Key::F6=>"f6",Key::F7=>"f7",Key::F8=>"f8",
                        Key::F9=>"f9",Key::F10=>"f10",Key::F11=>"f11",Key::F12=>"f12",
                        Key::Space=>"space",Key::Enter=>"enter",Key::Tab=>"tab",
                        Key::Backspace=>"backspace",
                        Key::Num0=>"0",Key::Num1=>"1",Key::Num2=>"2",Key::Num3=>"3",
                        Key::Num4=>"4",Key::Num5=>"5",Key::Num6=>"6",Key::Num7=>"7",
                        Key::Num8=>"8",Key::Num9=>"9",
                        Key::A=>"a",Key::B=>"b",Key::C=>"c",Key::D=>"d",Key::E=>"e",
                        Key::F=>"f",Key::G=>"g",Key::H=>"h",Key::I=>"i",Key::J=>"j",
                        Key::K=>"k",Key::L=>"l",Key::M=>"m",Key::N=>"n",Key::O=>"o",
                        Key::P=>"p",Key::Q=>"q",Key::R=>"r",Key::S=>"s",Key::T=>"t",
                        Key::U=>"u",Key::V=>"v",Key::W=>"w",Key::X=>"x",Key::Y=>"y",
                        Key::Z=>"z",
                        _ => return,
                    };
                    self.captured = n.to_string();
                }
            }
        });

        Window::new("capture").collapsible(false).resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                if self.captured.is_empty() {
                    ui.label("⌨ 按下目标按键…");
                } else {
                    ui.label(format!("已捕获: {}", self.captured));
                    ui.add_space(8.0);
                    if ui.button("✓ 确认").clicked() {
                        self.config.slot_hotkeys.insert(slot.to_string(), self.captured.clone());
                        save_config(&self.config);
                        self.status = format!("槽位 {} 快捷键: {}", slot+1, self.captured);
                        self.capturing = None;
                    }
                    if ui.button("✕ 取消").clicked() { self.capturing = None; }
                }
            });
    }

    // ─── 设置 ───

    fn render_settings(&mut self, ctx: &Context) {
        if !self.show_settings { return; }
        Window::new("⚙ 按键设置").collapsible(false).resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.heading("方向键绑定");
                for dir in &["↑","↓","←","→"] {
                    ui.horizontal(|ui| {
                        ui.label(format!("{}:", dir));
                        let v = self.settings_bindings.entry(dir.to_string()).or_default();
                        ui.text_edit_singleline(v);
                    });
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| { ui.label("激活键:"); ui.text_edit_singleline(&mut self.settings_key); });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("延迟(秒):");
                    ui.add(egui::DragValue::new(&mut self.settings_delay).speed(0.01).range(0.01..=0.5));
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("✓ 保存").clicked() {
                        self.config.key_bindings = self.settings_bindings.clone();
                        self.config.stratagem_key = self.settings_key.clone();
                        self.config.key_delay = self.settings_delay;
                        save_config(&self.config);
                        self.show_settings = false;
                        self.status = "设置已保存".into();
                    }
                    if ui.button("✕ 取消").clicked() { self.show_settings = false; }
                });
            });
    }
}

// ─── 样式 ───

fn apply_style(ctx: &Context) {
    let mut s = (*ctx.style()).clone();
    let v = &mut s.visuals;
    v.widgets.noninteractive.bg_fill = BG;
    v.widgets.inactive.bg_fill = BTN;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, BORDER_G);
    v.widgets.inactive.weak_bg_fill = BTN;
    v.widgets.hovered.bg_fill = BTN_H;
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, GOLD);
    v.widgets.active.bg_fill = GOLD;
    v.widgets.active.fg_stroke = Stroke::new(1.0, BG);
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.override_text_color = Some(TXT);
    ctx.set_style(s);
}

fn main() -> Result<(), eframe::Error> {
    eframe::run_native(
        "H2AC-RS",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([960.0, 620.0])
                .with_min_inner_size([800.0, 520.0])
                .with_title("⚔ H2AC-RS 绝地潜兵2 战备助手"),
            ..Default::default()
        },
        Box::new(|_cc| Ok(Box::new(H2ACApp::new()))),
    )
}
