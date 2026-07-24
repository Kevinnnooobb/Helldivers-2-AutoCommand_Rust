// 绝地潜兵2 战备数据库 — Helldivers 2 Stratagem Database
use serde::{Serialize, Deserialize};

pub const UP: &str = "↑";
pub const DOWN: &str = "↓";
pub const LEFT: &str = "←";
pub const RIGHT: &str = "→";

pub const CAT_MISSION: &str = "Mission Stratagems";
pub const CAT_ORBITAL: &str = "Orbital Strikes";
pub const CAT_EAGLE: &str = "Eagle Strikes";
pub const CAT_SUPPORT: &str = "Support Weapons";
pub const CAT_SENTRIES: &str = "Sentries";
pub const CAT_EMPLACEMENTS: &str = "Emplacements";
pub const CAT_BACKPACKS: &str = "Backpacks";
pub const CAT_VEHICLES: &str = "Vehicles";

#[derive(Debug, Clone, Serialize)]
pub struct Stratagem {
    pub category: &'static str,
    pub model: &'static str,
    pub name: &'static str,
    pub command: &'static [&'static str],
    pub description: &'static str,
    /// 图标资源键 — 对应 assets/icons/{icon}.png
    pub icon: &'static str,
}

macro_rules! cmd {
    ($($d:expr),*) => { &[$($d),*] };
}

pub static STRATAGEMS: &[Stratagem] = &[
    // ─── 任务战备 ───
    Stratagem { category: CAT_MISSION, model: "无型号", name: "增援",
        command: cmd![UP, DOWN, RIGHT, LEFT, UP], description: "呼叫 Helldiver 复活",
        icon: "reinforce" },
    Stratagem { category: CAT_MISSION, model: "无型号", name: "SOS 信标",
        command: cmd![UP, DOWN, RIGHT, UP], description: "提供任务优先与公开",
        icon: "sos_beacon" },
    Stratagem { category: CAT_MISSION, model: "无型号", name: "补给",
        command: cmd![DOWN, DOWN, UP, RIGHT], description: "呼叫补给",
        icon: "resupply" },
    Stratagem { category: CAT_MISSION, model: "无型号", name: "飞鹰重新装填",
        command: cmd![UP, UP, LEFT, UP, RIGHT], description: "令飞鹰返回补给弹药",
        icon: "eagle_rearm" },
    Stratagem { category: CAT_MISSION, model: "无型号", name: "SSSD 交付",
        command: cmd![UP, UP, DOWN, DOWN, RIGHT, DOWN], description: "呼叫 SSSD 硬盘",
        icon: "cargo_container" },
    Stratagem { category: CAT_MISSION, model: "无型号", name: "超级地球旗帜",
        command: cmd![DOWN, UP, DOWN, UP], description: "升旗任务主目标",
        icon: "super_earth_flag" },
    Stratagem { category: CAT_MISSION, model: "无型号", name: "地狱炸弹",
        command: cmd![DOWN, UP, LEFT, DOWN, UP, RIGHT, DOWN, UP], description: "呼叫地狱炸弹",
        icon: "hellbomb" },
    Stratagem { category: CAT_MISSION, model: "无型号", name: "上传数据",
        command: cmd![LEFT, RIGHT, UP, UP, UP], description: "上传逃生舱数据",
        icon: "upload_data" },
    Stratagem { category: CAT_MISSION, model: "无型号", name: "地震探测器",
        command: cmd![UP, UP, LEFT, RIGHT, DOWN, DOWN], description: "地质勘测副目标",
        icon: "seismic_probe" },
    Stratagem { category: CAT_MISSION, model: "无型号", name: "暗流体容器",
        command: cmd![UP, LEFT, RIGHT, DOWN, UP, UP], description: "暗流体任务专属",
        icon: "dark_fluid_vessel" },
    Stratagem { category: CAT_MISSION, model: "无型号", name: "蜂巢破碎钻机",
        command: cmd![LEFT, UP, DOWN, RIGHT, DOWN, DOWN], description: "核弹巢穴任务主目标",
        icon: "hive_breaker_drill" },
    Stratagem { category: CAT_MISSION, model: "无型号", name: "SEAF 火炮",
        command: cmd![RIGHT, UP, UP, DOWN], description: "完成 SEAF 火炮副目标后解锁",
        icon: "seaf_artillery" },
    Stratagem { category: CAT_MISSION, model: "无型号", name: "呼叫超级驱逐舰",
        command: cmd![UP, UP, DOWN, DOWN, LEFT, RIGHT, LEFT, RIGHT], description: "突击任务中呼叫支援",
        icon: "call_in_super_destroyer" },
    // ─── 轨道火力 ───
    Stratagem { category: CAT_ORBITAL, model: "无型号", name: "轨道加特林火力网",
        command: cmd![RIGHT, DOWN, LEFT, UP, UP], description: "轨道自动炮高爆弹幕",
        icon: "orbital_gatling_barrage" },
    Stratagem { category: CAT_ORBITAL, model: "无型号", name: "轨道空爆攻击",
        command: cmd![RIGHT, RIGHT, RIGHT], description: "空中爆炸弹片雨",
        icon: "orbital_airburst_strike" },
    Stratagem { category: CAT_ORBITAL, model: "无型号", name: "轨道120MM高爆弹火力网",
        command: cmd![RIGHT, RIGHT, DOWN, LEFT, RIGHT, DOWN], description: "小范围精确高爆炮击",
        icon: "orbital_120mm_he_barrage" },
    Stratagem { category: CAT_ORBITAL, model: "无型号", name: "轨道380MM高爆弹火力网",
        command: cmd![RIGHT, DOWN, UP, UP, LEFT, DOWN, DOWN], description: "大范围长时间高爆炮击",
        icon: "orbital_380mm_he_barrage" },
    Stratagem { category: CAT_ORBITAL, model: "无型号", name: "轨道游走火力网",
        command: cmd![RIGHT, DOWN, RIGHT, DOWN, RIGHT, DOWN], description: "线性推进弹幕",
        icon: "orbital_walking_barrage" },
    Stratagem { category: CAT_ORBITAL, model: "无型号", name: "轨道激光炮",
        command: cmd![RIGHT, DOWN, UP, RIGHT, DOWN], description: "激光扫射指定区域",
        icon: "orbital_laser" },
    Stratagem { category: CAT_ORBITAL, model: "无型号", name: "轨道凝固汽油弹火力网",
        command: cmd![RIGHT, RIGHT, DOWN, LEFT, RIGHT, UP], description: "大范围凝固汽油弹轰炸",
        icon: "orbital_napalm_barrage" },
    Stratagem { category: CAT_ORBITAL, model: "无型号", name: "轨道炮攻击",
        command: cmd![RIGHT, UP, DOWN, DOWN, RIGHT], description: "自动瞄准最大目标轨道炮",
        icon: "orbital_railcannon_strike" },
    // ─── 飞鹰 ───
    Stratagem { category: CAT_EAGLE, model: "无型号", name: "飞鹰机枪扫射",
        command: cmd![UP, RIGHT, RIGHT], description: "飞鹰机扫射轻型目标",
        icon: "eagle_strafing_run" },
    Stratagem { category: CAT_EAGLE, model: "无型号", name: "飞鹰空袭",
        command: cmd![UP, RIGHT, DOWN, RIGHT], description: "飞鹰投放爆炸地毯",
        icon: "eagle_airstrike" },
    Stratagem { category: CAT_EAGLE, model: "无型号", name: "飞鹰集束炸弹",
        command: cmd![UP, RIGHT, DOWN, DOWN, RIGHT], description: "飞鹰定点集束爆炸",
        icon: "eagle_cluster_bomb" },
    Stratagem { category: CAT_EAGLE, model: "无型号", name: "飞鹰凝固汽油弹空袭",
        command: cmd![UP, RIGHT, DOWN, UP], description: "飞鹰投放火墙",
        icon: "eagle_napalm_airstrike" },
    Stratagem { category: CAT_EAGLE, model: "无型号", name: "飞鹰烟雾攻击",
        command: cmd![UP, RIGHT, UP, DOWN], description: "飞鹰投放烟雾",
        icon: "eagle_smoke_strike" },
    Stratagem { category: CAT_EAGLE, model: "无型号", name: "飞鹰110MM火箭巢",
        command: cmd![UP, RIGHT, UP, LEFT], description: "飞鹰火箭弹攻击",
        icon: "eagle_110mm_rocket_pods" },
    Stratagem { category: CAT_EAGLE, model: "无型号", name: "飞鹰500KG炸弹",
        command: cmd![UP, RIGHT, DOWN, DOWN, DOWN], description: "飞鹰大型炸弹",
        icon: "eagle_500kg_bomb" },
    // ─── 支援武器 ───
    Stratagem { category: CAT_SUPPORT, model: "MG-43", name: "机枪",
        command: cmd![DOWN, LEFT, DOWN, UP, RIGHT], description: "固定机枪",
        icon: "machine_gun" },
    Stratagem { category: CAT_SUPPORT, model: "APW-1", name: "反器材步枪",
        command: cmd![DOWN, LEFT, RIGHT, UP, DOWN], description: "高口径狙击步枪",
        icon: "anti_materiel_rifle" },
    Stratagem { category: CAT_SUPPORT, model: "M-105", name: "盟友",
        command: cmd![DOWN, LEFT, DOWN, UP, UP, LEFT], description: "紧凑型轻机枪",
        icon: "stalwart" },
    Stratagem { category: CAT_SUPPORT, model: "EAT-17", name: "消耗性反坦克",
        command: cmd![DOWN, DOWN, LEFT, UP, RIGHT], description: "单次使用反坦克武器",
        icon: "expendable_anti_tank" },
    Stratagem { category: CAT_SUPPORT, model: "GR-8", name: "无后坐力炮",
        command: cmd![DOWN, LEFT, RIGHT, RIGHT, LEFT], description: "无后坐力炮",
        icon: "recoilless_rifle" },
    Stratagem { category: CAT_SUPPORT, model: "FLAM-40", name: "火焰喷射器",
        command: cmd![DOWN, LEFT, UP, DOWN, UP], description: "近距燃烧武器",
        icon: "flamethrower" },
    Stratagem { category: CAT_SUPPORT, model: "AC-8", name: "机炮",
        command: cmd![DOWN, LEFT, DOWN, UP, UP, RIGHT], description: "全自动机炮",
        icon: "autocannon" },
    Stratagem { category: CAT_SUPPORT, model: "MG-206", name: "重机枪",
        command: cmd![DOWN, LEFT, UP, DOWN, DOWN], description: "强力机枪",
        icon: "heavy_machine_gun" },
    Stratagem { category: CAT_SUPPORT, model: "RL-77", name: "空爆火箭弹发射器",
        command: cmd![DOWN, UP, UP, LEFT, RIGHT], description: "发射空爆火箭",
        icon: "airburst_rocket_launcher" },
    Stratagem { category: CAT_SUPPORT, model: "MLS-4X", name: "突击兵",
        command: cmd![DOWN, LEFT, UP, DOWN, RIGHT], description: "一次性激光制导导弹",
        icon: "commando" },
    Stratagem { category: CAT_SUPPORT, model: "RS-422", name: "磁轨炮",
        command: cmd![DOWN, RIGHT, DOWN, UP, LEFT, RIGHT], description: "实验性穿甲武器",
        icon: "railgun" },
    Stratagem { category: CAT_SUPPORT, model: "FAF-14", name: "飞矛",
        command: cmd![DOWN, DOWN, UP, DOWN, DOWN], description: "自导反坦克导弹",
        icon: "spear" },
    Stratagem { category: CAT_SUPPORT, model: "StA-X3", name: "W.A.S.P.发射器",
        command: cmd![DOWN, DOWN, UP, DOWN, RIGHT], description: "七枚追踪导弹",
        icon: "sta_x3_w_a_s_p_launcher" },
    Stratagem { category: CAT_SUPPORT, model: "GL-21", name: "榴弹发射器",
        command: cmd![DOWN, LEFT, UP, LEFT, DOWN], description: "对装甲步兵有效",
        icon: "grenade_launcher" },
    Stratagem { category: CAT_SUPPORT, model: "LAS-98", name: "激光大炮",
        command: cmd![DOWN, LEFT, DOWN, UP, LEFT], description: "连续激光武器",
        icon: "laser_cannon" },
    Stratagem { category: CAT_SUPPORT, model: "ARC-3", name: "电弧发射器",
        command: cmd![DOWN, RIGHT, DOWN, UP, LEFT, LEFT], description: "电弧武器近距",
        icon: "arc_thrower" },
    Stratagem { category: CAT_SUPPORT, model: "LAS-99", name: "类星体加农炮",
        command: cmd![DOWN, DOWN, UP, LEFT, RIGHT], description: "蓄能爆炸能量炮",
        icon: "quasar_cannon" },
    Stratagem { category: CAT_SUPPORT, model: "TX-41", name: "灭菌器",
        command: cmd![DOWN, LEFT, UP, DOWN, LEFT], description: "腐蚀化学雾化武器",
        icon: "sterilizer" },
    Stratagem { category: CAT_SUPPORT, model: "S-11", name: "矛枪",
        command: cmd![DOWN, RIGHT, DOWN, LEFT, UP, RIGHT], description: "反坦克鱼叉枪",
        icon: "speargun" },
    Stratagem { category: CAT_SUPPORT, model: "M-1000", name: "重装机枪",
        command: cmd![DOWN, LEFT, RIGHT, DOWN, UP, UP], description: "带式供弹旋转机枪",
        icon: "maxigun" },
    Stratagem { category: CAT_SUPPORT, model: "EAT-700", name: "消耗性凝固汽油弹",
        command: cmd![DOWN, DOWN, LEFT, UP, LEFT], description: "单发凝固汽油弹",
        icon: "expendable_napalm" },
    Stratagem { category: CAT_SUPPORT, model: "GL-52", name: "缓和使者",
        command: cmd![DOWN, RIGHT, UP, LEFT, RIGHT], description: "人道榴弹发射器",
        icon: "gl_52_de_escalator" },
    Stratagem { category: CAT_SUPPORT, model: "CQC-9", name: "除叶工具",
        command: cmd![DOWN, LEFT, RIGHT, RIGHT, DOWN], description: "清场工具",
        icon: "defoliation_tool" },
    Stratagem { category: CAT_SUPPORT, model: "CQC-20", name: "破门锤",
        command: cmd![DOWN, LEFT, RIGHT, LEFT, UP], description: "破坏工具",
        icon: "cqc_20" },
    // ─── 哨戒炮 ───
    Stratagem { category: CAT_SENTRIES, model: "A/MG-43", name: "哨戒机枪",
        command: cmd![DOWN, UP, RIGHT, RIGHT, UP], description: "自动机枪哨戒",
        icon: "machine_gun_sentry" },
    Stratagem { category: CAT_SENTRIES, model: "A/G-16", name: "加特林哨戒炮",
        command: cmd![DOWN, UP, RIGHT, LEFT], description: "高射速自动炮",
        icon: "gatling_sentry" },
    Stratagem { category: CAT_SENTRIES, model: "A/M-12", name: "迫击哨戒炮",
        command: cmd![DOWN, UP, RIGHT, RIGHT, DOWN], description: "高抛炮击",
        icon: "mortar_sentry" },
    Stratagem { category: CAT_SENTRIES, model: "A/AC-8", name: "自动哨戒炮",
        command: cmd![DOWN, UP, RIGHT, UP, LEFT, UP], description: "远程反装甲炮",
        icon: "autocannon_sentry" },
    Stratagem { category: CAT_SENTRIES, model: "AX/AR-23", name: "护卫犬",
        command: cmd![DOWN, UP, LEFT, UP, RIGHT, DOWN], description: "护卫无人机",
        icon: "guard_dog" },
    Stratagem { category: CAT_SENTRIES, model: "A/MLS-4X", name: "火箭哨戒炮",
        command: cmd![DOWN, UP, RIGHT, RIGHT, LEFT], description: "火箭弹哨戒",
        icon: "rocket_sentry" },
    Stratagem { category: CAT_SENTRIES, model: "AX/TX-41", name: "灭杀犬",
        command: cmd![DOWN, UP, LEFT, UP, RIGHT, UP], description: "化学护卫无人机",
        icon: "guard_dog_breath" },
    // ─── 地雷，盾和手操炮台 ───
    Stratagem { category: CAT_EMPLACEMENTS, model: "MD-6", name: "反步兵雷区",
        command: cmd![DOWN, LEFT, UP, RIGHT], description: "反步兵地雷",
        icon: "anti_personnel_minefield" },
    Stratagem { category: CAT_EMPLACEMENTS, model: "MD-I4", name: "燃烧地雷",
        command: cmd![DOWN, LEFT, LEFT, DOWN], description: "燃烧地雷区",
        icon: "incendiary_mines" },
    Stratagem { category: CAT_EMPLACEMENTS, model: "MD-17", name: "反坦克地雷",
        command: cmd![DOWN, LEFT, UP, UP], description: "反坦克地雷区",
        icon: "anti_tank_mines" },
    Stratagem { category: CAT_EMPLACEMENTS, model: "SH-32", name: "防护罩生成器",
        command: cmd![DOWN, UP, LEFT, RIGHT, LEFT, RIGHT], description: "护盾圆顶",
        icon: "shield_generator_pack" },
    Stratagem { category: CAT_EMPLACEMENTS, model: "FX-12", name: "防护罩生成中继器",
        command: cmd![DOWN, DOWN, LEFT, RIGHT, LEFT, RIGHT], description: "防护盾中继器",
        icon: "shield_generator_relay" },
    Stratagem { category: CAT_EMPLACEMENTS, model: "E/MG-101", name: "重机枪炮台",
        command: cmd![DOWN, UP, LEFT, RIGHT, RIGHT, LEFT], description: "手动操作重机枪炮台",
        icon: "hmg_emplacement" },
    Stratagem { category: CAT_EMPLACEMENTS, model: "AT-47", name: "反坦克炮台",
        command: cmd![DOWN, UP, LEFT, RIGHT, RIGHT, RIGHT], description: "手动操作反坦克炮台",
        icon: "anti_tank_emplacement" },
    // ─── 背包 ───
    Stratagem { category: CAT_BACKPACKS, model: "B-1", name: "补给背包",
        command: cmd![DOWN, LEFT, DOWN, UP, UP, DOWN], description: "弹药补给背包",
        icon: "supply_pack" },
    Stratagem { category: CAT_BACKPACKS, model: "LIFT-850", name: "喷射背包",
        command: cmd![DOWN, UP, UP, DOWN, UP], description: "飞行背包",
        icon: "jump_pack" },
    Stratagem { category: CAT_BACKPACKS, model: "AX/LAS-5", name: "护卫犬漫游车",
        command: cmd![DOWN, UP, LEFT, UP, RIGHT, RIGHT], description: "激光护卫无人机背包",
        icon: "guard_dog_rover" },
    Stratagem { category: CAT_BACKPACKS, model: "SH-20", name: "防弹护盾背包",
        command: cmd![DOWN, LEFT, DOWN, DOWN, UP, LEFT], description: "防弹护盾",
        icon: "ballistic_shield_backpack" },
    Stratagem { category: CAT_BACKPACKS, model: "SH-51", name: "定向护盾",
        command: cmd![DOWN, UP, LEFT, RIGHT, UP, UP], description: "定向护盾背包",
        icon: "directional_shield" },
    Stratagem { category: CAT_BACKPACKS, model: "N/A", name: "地狱炸弹",
        command: cmd![DOWN, RIGHT, UP, UP, UP], description: "核弹背包",
        icon: "hellbomb_portable" },
    // ─── 载具 ───
    Stratagem { category: CAT_VEHICLES, model: "M-102", name: "侦察吉普",
        command: cmd![LEFT, DOWN, RIGHT, DOWN, RIGHT, DOWN, UP], description: "快速侦察载具",
        icon: "fast_recon_vehicle" },
    Stratagem { category: CAT_VEHICLES, model: "PATRIOT", name: "爱国者外骨骼装甲",
        command: cmd![LEFT, DOWN, RIGHT, UP, LEFT, DOWN, DOWN], description: "重武器外骨骼",
        icon: "patriot_exosuit" },
    Stratagem { category: CAT_VEHICLES, model: "EMANCIPATOR", name: "解放者外骨骼装甲",
        command: cmd![LEFT, DOWN, RIGHT, UP, LEFT, DOWN, UP], description: "双自动炮外骨骼",
        icon: "emancipator_exosuit" },
];

pub fn get_categories() -> Vec<&'static str> {
    let mut cats: Vec<&'static str> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for s in STRATAGEMS {
        if seen.insert(s.category) {
            cats.push(s.category);
        }
    }
    cats
}

pub fn get_by_category(cat: &str) -> Vec<&'static Stratagem> {
    STRATAGEMS.iter().filter(|s| s.category == cat).collect()
}

/// 名称/型号子串搜索（跨分类）
pub fn search(query: &str) -> Vec<&'static Stratagem> {
    let q = query.trim();
    if q.is_empty() {
        return Vec::new();
    }
    STRATAGEMS
        .iter()
        .filter(|s| s.name.contains(q) || s.model.contains(q))
        .collect()
}

pub fn command_to_string(cmd: &[&str]) -> String {
    cmd.join("")
}

/// 英文字符串方向 → 箭头符号
pub fn dir_to_arrow(dir: &str) -> &'static str {
    match dir {
        "up" => "↑",
        "down" => "↓",
        "left" => "←",
        "right" => "→",
        _ => "?",
    }
}

// ─── 插件战备运行时类型 ───

/// 从插件 JSON 加载的战备（所有字段为自有 String）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginStratagem {
    pub name: String,
    pub category: String,
    pub model: String,
    pub command: Vec<String>,
    pub description: String,
    pub icon: String,
}

/// 统合引用：基础内置战备（静态引用）或插件战备（自有数据引用）
pub enum StratagemRef<'a> {
    Base(&'static Stratagem),
    Plugin(&'a PluginStratagem),
}

/// 自有版本（可脱离 borrow 使用）
pub enum OwnedStratagem {
    Base(&'static Stratagem),
    Plugin(PluginStratagem),
}

impl OwnedStratagem {
    pub fn name(&self) -> &str {
        match self { OwnedStratagem::Base(s) => s.name, OwnedStratagem::Plugin(s) => &s.name }
    }
    pub fn category(&self) -> &str {
        match self { OwnedStratagem::Base(s) => s.category, OwnedStratagem::Plugin(s) => &s.category }
    }
    pub fn model(&self) -> &str {
        match self { OwnedStratagem::Base(s) => s.model, OwnedStratagem::Plugin(s) => &s.model }
    }
    pub fn command(&self) -> Vec<&str> {
        match self {
            OwnedStratagem::Base(s) => s.command.to_vec(),
            OwnedStratagem::Plugin(s) => s.command.iter().map(|c| dir_to_arrow(c.as_str())).collect(),
        }
    }
    pub fn description(&self) -> &str {
        match self { OwnedStratagem::Base(s) => s.description, OwnedStratagem::Plugin(s) => &s.description }
    }
    pub fn icon(&self) -> &str {
        match self { OwnedStratagem::Base(s) => s.icon, OwnedStratagem::Plugin(s) => &s.icon }
    }
    pub fn as_ref(&self) -> StratagemRef<'_> {
        match self {
            OwnedStratagem::Base(s) => StratagemRef::Base(s),
            OwnedStratagem::Plugin(s) => StratagemRef::Plugin(s),
        }
    }
}

impl StratagemRef<'_> {
    pub fn name(&self) -> &str {
        match self {
            StratagemRef::Base(s) => s.name,
            StratagemRef::Plugin(s) => &s.name,
        }
    }
    pub fn category(&self) -> &str {
        match self {
            StratagemRef::Base(s) => s.category,
            StratagemRef::Plugin(s) => &s.category,
        }
    }
    pub fn model(&self) -> &str {
        match self {
            StratagemRef::Base(s) => s.model,
            StratagemRef::Plugin(s) => &s.model,
        }
    }
    pub fn command(&self) -> Vec<&str> {
        match self {
            StratagemRef::Base(s) => s.command.to_vec(),
            StratagemRef::Plugin(s) => s.command.iter().map(|s| dir_to_arrow(s.as_str())).collect(),
        }
    }
    pub fn description(&self) -> &str {
        match self {
            StratagemRef::Base(s) => s.description,
            StratagemRef::Plugin(s) => &s.description,
        }
    }
    pub fn icon(&self) -> &str {
        match self {
            StratagemRef::Base(s) => s.icon,
            StratagemRef::Plugin(s) => &s.icon,
        }
    }
}

/// 插件主题颜色
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginTheme {
    pub name: String,
    pub background_color: String,
    pub border_color: String,
    pub accent_color: String,
}

/// 插件清单（对应 plugins/*.json）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub stratagems: Vec<PluginStratagem>,
    #[serde(default)]
    pub themes: Vec<PluginTheme>,
}

fn default_true() -> bool { true }
