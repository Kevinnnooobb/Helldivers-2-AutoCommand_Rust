// 战备图标 — 嵌入 PNG 资源 + 运行时文件系统兜底
use eframe::egui::{ColorImage, Context, TextureHandle, TextureOptions};
use std::collections::HashMap;

macro_rules! icons {
    ($($key:ident),* $(,)?) => {
        &[ $(
            (
                stringify!($key),
                include_bytes!(concat!("../assets/icons/", stringify!($key), ".png")) as &[u8],
            )
        ),* ]
    };
}

static ICON_DATA: &[(&str, &[u8])] = icons![
    airburst_rocket_launcher, anti_materiel_rifle, anti_personnel_minefield, anti_tank_emplacement,
    anti_tank_mines, arc_thrower, autocannon, autocannon_sentry,
    ballistic_shield_backpack, bastion_mk_xvi, breakthrough_exosuit, bullet_storm,
    c4_pack, call_in_super_destroyer, cargo_container, commando,
    cqc_20, cremator, dark_fluid_vessel, defoliation_tool,
    directional_shield, eagle_110mm_rocket_pods, eagle_500kg_bomb, eagle_airstrike,
    eagle_cluster_bomb, eagle_napalm_airstrike, eagle_rearm, eagle_smoke_strike,
    eagle_strafing_run, eat_411, emancipator_exosuit, ems_mortar_sentry,
    epoch, expendable_anti_tank, expendable_napalm, fast_recon_vehicle,
    flame_sentry, flamethrower, gas_mine, gas_mortar_sentry,
    gatling_sentry, gl_28, gl_52_de_escalator, grenade_launcher,
    grenadier_battlement, guard_dog, guard_dog_breath, guard_dog_hot_dog,
    guard_dog_k_9, guard_dog_rover, heavy_machine_gun, hellbomb,
    hellbomb_portable, hive_breaker_drill, hmg_emplacement, hover_pack,
    incendiary_mines, incinerator_frv, jump_pack, laser_cannon,
    laser_sentry, lumberer_exosuit, machine_gun, machine_gun_sentry,
    maxigun, mortar_sentry, one_true_flag, orbital_120mm_he_barrage,
    orbital_380mm_he_barrage, orbital_airburst_strike, orbital_ems_strike, orbital_gas_strike,
    orbital_gatling_barrage, orbital_illumination_flare, orbital_laser, orbital_napalm_barrage,
    orbital_precision_strike, orbital_railcannon_strike, orbital_smoke_strike, orbital_walking_barrage,
    patriot_exosuit, prospecting_drill, quasar_cannon, railgun,
    recoilless_rifle, reinforce, resupply, rocket_sentry,
    seaf_artillery, seismic_probe, shield_generator_pack, shield_generator_relay,
    solo_silo, sos_beacon, spear, speargun,
    sta_x3_w_a_s_p_launcher, stalwart, sterilizer, super_earth_flag,
    supply_frv, supply_pack, tectonic_drill, tesla_tower,
    upload_data, warp_pack,
];

pub struct IconStore {
    map: HashMap<String, TextureHandle>,
}

fn load_png(ctx: &Context, key: String, bytes: &[u8]) -> Option<TextureHandle> {
    let img = image::load_from_memory(bytes).ok()?
        .resize(128, 128, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some(ctx.load_texture(key, color, TextureOptions::LINEAR))
}

impl IconStore {
    /// 启动时加载：嵌入图标 + 文件系统发现的额外 PNG
    pub fn load(ctx: &Context) -> Self {
        let mut map = HashMap::new();

        // 1. 嵌入的内置图标
        for (key, bytes) in ICON_DATA {
            if let Some(tex) = load_png(ctx, (*key).to_string(), bytes) {
                map.insert((*key).to_string(), tex);
            }
        }

        // 2. 运行时从磁盘加载额外图标（exe 同目录 + 项目根目录）
        let search_dirs = [
            std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("assets/icons"))),
            std::env::current_dir().ok().map(|d| d.join("assets/icons")),
        ];
        for icon_dir in search_dirs.into_iter().flatten() {
            if !icon_dir.exists() { continue; }
            if let Ok(entries) = std::fs::read_dir(&icon_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map_or(true, |e| e != "png") { continue; }
                    let key = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                    if map.contains_key(key) { continue; }
                    if let Ok(bytes) = std::fs::read(&path) {
                        if let Some(tex) = load_png(ctx, key.to_string(), &bytes) {
                            map.insert(key.to_string(), tex);
                        }
                    }
                }
            }
        }

        Self { map }
    }

    pub fn get(&self, key: &str) -> Option<&TextureHandle> {
        self.map.get(key)
    }
}
