// 战备图标 — 嵌入 PNG 资源与纹理解码
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

/// 所有内嵌图标（与 stratagems.rs 的 icon 字段一一对应）
static ICON_DATA: &[(&str, &[u8])] = icons![
    reinforce, sos_beacon, resupply, eagle_rearm, cargo_container, super_earth_flag,
    hellbomb, upload_data, seismic_probe, dark_fluid_vessel, hive_breaker_drill,
    seaf_artillery, call_in_super_destroyer,
    orbital_gatling_barrage, orbital_airburst_strike, orbital_120mm_he_barrage,
    orbital_380mm_he_barrage, orbital_walking_barrage, orbital_laser,
    orbital_napalm_barrage, orbital_railcannon_strike,
    eagle_strafing_run, eagle_airstrike, eagle_cluster_bomb, eagle_napalm_airstrike,
    eagle_smoke_strike, eagle_110mm_rocket_pods, eagle_500kg_bomb,
    machine_gun, anti_materiel_rifle, stalwart, expendable_anti_tank, recoilless_rifle,
    flamethrower, autocannon, heavy_machine_gun, airburst_rocket_launcher, commando,
    railgun, spear, sta_x3_w_a_s_p_launcher, grenade_launcher, laser_cannon, arc_thrower,
    quasar_cannon, sterilizer, speargun, maxigun, expendable_napalm, gl_52_de_escalator,
    defoliation_tool, cqc_20,
    machine_gun_sentry, gatling_sentry, mortar_sentry, autocannon_sentry, guard_dog,
    rocket_sentry, guard_dog_breath,
    anti_personnel_minefield, incendiary_mines, anti_tank_mines, shield_generator_pack,
    shield_generator_relay, hmg_emplacement, anti_tank_emplacement,
    supply_pack, jump_pack, guard_dog_rover, ballistic_shield_backpack, directional_shield,
    hellbomb_portable,
    fast_recon_vehicle, patriot_exosuit, emancipator_exosuit,
];

pub struct IconStore {
    map: HashMap<&'static str, TextureHandle>,
}

impl IconStore {
    /// 启动时解码全部图标（缩放至 128px 纹理）
    pub fn load(ctx: &Context) -> Self {
        let mut map = HashMap::new();
        for (key, bytes) in ICON_DATA {
            let Ok(img) = image::load_from_memory(bytes) else { continue };
            let img = img
                .resize(128, 128, image::imageops::FilterType::Lanczos3)
                .to_rgba8();
            let size = [img.width() as usize, img.height() as usize];
            let color = ColorImage::from_rgba_unmultiplied(size, img.as_raw());
            map.insert(*key, ctx.load_texture(*key, color, TextureOptions::LINEAR));
        }
        Self { map }
    }

    pub fn get(&self, key: &str) -> Option<&TextureHandle> {
        self.map.get(key)
    }
}
