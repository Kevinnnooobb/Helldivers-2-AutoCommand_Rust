use eframe::egui::{self, Context, Key, Ui, Vec2};
use crate::plugin;
use crate::state::CreatorTab;
use crate::theme::*;
use crate::widgets::*;
use crate::wiki_fetcher;
use crate::H2ACApp;

pub fn render_plugin_creator(app: &mut H2ACApp, ctx: &Context, m: &UiMetrics) {
    if !app.creator.open {
        return;
    }

    if app.creator.sequence_recording {
        ctx.input(|i| {
            for ev in &i.events {
                if let egui::Event::Key { key, pressed: true, modifiers, .. } = ev {
                    if modifiers.ctrl || modifiers.alt { continue; }
                    let dir: Option<&str> = match key {
                        Key::ArrowUp | Key::W => Some("↑"),
                        Key::ArrowDown | Key::S => Some("↓"),
                        Key::ArrowLeft | Key::A => Some("←"),
                        Key::ArrowRight | Key::D => Some("→"),
                        Key::Backspace => {
                            app.creator.stratagem_sequence.pop();
                            None
                        }
                        Key::Escape => {
                            app.creator.sequence_recording = false;
                            None
                        }
                        Key::Enter => {
                            app.creator.sequence_recording = false;
                            if !app.creator.stratagem_sequence.is_empty() && !app.creator.stratagem_name.is_empty()
                            {
                                app.creator.saved_entries.push((
                                    app.creator.stratagem_name.clone(),
                                    app.creator.stratagem_sequence.clone(),
                                    app.creator.icon_key.clone(),
                                ));
                                app.creator.stratagem_name.clear();
                                app.creator.stratagem_sequence.clear();
                                app.creator.status = "战备条目已保存".into();
                            }
                            None
                        }
                        _ => None,
                    };
                    if let Some(d) = dir {
                        app.creator.stratagem_sequence.push(d.to_string());
                    }
                }
            }
        });
    }

    let sz = Vec2::new(m.plugin_creator_w(), m.plugin_creator_h());
    egui::Area::new(egui::Id::new("plugin_creator"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            hud_panel(ui, sz, m, GOLD_DIM, |ui| {
                ui.horizontal(|ui| {
                    let tab_fetch = ui.selectable_label(
                        app.creator.tab == CreatorTab::Fetch,
                        egui::RichText::new("📡 拉取数据").font(m.hud(13.0)),
                    );
                    if tab_fetch.clicked() { app.creator.tab = CreatorTab::Fetch; }
                    let tab_create = ui.selectable_label(
                        app.creator.tab == CreatorTab::Create,
                        egui::RichText::new("🛠 创建战备").font(m.hud(13.0)),
                    );
                    if tab_create.clicked() { app.creator.tab = CreatorTab::Create; }
                    let tab_themes = ui.selectable_label(
                        app.creator.tab == CreatorTab::Themes,
                        egui::RichText::new("🎨 创建主题").font(m.hud(13.0)),
                    );
                    if tab_themes.clicked() { app.creator.tab = CreatorTab::Themes; }
                });
                ui.add_space(8.0);

                match app.creator.tab {
                    CreatorTab::Fetch => render_fetch_tab(app, ui, m),
                    CreatorTab::Create => render_create_tab(app, ui, m),
                    CreatorTab::Themes => render_themes_tab(app, ui, m),
                }

                ui.add_space(4.0);
                if hud_button(ui, "关 闭", Vec2::new(120.0, 28.0), m, TEXT_SUB, false).clicked() {
                    app.creator.open = false;
                }
            });
        });

    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        app.creator.open = false;
        app.creator.sequence_recording = false;
    }
}

pub fn render_fetch_tab(app: &mut H2ACApp, ui: &mut Ui, m: &UiMetrics) {
    ui.label(egui::RichText::new("从社区 Wiki 拉取最新战备数据").font(m.hud_b(14.0)).color(GOLD));
    ui.add_space(6.0);
    ui.label(egui::RichText::new(format!("数据源: {}", wiki_fetcher::STRATAGEM_DATA_URL))
        .font(m.hud(9.0)).color(TEXT_DIM));
    ui.add_space(8.0);

    let fetching = app.wiki.fetch_rx.is_some();

    if fetching {
        ui.label(egui::RichText::new(&app.wiki.fetch_status).font(m.hud(13.0)).color(TEXT_SUB));
        ui.add_space(4.0);
        ui.label(egui::RichText::new("⟳ 拉取中…").font(m.hud(13.0)).color(GOLD));
    } else {
        let status = if app.wiki.cache_exists {
            format!("已缓存数据 | 可刷新")
        } else {
            "尚未拉取过数据".into()
        };
        ui.label(egui::RichText::new(&status).font(m.hud(12.0)).color(TEXT_SUB));
    }

    ui.add_space(10.0);

    ui.horizontal(|ui| {
        if hud_button(ui, "拉取数据", Vec2::new(140.0, 30.0), m, GOLD, false).clicked() && !fetching {
            app.start_wiki_fetch();
        }
        if app.wiki.cache_exists && !fetching {
            if hud_button(ui, "清除缓存", Vec2::new(100.0, 30.0), m, DANGER, true).clicked() {
                let path = wiki_fetcher::stratagem_cache_path();
                let _ = std::fs::remove_file(&path);
                app.wiki.cache_exists = false;
                app.creator.status = "缓存已清除".into();
            }
        }
    });

    if !app.creator.status.is_empty() {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(&app.creator.status).font(m.hud(11.0)).color(OK));
    }
}

pub fn render_create_tab(app: &mut H2ACApp, ui: &mut Ui, m: &UiMetrics) {
    ui.label(egui::RichText::new("创建战备插件").font(m.hud_b(14.0)).color(GOLD));
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("插件名:").font(m.hud(13.0)));
        ui.add(egui::TextEdit::singleline(&mut app.creator.plugin_name)
            .font(m.hud(13.0)).desired_width(160.0));
        ui.add_space(16.0);
        ui.label(egui::RichText::new("部门:").font(m.hud(13.0)));
        ui.add(egui::TextEdit::singleline(&mut app.creator.department)
            .font(m.hud(13.0)).desired_width(120.0));
    });

    ui.add_space(8.0);

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("名称:").font(m.hud(13.0)));
        ui.add(egui::TextEdit::singleline(&mut app.creator.stratagem_name)
            .font(m.hud(13.0)).desired_width(120.0));
        ui.add_space(8.0);

        if app.creator.sequence_recording {
            if hud_button(ui, "停止录制", Vec2::new(88.0, 26.0), m, DANGER, true).clicked() {
                app.creator.sequence_recording = false;
            }
        } else {
            if hud_button(ui, "🎬 录制", Vec2::new(80.0, 26.0), m, GOLD, false).clicked() {
                app.creator.sequence_recording = true;
                app.creator.stratagem_sequence.clear();
            }
        }
        ui.add_space(8.0);
        ui.label(egui::RichText::new("图标:").font(m.hud(13.0)));
        ui.add(egui::TextEdit::singleline(&mut app.creator.icon_key)
            .font(m.hud(13.0)).desired_width(100.0));
    });

    let seq_str = app.creator.stratagem_sequence.join(" ");
    let status = if app.creator.sequence_recording {
        format!("录制中: {} (方向键/WASD, Enter保存, Esc取消)", seq_str)
    } else if !seq_str.is_empty() {
        format!("指令: {}", seq_str)
    } else {
        "点击录制后按方向键…".into()
    };
    ui.label(egui::RichText::new(&status).font(m.hud(12.0)).color(TEXT_SUB));

    ui.add_space(8.0);

    ui.label(egui::RichText::new("已保存条目:").font(m.hud_b(12.0)).color(TEXT));
    let mut remove_idx = None;
    for (i, (name, seq, icon)) in app.creator.saved_entries.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("{}. {} [{}] ({})", i+1, name, seq.join(""), icon))
                .font(m.hud(11.0)).color(TEXT_SUB));
            if hud_button(ui, "✕", Vec2::new(22.0, 20.0), m, DANGER, true).clicked() {
                remove_idx = Some(i);
            }
        });
    }
    if let Some(i) = remove_idx { app.creator.saved_entries.remove(i); }

    ui.add_space(10.0);

    ui.horizontal(|ui| {
        if hud_button(ui, "保存插件", Vec2::new(130.0, 32.0), m, GOLD, false).clicked() {
            save_plugin_from_creator(app);
        }
        if !app.creator.status.is_empty() {
            ui.label(egui::RichText::new(&app.creator.status).font(m.hud(11.0)));
        }
    });
}

pub fn render_themes_tab(app: &mut H2ACApp, ui: &mut Ui, m: &UiMetrics) {
    ui.label(egui::RichText::new("创建主题插件").font(m.hud_b(14.0)).color(GOLD));
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("主题名:").font(m.hud(13.0)));
        ui.add(egui::TextEdit::singleline(&mut app.creator.theme_name)
            .font(m.hud(13.0)).desired_width(160.0));
    });
    ui.add_space(8.0);

    let mut colors = [
        ("背景色", &mut app.creator.bg_color),
        ("边框色", &mut app.creator.border_color),
        ("强调色", &mut app.creator.accent_color),
    ];
    for (label, color) in &mut colors {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(*label).font(m.hud(13.0)));
            ui.color_edit_button_rgb(color);
        });
    }

    ui.add_space(10.0);
    if hud_button(ui, "保存主题", Vec2::new(130.0, 32.0), m, GOLD, false).clicked() {
        save_theme_from_creator(app);
    }
    if !app.creator.status.is_empty() {
        ui.label(egui::RichText::new(&app.creator.status).font(m.hud(11.0)));
    }
}

pub fn save_plugin_from_creator(app: &mut H2ACApp) {
    let name = app.creator.plugin_name.trim();
    if name.is_empty() {
        app.creator.status = "请输入插件名".into();
        return;
    }
    if app.creator.saved_entries.is_empty() {
        app.creator.status = "请添加至少一个战备条目".into();
        return;
    }

    let stratagems: Vec<crate::stratagems::PluginStratagem> = app.creator.saved_entries.iter().map(|(n, seq, icon)| {
        crate::stratagems::PluginStratagem {
            name: n.clone(),
            category: app.creator.department.clone(),
            model: String::new(),
            command: seq.clone(),
            description: String::new(),
            icon: icon.clone(),
        }
    }).collect();

    let manifest = crate::stratagems::PluginManifest {
        id: name.to_lowercase().replace(' ', "_"),
        name: name.to_string(),
        enabled: true,
        stratagems,
        themes: Vec::new(),
    };

    let dir = plugin::plugins_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{}.json", manifest.id));
    match serde_json::to_string_pretty(&manifest) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                app.creator.status = format!("保存失败: {e}");
            } else {
                app.creator.status = format!("插件已保存: plugins/{}.json", manifest.id);
                reload_plugins(app);
            }
        }
        Err(e) => { app.creator.status = format!("序列化失败: {e}"); }
    }
}

pub fn save_theme_from_creator(app: &mut H2ACApp) {
    let name = app.creator.theme_name.trim();
    if name.is_empty() {
        app.creator.status = "请输入主题名".into();
        return;
    }
    let theme = crate::stratagems::PluginTheme {
        name: name.to_string(),
        background_color: format!("#{:02X}{:02X}{:02X}",
            (app.creator.bg_color[0] * 255.0) as u8,
            (app.creator.bg_color[1] * 255.0) as u8,
            (app.creator.bg_color[2] * 255.0) as u8),
        border_color: format!("#{:02X}{:02X}{:02X}",
            (app.creator.border_color[0] * 255.0) as u8,
            (app.creator.border_color[1] * 255.0) as u8,
            (app.creator.border_color[2] * 255.0) as u8),
        accent_color: format!("#{:02X}{:02X}{:02X}",
            (app.creator.accent_color[0] * 255.0) as u8,
            (app.creator.accent_color[1] * 255.0) as u8,
            (app.creator.accent_color[2] * 255.0) as u8),
    };
    let manifest = crate::stratagems::PluginManifest {
        id: name.to_lowercase().replace(' ', "_"),
        name: name.to_string(),
        enabled: true,
        stratagems: Vec::new(),
        themes: vec![theme],
    };
    let dir = plugin::plugins_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{}.json", manifest.id));
    match serde_json::to_string_pretty(&manifest) {
        Ok(json) => {
            let _ = std::fs::write(&path, json);
            app.creator.status = format!("主题已保存: plugins/{}.json", manifest.id);
        }
        Err(e) => { app.creator.status = format!("序列化失败: {e}"); }
    }
}

pub fn reload_plugins(app: &mut H2ACApp) {
    let (stratagems, themes) = plugin::load_all();
    app.plugins.stratagems.retain(|p| !p.name.starts_with("(Plugin)"));
    app.plugins.stratagems.extend(stratagems);
    app.plugins.themes = themes;
}
